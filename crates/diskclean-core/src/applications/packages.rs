//! System packages have distributed files, not a safe installation-directory root.
use super::*;
use diskclean_platform::packages::{PackageInventory, PackageManager};
const MAX_ENTRIES: usize = 20_000;
const MAX_BYTES: u64 = 64 * 1024 * 1024;

pub fn discover_packages(
    roots: &[PathBuf],
    manager: PackageManager,
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
    let mut visited = HashSet::new();
    let mut ids = HashSet::new();
    let mut bytes = 0;
    for root in roots {
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let root = std::path::absolute(root).map_err(|source| ScanError::Root {
            path: root.clone(),
            source,
        })?;
        let paths = (|| -> std::io::Result<Vec<PathBuf>> {
            permitted(&root, &root, options, &mounts)?;
            let meta = metadata(&root)?;
            if meta.kind != FileKind::Directory {
                return Err(std::io::ErrorKind::NotADirectory.into());
            }
            if meta.ino != 0 && !visited.insert((meta.dev, meta.ino)) {
                return Ok(Vec::new());
            }
            if manager == PackageManager::Dpkg {
                return Ok(vec![root.join("status")]);
            }
            let mut paths = Vec::new();
            for (index, entry) in fs::read_dir(&root)?.enumerate() {
                if control.is_cancelled() {
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                if index >= MAX_ENTRIES {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "package directory entry limit reached",
                    ));
                }
                let entry = entry?;
                let path = entry.path();
                let meta = metadata(&path)?;
                if meta.kind == FileKind::File {
                    continue;
                } // ALPM_DB_VERSION etc.
                paths.push(path.join("desc"));
            }
            paths.sort();
            Ok(paths)
        })();
        let paths = match paths {
            Ok(paths) => paths,
            Err(error) => {
                if control.is_cancelled() {
                    return Err(ScanError::Cancelled);
                }
                report.issues.push(ScanIssue::from_io(
                    root.clone(),
                    "read package database",
                    &error,
                ));
                continue;
            }
        };
        for path in paths {
            if control.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let result = (|| -> std::io::Result<PackageInventory> {
                permitted(&root, &path, options, &mounts)?;
                let meta = metadata(&path)?;
                if meta.logical_size > MAX_BYTES - bytes {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "package metadata byte limit reached",
                    ));
                }
                bytes += meta.logical_size;
                match manager {
                    PackageManager::Dpkg => {
                        diskclean_platform::packages::read_dpkg(&path, || control.is_cancelled())
                    }
                    PackageManager::Pacman => {
                        diskclean_platform::packages::read_pacman(&path, || control.is_cancelled())
                    }
                }
            })();
            if control.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            match result {
                Ok(inventory) => {
                    if inventory.invalid_records > 0 {
                        report.issues.push(ScanIssue::new(
                            path.clone(),
                            crate::IssueKind::Other,
                            format!(
                                "invalid package records or size fields: {}",
                                inventory.invalid_records
                            ),
                        ));
                    }
                    for package in inventory.packages {
                        if control.is_cancelled() {
                            return Err(ScanError::Cancelled);
                        }
                        if !ids.insert((manager.label(), package.id.clone())) {
                            report.issues.push(ScanIssue::new(
                                path.clone(),
                                crate::IssueKind::Other,
                                "duplicate package identity",
                            ));
                            continue;
                        }
                        if report.applications.len() >= MAX_ENTRIES {
                            report.issues.push(ScanIssue::new(
                                path.clone(),
                                crate::IssueKind::Other,
                                "package inventory entry limit reached",
                            ));
                            return Ok(report);
                        }
                        report.applications.push(Application {
                            id: package.id,
                            name: package.name,
                            launcher: manager.label().into(),
                            status: InstallationStatus::Installed,
                            install_path: PathBuf::new(),
                            manifest_path: path.clone(),
                            registry_key: None,
                            reported_size_bytes: package.reported_size_bytes,
                            archive_path: None,
                            snap_state: None,
                            allocated_bytes: None,
                            logical_bytes: None,
                            size_complete: false,
                            build_id: Some(package.version),
                            last_played_unix: None,
                        });
                    }
                }
                Err(error) => {
                    report
                        .issues
                        .push(ScanIssue::from_io(path, "read package metadata", &error))
                }
            }
        }
    }
    report
        .applications
        .sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dpkg_inventory_retains_estimates_without_scanning_distributed_files() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("status");
        let content = "Package: example\nStatus: install ok installed\nVersion: 1\nArchitecture: amd64\nInstalled-Size: 3\n\nPackage: removed\nStatus: deinstall ok config-files\n";
        fs::write(&path, content).expect("write");
        let report = discover_packages(
            &[temp.path().into(), temp.path().into()],
            PackageManager::Dpkg,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 1);
        assert!(report.issues.is_empty());
        let app = &report.applications[0];
        assert_eq!(app.reported_size_bytes, Some(3072));
        assert_eq!(app.allocated_bytes, None);
        assert_eq!(app.logical_bytes, None);
        assert!(app.install_path.as_os_str().is_empty());
        assert_eq!(app.manifest_path, path);
        assert_eq!(fs::read_to_string(path).expect("after"), content);
        let control = ScanControl::new();
        control.cancel();
        assert!(matches!(
            discover_packages(&[], PackageManager::Dpkg, &ScanOptions::default(), &control),
            Err(ScanError::Cancelled)
        ));
    }
    #[test]
    fn pacman_local_database_preserves_valid_neighbours_and_flags_duplicates() {
        let temp = tempfile::tempdir().expect("temp");
        for dir in ["one-1-1", "duplicate", "bad"] {
            fs::create_dir(temp.path().join(dir)).expect("dir");
        }
        let content = "%NAME%\nexample\n\n%VERSION%\n1-1\n\n%ARCH%\nx86_64\n\n%SIZE%\n321\n";
        for dir in ["one-1-1", "duplicate"] {
            fs::write(temp.path().join(dir).join("desc"), content).expect("desc");
        }
        fs::write(temp.path().join("bad/desc"), "broken").expect("bad");
        fs::write(temp.path().join("ALPM_DB_VERSION"), "9").expect("version");
        let report = discover_packages(
            &[temp.path().into()],
            PackageManager::Pacman,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 1);
        assert_eq!(report.applications[0].reported_size_bytes, Some(321));
        assert_eq!(report.issues.len(), 2);
    }
    #[cfg(unix)]
    #[test]
    fn package_sources_reject_links_without_hiding_neighbour_packages() {
        let temp = tempfile::tempdir().expect("temp");
        let real = temp.path().join("real");
        fs::create_dir(&real).expect("dir");
        fs::write(
            real.join("desc"),
            "%NAME%\nexample\n\n%VERSION%\n1-1\n\n%ARCH%\nx86_64\n",
        )
        .expect("desc");
        std::os::unix::fs::symlink(&real, temp.path().join("linked")).expect("link");
        let report = discover_packages(
            &[temp.path().into()],
            PackageManager::Pacman,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 1);
        assert_eq!(report.issues.len(), 1);
    }
}
