//! Read-only launcher application inventory.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use diskclean_platform::{FileKind, MountPoints, check_path_components, metadata, steam};
use serde::Serialize;

use crate::{ScanControl, ScanError, ScanIssue, ScanOptions, scan};

mod gog;
pub use gog::discover_gog;

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
            measure_installation(
                &mut application,
                manifest.state_flags.is_some_and(|flags| flags & 4 != 0),
                &library,
                options,
                control,
                &mounts,
                &mut sizes,
                &mut report.issues,
            )?;
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

/// Read Epic's manifest directory, including explicit directories on Linux.
/// Foreign Windows paths in Wine metadata are reported, never translated.
pub fn discover_epic(
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
    let mut sizes = HashMap::new();
    let mut visited = HashSet::new();
    for root in roots {
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let root = std::path::absolute(root).map_err(|source| ScanError::Root {
            path: root.clone(),
            source,
        })?;
        let entries = match permitted(&root, &root, options, &mounts).and_then(|()| {
            let meta = metadata(&root)?;
            if meta.ino != 0 && !visited.insert((meta.dev, meta.ino)) {
                return Ok(None);
            }
            fs::read_dir(&root).map(Some)
        }) {
            Ok(Some(entries)) => entries,
            Ok(None) => continue,
            Err(error) => {
                report
                    .issues
                    .push(ScanIssue::from_io(root, "read directory", &error));
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
                        root.clone(),
                        "read directory entry",
                        &error,
                    ));
                    continue;
                }
            };
            if !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("item"))
            {
                continue;
            }
            let manifest = match permitted(&root, &path, options, &mounts)
                .and_then(|()| diskclean_platform::epic::read_manifest(&path))
            {
                Ok(manifest) => manifest,
                Err(error) => {
                    report
                        .issues
                        .push(ScanIssue::from_io(path, "read Epic manifest", &error));
                    continue;
                }
            };
            let installed =
                manifest.incomplete == Some(false) && manifest.needs_validation == Some(false);
            let mut application = Application {
                id: manifest.app_name,
                name: manifest.display_name,
                launcher: "Epic Games".into(),
                status: InstallationStatus::Unknown,
                install_path: manifest.install_location,
                manifest_path: path,
                allocated_bytes: None,
                logical_bytes: None,
                size_complete: false,
                build_id: manifest.app_version_string,
                last_played_unix: None,
            };
            measure_installation(
                &mut application,
                installed,
                &root,
                options,
                control,
                &mounts,
                &mut sizes,
                &mut report.issues,
            )?;
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

/// Source format for explicit roots. Automatic discovery searches all sources.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ApplicationSource {
    #[default]
    Steam,
    Epic,
    Legendary,
    Heroic,
    Gog,
    HeroicGog,
}

/// Legendary config directories, including Heroic's Epic backend. Account-wide
/// catalogs and user.json are never read. Heroic GOG/Amazon are not included.
pub fn discover_legendary(
    roots: &[PathBuf],
    heroic: bool,
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
    let mut sizes = HashMap::new();
    let mut visited = HashSet::new();
    for root in roots {
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let root = std::path::absolute(root).map_err(|source| ScanError::Root {
            path: root.clone(),
            source,
        })?;
        let path = root.join("installed.json");
        let installed = match permitted(&root, &path, options, &mounts).and_then(|()| {
            let meta = metadata(&path)?;
            if meta.ino != 0 && !visited.insert((meta.dev, meta.ino)) {
                return Ok(None);
            }
            diskclean_platform::legendary::read_installed(&path).map(Some)
        }) {
            Ok(Some(records)) => records,
            Ok(None) => continue,
            Err(error) => {
                report.issues.push(ScanIssue::from_io(
                    path,
                    "read Legendary installations",
                    &error,
                ));
                continue;
            }
        };
        for id in installed.invalid_ids {
            report.issues.push(ScanIssue::new(
                path.clone(),
                crate::IssueKind::Other,
                format!("invalid Legendary installed-game record: {id}"),
            ));
        }
        for game in installed.games {
            if control.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let complete =
                game.needs_verification == Some(false) && game.is_preloaded == Some(false);
            let mut application = Application {
                id: game.app_name,
                name: game.title,
                launcher: if heroic { "Heroic (Epic)" } else { "Legendary" }.into(),
                status: InstallationStatus::Unknown,
                install_path: game.install_path,
                manifest_path: path.clone(),
                allocated_bytes: None,
                logical_bytes: None,
                size_complete: false,
                build_id: game.version,
                last_played_unix: None,
            };
            measure_installation(
                &mut application,
                complete,
                &root,
                options,
                control,
                &mounts,
                &mut sizes,
                &mut report.issues,
            )?;
            report.applications.push(application);
        }
    }
    if control.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    report.applications.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.id.cmp(&b.id))
            .then(a.install_path.cmp(&b.install_path))
    });
    Ok(report)
}

