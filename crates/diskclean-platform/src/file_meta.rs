//! Cross-platform file metadata.
//!
//! `allocated_size` is the space the file actually occupies on disk when the
//! platform exposes it. On Linux it comes from `st_blocks`; on Windows it is
//! read with `GetCompressedFileSizeW`, which accounts for compressed and sparse
//! files. If the platform call fails, the scanner falls back to the logical
//! size instead of dropping the file.

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
    fn from_std(_path: &Path, meta: &fs::Metadata) -> Self {
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
                allocated_size: if kind == FileKind::File {
                    windows_allocated_size(_path, meta.len())
                } else {
                    meta.len()
                },
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
    fs::symlink_metadata(path).map(|meta| FileMeta::from_std(path, &meta))
}

// The standard library does not expose Windows allocation size. Keep the
// unsafe surface to these two Win32 calls.
#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_allocated_size(path: &Path, logical_size: u64) -> u64 {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
    use windows_sys::Win32::Storage::FileSystem::GetCompressedFileSizeW;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let mut high = 0_u32;

    // SAFETY: `wide` is a NUL-terminated UTF-16 path that lives for the call,
    // and `high` points to a valid `u32`. The function only writes that value.
    let low = unsafe {
        SetLastError(0);
        GetCompressedFileSizeW(wide.as_ptr(), &mut high)
    };
    if low == u32::MAX {
        // `u32::MAX` is both a valid low word and the error sentinel.
        // SAFETY: reading the thread-local last-error code has no preconditions.
        let error = unsafe { GetLastError() };
        if error != 0 {
            return logical_size;
        }
    }

    (u64::from(high) << 32) | u64::from(low)
}

fn system_time_to_unix(value: io::Result<SystemTime>) -> i64 {
    value
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}
