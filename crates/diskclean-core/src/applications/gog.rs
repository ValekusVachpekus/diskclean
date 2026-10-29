//! GOG installation metadata and Heroic's local installed GOG store.
use super::*;
use diskclean_platform::gog;

const MAX_ENTRIES: usize = 100_000;

/// Heroic roots contain gog_store/installed.json. Direct GOG roots are a game
/// folder or a library with game folders immediately below it; no recursive hunt.
pub fn discover_gog(
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
        let source = if heroic {
            root.join("gog_store/installed.json")
        } else {
            root.clone()
        };
        let meta = match permitted(&root, &source, options, &mounts)
            .and_then(|()| metadata(&source))
        {
            Ok(meta) => meta,
            Err(error) => {
                report
                    .issues
                    .push(ScanIssue::from_io(source, "read GOG installations", &error));
                continue;
            }
        };
        if meta.ino != 0 && !visited.insert((meta.dev, meta.ino)) {
            continue;
        }
        let mut records = Vec::new();
        if heroic {
            match gog::read_heroic(&source) {
                Ok(installed) => {
                    for index in installed.invalid_records {
                        report.issues.push(ScanIssue::new(
                            source.clone(),
                            crate::IssueKind::Other,
                            format!("invalid Heroic GOG installed record: {index}"),
                        ));
                    }
                    for game in installed.games {
                        let name = game
                            .install_path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .filter(|name| !name.trim().is_empty())
                            .unwrap_or_else(|| format!("GOG {}", game.id));
                        records.push(Application {
                            id: game.id,
                            name,
                            launcher: "Heroic (GOG)".into(),
                            status: InstallationStatus::Unknown,
                            install_path: game.install_path,
                            manifest_path: source.clone(),
                            registry_key: None,
                            reported_size_bytes: None,
                            archive_path: None,
                            snap_state: None,
                            allocated_bytes: None,
                            logical_bytes: None,
                            size_complete: false,
                            build_id: game.build.or(game.version),
                            last_played_unix: None,
                        });
                    }
                }
                Err(error) => {
                    report.issues.push(ScanIssue::from_io(
                        source,
                        "read Heroic GOG installations",
                        &error,
                    ));
                    continue;
                }
            }
        } else {
            let mut paths = info_paths(&root, options, &mounts, control, &mut report.issues)?;
            paths.sort();
            for path in paths {
                if control.is_cancelled() {
                    return Err(ScanError::Cancelled);
                }
                match permitted(&root, &path, options, &mounts).and_then(|()| gog::read_info(&path))
                {
                    Ok(info) => {
                        let Some(parent) = path.parent() else {
                            continue;
                        };
                        records.push(Application {
                            id: info.id,
                            name: info.name,
                            launcher: "GOG".into(),
                            status: InstallationStatus::Unknown,
                            install_path: parent.into(),
                            manifest_path: path,
                            registry_key: None,
                            reported_size_bytes: None,
                            archive_path: None,
                            snap_state: None,
                            allocated_bytes: None,
                            logical_bytes: None,
                            size_complete: false,
                            build_id: info.build,
                            last_played_unix: None,
                        });
                    }
                    Err(error) => {
                        report
                            .issues
                            .push(ScanIssue::from_io(path, "read GOG info", &error))
                    }
                }
            }
        }
        for mut app in records {
            if control.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            measure_installation(
                &mut app,
                true,
                &root,
                options,
                control,
                &mounts,
                &mut sizes,
                &mut report.issues,
            )?;
            if app.allocated_bytes.is_some() {
                if heroic {
                    let path = app.install_path.join(format!("goggame-{}.info", app.id));
                    match permitted(&root, &path, options, &mounts)
                        .and_then(|()| gog::read_info(&path))
                    {
                        Ok(info) => {
                            app.name = info.name;
                            if info.build.is_some() {
                                app.build_id = info.build;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            app.status = InstallationStatus::Unknown;
                            report
                                .issues
                                .push(ScanIssue::from_io(path, "read GOG info", &error));
                        }
                    }
                }
                // A gogdl resume file indicates an unfinished download. Its contents
                // are never read and links/reparse points still produce an issue.
                let path = app.install_path.join(".gogdl-resume");
                match permitted(&root, &path, options, &mounts) {
                    Ok(()) => app.status = InstallationStatus::Unknown,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        app.status = InstallationStatus::Unknown;
                        report
                            .issues
                            .push(ScanIssue::from_io(path, "read metadata", &error));
                    }
                }
            }
            report.applications.push(app);
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

fn info_paths(
    root: &Path,
    options: &ScanOptions,
    mounts: &MountPoints,
    control: &ScanControl,
    issues: &mut Vec<ScanIssue>,
) -> Result<Vec<PathBuf>, ScanError> {
    let mut dirs = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    let mut entries_seen = 0;
    while let Some(dir) = dirs.pop() {
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        if let Err(error) = permitted(root, &dir, options, mounts) {
            issues.push(ScanIssue::from_io(dir, "read metadata", &error));
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                issues.push(ScanIssue::from_io(dir, "read directory", &error));
                continue;
            }
        };
        for entry in entries {
            if control.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            entries_seen += 1;
            if entries_seen > MAX_ENTRIES {
                issues.push(ScanIssue::new(
                    root.into(),
                    crate::IssueKind::Other,
                    "GOG metadata discovery entry limit reached",
                ));
                return Ok(paths);
            }
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    issues.push(ScanIssue::from_io(
                        dir.clone(),
                        "read directory entry",
                        &error,
                    ));
                    continue;
                }
            };
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with("goggame-") && name.ends_with(".info") {
                paths.push(path);
            } else if dir == root {
                match metadata(&path) {
                    Ok(meta) if meta.kind == FileKind::Directory => dirs.push(path),
                    Ok(meta) if meta.kind == FileKind::Symlink => issues.push(ScanIssue::new(
                        path,
                        crate::IssueKind::Other,
                        "linked GOG discovery path skipped",
                    )),
                    Ok(_) => {}
                    Err(error) => issues.push(ScanIssue::from_io(path, "read metadata", &error)),
                }
            }
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn info(dir: &Path, id: &str, name: &str) -> u64 {
        fs::create_dir_all(dir).expect("game");
        let data = serde_json::json!({"gameId":id,"name":name,"buildId":"42"}).to_string();
        fs::write(dir.join(format!("goggame-{id}.info")), &data).expect("info");
        data.len() as u64
    }
    fn store(root: &Path, rows: serde_json::Value) {
        fs::create_dir_all(root.join("gog_store")).expect("store");
        fs::write(
            root.join("gog_store/installed.json"),
            serde_json::json!({"installed":rows}).to_string(),
        )
        .expect("installed");
    }
    #[test]
    fn direct_gog_library_and_game_roots_measure_shared_metadata_and_resume() {
        let temp = tempfile::tempdir().expect("temp");
        let game = temp.path().join("Game");
        let bytes = info(&game, "123", "Game") + info(&game, "124", "DLC");
        fs::write(game.join("data"), [0; 123]).expect("data");
        fs::hard_link(game.join("data"), game.join("alias")).expect("hardlink");
        info(
            &game.join("nested/deeper"),
            "999",
            "Must not discover recursively",
        );
        fs::write(temp.path().join("goggame-666.info"), "broken").expect("broken");
        let report = discover_gog(
            &[temp.path().into(), temp.path().into()],
            false,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 2);
        assert_eq!(report.issues.len(), 1);
        let expected = bytes
            + 123
            + fs::metadata(game.join("nested/deeper/goggame-999.info"))
                .expect("meta")
                .len();
        for app in &report.applications {
            assert_eq!(app.logical_bytes, Some(expected));
            assert_eq!(app.status, InstallationStatus::Installed);
            assert_eq!(app.launcher, "GOG");
        }
        assert_eq!(
            report.applications[0].allocated_bytes,
            report.applications[1].allocated_bytes
        );
        fs::write(game.join(".gogdl-resume"), "unfinished").expect("resume");
        let report = discover_gog(
            std::slice::from_ref(&game),
            false,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 2);
        assert!(
            report
                .applications
                .iter()
                .all(|app| app.status == InstallationStatus::Unknown)
        );
        let control = ScanControl::new();
        control.cancel();
        assert!(matches!(
            discover_gog(&[game], false, &ScanOptions::default(), &control),
            Err(ScanError::Cancelled)
        ));
    }
    #[test]
    fn heroic_gog_keeps_missing_records_and_enriches_names_without_catalog_access() {
        let temp = tempfile::tempdir().expect("temp");
        let config = temp.path().join("heroic");
        let game = temp.path().join("Readable folder");
        let info_bytes = info(&game, "123", "Actual GOG title");
        fs::write(game.join("data"), [0; 222]).expect("data");
        store(
            &config,
            serde_json::json!([
            {"appName":"123","install_path":game,"version":"v1","install_size":"999 GiB"},
            {"appName":"124","install_path":game},
            {"appName":"125","install_path":temp.path().join("Absent")},null]),
        );
        fs::write(config.join("gog_store/auth.json"), "never read").expect("auth");
        let report = discover_gog(
            &[config.clone(), config],
            true,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 3);
        assert_eq!(report.issues.len(), 2);
        let installed = report
            .applications
            .iter()
            .find(|app| app.id == "123")
            .expect("123");
        assert_eq!(installed.name, "Actual GOG title");
        assert_eq!(installed.build_id.as_deref(), Some("42"));
        assert_eq!(installed.logical_bytes, Some(222 + info_bytes));
        assert_eq!(installed.status, InstallationStatus::Installed);
        let fallback = report
            .applications
            .iter()
            .find(|app| app.id == "124")
            .expect("124");
        assert_eq!(fallback.name, "Readable folder");
        assert_eq!(fallback.allocated_bytes, installed.allocated_bytes);
        let missing = report
            .applications
            .iter()
            .find(|app| app.id == "125")
            .expect("125");
        assert_eq!(missing.status, InstallationStatus::Missing);
        assert_eq!(missing.allocated_bytes, None);
    }
    #[cfg(unix)]
    #[test]
    fn linked_gog_games_info_and_heroic_stores_are_rejected() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().expect("temp");
        let library = temp.path().join("Library");
        fs::create_dir_all(&library).expect("library");
        let game = temp.path().join("Real");
        info(&game, "123", "Linked game");
        symlink(&game, library.join("Link")).expect("link");
        symlink(
            game.join("goggame-123.info"),
            library.join("goggame-123.info"),
        )
        .expect("info link");
        let report = discover_gog(
            &[library],
            false,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert!(report.applications.is_empty());
        assert_eq!(report.issues.len(), 2);
        let config = temp.path().join("Config");
        store(
            &config,
            serde_json::json!([{ "appName":"123","install_path":temp.path().join("Link")} ]),
        );
        symlink(&game, temp.path().join("Link")).expect("install link");
        let report = discover_gog(
            std::slice::from_ref(&config),
            true,
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications[0].allocated_bytes, None);
        assert_eq!(report.applications[0].status, InstallationStatus::Unknown);
        let alias = temp.path().join("Alias");
        fs::create_dir_all(alias.join("gog_store")).expect("alias");
        symlink(
            config.join("gog_store/installed.json"),
            alias.join("gog_store/installed.json"),
        )
        .expect("store link");
        let report = discover_gog(&[alias], true, &ScanOptions::default(), &ScanControl::new())
            .expect("report");
        assert!(report.applications.is_empty());
        assert_eq!(report.issues.len(), 1);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn heroic_gog_mount_crossings_are_skipped() {
        let temp = tempfile::tempdir().expect("temp");
        store(
            temp.path(),
            serde_json::json!([{"appName":"123","install_path":"/proc"}]),
        );
        let report = discover_gog(
            &[temp.path().into()],
            true,
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
}
