//! Read-only application inventory. Steam is the first supported provider.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use diskclean_platform::{FileKind, MountPoints, check_path_components, metadata, steam};
use serde::Serialize;

use crate::{ScanControl, ScanError, ScanIssue, ScanOptions, scan};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationStatus {
    Installed,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub launcher: String,
    pub status: InstallationStatus,
    #[serde(serialize_with = "crate::error::serialize_path")]
    pub install_path: PathBuf,
    #[serde(serialize_with = "crate::error::serialize_path")]
    pub manifest_path: PathBuf,
    pub allocated_bytes: Option<u64>,
    pub logical_bytes: Option<u64>,
    /// A scan issue or skipped mount makes the measured size a lower bound.
    pub size_complete: bool,
    pub build_id: Option<String>,
    pub last_played_unix: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplicationsReport {
    pub applications: Vec<Application>,
    pub issues: Vec<ScanIssue>,
}

/// Discover explicit Steam installation/library roots. Automatic locations are
/// selected by the caller, so tests and CLI never need to probe the real profile.
/// Additional libraries are followed only when mount policy permits it.
pub fn discover_steam(
    roots: &[PathBuf],
    options: &ScanOptions,
    control: &ScanControl,
) -> Result<ApplicationsReport, ScanError> {
    if control.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    if options.follow_symlinks {
        return Err(ScanError::FollowSymlinksUnsupported);
    }
    let mounts = MountPoints::read().map_err(|source| ScanError::Root {
        path: PathBuf::from("/"),
        source,
    })?;
    let mut report = ApplicationsReport::default();
    let mut libraries = BTreeSet::new();
    for root in roots {
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let root = std::path::absolute(root).map_err(|source| ScanError::Root {
            path: root.clone(),
            source,
        })?;
        if let Err(error) = check_path_components(&root) {
            report
                .issues
                .push(ScanIssue::from_io(root, "read metadata", &error));
            continue;
        }
        libraries.insert(root.clone());
        for relative in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
            let path = root.join(relative);
            match permitted(&root, &path, options, &mounts)
                .and_then(|()| steam::read_libraries(&path))
            {
                Ok(paths) => {
                    for library in paths {
                        match permitted(&root, &library, options, &mounts) {
                            Ok(()) => {
                                libraries.insert(library);
                            }
                            Err(error) => report.issues.push(ScanIssue::from_io(
                                library,
                                "read metadata",
                                &error,
                            )),
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    report
                        .issues
                        .push(ScanIssue::from_io(path, "read Steam libraries", &error))
                }
            }
        }
    }
    // Conflicting manifests for the same directory share one measurement.
    let mut sizes = HashMap::new();
    let mut visited = HashSet::new();
    for library in libraries {
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let apps = library.join("steamapps");
        if let Err(error) = permitted(&library, &apps, options, &mounts) {
            report
                .issues
                .push(ScanIssue::from_io(apps, "read metadata", &error));
            continue;
        }
        // Windows path spelling/case and bind aliases may name the same library.
        match metadata(&apps) {
            Ok(meta) if meta.ino != 0 && !visited.insert((meta.dev, meta.ino)) => continue,
            Ok(_) => {}
            Err(error) => {
                report
                    .issues
                    .push(ScanIssue::from_io(apps, "read metadata", &error));
                continue;
            }
        }
        let entries = match fs::read_dir(&apps) {
            Ok(entries) => entries,
            Err(error) => {
                report
                    .issues
                    .push(ScanIssue::from_io(apps, "read directory", &error));
                continue;
            }
        };
        for entry in entries {
            if control.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    report.issues.push(ScanIssue::from_io(
                        apps.clone(),
                        "read directory entry",
                        &error,
                    ));
                    continue;
                }
            };
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
                continue;
            }
            let manifest = match permitted(&library, &path, options, &mounts)
                .and_then(|()| steam::read_manifest(&path))
            {
                Ok(manifest) => manifest,
                Err(error) => {
                    report
                        .issues
                        .push(ScanIssue::from_io(path, "read Steam manifest", &error));
                    continue;
                }
            };
            let install_path = apps.join("common").join(&manifest.install_dir);
            let mut application = Application {
                id: manifest.app_id.to_string(),
                name: manifest.name,
                launcher: "Steam".into(),
                status: InstallationStatus::Unknown,
                install_path: install_path.clone(),
                manifest_path: path,
                allocated_bytes: None,
                logical_bytes: None,
                size_complete: false,
                build_id: manifest.build_id,
                last_played_unix: manifest.last_played,
            };
            match permitted(&library, &install_path, options, &mounts) {
                Ok(()) => {
                    let meta = match metadata(&install_path) {
                        Ok(meta) => meta,
                        Err(error) => {
                            report.issues.push(ScanIssue::from_io(
                                install_path,
                                "read metadata",
                                &error,
                            ));
                            report.applications.push(application);
                            continue;
                        }
                    };
                    if meta.kind == FileKind::Directory {
                        if manifest.state_flags.is_some_and(|flags| flags & 4 != 0) {
                            application.status = InstallationStatus::Installed;
                        }
                        let key = (
                            meta.dev,
                            meta.ino,
                            if meta.ino == 0 {
                                install_path.clone()
                            } else {
                                PathBuf::new()
                            },
                        );
                        let size = if let Some(size) = sizes.get(&key) {
                            Some(*size)
                        } else {
                            match scan(&install_path, options, control) {
                                Ok(tree) => {
                                    let root = &tree.nodes[0];
                                    let size = (
                                        root.allocated_size,
                                        root.logical_size,
                                        tree.issues.is_empty()
                                            && tree.stats.skipped_mounts == 0
                                            && tree.stats.symlinks == 0
                                            && options.max_depth.is_none()
                                            && options.exclude.is_empty()
                                            && !options.skip_hidden,
                                    );
                                    report.issues.extend(tree.issues);
                                    sizes.insert(key, size);
                                    Some(size)
                                }
                                Err(ScanError::Cancelled) => return Err(ScanError::Cancelled),
                                Err(error) => {
                                    report.issues.push(ScanIssue::new(
                                        install_path.clone(),
                                        crate::IssueKind::Other,
                                        error.to_string(),
                                    ));
                                    None
                                }
                            }
                        };
                        if let Some((allocated, logical, complete)) = size {
                            application.allocated_bytes = Some(allocated);
                            application.logical_bytes = Some(logical);
                            application.size_complete = complete;
                        }
                    } else {
                        report.issues.push(ScanIssue::new(
                            install_path.clone(),
                            crate::IssueKind::Other,
                            "installation path is not a directory",
                        ));
                    }
                }
                Err(error) => {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        application.status = InstallationStatus::Missing;
                    }
                    report
                        .issues
                        .push(ScanIssue::from_io(install_path, "read metadata", &error));
                }
            }
            report.applications.push(application);
        }
    }
    if control.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    report.applications.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.install_path.cmp(&b.install_path))
            .then(a.id.cmp(&b.id))
    });
    Ok(report)
}

