//! Lutris metadata discovery and shared installation-folder measurement.
use super::*;

pub fn discover_lutris(
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
        let path = root.join("pga.db");
        let result =
            (|| -> std::io::Result<Option<diskclean_platform::lutris::LutrisInventory>> {
                permitted(&root, &path, options, &mounts)?;
                let meta = metadata(&path)?;
                if meta.ino != 0 && !visited.insert((meta.dev, meta.ino)) {
                    return Ok(None);
                }
                for suffix in ["-wal", "-journal"] {
                    let mut name = path.as_os_str().to_os_string();
                    name.push(suffix);
                    let sidecar = PathBuf::from(name);
                    match permitted(&root, &sidecar, options, &mounts) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error),
                    }
                }
                let worker = control.clone();
                diskclean_platform::lutris::read_database(&path, move || worker.is_cancelled())
                    .map(Some)
            })();
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let records = match result {
            Ok(Some(records)) => records,
            Ok(None) => continue,
            Err(error) => {
                report
                    .issues
                    .push(ScanIssue::from_io(path, "read Lutris database", &error));
                continue;
            }
        };
        if records.invalid_rows > 0 {
            report.issues.push(ScanIssue::new(
                path.clone(),
                crate::IssueKind::Other,
                format!(
                    "invalid Lutris installed-game records: {}",
                    records.invalid_rows
                ),
            ));
        }
        for game in records.games {
            if control.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let mut app = Application {
                id: game.id.to_string(),
                name: game.name,
                launcher: "Lutris".into(),
                status: InstallationStatus::Unknown,
                install_path: game.directory,
                manifest_path: path.clone(),
                allocated_bytes: None,
                logical_bytes: None,
                size_complete: false,
                build_id: None,
                last_played_unix: game.last_played,
            };
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    fn fixture(root: &Path, directory: &Path) {
        fs::create_dir_all(root).expect("root");
        let db = rusqlite::Connection::open(root.join("pga.db")).expect("db");
        db.execute_batch("CREATE TABLE games(id INTEGER PRIMARY KEY,name TEXT,directory TEXT,lastplayed INTEGER,installed INTEGER);").expect("schema");
        for (id, name, path) in [
            (1, "A Game", directory.to_path_buf()),
            (2, "B Shared", directory.to_path_buf()),
            (3, "C Missing", directory.join("absent")),
        ] {
            db.execute(
                "INSERT INTO games VALUES(?1,?2,?3,123,1)",
                rusqlite::params![id, name, path.to_string_lossy()],
            )
            .expect("row");
        }
        db.execute_batch("INSERT INTO games VALUES(4,'Catalog','/not-installed',0,0);INSERT INTO games VALUES(5,'Bad','relative',0,1);").expect("rows");
    }
    #[test]
    fn lutris_shared_sizes_installed_missing_metadata_and_cancellation() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("Lutris");
        let game = temp.path().join("Game");
        fs::create_dir_all(&game).expect("game");
        fs::write(game.join("data"), [0; 234]).expect("data");
        fs::hard_link(game.join("data"), game.join("alias")).expect("hardlink");
        fixture(&root, &game);
        let before = fs::read(root.join("pga.db")).expect("before");
        let report = discover_lutris(
            &[root.clone(), root.clone()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 3);
        assert_eq!(report.issues.len(), 2);
        assert_eq!(report.applications[0].logical_bytes, Some(234));
        assert_eq!(report.applications[0].status, InstallationStatus::Installed);
        assert_eq!(report.applications[0].last_played_unix, Some(123));
        assert_eq!(report.applications[0].launcher, "Lutris");
        assert_eq!(
            report.applications[0].allocated_bytes,
            report.applications[1].allocated_bytes
        );
        assert_eq!(report.applications[2].status, InstallationStatus::Missing);
        assert_eq!(report.applications[2].allocated_bytes, None);
        assert_eq!(fs::read(root.join("pga.db")).expect("after"), before);
        let partial = discover_lutris(
            std::slice::from_ref(&root),
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
            discover_lutris(&[root], &ScanOptions::default(), &control),
            Err(ScanError::Cancelled)
        ));
    }
    #[test]
    fn lutris_rejects_linked_installs_databases_and_sidecars_and_mounts() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("Lutris");
        let game = temp.path().join("Real");
        fs::create_dir_all(&game).expect("real");
        let linked = temp.path().join("Linked");
        symlink(&game, &linked).expect("link");
        fixture(&root, &linked);
        let report = discover_lutris(
            std::slice::from_ref(&root),
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
        let alias = temp.path().join("Alias");
        fs::create_dir_all(&alias).expect("alias");
        symlink(root.join("pga.db"), alias.join("pga.db")).expect("db link");
        let report = discover_lutris(&[alias], &ScanOptions::default(), &ScanControl::new())
            .expect("report");
        assert!(report.applications.is_empty());
        assert_eq!(report.issues.len(), 1);
        symlink(root.join("pga.db"), root.join("pga.db-wal")).expect("wal link");
        let report =
            discover_lutris(&[root], &ScanOptions::default(), &ScanControl::new()).expect("report");
        assert!(report.applications.is_empty());
        let root = temp.path().join("Mount");
        fixture(&root, Path::new("/proc"));
        let report =
            discover_lutris(&[root], &ScanOptions::default(), &ScanControl::new()).expect("report");
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.message.contains("filesystem boundary"))
        );
    }
}
