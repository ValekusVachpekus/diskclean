//! Read-only Lutris metadata. SQLite sees only a private, bounded snapshot.
use std::path::PathBuf;

#[derive(Debug)]
pub struct LutrisGame {
    pub id: i64,
    pub name: String,
    pub directory: PathBuf,
    pub last_played: Option<u64>,
}
#[derive(Debug, Default)]
pub struct LutrisInventory {
    pub games: Vec<LutrisGame>,
    pub invalid_rows: usize,
}

pub fn default_roots() -> Vec<PathBuf> {
    #[cfg(all(target_os = "linux", feature = "lutris"))]
    {
        let mut roots = Vec::new();
        if let Some(home) = crate::home_dir() {
            let data = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(".local/share"));
            roots.push(data.join("lutris"));
            roots.push(home.join(".var/app/net.lutris.Lutris/data/lutris"));
        }
        roots
    }
    #[cfg(not(all(target_os = "linux", feature = "lutris")))]
    Vec::new()
}

#[cfg(not(all(target_os = "linux", feature = "lutris")))]
pub fn read_database(
    _path: &std::path::Path,
    _cancelled: impl Fn() -> bool + Send + 'static,
) -> std::io::Result<LutrisInventory> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Lutris inventory requires Linux and the lutris feature",
    ))
}

#[cfg(all(target_os = "linux", feature = "lutris"))]
pub use sqlite::read_database;

