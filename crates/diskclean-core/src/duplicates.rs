//! Read-only exact-duplicate discovery.
//!
//! The engine uses the classic three-stage pipeline:
//!
//! 1. group regular files by logical size;
//! 2. hash a bounded prefix of each candidate;
//! 3. hash the whole file with BLAKE3 only for prefix matches.
//!
//! Symlinks and special files are never followed. Hardlink aliases are skipped
//! because removing one link does not free space while another link remains.
//! All operations are read-only and can be cancelled between any two reads.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use blake3::Hasher;
use diskclean_platform::{
    FileKind, FileMeta, MountPoints, check_path_components, metadata, open_regular_file,
    opened_identity,
};
use serde::Serialize;
use thiserror::Error;

use crate::error::{serialize_optional_path, serialize_path};

/// Read size used for prefix and full-content hashing.
const READ_BUFFER: usize = 64 * 1024;

/// Options for [`find_duplicates`].
#[derive(Debug, Clone)]
pub struct DuplicateOptions {
    /// Ignore files smaller than this many logical bytes.
    pub min_size: u64,
    /// Number of leading bytes hashed before a full-file hash.
    pub prefix_bytes: usize,
    /// Skip dotfiles and platform-hidden entries.
    pub skip_hidden: bool,
    /// Cross filesystem/mount boundaries while collecting candidates.
    pub cross_filesystems: bool,
    /// Safety limit for the number of candidate files held in memory.
    pub max_files: usize,
}

impl Default for DuplicateOptions {
    fn default() -> Self {
        Self {
            min_size: 1,
            prefix_bytes: 16 * 1024,
            skip_hidden: false,
            cross_filesystems: false,
            max_files: 1_000_000,
        }
    }
}

/// Phase reported by [`DuplicateControl::snapshot`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicatePhase {
    #[default]
    Collecting,
    Hashing,
    Done,
}

/// Thread-safe progress and cancellation for duplicate discovery.
#[derive(Debug, Clone, Default)]
pub struct DuplicateControl {
    inner: Arc<DuplicateState>,
}

#[derive(Debug, Default)]
struct DuplicateState {
    cancelled: AtomicBool,
    files_scanned: AtomicU64,
    files_hashed: AtomicU64,
    bytes_hashed: AtomicU64,
    groups_found: AtomicU64,
    errors: AtomicU64,
    current: Mutex<Option<PathBuf>>,
    phase: Mutex<DuplicatePhase>,
}

impl DuplicateControl {
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
    pub fn snapshot(&self) -> DuplicateProgress {
        let current_path = self
            .inner
            .current
            .lock()
            .map(|path| path.clone())
            .unwrap_or_default();
        let phase = self
            .inner
            .phase
            .lock()
            .map_or(DuplicatePhase::Collecting, |phase| *phase);

        DuplicateProgress {
            phase,
            files_scanned: self.inner.files_scanned.load(Ordering::Relaxed),
            files_hashed: self.inner.files_hashed.load(Ordering::Relaxed),
            bytes_hashed: self.inner.bytes_hashed.load(Ordering::Relaxed),
            groups_found: self.inner.groups_found.load(Ordering::Relaxed),
            errors: self.inner.errors.load(Ordering::Relaxed),
            current_path,
            cancelled: self.is_cancelled(),
        }
    }

    pub(crate) fn set_phase(&self, phase: DuplicatePhase) {
        if let Ok(mut current) = self.inner.phase.lock() {
            *current = phase;
        }
    }

    pub(crate) fn set_current(&self, path: &Path) {
        if let Ok(mut current) = self.inner.current.lock() {
            *current = Some(path.to_path_buf());
        }
    }

