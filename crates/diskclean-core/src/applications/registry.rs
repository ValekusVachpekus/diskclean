//! Registry evidence mapped into the shared inventory/measurement model.
use super::*;
use diskclean_platform::registry::{RegistryApplication, RegistryInventory};

pub fn discover_registry(
    options: &ScanOptions,
    control: &ScanControl,
) -> Result<ApplicationsReport, ScanError> {
    if control.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    if options.follow_symlinks {
        return Err(ScanError::FollowSymlinksUnsupported);
    }
    let inventory =
        diskclean_platform::registry::discover(|| control.is_cancelled()).map_err(|source| {
            if control.is_cancelled() {
                ScanError::Cancelled
            } else {
                ScanError::Root {
                    path: PathBuf::from("Windows registry"),
                    source,
                }
            }
        })?;
    map_inventory(inventory, options, control)
}

fn map_inventory(
    inventory: RegistryInventory,
    options: &ScanOptions,
    control: &ScanControl,
) -> Result<ApplicationsReport, ScanError> {
    let mounts = MountPoints::read().map_err(|source| ScanError::Root {
        path: PathBuf::from("Windows registry"),
        source,
    })?;
    let mut report = ApplicationsReport::default();
    let mut sizes = HashMap::new();
    for (key, message) in inventory.issues {
        report.issues.push(ScanIssue::new(
            PathBuf::from(key),
            crate::IssueKind::Other,
            message,
        ));
    }
    for RegistryApplication {
        key,
        name,
        version,
        install_path,
    } in inventory.applications
    {
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let mut app = Application {
            id: key.clone(),
            name,
            launcher: "Windows Registry".into(),
            status: InstallationStatus::Unknown,
            install_path: install_path.unwrap_or_default(),
            manifest_path: PathBuf::new(),
            registry_key: Some(key),
            reported_size_bytes: None,
            archive_path: None,
            snap_state: None,
            allocated_bytes: None,
            logical_bytes: None,
            size_complete: false,
            build_id: version,
            last_played_unix: None,
        };
        // Each recorded directory is a scan root: registry metadata has no
        // enclosing filesystem library. The scanner rejects nested mounts.
        let anchor = app.install_path.clone();
        measure_installation(
            &mut app,
            true,
            &anchor,
            options,
            control,
            &mounts,
            &mut sizes,
            &mut report.issues,
        )?;
        report.applications.push(app);
    }
    if control.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    report
        .applications
        .sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(id: &str, path: Option<PathBuf>) -> RegistryApplication {
        RegistryApplication {
            key: format!("HKCU[64]\\Uninstall\\{id}"),
            name: id.into(),
            version: Some("1.2".into()),
            install_path: path,
        }
    }
    #[test]
    fn registry_unknown_paths_shared_sizes_missing_and_evidence() {
        let temp = tempfile::tempdir().expect("temp");
        let game = temp.path().join("Game");
        fs::create_dir(&game).expect("game");
        fs::write(game.join("data"), [0; 321]).expect("data");
        fs::hard_link(game.join("data"), game.join("alias")).expect("alias");
        let report = map_inventory(
            RegistryInventory {
                applications: vec![
                    row("A", Some(game.clone())),
                    row("B", Some(game)),
                    row("C", None),
                    row("D", Some(temp.path().join("missing"))),
                ],
                issues: vec![("HKLM[32]".into(), "access denied".into())],
            },
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 4);
        let a = &report.applications;
        assert_eq!(a[0].logical_bytes, Some(321));
        assert_eq!(a[0].allocated_bytes, a[1].allocated_bytes);
        assert_eq!(a[0].status, InstallationStatus::Installed);
        assert!(a[0].manifest_path.as_os_str().is_empty());
        assert_eq!(a[0].registry_key.as_deref(), Some("HKCU[64]\\Uninstall\\A"));
        assert_eq!(a[2].status, InstallationStatus::Unknown);
        assert_eq!(a[2].logical_bytes, None);
        assert!(a[2].install_path.as_os_str().is_empty());
        assert_eq!(a[3].status, InstallationStatus::Missing);
        assert_eq!(report.issues.len(), 2);
    }
    #[test]
    fn registry_cancel_and_explicit_path_rejected() {
        let control = ScanControl::new();
        control.cancel();
        assert!(matches!(
            discover_registry(&ScanOptions::default(), &control),
            Err(ScanError::Cancelled)
        ));
        assert!(
            discover_applications(
                Some(&[PathBuf::from("unused")]),
                ApplicationSource::Registry,
                &ScanOptions::default(),
                &ScanControl::new()
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn registry_installation_links_are_never_measured() {
        let temp = tempfile::tempdir().expect("temp");
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(temp.path(), &link).expect("link");
        let report = map_inventory(
            RegistryInventory {
                applications: vec![row("Link", Some(link))],
                issues: vec![],
            },
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications[0].logical_bytes, None);
        assert_eq!(report.applications[0].status, InstallationStatus::Unknown);
        assert_eq!(report.issues.len(), 1);
    }
}
