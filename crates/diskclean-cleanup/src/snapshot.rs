//! Immutable evidence captured when an item enters the review basket.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use diskclean_platform::{FileKind, FileMeta, MountPoints, check_path_components, metadata};
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
    pub modified: SystemTime,
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    pub direct_children: Option<u64>,
    /// Sorted relative paths and metadata of every descendant. Never partial.
    pub descendants: Vec<DescendantSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DescendantSnapshot {
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    pub kind: ItemKind,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub modified: SystemTime,
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
}

impl DescendantSnapshot {
    fn from_meta(path: PathBuf, meta: FileMeta) -> io::Result<Self> {
        let kind = match meta.kind {
            FileKind::File => ItemKind::File,
            FileKind::Directory => ItemKind::Directory,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "symlink, reparse point or special file in cleanup candidate",
                ));
            }
        };
        if meta.dev == 0 || meta.ino == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "file identity unavailable; refusing cleanup",
            ));
        }
        Ok(Self {
            path,
            kind,
            logical_size: meta.logical_size,
            allocated_size: meta.allocated_size,
            modified: meta
                .modified
                .ok_or_else(|| io::Error::other("modification time unavailable"))?,
            dev: meta.dev,
            ino: meta.ino,
            nlink: meta.nlink,
        })
    }
}

impl FileSnapshot {
    /// Capture metadata without following symlinks. Directories are measured
    /// recursively without crossing mount points.
    pub fn capture(path: &Path) -> io::Result<Self> {
        Self::capture_with_cancel(path, &|| false)
    }

    pub(crate) fn capture_with_cancel(
        path: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<Self> {
        check_path_components(path)?;
        let mounts = MountPoints::read()?;
        if mounts.contains(path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cleanup candidate is a mount point",
            ));
        }
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

        let stamp = DescendantSnapshot::from_meta(PathBuf::new(), meta)?;
        let (measured_size, direct_children, descendants) = match kind {
            ItemKind::File => (meta.allocated_size, None, Vec::new()),
            ItemKind::Directory => {
                let descendants =
                    capture_directory(path, meta.dev, MAX_MEASURE_ENTRIES, &mounts, cancelled)?;
                let children = descendants
                    .iter()
                    .filter(|entry| entry.path.components().count() == 1)
                    .count() as u64;
                let mut seen = HashSet::new();
                let size = descendants
                    .iter()
                    .filter(|entry| entry.kind == ItemKind::File)
                    .filter(|entry| seen.insert((entry.dev, entry.ino)))
                    .fold(0_u64, |size, entry| {
                        size.saturating_add(entry.allocated_size)
                    });
                // Reject changes to the root while walking it as well.
                if stamp != DescendantSnapshot::from_meta(PathBuf::new(), metadata(path)?)? {
                    return Err(io::Error::other("directory changed while taking snapshot"));
                }
                (size, Some(children), descendants)
            }
        };

        Ok(Self {
            kind,
            measured_size,
            logical_size: meta.logical_size,
            modified_unix: meta.modified_unix,
            modified: stamp.modified,
            dev: meta.dev,
            ino: meta.ino,
            nlink: meta.nlink,
            direct_children,
            descendants,
        })
    }
}

fn capture_directory(
    path: &Path,
    root_dev: u64,
    max_entries: usize,
    mounts: &MountPoints,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<Vec<DescendantSnapshot>> {
    let mut result = Vec::new();
    let mut stack = vec![path.to_path_buf()];

    while let Some(directory) = stack.pop() {
        check_path_components(&directory)?;
        let entries = fs::read_dir(&directory)?;
        for entry in entries {
            if cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "cleanup cancelled",
                ));
            }
            let entry = entry?;
            if result.len() >= max_entries {
                return Err(io::Error::other(
                    "directory snapshot limit reached; refusing partial review",
                ));
            }
            let entry_path = entry.path();
            let meta = metadata(&entry_path)?;
            if meta.dev != root_dev || mounts.contains(&entry_path) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "mount boundary inside cleanup candidate",
                ));
            }
            let relative = entry_path
                .strip_prefix(path)
                .map_err(io::Error::other)?
                .to_path_buf();
            let snapshot = DescendantSnapshot::from_meta(relative, meta)?;
            if snapshot.kind == ItemKind::Directory {
                stack.push(entry_path);
            }
            result.push(snapshot);
        }
    }

    result.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(result)
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

    #[cfg(unix)]
    #[test]
    fn rejects_linked_ancestors_and_descendants() {
        let temp = tempfile::tempdir().expect("temp");
        let dir = temp.path().join("dir");
        fs::create_dir(&dir).expect("dir");
        fs::write(dir.join("key"), b"secret").expect("key");
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&dir, &alias).expect("link");
        assert!(FileSnapshot::capture(&alias.join("key")).is_err());
        std::os::unix::fs::symlink(dir.join("key"), dir.join("link")).expect("link");
        assert!(FileSnapshot::capture(&dir).is_err());
    }

    #[test]
    fn refuses_partial_directory_snapshots() {
        let temp = tempfile::tempdir().expect("temp");
        fs::write(temp.path().join("a"), b"a").expect("a");
        fs::write(temp.path().join("b"), b"b").expect("b");
        let dev = metadata(temp.path()).expect("meta").dev;
        let mounts = MountPoints::read().expect("mounts");
        assert!(capture_directory(temp.path(), dev, 1, &mounts, &|| false).is_err());
        assert!(capture_directory(temp.path(), dev, 10, &mounts, &|| true).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn refuses_mount_roots() {
        assert!(FileSnapshot::capture(Path::new("/proc")).is_err());
    }

    #[test]
    fn directory_size_counts_hardlinks_once() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("a");
        fs::write(&path, b"payload").expect("a");
        fs::hard_link(&path, temp.path().join("b")).expect("link");
        let snapshot = FileSnapshot::capture(temp.path()).expect("snapshot");
        assert_eq!(
            snapshot.measured_size,
            metadata(&path).expect("meta").allocated_size
        );
        assert_eq!(snapshot.descendants.len(), 2);
    }
}