    pub(crate) fn record_scanned(&self) {
        self.inner.files_scanned.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_hashed(&self) {
        self.inner.files_hashed.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bytes(&self, bytes: u64) {
        self.inner.bytes_hashed.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn record_group(&self) {
        self.inner.groups_found.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_error(&self) {
        self.inner.errors.fetch_add(1, Ordering::Relaxed);
    }
}

/// A point-in-time snapshot of duplicate discovery.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateProgress {
    pub phase: DuplicatePhase,
    pub files_scanned: u64,
    pub files_hashed: u64,
    pub bytes_hashed: u64,
    pub groups_found: u64,
    pub errors: u64,
    #[serde(serialize_with = "serialize_optional_path")]
    pub current_path: Option<PathBuf>,
    pub cancelled: bool,
}

/// Fatal duplicate-scan failures.
#[derive(Debug, Error)]
pub enum DuplicateError {
    #[error("cannot access root '{path}': {source}")]
    Root {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("root path is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("duplicate search cancelled by user")]
    Cancelled,
}

/// A non-fatal problem encountered while collecting or hashing.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateIssue {
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    pub message: String,
}

/// One regular file in a duplicate group.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateFile {
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub modified_unix: i64,
    pub modified: Option<SystemTime>,
    pub nlink: u64,
    pub dev: u64,
    pub ino: u64,
}

impl DuplicateFile {
    #[must_use]
    pub fn size(&self) -> u64 {
        self.allocated_size
    }
}

/// A set of two or more regular files with identical contents.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateGroup {
    /// BLAKE3 digest of the complete file contents, in lowercase hex.
    pub fingerprint: String,
    pub logical_size: u64,
    pub files: Vec<DuplicateFile>,
    /// Space that would be freed by keeping the first file and trashing the
    /// rest, based on currently allocated sizes.
    pub reclaimable_bytes: u64,
}

impl DuplicateGroup {
    #[must_use]
    pub fn copies(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn total_allocated(&self) -> u64 {
        self.files.iter().fold(0_u64, |total, file| {
            total.saturating_add(file.allocated_size)
        })
    }

    /// Estimated recovery after keeping a specific file. If the keeper is not
    /// in this group, the first file is treated as the keeper.
    #[must_use]
    pub fn savings_keeping(&self, keeper: &Path) -> u64 {
        let keeper = self
            .files
            .iter()
            .find(|file| file.path == keeper)
            .or_else(|| self.files.first());
        self.files
            .iter()
            .filter(|file| Some(&file.path) != keeper.map(|file| &file.path) && file.nlink == 1)
            .fold(0_u64, |size, file| size.saturating_add(file.allocated_size))
    }
}

/// Result of a completed duplicate search.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateReport {
    #[serde(serialize_with = "serialize_path")]
    pub root_path: PathBuf,
    pub generated_at_unix_ms: u64,
    pub groups: Vec<DuplicateGroup>,
    pub issues: Vec<DuplicateIssue>,
    pub files_scanned: u64,
    pub hardlink_duplicates: u64,
    pub bytes_hashed: u64,
    pub duration_ms: u64,
}

impl DuplicateReport {
    #[must_use]
    pub fn total_reclaimable(&self) -> u64 {
        self.groups.iter().fold(0_u64, |total, group| {
            total.saturating_add(group.reclaimable_bytes)
        })
    }

    #[must_use]
    pub fn duplicate_file_count(&self) -> usize {
        self.groups.iter().map(DuplicateGroup::copies).sum()
    }

    #[must_use]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    size: u64,
    allocated_size: u64,
    modified_unix: i64,
    modified: Option<SystemTime>,
    nlink: u64,
    dev: u64,
    ino: u64,
}

impl Candidate {
    fn from_meta(path: PathBuf, meta: FileMeta) -> Self {
        Self {
            path,
            size: meta.logical_size,
            allocated_size: meta.allocated_size,
            modified_unix: meta.modified_unix,
            modified: meta.modified,
            nlink: meta.nlink,
            dev: meta.dev,
            ino: meta.ino,
        }
    }

    fn to_file(&self) -> DuplicateFile {
        DuplicateFile {
            path: self.path.clone(),
            logical_size: self.size,
            allocated_size: self.allocated_size,
            modified_unix: self.modified_unix,
            modified: self.modified,
            nlink: self.nlink,
            dev: self.dev,
            ino: self.ino,
        }
    }
}

#[derive(Debug)]
struct PrefixRecord {
    candidate: Candidate,
    hash: blake3::Hash,
    complete: bool,
}

/// Find exact duplicate regular files below `root`.
///
/// The function is read-only. Non-fatal issues such as unreadable directories
/// or files that changed while hashing are returned in
/// [`DuplicateReport::issues`]. Cancellation is reported as
/// [`DuplicateError::Cancelled`].
pub fn find_duplicates(
    root: impl AsRef<Path>,
    options: &DuplicateOptions,
    control: &DuplicateControl,
) -> Result<DuplicateReport, DuplicateError> {
    let started = Instant::now();
    let requested = root.as_ref();
    let root_path = std::path::absolute(requested).map_err(|source| DuplicateError::Root {
        path: requested.to_path_buf(),
        source,
    })?;
    let root_meta = metadata(&root_path).map_err(|source| DuplicateError::Root {
        path: root_path.clone(),
        source,
    })?;
    check_path_components(&root_path).map_err(|source| DuplicateError::Root {
        path: root_path.clone(),
        source,
    })?;
    if root_meta.kind != FileKind::Directory {
        return Err(DuplicateError::NotDirectory(root_path));
    }

    control.set_phase(DuplicatePhase::Collecting);
    let mut issues = Vec::new();
    let (candidates, hardlink_duplicates) =
        collect_candidates(&root_path, root_meta.dev, options, control, &mut issues)?;

    control.set_phase(DuplicatePhase::Hashing);
    let groups = hash_candidates(candidates, options, control, &mut issues)?;
    control.set_phase(DuplicatePhase::Done);

    let snapshot = control.snapshot();
    let report = DuplicateReport {
        root_path,
        generated_at_unix_ms: now_unix_ms(),
        groups,
        issues,
        files_scanned: snapshot.files_scanned,
        hardlink_duplicates,
        bytes_hashed: snapshot.bytes_hashed,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    };
    for _ in 0..report.group_count() {
        control.record_group();
    }
    Ok(report)
}

fn collect_candidates(
    root: &Path,
    root_dev: u64,
    options: &DuplicateOptions,
    control: &DuplicateControl,
    issues: &mut Vec<DuplicateIssue>,
) -> Result<(Vec<Candidate>, u64), DuplicateError> {
    let mounts = MountPoints::read().map_err(|source| DuplicateError::Root {
        path: root.to_path_buf(),
        source,
    })?;
    let mut candidates = Vec::new();
    let mut hardlink_duplicates = 0_u64;
    let mut seen_hardlinks = HashSet::new();
    let mut stack = vec![root.to_path_buf()];
    let mut limit_reported = false;

    while let Some(directory) = stack.pop() {
        if cancellation(control) {
            return Err(DuplicateError::Cancelled);
        }
        control.set_current(&directory);

        let entries =
            match check_path_components(&directory).and_then(|()| fs::read_dir(&directory)) {
                Ok(entries) => entries,
                Err(error) => {
                    push_issue(control, issues, &directory, "read directory", &error);
                    continue;
                }
            };

        let mut paths = Vec::new();
        for entry in entries {
            if cancellation(control) {
                return Err(DuplicateError::Cancelled);
            }
            match entry {
                Ok(entry) => paths.push(entry.path()),
                Err(error) => {
                    push_issue(control, issues, &directory, "read directory entry", &error)
                }
            }
        }
        paths.sort();

        for path in paths {
            if cancellation(control) {
                return Err(DuplicateError::Cancelled);
            }
            let meta = match metadata(&path) {
                Ok(meta) => meta,
                Err(error) => {
                    push_issue(control, issues, &path, "read metadata", &error);
                    continue;
                }
            };
            if options.skip_hidden && is_hidden(&path, &meta) {
                continue;
            }

            match meta.kind {
                FileKind::Directory => {
                    if options.cross_filesystems
                        || (meta.dev == root_dev && !mounts.contains(&path))
                    {
                        stack.push(path);
                    }
                }
                FileKind::File => {
                    if meta.logical_size == 0 || meta.logical_size < options.min_size {
                        continue;
                    }
                    if meta.nlink > 1
                        && meta.dev != 0
                        && meta.ino != 0
                        && !seen_hardlinks.insert((meta.dev, meta.ino))
                    {
                        hardlink_duplicates = hardlink_duplicates.saturating_add(1);
                        continue;
                    }
                    if candidates.len() >= options.max_files {
                        if !limit_reported {
                            limit_reported = true;
                            issues.push(DuplicateIssue {
                                path: directory.clone(),
                                message: format!(
                                    "candidate limit of {} files reached; results may be incomplete",
                                    options.max_files
                                ),
                            });
                            control.record_error();
                        }
                        continue;
                    }
                    control.record_scanned();
                    candidates.push(Candidate::from_meta(path, meta));
                }
                FileKind::Symlink | FileKind::Other => {}
            }
        }
    }

    Ok((candidates, hardlink_duplicates))
}

fn hash_candidates(
    candidates: Vec<Candidate>,
    options: &DuplicateOptions,
    control: &DuplicateControl,
    issues: &mut Vec<DuplicateIssue>,
) -> Result<Vec<DuplicateGroup>, DuplicateError> {
    let mut by_size: BTreeMap<u64, Vec<Candidate>> = BTreeMap::new();
    for candidate in candidates {
        by_size.entry(candidate.size).or_default().push(candidate);
    }

    let mut groups = Vec::new();
    for (_, size_group) in by_size {
        if size_group.len() < 2 {
            continue;
        }
        let mut by_prefix: HashMap<String, Vec<PrefixRecord>> = HashMap::new();
        for candidate in size_group {
            if cancellation(control) {
                return Err(DuplicateError::Cancelled);
            }
            let prefix_len = options
                .prefix_bytes
                .min(usize::try_from(candidate.size).unwrap_or(usize::MAX));
            let complete = prefix_len == usize::try_from(candidate.size).unwrap_or(usize::MAX);
            match hash_prefix(&candidate, prefix_len, control) {
                Ok(hash) => {
                    by_prefix
                        .entry(hash.to_hex().to_string())
                        .or_default()
                        .push(PrefixRecord {
                            candidate,
                            hash,
                            complete,
                        });
                }
                Err(error) => {
                    if control.is_cancelled() {
                        return Err(DuplicateError::Cancelled);
                    }
                    push_hash_issue(control, issues, &candidate.path, error);
                }
            }
        }

        for (_, records) in by_prefix {
            if records.len() < 2 {
                continue;
            }
            if records.iter().all(|record| record.complete) {
                let fingerprint = records[0].hash.to_hex().to_string();
                groups.push(build_group(fingerprint, records));
                continue;
            }

            let mut by_full: HashMap<String, Vec<Candidate>> = HashMap::new();
            for record in records {
                if cancellation(control) {
                    return Err(DuplicateError::Cancelled);
                }
                match hash_full(&record.candidate, control) {
                    Ok(hash) => {
                        by_full
                            .entry(hash.to_hex().to_string())
                            .or_default()
                            .push(record.candidate);
                    }
                    Err(error) => {
                        if control.is_cancelled() {
                            return Err(DuplicateError::Cancelled);
                        }
                        push_hash_issue(control, issues, &record.candidate.path, error);
                    }
                }
            }
            for (fingerprint, files) in by_full {
                if files.len() >= 2 {
                    groups.push(build_duplicate_group(fingerprint, files));
                }
            }
        }
    }

    groups.sort_unstable_by(|left, right| {
        right
            .reclaimable_bytes
            .cmp(&left.reclaimable_bytes)
            .then_with(|| right.logical_size.cmp(&left.logical_size))
            .then_with(|| left.files[0].path.cmp(&right.files[0].path))
    });
    Ok(groups)
}

fn build_group(fingerprint: String, records: Vec<PrefixRecord>) -> DuplicateGroup {
    build_duplicate_group(
        fingerprint,
        records.into_iter().map(|record| record.candidate).collect(),
    )
}

fn build_duplicate_group(fingerprint: String, mut files: Vec<Candidate>) -> DuplicateGroup {
    files.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    let logical_size = files.first().map_or(0, |file| file.size);
    let duplicate_files: Vec<DuplicateFile> = files.iter().map(Candidate::to_file).collect();
    let reclaimable_bytes = duplicate_files
        .iter()
        .skip(1)
        .filter(|file| file.nlink == 1)
        .fold(0_u64, |size, file| size.saturating_add(file.allocated_size));
    DuplicateGroup {
        fingerprint,
        logical_size,
        files: duplicate_files,
        reclaimable_bytes,
    }
}

fn hash_prefix(
    candidate: &Candidate,
    length: usize,
    control: &DuplicateControl,
) -> Result<blake3::Hash, String> {
    hash_range(candidate, length, control)
}

fn hash_full(candidate: &Candidate, control: &DuplicateControl) -> Result<blake3::Hash, String> {
    hash_range(
        candidate,
        usize::try_from(candidate.size).unwrap_or(usize::MAX),
        control,
    )
}

fn hash_range(
    candidate: &Candidate,
    length: usize,
    control: &DuplicateControl,
) -> Result<blake3::Hash, String> {
    control.set_current(&candidate.path);
    let mut file =
        open_regular_file(&candidate.path).map_err(|error| format!("open for hashing: {error}"))?;
    validate_open_file(&file, candidate)?;

    let mut hasher = Hasher::new();
    let mut buffer = [0_u8; READ_BUFFER];
    let mut remaining = length;
    let mut read_total = 0_usize;

    while remaining > 0 {
        if cancellation(control) {
            return Err("cancelled".to_owned());
        }
        let limit = remaining.min(buffer.len());
        let read = file
            .read(&mut buffer[..limit])
            .map_err(|error| format!("read for hashing: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        remaining -= read;
        read_total = read_total.saturating_add(read);
        control.record_bytes(u64::try_from(read).unwrap_or(u64::MAX));
    }

    validate_open_file(&file, candidate)?;
    if read_total != length {
        return Err("file changed or became shorter while hashing".to_owned());
    }
    control.record_hashed();
    Ok(hasher.finalize())
}

fn validate_open_file(file: &File, candidate: &Candidate) -> Result<(), String> {
    let meta = file
        .metadata()
        .map_err(|error| format!("read opened file metadata: {error}"))?;
    if meta.len() != candidate.size {
        return Err("file size changed while hashing".to_owned());
    }
    if candidate.modified.is_none() || meta.modified().ok() != candidate.modified {
        return Err("file modification time changed while hashing".to_owned());
    }

    let (dev, ino) = opened_identity(file).map_err(|error| error.to_string())?;
    if candidate.dev == 0 || candidate.ino == 0 || dev != candidate.dev || ino != candidate.ino {
        return Err("file identity changed or unavailable while hashing".to_owned());
    }

    Ok(())
}

fn push_hash_issue(
    control: &DuplicateControl,
    issues: &mut Vec<DuplicateIssue>,
    path: &Path,
    message: String,
) {
    if message == "cancelled" {
        return;
    }
    control.record_error();
    issues.push(DuplicateIssue {
        path: path.to_path_buf(),
        message,
    });
}

fn push_issue(
    control: &DuplicateControl,
    issues: &mut Vec<DuplicateIssue>,
    path: &Path,
    action: &str,
    error: &io::Error,
) {
    control.record_error();
    issues.push(DuplicateIssue {
        path: path.to_path_buf(),
        message: format!("{action}: {error}"),
    });
}

fn cancellation(control: &DuplicateControl) -> bool {
    control.is_cancelled()
}

fn is_hidden(path: &Path, meta: &FileMeta) -> bool {
    meta.is_hidden
        || path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;

    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        let mut file = File::create(path).expect("create");
        file.write_all(bytes).expect("write");
    }

    fn options() -> DuplicateOptions {
        DuplicateOptions {
            min_size: 0,
            prefix_bytes: 4,
            ..DuplicateOptions::default()
        }
    }

    #[test]
    fn finds_exact_duplicates_and_ignores_unique_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        write(&temp.path().join("a.bin"), b"same content");
        write(&temp.path().join("b.bin"), b"same content");
        write(&temp.path().join("c.bin"), b"same content");
        write(&temp.path().join("unique.bin"), b"different!!");

        let control = DuplicateControl::new();
        let report = find_duplicates(temp.path(), &options(), &control).expect("search");
        assert_eq!(report.group_count(), 1);
        assert_eq!(report.groups[0].copies(), 3);
        assert_eq!(report.duplicate_file_count(), 3);
        assert!(report.total_reclaimable() > 0);
        assert_eq!(report.hardlink_duplicates, 0);
    }

    #[test]
    fn same_size_is_not_enough() {
        let temp = tempfile::tempdir().expect("tempdir");
        write(&temp.path().join("a.bin"), b"abcde");
        write(&temp.path().join("b.bin"), b"abcdf");
        let control = DuplicateControl::new();
        let report = find_duplicates(temp.path(), &options(), &control).expect("search");
        assert!(report.groups.is_empty());
    }

    #[test]
    fn prefix_collision_requires_a_full_hash() {
        let temp = tempfile::tempdir().expect("tempdir");
        write(&temp.path().join("a.bin"), b"sameA");
        write(&temp.path().join("b.bin"), b"sameB");
        let report =
            find_duplicates(temp.path(), &options(), &DuplicateControl::new()).expect("search");
        assert!(report.groups.is_empty());
    }

    #[test]
    fn empty_files_are_not_reported_as_duplicates() {
        let temp = tempfile::tempdir().expect("tempdir");
        write(&temp.path().join("a.bin"), b"");
        write(&temp.path().join("b.bin"), b"");
        let report =
            find_duplicates(temp.path(), &options(), &DuplicateControl::new()).expect("search");
        assert!(report.groups.is_empty());
    }

    #[test]
    fn min_size_filters_small_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        write(&temp.path().join("a.bin"), b"tiny");
        write(&temp.path().join("b.bin"), b"tiny");
        let options = DuplicateOptions {
            min_size: 100,
            ..options()
        };
        let report =
            find_duplicates(temp.path(), &options, &DuplicateControl::new()).expect("search");
        assert!(report.groups.is_empty());
    }

    #[test]
    fn hardlink_aliases_do_not_inflate_savings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let first = temp.path().join("first.bin");
        let alias = temp.path().join("alias.bin");
        write(&first, b"hardlinked content");
        fs::hard_link(&first, &alias).expect("hard link");

        let report =
            find_duplicates(temp.path(), &options(), &DuplicateControl::new()).expect("search");
        assert_eq!(report.hardlink_duplicates, 1);
        assert!(report.groups.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_never_followed() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        write(&outside.path().join("secret.bin"), b"secret");
        symlink(
            outside.path().join("secret.bin"),
            temp.path().join("link.bin"),
        )
        .expect("symlink");
        write(&temp.path().join("regular.bin"), b"secret");

        let report =
            find_duplicates(temp.path(), &options(), &DuplicateControl::new()).expect("search");
        assert!(report.groups.is_empty());
    }

    #[test]
    fn hidden_files_can_be_skipped() {
        let temp = tempfile::tempdir().expect("tempdir");
        write(&temp.path().join(".hidden.bin"), b"same");
        write(&temp.path().join("visible.bin"), b"same");
        let options = DuplicateOptions {
            skip_hidden: true,
            ..options()
        };
        let report =
            find_duplicates(temp.path(), &options, &DuplicateControl::new()).expect("search");
        assert!(report.groups.is_empty());
    }

    #[test]
    fn changed_files_are_rejected_before_hashing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("changed.bin");
        write(&path, b"original content");
        let candidate =
            Candidate::from_meta(path.clone(), metadata(&path).expect("metadata snapshot"));
        write(&path, b"short");

        let error = hash_range(&candidate, 16, &DuplicateControl::new())
            .expect_err("changed size must be rejected");
        assert!(error.contains("size changed"), "unexpected error: {error}");
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directories_are_reported_as_issues() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let blocked = temp.path().join("blocked");
        write(&blocked.join("a.bin"), b"same content");
        write(&blocked.join("b.bin"), b"same content");

        let original = fs::metadata(&blocked).expect("metadata").permissions();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))
            .expect("remove permissions");
        if fs::read_dir(&blocked).is_ok() {
            fs::set_permissions(&blocked, original).expect("restore permissions");
            return;
        }

        let report =
            find_duplicates(temp.path(), &options(), &DuplicateControl::new()).expect("search");
        fs::set_permissions(&blocked, original).expect("restore permissions");

        assert!(report.groups.is_empty());
        assert!(
            report.issues.iter().any(|issue| issue.path == blocked),
            "unreadable directory should be reported"
        );
    }

    #[test]
    fn cancellation_is_reported() {
        let temp = tempfile::tempdir().expect("tempdir");
        write(&temp.path().join("a.bin"), b"a");
        let control = DuplicateControl::new();
        control.cancel();
        assert!(matches!(
            find_duplicates(temp.path(), &options(), &control),
            Err(DuplicateError::Cancelled)
        ));
    }

    #[test]
    fn group_savings_uses_the_chosen_keeper() {
        let temp = tempfile::tempdir().expect("tempdir");
        write(&temp.path().join("a.bin"), b"same");
        write(&temp.path().join("b.bin"), b"same");
        let report =
            find_duplicates(temp.path(), &options(), &DuplicateControl::new()).expect("search");
        let group = &report.groups[0];
        let keeper = &group.files[0].path;
        assert_eq!(group.savings_keeping(keeper), group.reclaimable_bytes);
    }

    #[test]
    fn replacement_with_same_size_and_time_has_different_identity() {
        let temp = tempfile::tempdir().expect("temp");
        let original = temp.path().join("original");
        let replacement = temp.path().join("replacement");
        write(&original, b"same");
        write(&replacement, b"same");
        let candidate = Candidate::from_meta(original.clone(), metadata(&original).expect("meta"));
        let file = File::options()
            .read(true)
            .write(true)
            .open(&replacement)
            .expect("replacement");
        file.set_modified(candidate.modified.expect("mtime"))
            .expect("time");
        assert!(validate_open_file(&file, &candidate).is_err());
    }

    #[test]
    fn external_hardlinks_do_not_promise_reclaimable_bytes() {
        let temp = tempfile::tempdir().expect("temp");
        write(&temp.path().join("a"), b"same");
        write(&temp.path().join("b"), b"same");
        let outside = tempfile::tempdir().expect("outside");
        fs::hard_link(temp.path().join("b"), outside.path().join("alias")).expect("link");
        let report = find_duplicates(temp.path(), &options(), &Default::default()).expect("scan");
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].reclaimable_bytes, 0);
    }
}