/// Explicit roots never mix with profile probes. With no roots all native
/// providers are searched; their source evidence is retained in separate records.
pub fn discover_applications(
    roots: Option<&[PathBuf]>,
    source: ApplicationSource,
    options: &ScanOptions,
    control: &ScanControl,
) -> Result<ApplicationsReport, ScanError> {
    if let Some(roots) = roots {
        return match source {
            ApplicationSource::Steam => discover_steam(roots, options, control),
            ApplicationSource::Epic => discover_epic(roots, options, control),
            ApplicationSource::Gog | ApplicationSource::HeroicGog => discover_gog(
                roots,
                source == ApplicationSource::HeroicGog,
                options,
                control,
            ),
            ApplicationSource::Legendary | ApplicationSource::Heroic => {
                discover_legendary(roots, source == ApplicationSource::Heroic, options, control)
            }
        };
    }
    let mut report = ApplicationsReport::default();
    for (source, roots) in [
        (ApplicationSource::Steam, steam::default_roots()),
        (
            ApplicationSource::Epic,
            diskclean_platform::epic::default_roots(),
        ),
        (
            ApplicationSource::Legendary,
            diskclean_platform::legendary::default_roots(),
        ),
        (
            ApplicationSource::Heroic,
            diskclean_platform::legendary::heroic_roots(),
        ),
        (
            ApplicationSource::HeroicGog,
            diskclean_platform::gog::heroic_roots(),
        ),
    ] {
        let roots: Vec<_> = roots
            .into_iter()
            .filter(|path| {
                let probe = if source == ApplicationSource::HeroicGog {
                    path.join("gog_store/installed.json")
                } else {
                    path.clone()
                };
                probe.symlink_metadata().is_ok()
            })
            .collect();
        let other = discover_applications(Some(&roots), source, options, control)?;
        report.applications.extend(other.applications);
        report.issues.extend(other.issues);
    }
    Ok(report)
}

type SizeCache = HashMap<(u64, u64, PathBuf), (u64, u64, bool)>;