#[cfg(all(target_os = "linux", feature = "lutris"))]
mod sqlite {
    use super::*;
    use rusqlite::{Connection, OpenFlags, limits::Limit};
    use std::fs;
    use std::io::{self, Read, Write};
    use std::path::{Component, Path};
    const MAX_FILE: u64 = 64 * 1024 * 1024;
    const MAX_ROWS: usize = 20_000;
    fn error(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message)
    }
    fn sql_error(error: rusqlite::Error) -> io::Error {
        io::Error::other(format!("Lutris SQLite: {error}"))
    }
    fn stamp(path: &Path) -> io::Result<Option<crate::FileMeta>> {
        match crate::metadata(path) {
            Ok(meta) => {
                crate::check_path_components(path)?;
                Ok(Some(meta))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
    fn unchanged(a: Option<crate::FileMeta>, b: Option<crate::FileMeta>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.dev == b.dev
                    && a.ino == b.ino
                    && a.logical_size == b.logical_size
                    && a.modified.is_some()
                    && a.modified == b.modified
            }
            _ => false,
        }
    }
    fn copy_snapshot(path: &Path, cancelled: &impl Fn() -> bool) -> io::Result<tempfile::TempDir> {
        let temp = tempfile::tempdir()?;
        let mut snapshots = Vec::new();
        for suffix in ["", "-wal", "-journal"] {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            let source = PathBuf::from(name);
            let before = stamp(&source)?;
            if suffix.is_empty() && before.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Lutris database missing",
                ));
            }
            if let Some(meta) = before {
                if meta.kind != crate::FileKind::File || meta.logical_size > MAX_FILE {
                    return Err(error(
                        "Lutris snapshot file is not regular or exceeds 64 MiB",
                    ));
                }
                if suffix == "-journal" && meta.logical_size > 0 {
                    return Err(error(
                        "Lutris rollback journal present; retry after the writer finishes",
                    ));
                }
            }
            snapshots.push((source, before));
        }
        for (index, (source, before)) in snapshots.iter().enumerate().take(2) {
            let Some(meta) = before else {
                continue;
            };
            let mut input = crate::open_regular_file(source)?;
            if crate::opened_identity(&input)? != (meta.dev, meta.ino) {
                return Err(error("Lutris database changed during snapshot"));
            }
            let mut output = fs::File::create(temp.path().join(if index == 0 {
                "pga.db"
            } else {
                "pga.db-wal"
            }))?;
            let mut copied = 0;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                if cancelled() {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Lutris discovery cancelled",
                    ));
                }
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                copied += count as u64;
                if copied > MAX_FILE {
                    return Err(error("Lutris snapshot exceeds 64 MiB"));
                }
                output.write_all(&buffer[..count])?;
            }
            if copied != meta.logical_size {
                return Err(error("Lutris database changed during snapshot"));
            }
        }
        for (source, before) in snapshots {
            if !unchanged(before, stamp(&source)?) {
                return Err(error("Lutris database changed during snapshot; retry"));
            }
        }
        Ok(temp)
    }
    pub fn read_database(
        path: &Path,
        cancelled: impl Fn() -> bool + Send + 'static,
    ) -> io::Result<LutrisInventory> {
        if cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Lutris discovery cancelled",
            ));
        }
        let snapshot = copy_snapshot(path, &cancelled)?;
        // WAL recovery/shared-memory files may be created only inside this private
        // temp directory. The source database is never opened by SQLite.
        let mut db = Connection::open_with_flags(
            snapshot.path().join("pga.db"),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sql_error)?;
        let mut ticks = 0;
        db.progress_handler(
            1000,
            Some(move || {
                ticks += 1;
                ticks > 10_000 || cancelled()
            }),
        )
        .map_err(sql_error)?;
        db.set_limit(Limit::SQLITE_LIMIT_LENGTH, 1024 * 1024)
            .map_err(sql_error)?;
        db.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 16 * 1024)
            .map_err(sql_error)?;
        db.set_limit(Limit::SQLITE_LIMIT_COLUMN, 128)
            .map_err(sql_error)?;
        db.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)
            .map_err(sql_error)?;
        db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-4096;").map_err(sql_error)?;
        let table: String = db
            .query_row(
                "SELECT type FROM sqlite_schema WHERE name='games'",
                [],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if table != "table" {
            return Err(error("Lutris games must be a real table"));
        }
        let mut statement=db.prepare("SELECT id,name,directory,lastplayed FROM games WHERE installed=1 ORDER BY id LIMIT 20001").map_err(sql_error)?;
        let mut rows = statement.query([]).map_err(sql_error)?;
        let mut result = LutrisInventory::default();
        let mut seen = std::collections::HashSet::new();
        let mut count = 0;
        while let Some(row) = rows.next().map_err(sql_error)? {
            count += 1;
            if count > MAX_ROWS {
                return Err(error("Lutris installed-record limit exceeded"));
            }
            let parsed = (|| -> rusqlite::Result<LutrisGame> {
                Ok(LutrisGame {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    directory: PathBuf::from(row.get::<_, String>(2)?),
                    last_played: row
                        .get::<_, Option<i64>>(3)?
                        .and_then(|value| u64::try_from(value).ok())
                        .filter(|value| *value > 0),
                })
            })();
            match parsed {
                Ok(game)
                    if game.id > 0
                        && !game.name.trim().is_empty()
                        && game.directory.is_absolute()
                        && game.directory.parent().is_some()
                        && !game
                            .directory
                            .components()
                            .any(|part| part == Component::ParentDir)
                        && !game.directory.to_string_lossy().contains(['\\', ':'])
                        && seen.insert(game.id) =>
                {
                    result.games.push(game)
                }
                _ => result.invalid_rows += 1,
            }
        }
        Ok(result)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        fn fixture(path: &Path) -> Connection {
            let db = Connection::open(path).expect("db");
            db.execute_batch("CREATE TABLE games(id INTEGER PRIMARY KEY,name TEXT,directory TEXT,lastplayed INTEGER,installed INTEGER);").expect("schema");
            db
        }
        #[test]
        fn snapshot_reads_installed_rows_and_never_modifies_source() {
            let temp = tempfile::tempdir().expect("temp");
            let path = temp.path().join("pga.db");
            let db = fixture(&path);
            db.execute(
                "INSERT INTO games VALUES(1,'Game',?1,123,1)",
                [temp.path().join("Game").to_string_lossy().as_ref()],
            )
            .expect("row");
            db.execute_batch("INSERT INTO games VALUES(2,'Catalog','/not-installed',0,0); INSERT INTO games VALUES(3,'Bad','relative',0,1);").expect("rows");
            drop(db);
            let before = fs::read(&path).expect("before");
            let stamp_before = stamp(&path).expect("meta");
            let result = read_database(&path, || false).expect("read");
            assert_eq!(result.games.len(), 1);
            assert_eq!(result.games[0].last_played, Some(123));
            assert_eq!(result.invalid_rows, 1);
            assert_eq!(fs::read(&path).expect("after"), before);
            assert!(unchanged(stamp_before, stamp(&path).expect("meta")));
            assert_eq!(fs::read_dir(temp.path()).expect("dir").count(), 1);
        }
        #[test]
        fn wal_snapshot_includes_committed_rows_without_touching_source_sidecars() {
            let temp = tempfile::tempdir().expect("temp");
            let path = temp.path().join("pga.db");
            let db = fixture(&path);
            db.execute_batch("PRAGMA journal_mode=WAL;").expect("wal");
            db.execute(
                "INSERT INTO games VALUES(1,'WAL game',?1,0,1)",
                [temp.path().join("Game").to_string_lossy().as_ref()],
            )
            .expect("row");
            let before: Vec<_> = ["pga.db", "pga.db-wal", "pga.db-shm"]
                .into_iter()
                .map(|name| fs::read(temp.path().join(name)).expect("read"))
                .collect();
            assert_eq!(read_database(&path, || false).expect("read").games.len(), 1);
            for (name, bytes) in ["pga.db", "pga.db-wal", "pga.db-shm"]
                .into_iter()
                .zip(before)
            {
                assert_eq!(fs::read(temp.path().join(name)).expect("after"), bytes);
            }
        }
        #[test]
        fn snapshot_rejects_concurrent_changes_and_non_table_schemas() {
            let temp = tempfile::tempdir().expect("temp");
            let path = temp.path().join("pga.db");
            drop(fixture(&path));
            let changed_path = path.clone();
            let calls = std::sync::atomic::AtomicUsize::new(0);
            let error = read_database(&path, move || {
                if calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 1 {
                    fs::write(&changed_path, "changed by concurrent writer")
                        .expect("mutate fixture");
                }
                false
            })
            .expect_err("changed");
            assert!(error.to_string().contains("changed"));
            let path = temp.path().join("view.db");
            let db = Connection::open(&path).expect("db");
            db.execute_batch("CREATE VIEW games AS SELECT 1 AS id,'Game' AS name,'/tmp/game' AS directory,0 AS lastplayed,1 AS installed;").expect("view");
            drop(db);
            assert!(
                read_database(&path, || false)
                    .expect_err("view")
                    .to_string()
                    .contains("real table")
            );
        }

        #[test]
        fn links_journals_corruption_cancellation_and_bounds_are_rejected() {
            use std::os::unix::fs::symlink;
            let temp = tempfile::tempdir().expect("temp");
            let path = temp.path().join("pga.db");
            drop(fixture(&path));
            assert_eq!(
                read_database(&path, || true).expect_err("cancel").kind(),
                io::ErrorKind::Interrupted
            );
            symlink(&path, temp.path().join("alias.db")).expect("link");
            assert!(read_database(&temp.path().join("alias.db"), || false).is_err());
            fs::write(temp.path().join("pga.db-journal"), "active").expect("journal");
            assert!(read_database(&path, || false).is_err());
            let large = temp.path().join("large.db");
            fs::File::create(&large)
                .expect("file")
                .set_len(MAX_FILE + 1)
                .expect("sparse");
            assert!(read_database(&large, || false).is_err());
            let bad = temp.path().join("bad.db");
            fs::write(&bad, "not sqlite").expect("bad");
            assert!(read_database(&bad, || false).is_err());
        }
    }
}
