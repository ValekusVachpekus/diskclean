//! Cross-platform file metadata.
//!
//! `allocated_size` is the space the file actually occupies on disk when the
//! platform exposes it. On Linux it comes from `st_blocks`; on Windows it is
//! currently the logical size until the Win32 allocation-size wrapper lands.

use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// The filesystem object kind as reported by `lstat`-style metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Directory,
    File,
    Symlink,
    Other,
}

/// A compact, owned snapshot of the metadata the scanner needs.
#[derive(Debug, Clone, Copy)]
pub struct FileMeta {
    pub kind: FileKind,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub modified_unix: i64,
    pub accessed_unix: i64,
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    pub is_hidden: bool,
}

impl FileMeta {
    fn from_std(meta: &fs::Metadata) -> Self {
        let kind = if meta.file_type().is_symlink() {
            FileKind::Symlink
        } else if meta.is_dir() {
            FileKind::Directory
        } else if meta.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        };

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            Self {
                kind,
                logical_size: meta.len(),
                allocated_size: meta.blocks().saturating_mul(512),
                modified_unix: system_time_to_unix(meta.modified()),
                accessed_unix: system_time_to_unix(meta.accessed()),
                dev: meta.dev(),
                ino: meta.ino(),
                nlink: meta.nlink(),
                is_hidden: false,
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;

            const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

            Self {
                kind,
                logical_size: meta.len(),
                // TODO(windows): use FILE_STANDARD_INFO.AllocationSize or
                // GetCompressedFileSizeW for sparse/compressed files.
                allocated_size: meta.len(),
                modified_unix: system_time_to_unix(meta.modified()),
                accessed_unix: system_time_to_unix(meta.accessed()),
                dev: 0,
                ino: 0,
                nlink: 1,
                is_hidden: meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0,
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            Self {
                kind,
                logical_size: meta.len(),
                allocated_size: meta.len(),
                modified_unix: system_time_to_unix(meta.modified()),
                accessed_unix: system_time_to_unix(meta.accessed()),
                dev: 0,
                ino: 0,
                nlink: 1,
                is_hidden: false,
            }
        }
    }
}

/// Read metadata without following symlinks.
pub fn metadata(path: &Path) -> io::Result<FileMeta> {
    fs::symlink_metadata(path).map(|meta| FileMeta::from_std(&meta))
}

fn system_time_to_unix(value: io::Result<SystemTime>) -> i64 {
    value
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}