#[allow(clippy::too_many_arguments)]
fn measure_installation(
    application: &mut Application,
    installed: bool,
    library: &Path,
    options: &ScanOptions,
    control: &ScanControl,
    mounts: &MountPoints,
    sizes: &mut SizeCache,
    issues: &mut Vec<ScanIssue>,
) -> Result<(), ScanError> {
    let install_path = application.install_path.clone();
    match permitted(library, &install_path, options, mounts) {
        Ok(()) => {
            let meta = match metadata(&install_path) {
                Ok(meta) => meta,
                Err(error) => {
                    issues.push(ScanIssue::from_io(install_path, "read metadata", &error));
                    return Ok(());
                }
            };
            if meta.kind == FileKind::Directory {
                if installed {
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
                            issues.extend(tree.issues);
                            sizes.insert(key, size);
                            Some(size)
                        }
                        Err(ScanError::Cancelled) => return Err(ScanError::Cancelled),
                        Err(error) => {
                            issues.push(ScanIssue::new(
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
                issues.push(ScanIssue::new(
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
            issues.push(ScanIssue::from_io(install_path, "read metadata", &error));
        }
    }
    Ok(())
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

    fn legendary_fixture(root: &Path, install: &Path) {
        fs::create_dir_all(root).expect("config");
        let good = serde_json::json!({"app_name":"installed", "title":"A Game", "install_path":install, "version":"v2", "needs_verification":false, "is_preloaded":false, "install_size":999999});
        let mut rows = serde_json::json!({"installed":good, "broken": {"title":"broken"}});
        for (id, title, flag) in [
            ("verify", "B Verify", "needs_verification"),
            ("preload", "C Preload", "is_preloaded"),
        ] {
            let mut game = good.clone();
            game["app_name"] = id.into();
            game["title"] = title.into();
            game[flag] = true.into();
            rows[id] = game;
        }
        let mut missing = good.clone();
        missing["app_name"] = "missing".into();
        missing["title"] = "D Missing".into();
        missing["install_path"] = install.join("absent").to_string_lossy().as_ref().into();
        rows["missing"] = missing;
        let mut uncertain = good;
        uncertain["app_name"] = "unknown".into();
        uncertain["title"] = "E Unknown".into();
        uncertain
            .as_object_mut()
            .expect("object")
            .remove("is_preloaded");
        rows["unknown"] = uncertain;
        fs::write(root.join("installed.json"), rows.to_string()).expect("metadata");
    }

    #[test]
    fn legendary_shared_sizes_status_evidence_invalid_rows_and_cancel() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("Config");
        let install = temp.path().join("Game");
        fs::create_dir_all(&install).expect("game");
        fs::write(install.join("data"), [0; 111]).expect("data");
        fs::hard_link(install.join("data"), install.join("alias")).expect("hardlink");
        legendary_fixture(&root, &install);
        for source in [ApplicationSource::Legendary, ApplicationSource::Heroic] {
            let report = discover_applications(
                Some(&[root.clone(), root.clone()]),
                source,
                &ScanOptions::default(),
                &ScanControl::new(),
            )
            .expect("report");
            assert_eq!(report.applications.len(), 5);
            assert_eq!(report.issues.len(), 2); // invalid row and missing directory
            let installed = &report.applications[0];
            assert_eq!(installed.status, InstallationStatus::Installed);
            assert_eq!(installed.logical_bytes, Some(111));
            assert_eq!(installed.build_id.as_deref(), Some("v2"));
            assert_eq!(installed.manifest_path, root.join("installed.json"));
            assert_eq!(
                installed.launcher,
                if source == ApplicationSource::Heroic {
                    "Heroic (Epic)"
                } else {
                    "Legendary"
                }
            );
            assert!(installed.size_complete);
            for index in [1, 2, 4] {
                assert_eq!(
                    report.applications[index].status,
                    InstallationStatus::Unknown
                );
                assert_eq!(
                    report.applications[index].allocated_bytes,
                    installed.allocated_bytes
                );
            }
            assert_eq!(report.applications[3].status, InstallationStatus::Missing);
            assert_eq!(report.applications[3].allocated_bytes, None);
        }
        let partial = discover_legendary(
            std::slice::from_ref(&root),
            false,
            &ScanOptions {
                max_depth: Some(0),
                ..Default::default()
            },
            &ScanControl::new(),
        )
        .expect("partial");
        assert!(!partial.applications[0].size_complete);
        let control = ScanControl::new();
        control.cancel();
        assert!(matches!(
            discover_legendary(&[root], false, &ScanOptions::default(), &control),
            Err(ScanError::Cancelled)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn legendary_rejects_linked_metadata_and_installation() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("Config");
        let real = temp.path().join("Real");
        let linked = temp.path().join("Linked");
        fs::create_dir_all(&real).expect("real");
        symlink(&real, &linked).expect("link");
        legendary_fixture(&root, &linked);
        let report = discover_legendary(
            std::slice::from_ref(&root),
            false,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert!(report.applications.iter().all(
            |app| app.allocated_bytes.is_none() && app.status != InstallationStatus::Installed
        ));
        let alias = temp.path().join("Alias");
        fs::create_dir_all(&alias).expect("alias");
        symlink(root.join("installed.json"), alias.join("installed.json")).expect("metadata link");
        let report = discover_legendary(
            &[alias],
            false,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert!(report.applications.is_empty());
        assert_eq!(report.issues.len(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn legendary_installations_cannot_cross_mounts_by_default() {
        let temp = tempfile::tempdir().expect("temp");
        legendary_fixture(temp.path(), Path::new("/proc"));
        let report = discover_legendary(
            &[temp.path().into()],
            false,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert!(
            report
                .applications
                .iter()
                .all(|app| app.allocated_bytes.is_none())
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.message.contains("filesystem boundary"))
        );
    }

    fn epic_fixture(root: &Path, file: &str, install: &Path, incomplete: bool) {
        fs::create_dir_all(root).expect("manifests");
        fs::write(
            root.join(file),
            serde_json::json!({
                "AppName": file, "DisplayName": file, "InstallLocation": install,
                "AppVersionString": "1.2", "bIsIncompleteInstall": incomplete,
                "bNeedsValidation": false, "InstallSize": 9999999
            })
            .to_string(),
        )
        .expect("manifest");
    }

    #[test]
    fn epic_measures_shared_installs_and_preserves_uncertain_and_missing_records() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("Manifests");
        let install = temp.path().join("Game");
        fs::create_dir_all(&install).expect("install");
        fs::write(install.join("data"), [0; 321]).expect("data");
        epic_fixture(&root, "a.item", &install, false);
        epic_fixture(&root, "b.item", &install, true);
        epic_fixture(&root, "c.item", &temp.path().join("Missing"), false);
        fs::write(root.join("bad.item"), "broken").expect("bad");
        let report = discover_applications(
            Some(&[root.clone(), root]),
            ApplicationSource::Epic,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 3);
        let a = &report.applications[0];
        assert_eq!(a.launcher, "Epic Games");
        assert_eq!(a.status, InstallationStatus::Installed);
        assert_eq!(a.logical_bytes, Some(321));
        assert_eq!(a.build_id.as_deref(), Some("1.2"));
        assert!(a.size_complete);
        assert_eq!(report.applications[1].status, InstallationStatus::Unknown);
        assert_eq!(a.allocated_bytes, report.applications[1].allocated_bytes);
        assert_eq!(report.applications[2].status, InstallationStatus::Missing);
        assert_eq!(report.issues.len(), 2);
        let control = ScanControl::new();
        control.cancel();
        assert!(matches!(
            discover_epic(&[], &ScanOptions::default(), &control),
            Err(ScanError::Cancelled)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn epic_rejects_links_in_metadata_and_install_paths() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("Manifests");
        let real = temp.path().join("Real");
        fs::create_dir_all(&real).expect("real");
        let linked = temp.path().join("Linked");
        symlink(&real, &linked).expect("link");
        epic_fixture(&root, "game.item", &linked, false);
        symlink(root.join("game.item"), root.join("alias.item")).expect("manifest link");
        let report =
            discover_epic(&[root], &ScanOptions::default(), &ScanControl::new()).expect("report");
        assert_eq!(report.applications.len(), 1);
        assert_eq!(report.applications[0].status, InstallationStatus::Unknown);
        assert_eq!(report.applications[0].allocated_bytes, None);
        assert_eq!(report.issues.len(), 2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn epic_install_mounts_require_explicit_crossing() {
        let temp = tempfile::tempdir().expect("temp");
        epic_fixture(temp.path(), "game.item", Path::new("/proc"), false);
        let report = discover_epic(
            &[temp.path().into()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications[0].allocated_bytes, None);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.message.contains("filesystem boundary"))
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
