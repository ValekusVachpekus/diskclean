//! Thread-safe scan progress and cancellation.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::error::serialize_optional_path;

/// A cloneable handle used by the scanner to publish progress and by the UI/CLI
/// to poll it and request cancellation.
#[derive(Debug, Clone, Default)]
pub struct ScanControl {
    inner: Arc<ProgressState>,
}

#[derive(Debug, Default)]
struct ProgressState {
    cancelled: AtomicBool,
    directories: AtomicU64,
    files: AtomicU64,
    symlinks: AtomicU64,
    other: AtomicU64,
    logical_bytes: AtomicU64,
    allocated_bytes: AtomicU64,
    errors: AtomicU64,
    current: Mutex<Option<PathBuf>>,
}

/// A point-in-time snapshot of scan progress.
#[derive(Debug, Clone, Serialize)]
pub struct ProgressSnapshot {
    pub directories: u64,
    pub files: u64,
    pub symlinks: u64,
    pub other: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub errors: u64,
    #[serde(serialize_with = "serialize_optional_path")]
    pub current_path: Option<PathBuf>,
    pub cancelled: bool,
}

impl ScanControl {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn snapshot(&self) -> ProgressSnapshot {
        let current_path = self
            .inner
            .current
            .lock()
            .map(|path| path.clone())
            .unwrap_or_default();

        ProgressSnapshot {
            directories: self.inner.directories.load(Ordering::Relaxed),
            files: self.inner.files.load(Ordering::Relaxed),
            symlinks: self.inner.symlinks.load(Ordering::Relaxed),
            other: self.inner.other.load(Ordering::Relaxed),
            logical_bytes: self.inner.logical_bytes.load(Ordering::Relaxed),
            allocated_bytes: self.inner.allocated_bytes.load(Ordering::Relaxed),
            errors: self.inner.errors.load(Ordering::Relaxed),
            current_path,
            cancelled: self.is_cancelled(),
        }
    }

    pub(crate) fn record_directory(&self) {
        self.inner.directories.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_file(&self, logical: u64, allocated: u64) {
        self.inner.files.fetch_add(1, Ordering::Relaxed);
        self.inner
            .logical_bytes
            .fetch_add(logical, Ordering::Relaxed);
        self.inner
            .allocated_bytes
            .fetch_add(allocated, Ordering::Relaxed);
    }

    pub(crate) fn record_symlink(&self) {
        self.inner.symlinks.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_other(&self) {
        self.inner.other.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_error(&self) {
        self.inner.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn set_current(&self, path: &std::path::Path) {
        if let Ok(mut current) = self.inner.current.lock() {
            *current = Some(path.to_path_buf());
        }
    }
}