fn permitted(
    root: &Path,
    path: &Path,
    options: &ScanOptions,
    mounts: &MountPoints,
) -> std::io::Result<()> {
    check_path_components(path)?;
    let meta = metadata(path)?;
    if !options.cross_filesystems {
        let root_meta = metadata(root)?;
        // Check the entire path between root and destination, including Linux bind mounts.
        let common = root.ancestors().find(|ancestor| path.starts_with(ancestor));
        let mut boundary = meta.dev != root_meta.dev;
        for endpoint in [root, path] {
            for ancestor in endpoint
                .ancestors()
                .take_while(|ancestor| Some(*ancestor) != common)
            {
                boundary |= mounts.contains(ancestor);
            }
        }
        if boundary {
            return Err(std::io::Error::other("filesystem boundary skipped"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path, id: u32, dir: &str, flags: &str) {
        fs::create_dir_all(root.join("steamapps")).expect("apps");
        fs::write(root.join(format!("steamapps/appmanifest_{id}.acf")), format!(r#""AppState" {{ "appid" "{id}" "name" "Game {id}" "installdir" "{dir}" "StateFlags" "{flags}" "SizeOnDisk" "999999" "buildid" "42" }}"#)).expect("manifest");
    }
    #[test]
    fn installed_missing_unknown_and_actual_size() {
        let temp = tempfile::tempdir().expect("temp");
        fixture(temp.path(), 1, "Installed", "4");
        fixture(temp.path(), 2, "Missing", "4");
        fixture(temp.path(), 3, "Partial", "1026");
        for dir in ["Installed", "Partial"] {
            let path = temp.path().join("steamapps/common").join(dir);
            fs::create_dir_all(&path).expect("game");
            fs::write(path.join("data"), b"game").expect("data");
        }
        let report = discover_steam(
            &[temp.path().into()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 3);
        assert_eq!(report.applications[0].status, InstallationStatus::Installed);
        assert_eq!(report.applications[0].logical_bytes, Some(4));
        assert_eq!(report.applications[1].status, InstallationStatus::Missing);
        assert_eq!(report.applications[1].allocated_bytes, None);
        assert_eq!(report.applications[2].status, InstallationStatus::Unknown);
    }
    #[test]
    fn libraries_deduplicate_and_bad_manifest_does_not_hide_games() {
        let temp = tempfile::tempdir().expect("temp");
        let extra = temp.path().join("extra");
        fixture(&extra, 1, "Game", "4");
        let data = extra.join("steamapps/common/Game");
        fs::create_dir_all(&data).expect("dir");
        fs::create_dir_all(temp.path().join("steamapps")).expect("apps");
        let escaped = extra.to_string_lossy().replace('\\', "\\\\");
        fs::write(
            temp.path().join("steamapps/libraryfolders.vdf"),
            format!(r#""libraryfolders" {{ "0" {{ "path" "{escaped}" }} "1" "{escaped}" }}"#),
        )
        .expect("libraries");
        fs::write(extra.join("steamapps/appmanifest_2.acf"), "broken").expect("bad");
        let report = discover_steam(
            &[temp.path().into(), extra],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 1);
        assert_eq!(report.issues.len(), 1);
    }
    #[test]
    fn cancellation_and_partial_sizes() {
        let control = ScanControl::new();
        control.cancel();
        assert!(matches!(
            discover_steam(&[], &ScanOptions::default(), &control),
            Err(ScanError::Cancelled)
        ));
        let temp = tempfile::tempdir().expect("temp");
        fixture(temp.path(), 1, "Game", "4");
        fs::create_dir_all(temp.path().join("steamapps/common/Game/nested")).expect("game");
        let report = discover_steam(
            &[temp.path().into()],
            &ScanOptions {
                max_depth: Some(0),
                ..Default::default()
            },
            &ScanControl::new(),
        )
        .expect("report");
        assert!(!report.applications[0].size_complete);
    }
    #[test]
    fn shared_folder_and_hardlinks_do_not_use_manifest_estimates() {
        let temp = tempfile::tempdir().expect("temp");
        fixture(temp.path(), 1, "Shared", "4");
        fixture(temp.path(), 2, "Shared", "4");
        let path = temp.path().join("steamapps/common/Shared");
        fs::create_dir_all(&path).expect("dir");
        fs::write(path.join("data"), [0; 123]).expect("file");
        fs::hard_link(path.join("data"), path.join("alias")).expect("hardlink");
        let report = discover_steam(
            &[temp.path().into()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 2);
        assert_eq!(report.applications[0].logical_bytes, Some(123));
        assert_eq!(
            report.applications[0].allocated_bytes,
            report.applications[1].allocated_bytes
        );
        assert_eq!(
            report.applications[0].install_path,
            report.applications[1].install_path
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn referenced_library_cannot_cross_a_mount_by_default() {
        let temp = tempfile::tempdir().expect("temp");
        fs::create_dir_all(temp.path().join("steamapps")).expect("apps");
        fs::write(
            temp.path().join("steamapps/libraryfolders.vdf"),
            r#""libraryfolders" { "0" { "path" "/proc" } }"#,
        )
        .expect("library");
        let report = discover_steam(
            &[temp.path().into()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert!(report.applications.is_empty());
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.path == Path::new("/proc")
                    && issue.message.contains("filesystem boundary"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn case_aliases_do_not_duplicate_a_library() {
        let temp = tempfile::tempdir().expect("temp");
        fixture(temp.path(), 1, "Game", "4");
        fs::create_dir_all(temp.path().join("steamapps/common/Game")).expect("game");
        let alias = PathBuf::from(temp.path().to_string_lossy().to_uppercase());
        let report = discover_steam(
            &[temp.path().into(), alias],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn linked_installations_and_metadata_are_rejected() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().expect("temp");
        fixture(temp.path(), 1, "Game", "4");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).expect("outside");
        fs::create_dir_all(temp.path().join("steamapps/common")).expect("common");
        symlink(&outside, temp.path().join("steamapps/common/Game")).expect("link");
        symlink(
            temp.path().join("steamapps/appmanifest_1.acf"),
            temp.path().join("steamapps/appmanifest_2.acf"),
        )
        .expect("link");
        let report = discover_steam(
            &[temp.path().into()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 1);
        assert_eq!(report.applications[0].status, InstallationStatus::Unknown);
        assert_eq!(report.applications[0].allocated_bytes, None);
        assert_eq!(report.issues.len(), 2);
    }
}
