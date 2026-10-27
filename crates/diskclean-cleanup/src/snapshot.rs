//! Immutable evidence captured when an item enters the review basket.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use diskclean_platform::{FileKind, metadata};
use diskclean_rules::{Category, Risk};
use serde::{Serialize, Serializer};

const MAX_MEASURE_ENTRIES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    File,
    Directory,
}

/// Metadata needed to detect changes immediately before a destructive action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileSnapshot {
    pub kind: ItemKind,
    pub measured_size: u64,
    pub logical_size: u64,
    pub modified_unix: i64,
    pub dev: u64,
    pub ino: u64,
    pub direct_children: Option<u64>,
}

impl FileSnapshot {
    /// Capture metadata without following symlinks. Directories are measured
    /// recursively without crossing mount points.
    pub fn capture(path: &Path) -> io::Result<Self> {
        let meta = metadata(path)?;
        let kind = match meta.kind {
            FileKind::File => ItemKind::File,
            FileKind::Directory => ItemKind::Directory,
            FileKind::Symlink => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "refusing to snapshot a symlink or reparse point",
                ));
            }
            FileKind::Other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "refusing to snapshot a special file",
                ));
            }
        };

        let (measured_size, direct_children) = match kind {
            ItemKind::File => (meta.allocated_size, None),
            ItemKind::Directory => {
                let (size, _) = measure_directory(path, meta.dev, MAX_MEASURE_ENTRIES);
                (size, Some(count_children(path)?))
            }
        };

        Ok(Self {
            kind,
            measured_size,
            logical_size: meta.logical_size,
            modified_unix: meta.modified_unix,
            dev: meta.dev,
            ino: meta.ino,
            direct_children,
        })
    }
}

fn count_children(path: &Path) -> io::Result<u64> {
    let mut count = 0_u64;
    for entry in fs::read_dir(path)? {
        let _ = entry?;
        count = count.saturating_add(1);
    }
    Ok(count)
}

fn measure_directory(path: &Path, root_dev: u64, max_entries: usize) -> (u64, bool) {
    let mut total = 0_u64;
    let mut visited = 0_usize;
    let mut stack = vec![path.to_path_buf()];

    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            visited = visited.saturating_add(1);
            if visited > max_entries {
                return (total, true);
            }
            let Ok(meta) = metadata(&entry.path()) else {
                continue;
            };
            if meta.dev != root_dev && root_dev != 0 {
                continue;
            }
            match meta.kind {
                FileKind::File => {
                    total = total.saturating_add(meta.allocated_size);
                }
                FileKind::Directory => stack.push(entry.path()),
                FileKind::Symlink | FileKind::Other => {}
            }
        }
    }

    (total, false)
}

/// A reviewed item. Creating one captures the metadata that must still match
/// when the user confirms the cleanup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CleanupItem {
    pub rule_id: String,
    pub category: Category,
    pub risk: Risk,
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    pub snapshot: FileSnapshot,
}

impl CleanupItem {
    /// Capture an item with current filesystem metadata.
    pub fn new(
        path: impl Into<PathBuf>,
        rule_id: impl Into<String>,
        category: Category,
        risk: Risk,
    ) -> io::Result<Self> {
        if risk == Risk::SystemOnly {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "system-only rules can never enter the trash review basket",
            ));
        }
        let path = path.into();
        let snapshot = FileSnapshot::capture(&path)?;
        Ok(Self {
            rule_id: rule_id.into(),
            category,
            risk,
            path,
            snapshot,
        })
    }

    #[must_use]
    pub fn size(&self) -> u64 {
        self.snapshot.measured_size
    }

    #[must_use]
    pub fn kind(&self) -> ItemKind {
        self.snapshot.kind
    }
}

/// Convenience wrapper around [`CleanupItem::new`].
pub fn snapshot(
    path: impl Into<PathBuf>,
    rule_id: impl Into<String>,
    category: Category,
    risk: Risk,
) -> io::Result<CleanupItem> {
    CleanupItem::new(path, rule_id, category, risk)
}

pub(crate) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

pub(crate) fn serialize_path<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn snapshots_files_and_directories() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = temp.path().join("file.bin");
        fs::write(&file, b"hello").expect("write");
        let file_item =
            CleanupItem::new(&file, "test", Category::Temporary, Risk::Low).expect("file snapshot");
        assert_eq!(file_item.kind(), ItemKind::File);
        assert_eq!(file_item.snapshot.logical_size, 5);
        assert!(file_item.snapshot.direct_children.is_none());

        let directory = temp.path().join("dir");
        fs::create_dir(&directory).expect("mkdir");
        let nested = directory.join("nested");
        fs::create_dir(&nested).expect("nested");
        let mut handle = fs::File::create(nested.join("a")).expect("create");
        handle.write_all(&[0_u8; 32]).expect("write");
        let dir_item = CleanupItem::new(&directory, "test", Category::Temporary, Risk::Low)
            .expect("dir snapshot");
        assert_eq!(dir_item.kind(), ItemKind::Directory);
        assert_eq!(dir_item.snapshot.direct_children, Some(1));
    }

    #[test]
    fn refuses_symlinks() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("tempdir");
            let target = temp.path().join("target");
            let link = temp.path().join("link");
            fs::write(&target, b"data").expect("write");
            symlink(&target, &link).expect("symlink");
            let error = CleanupItem::new(&link, "test", Category::Temporary, Risk::Low)
                .expect_err("symlink must be refused");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn system_only_items_cannot_be_created() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = temp.path().join("file");
        fs::write(&file, b"x").expect("write");
        let error = CleanupItem::new(&file, "system", Category::System, Risk::SystemOnly)
            .expect_err("must refuse system-only item");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
