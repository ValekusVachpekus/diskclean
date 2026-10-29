//! Offline snapd state and archive inventory; mounted /snap trees are excluded.
use super::*;
use diskclean_platform::snap::{self, RevisionState, SnapRevision};

const MAX_ENTRIES: usize = 20_000;
const MAX_BYTES: u64 = 64 * 1024 * 1024;

pub fn discover_snap(
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
        path: "/".into(),
        source,
    })?;
    let mut report = ApplicationsReport::default();
    let mut visited = HashSet::new();
    let mut entries = 0;
    let mut bytes = 0;
    for root in roots {
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        if entries >= MAX_ENTRIES || report.applications.len() >= MAX_ENTRIES {
            issue(
                &mut report,
                root.clone(),
                std::io::Error::other("Snap inventory limit reached; remaining sources skipped"),
            );
            break;
        }
        let root = std::path::absolute(root).map_err(|source| ScanError::Root {
            path: root.clone(),
            source,
        })?;
        let meta = match permitted(&root, &root, options, &mounts).and_then(|()| metadata(&root)) {
            Ok(meta) if meta.kind == FileKind::Directory => meta,
            Ok(_) => {
                issue(&mut report, root, std::io::ErrorKind::NotADirectory.into());
                continue;
            }
            Err(error) => {
                issue(&mut report, root, error);
                continue;
            }
        };
        if meta.ino != 0 && !visited.insert((meta.dev, meta.ino)) {
            continue;
        }
        let state_path = root.join("state.json");
        let state = match (|| {
            permitted(&root, &state_path, options, &mounts)?;
            let meta = metadata(&state_path)?;
            if meta.logical_size > MAX_BYTES - bytes {
                return Err(std::io::Error::other("Snap metadata byte limit reached"));
            }
            bytes += meta.logical_size;
            snap::read_state(&state_path, || control.is_cancelled())
        })() {
            Ok(state) => Some(state),
            Err(error) => {
                issue(&mut report, state_path.clone(), error);
                None
            }
        };
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let mut revisions = BTreeSet::new();
        let mut records = std::collections::BTreeMap::new();
        if let Some(state) = &state {
            if state.invalid_records != 0 {
                report.issues.push(ScanIssue::new(
                    state_path.clone(),
                    crate::IssueKind::Other,
                    format!("invalid Snap state records: {}", state.invalid_records),
                ));
            }
            for record in &state.revisions {
                let key = (record.instance.clone(), record.revision.clone());
                revisions.insert(key.clone());
                records.insert(key, record);
            }
        }
        let archives = root.join("snaps");
        match permitted(&root, &archives, options, &mounts).and_then(|()| fs::read_dir(&archives)) {
            Ok(dir) => {
                for entry in dir {
                    if control.is_cancelled() {
                        return Err(ScanError::Cancelled);
                    }
                    if entries >= MAX_ENTRIES {
                        issue(
                            &mut report,
                            archives.clone(),
                            std::io::Error::other("Snap directory entry limit reached"),
                        );
                        break;
                    }
                    entries += 1;
                    match entry {
                        Ok(entry) => {
                            let name = entry.file_name();
                            let Some(name) = name.to_str() else {
                                continue;
                            };
                            if let Some(key) = snap::archive_identity(name) {
                                revisions.insert(key);
                            } else if name.ends_with(".snap") {
                                issue(
                                    &mut report,
                                    entry.path(),
                                    std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        "invalid Snap archive filename",
                                    ),
                                );
                            }
                        }
                        Err(error) => issue(&mut report, archives.clone(), error),
                    }
                }
            }
            Err(error) => issue(&mut report, archives.clone(), error),
        }
        let start = report.applications.len();
        for (instance, revision) in revisions {
            if control.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            if report.applications.len() >= MAX_ENTRIES {
                issue(
                    &mut report,
                    root.clone(),
                    std::io::Error::other("Snap inventory entry limit reached"),
                );
                break;
            }
            let record = records.get(&(instance.clone(), revision.clone())).copied();
            let archive = archives.join(format!("{instance}_{revision}.snap"));
            let mut app = application(instance, revision, archive.clone(), &state_path, record);
            match permitted(&root, &archive, options, &mounts)
                .and_then(|()| snap::archive_metadata(&archive))
            {
                Ok(meta) => {
                    app.allocated_bytes = Some(meta.allocated_size);
                    app.logical_bytes = Some(meta.logical_size);
                    app.size_complete = true;
                    if record.is_some() {
                        app.status = InstallationStatus::Installed;
                    }
                }
                Err(error) => {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        app.status = InstallationStatus::Missing;
                    }
                    issue(&mut report, archive, error);
                }
            }
            report.applications.push(app);
        }
        if let Some(state) = state
            && !state.unchanged(&state_path).unwrap_or(false)
        {
            for app in &mut report.applications[start..] {
                app.snap_state = Some(SnapRevisionState::Unknown);
                if app.status == InstallationStatus::Installed {
                    app.status = InstallationStatus::Unknown;
                }
                // Shared-component classification depended on this state.
                app.launcher = "Snap".into();
            }
            issue(
                &mut report,
                state_path,
                std::io::Error::other(
                    "Snap state changed during inventory; refresh after snapd finishes",
                ),
            );
        }
    }
    if control.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    report.applications.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.id.cmp(&b.id))
            .then(a.archive_path.cmp(&b.archive_path))
    });
    Ok(report)
}

fn issue(report: &mut ApplicationsReport, path: PathBuf, error: std::io::Error) {
    report
        .issues
        .push(ScanIssue::from_io(path, "read Snap inventory", &error));
}
fn application(
    instance: String,
    revision: String,
    archive: PathBuf,
    state_path: &Path,
    record: Option<&SnapRevision>,
) -> Application {
    Application {
        id: format!("{instance}/{revision}"),
        name: instance,
        launcher: if record.is_some_and(|r| r.shared_component) {
            "Snap runtime"
        } else {
            "Snap"
        }
        .into(),
        status: InstallationStatus::Unknown,
        // An archive file must never be offered as a directory scan root.
        install_path: PathBuf::new(),
        manifest_path: if record.is_some() {
            state_path.into()
        } else {
            archive.clone()
        },
        registry_key: None,
        reported_size_bytes: None,
        archive_path: Some(archive),
        snap_state: Some(match record.map(|r| r.state) {
            Some(RevisionState::CurrentEnabled) => SnapRevisionState::CurrentEnabled,
            Some(RevisionState::CurrentDisabled) => SnapRevisionState::CurrentDisabled,
            Some(RevisionState::Retained) => SnapRevisionState::Retained,
            None => SnapRevisionState::Unknown,
        }),
        allocated_bytes: None,
        logical_bytes: None,
        size_complete: false,
        build_id: Some(revision),
        last_played_unix: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(path: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        let mut bytes = [0; 96];
        bytes[..4].copy_from_slice(b"hsqs");
        bytes[12..16].copy_from_slice(&131072u32.to_le_bytes());
        bytes[28] = 4;
        bytes[40..48].copy_from_slice(&96u64.to_le_bytes());
        fs::write(path, bytes).expect("archive");
    }
    fn state(root: &Path) -> String {
        let text = r#"{"data":{"snaps":{
            "example":{"type":"app","active":true,"current":"2","sequence":[{"name":"example","revision":"1"},{"name":"example","revision":"2"},{"name":"example","revision":"3"}]},
            "core22":{"type":"base","current":"4","sequence":[{"name":"core22","revision":"4"}]}
        }}}"#;
        fs::write(root.join("state.json"), text).expect("state");
        text.into()
    }
    #[test]
    fn inventory_measures_archives_retains_revisions_and_separates_system_components() {
        let temp = tempfile::tempdir().expect("temp");
        let text = state(temp.path());
        for name in [
            "example_1.snap",
            "example_2.snap",
            "core22_4.snap",
            "local_test_x1.snap",
        ] {
            archive(&temp.path().join("snaps").join(name));
        }
        let report = discover_snap(
            &[temp.path().into(), temp.path().into()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 5);
        assert_eq!(report.issues.len(), 1);
        let find = |id| {
            report
                .applications
                .iter()
                .find(|a| a.id == id)
                .expect("record")
        };
        assert_eq!(
            find("example/2").snap_state,
            Some(SnapRevisionState::CurrentEnabled)
        );
        assert_eq!(
            find("example/1").snap_state,
            Some(SnapRevisionState::Retained)
        );
        assert_eq!(
            find("core22/4").snap_state,
            Some(SnapRevisionState::CurrentDisabled)
        );
        assert_eq!(find("core22/4").launcher, "Snap runtime");
        assert_eq!(find("example/3").status, InstallationStatus::Missing);
        assert_eq!(find("local_test/x1").status, InstallationStatus::Unknown);
        for app in &report.applications {
            assert!(app.install_path.as_os_str().is_empty());
            assert!(app.reported_size_bytes.is_none());
            if app.status != InstallationStatus::Missing {
                assert_eq!(app.logical_bytes, Some(96));
                assert!(app.allocated_bytes.is_some());
            }
        }
        assert_eq!(
            fs::read_to_string(temp.path().join("state.json")).expect("unchanged"),
            text
        );
    }
    #[test]
    fn absent_or_bad_state_keeps_archives_uncertain_and_never_scans_mounted_content() {
        let temp = tempfile::tempdir().expect("temp");
        archive(&temp.path().join("snaps/example_1.snap"));
        fs::create_dir_all(temp.path().join("snap/example/1")).expect("mount fixture");
        fs::write(temp.path().join("snap/example/1/huge"), vec![0; 8192]).expect("data");
        for state in [
            None,
            Some("broken"),
            Some(r#"{"data":{"snaps":{"bad":42}}}"#),
        ] {
            if let Some(state) = state {
                fs::write(temp.path().join("state.json"), state).expect("state");
            }
            let control = ScanControl::new();
            let report = discover_snap(&[temp.path().into()], &ScanOptions::default(), &control)
                .expect("report");
            assert_eq!(report.applications.len(), 1);
            assert!(!report.issues.is_empty());
            let app = &report.applications[0];
            assert_eq!(app.logical_bytes, Some(96));
            assert_eq!(app.snap_state, Some(SnapRevisionState::Unknown));
            assert_eq!(app.status, InstallationStatus::Unknown);
            assert_eq!(control.snapshot().files, 0);
        }
        let control = ScanControl::new();
        control.cancel();
        assert!(matches!(
            discover_snap(&[temp.path().into()], &ScanOptions::default(), &control),
            Err(ScanError::Cancelled)
        ));
    }
    #[cfg(unix)]
    #[test]
    fn linked_archives_and_metadata_are_not_traversed() {
        let temp = tempfile::tempdir().expect("temp");
        state(temp.path());
        archive(&temp.path().join("real.snap"));
        fs::create_dir(temp.path().join("snaps")).expect("dir");
        std::os::unix::fs::symlink(
            temp.path().join("real.snap"),
            temp.path().join("snaps/example_2.snap"),
        )
        .expect("link");
        archive(&temp.path().join("snaps/core22_4.snap"));
        let report = discover_snap(
            &[temp.path().into()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        let app = report
            .applications
            .iter()
            .find(|a| a.id == "example/2")
            .expect("app");
        assert_eq!(app.allocated_bytes, None);
        assert_eq!(app.status, InstallationStatus::Unknown);
        assert_eq!(
            report
                .applications
                .iter()
                .find(|a| a.id == "core22/4")
                .expect("valid neighbour")
                .logical_bytes,
            Some(96)
        );
    }
}
