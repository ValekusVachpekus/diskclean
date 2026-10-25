//! Parallel, read-only filesystem scanner.

use std::collections::{BTreeMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use diskclean_platform::file_meta::{self, FileKind, FileMeta};
use rayon::prelude::*;

use crate::error::{ScanError, ScanIssue};
use crate::progress::ScanControl;
use crate::tree::{ExtensionTotals, Node, NodeId, NodeKind, ROOT_ID, ScanStats, Tree};

/// Options controlling a scan.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Follow mount points and scan across filesystem boundaries.
    pub cross_filesystems: bool,
    /// Maximum directory depth. `Some(0)` scans the root directory only.
    pub max_depth: Option<u16>,
    /// Absolute or relative paths that should not be entered.
    pub exclude: Vec<PathBuf>,
    /// Skip dotfiles and platform-hidden entries.
    pub skip_hidden: bool,
    /// Number of largest files to keep in the summary.
    pub top_files: usize,
    /// Reserved for a future implementation. Currently rejected by [`scan`].
    pub follow_symlinks: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            cross_filesystems: false,
            max_depth: None,
            exclude: Vec::new(),
            skip_hidden: false,
            top_files: 100,
            follow_symlinks: false,
        }
    }
}

/// Scan a directory and build a [`Tree`].
///
/// The scan is read-only. Non-fatal filesystem problems are collected in
/// [`Tree::issues`]; only an inaccessible/non-directory root or cancellation is
/// returned as an error.
pub fn scan(
    root: impl AsRef<Path>,
    options: &ScanOptions,
    control: &ScanControl,
) -> Result<Tree, ScanError> {
    if options.follow_symlinks {
        return Err(ScanError::FollowSymlinksUnsupported);
    }

    let requested = root.as_ref();
    let root_path = std::path::absolute(requested).map_err(|source| ScanError::Root {
        path: requested.to_path_buf(),
        source,
    })?;

    let root_meta = file_meta::metadata(&root_path).map_err(|source| ScanError::Root {
        path: root_path.clone(),
        source,
    })?;
    if root_meta.kind != FileKind::Directory {
        return Err(ScanError::NotDirectory(root_path));
    }

    let mut excludes = normalize_excludes(&root_path, &options.exclude);
    if root_path == Path::new("/") {
        for system_path in ["/proc", "/sys", "/dev", "/run"] {
            let path = PathBuf::from(system_path);
            if !excludes.contains(&path) {
                excludes.push(path);
            }
        }
    }

    let started_at_unix_ms = system_time_to_unix_ms(SystemTime::now());
    let started = Instant::now();
    let root_name = root_path
        .file_name()
        .map_or_else(|| OsString::from("/"), OsString::from);

    let scanner = Scanner {
        options,
        control,
        root_dev: root_meta.dev,
        excludes: &excludes,
    };
    let raw = scanner.scan_dir(&root_path, root_name.clone(), root_meta, 0);

    if control.is_cancelled() {
        return Err(ScanError::Cancelled);
    }

    let mut builder = TreeBuilder::new(root_path, root_name, root_meta, options);
    builder.flatten_dir(&raw, ROOT_ID, 0);
    Ok(builder.finish(started_at_unix_ms, started.elapsed()))
}

fn normalize_excludes(root: &Path, excludes: &[PathBuf]) -> Vec<PathBuf> {
    let mut normalized = Vec::with_capacity(excludes.len());
    for exclude in excludes {
        let candidate = if exclude.is_absolute() {
            exclude.clone()
        } else {
            root.join(exclude)
        };
        let absolute = match std::path::absolute(&candidate) {
            Ok(path) => path,
            Err(_) => candidate,
        };
        if !normalized.contains(&absolute) {
            normalized.push(absolute);
        }
    }
    normalized
}

fn system_time_to_unix_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

struct RawDir {
    name: OsString,
    meta: FileMeta,
    dirs: Vec<RawDir>,
    entries: Vec<RawEntry>,
    issues: Vec<ScanIssue>,
    skipped_excluded: u64,
    skipped_mounts: u64,
    skipped_depth: u64,
}

struct RawEntry {
    name: OsString,
    meta: FileMeta,
}

struct Scanner<'a> {
    options: &'a ScanOptions,
    control: &'a ScanControl,
    root_dev: u64,
    excludes: &'a [PathBuf],
}

impl Scanner<'_> {
    fn scan_dir(&self, path: &Path, name: OsString, meta: FileMeta, depth: u16) -> RawDir {
        let mut raw = RawDir {
            name,
            meta,
            dirs: Vec::new(),
            entries: Vec::new(),
            issues: Vec::new(),
            skipped_excluded: 0,
            skipped_mounts: 0,
            skipped_depth: 0,
        };

        if self.control.is_cancelled() {
            return raw;
        }

        self.control.record_directory();
        self.control.set_current(path);

        let read_dir = match fs::read_dir(path) {
            Ok(read_dir) => read_dir,
            Err(error) => {
                self.record_issue(&mut raw, path, "read directory", &error);
                return raw;
            }
        };

        let mut child_dirs: Vec<(PathBuf, OsString, FileMeta)> = Vec::new();

        for entry_result in read_dir {
            if self.control.is_cancelled() {
                break;
            }

            let entry = match entry_result {
                Ok(entry) => entry,
                Err(error) => {
                    self.record_issue(&mut raw, path, "read directory entry", &error);
                    continue;
                }
            };

            let entry_path = entry.path();
            let entry_name = entry.file_name();

            if self.is_excluded(&entry_path) {
                raw.skipped_excluded = raw.skipped_excluded.saturating_add(1);
                continue;
            }

            let entry_meta = match file_meta::metadata(&entry_path) {
                Ok(meta) => meta,
                Err(error) => {
                    self.record_issue(&mut raw, &entry_path, "read metadata", &error);
                    continue;
                }
            };

            if self.options.skip_hidden && is_hidden_entry(&entry_name, &entry_meta) {
                raw.skipped_excluded = raw.skipped_excluded.saturating_add(1);
                continue;
            }

            match entry_meta.kind {
                FileKind::Directory => {
                    if self.options.max_depth.is_some_and(|max| depth >= max) {
                        raw.skipped_depth = raw.skipped_depth.saturating_add(1);
                        continue;
                    }
                    if entry_meta.dev != self.root_dev && !self.options.cross_filesystems {
                        raw.skipped_mounts = raw.skipped_mounts.saturating_add(1);
                        continue;
                    }
                    child_dirs.push((entry_path, entry_name, entry_meta));
                }
                FileKind::File => {
                    self.control
                        .record_file(entry_meta.logical_size, entry_meta.allocated_size);
                    raw.entries.push(RawEntry {
                        name: entry_name,
                        meta: entry_meta,
                    });
                }
                FileKind::Symlink => {
                    self.control.record_symlink();
                    raw.entries.push(RawEntry {
                        name: entry_name,
                        meta: entry_meta,
                    });
                }
                FileKind::Other => {
                    self.control.record_other();
                    raw.entries.push(RawEntry {
                        name: entry_name,
                        meta: entry_meta,
                    });
                }
            }
        }

        raw.dirs = child_dirs
            .into_par_iter()
            .map(|(child_path, child_name, child_meta)| {
                self.scan_dir(&child_path, child_name, child_meta, depth.saturating_add(1))
            })
            .collect();

        raw
    }

    fn is_excluded(&self, path: &Path) -> bool {
        self.excludes
            .iter()
            .any(|exclude| path.starts_with(exclude))
    }

    fn record_issue(&self, raw: &mut RawDir, path: &Path, action: &str, error: &io::Error) {
        raw.issues
            .push(ScanIssue::from_io(path.to_path_buf(), action, error));
        self.control.record_error();
    }
}

fn is_hidden_entry(name: &OsStr, meta: &FileMeta) -> bool {
    meta.is_hidden || name_is_dot_hidden(name)
}

#[cfg(unix)]
fn name_is_dot_hidden(name: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;

    name.as_bytes().first() == Some(&b'.')
}

#[cfg(not(unix))]
fn name_is_dot_hidden(name: &OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

struct TreeBuilder<'a> {
    options: &'a ScanOptions,
    root_path: PathBuf,
    nodes: Vec<Node>,
    last_child: Vec<Option<NodeId>>,
    seen_hardlinks: HashSet<(u64, u64)>,
    file_nodes: Vec<NodeId>,
    extensions: BTreeMap<String, ExtensionTotals>,
    issues: Vec<ScanIssue>,
    stats: ScanStats,
}

impl<'a> TreeBuilder<'a> {
    fn new(
        root_path: PathBuf,
        root_name: OsString,
        root_meta: FileMeta,
        options: &'a ScanOptions,
    ) -> Self {
        let root = Node {
            parent: None,
            first_child: None,
            next_sibling: None,
            name: root_name,
            kind: NodeKind::Directory,
            logical_size: 0,
            allocated_size: root_meta.allocated_size,
            actual_logical_size: 0,
            actual_allocated_size: root_meta.allocated_size,
            file_count: 0,
            dir_count: 1,
            modified_unix: root_meta.modified_unix,
            dev: root_meta.dev,
            ino: root_meta.ino,
            nlink: root_meta.nlink,
            is_hidden: false,
            is_hardlink_duplicate: false,
            depth: 0,
        };

        let stats = ScanStats {
            directories: 1,
            allocated_bytes: root_meta.allocated_size,
            ..ScanStats::default()
        };

        Self {
            options,
            root_path,
            nodes: vec![root],
            last_child: vec![None],
            seen_hardlinks: HashSet::new(),
            file_nodes: Vec::new(),
            extensions: BTreeMap::new(),
            issues: Vec::new(),
            stats,
        }
    }

    fn flatten_dir(&mut self, raw: &RawDir, parent: NodeId, depth: u16) {
        self.issues.extend(raw.issues.iter().cloned());
        self.stats.skipped_excluded = self
            .stats
            .skipped_excluded
            .saturating_add(raw.skipped_excluded);
        self.stats.skipped_mounts = self.stats.skipped_mounts.saturating_add(raw.skipped_mounts);
        self.stats.skipped_depth = self.stats.skipped_depth.saturating_add(raw.skipped_depth);

        for entry in &raw.entries {
            let is_duplicate = entry.meta.kind == FileKind::File
                && entry.meta.nlink > 1
                && !self.seen_hardlinks.insert((entry.meta.dev, entry.meta.ino));

            let kind = node_kind(entry.meta.kind);
            let (actual_logical, actual_allocated) = match kind {
                NodeKind::File => (entry.meta.logical_size, entry.meta.allocated_size),
                _ => (0, 0),
            };
            let (counted_logical, counted_allocated) = if is_duplicate {
                (0, 0)
            } else {
                (actual_logical, actual_allocated)
            };

            let node = Node {
                parent: None,
                first_child: None,
                next_sibling: None,
                name: entry.name.clone(),
                kind,
                logical_size: counted_logical,
                allocated_size: counted_allocated,
                actual_logical_size: actual_logical,
                actual_allocated_size: actual_allocated,
                file_count: if kind == NodeKind::File { 1 } else { 0 },
                dir_count: 0,
                modified_unix: entry.meta.modified_unix,
                dev: entry.meta.dev,
                ino: entry.meta.ino,
                nlink: entry.meta.nlink,
                is_hidden: is_hidden_entry(&entry.name, &entry.meta),
                is_hardlink_duplicate: is_duplicate,
                depth: depth.saturating_add(1),
            };
            let node_id = self.push_node(node);
            self.add_child(parent, node_id);

            let parent_node = &mut self.nodes[parent as usize];
            parent_node.logical_size = parent_node.logical_size.saturating_add(counted_logical);
            parent_node.allocated_size =
                parent_node.allocated_size.saturating_add(counted_allocated);
            if kind == NodeKind::File {
                parent_node.file_count = parent_node.file_count.saturating_add(1);
            }

            match kind {
                NodeKind::File => {
                    self.stats.files = self.stats.files.saturating_add(1);
                    self.stats.logical_bytes =
                        self.stats.logical_bytes.saturating_add(counted_logical);
                    self.stats.allocated_bytes =
                        self.stats.allocated_bytes.saturating_add(counted_allocated);
                    if is_duplicate {
                        self.stats.hardlink_duplicates =
                            self.stats.hardlink_duplicates.saturating_add(1);
                    }
                    self.record_extension(&entry.name, counted_logical, counted_allocated);
                    self.file_nodes.push(node_id);
                }
                NodeKind::Symlink => {
                    self.stats.symlinks = self.stats.symlinks.saturating_add(1);
                }
                NodeKind::Other => {
                    self.stats.other = self.stats.other.saturating_add(1);
                }
                NodeKind::Directory => {
                    // A directory is represented by RawDir, not RawEntry.
                }
            }
        }

        for child in &raw.dirs {
            let child_node = Node {
                parent: None,
                first_child: None,
                next_sibling: None,
                name: child.name.clone(),
                kind: NodeKind::Directory,
                logical_size: 0,
                allocated_size: child.meta.allocated_size,
                actual_logical_size: 0,
                actual_allocated_size: child.meta.allocated_size,
                file_count: 0,
                dir_count: 1,
                modified_unix: child.meta.modified_unix,
                dev: child.meta.dev,
                ino: child.meta.ino,
                nlink: child.meta.nlink,
                is_hidden: is_hidden_entry(&child.name, &child.meta),
                is_hardlink_duplicate: false,
                depth: depth.saturating_add(1),
            };
            let child_id = self.push_node(child_node);
            self.add_child(parent, child_id);

            self.stats.directories = self.stats.directories.saturating_add(1);
            self.stats.allocated_bytes = self
                .stats
                .allocated_bytes
                .saturating_add(child.meta.allocated_size);

            self.flatten_dir(child, child_id, depth.saturating_add(1));

            let child_node = &self.nodes[child_id as usize];
            let child_logical = child_node.logical_size;
            let child_allocated = child_node.allocated_size;
            let child_files = child_node.file_count;
            let child_dirs = child_node.dir_count;

            let parent_node = &mut self.nodes[parent as usize];
            parent_node.logical_size = parent_node.logical_size.saturating_add(child_logical);
            parent_node.allocated_size = parent_node.allocated_size.saturating_add(child_allocated);
            parent_node.file_count = parent_node.file_count.saturating_add(child_files);
            parent_node.dir_count = parent_node.dir_count.saturating_add(child_dirs);

            let child_node = &mut self.nodes[child_id as usize];
            child_node.actual_logical_size = child_logical;
            child_node.actual_allocated_size = child_allocated;
        }
    }

    fn push_node(&mut self, node: Node) -> NodeId {
        assert!(
            self.nodes.len() < NodeId::MAX as usize,
            "tree exceeds the maximum node count"
        );
        let id = NodeId::try_from(self.nodes.len()).expect("node count fits in u32");
        self.nodes.push(node);
        self.last_child.push(None);
        id
    }

    fn add_child(&mut self, parent: NodeId, child: NodeId) {
        self.nodes[child as usize].parent = Some(parent);

        if self.nodes[parent as usize].first_child.is_none() {
            self.nodes[parent as usize].first_child = Some(child);
        } else if let Some(last) = self.last_child[parent as usize] {
            self.nodes[last as usize].next_sibling = Some(child);
        }
        self.last_child[parent as usize] = Some(child);
    }

    fn record_extension(&mut self, name: &OsStr, logical: u64, allocated: u64) {
        let extension = Path::new(name)
            .extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "(none)".to_owned());

        let totals = self.extensions.entry(extension).or_default();
        totals.files = totals.files.saturating_add(1);
        totals.logical_bytes = totals.logical_bytes.saturating_add(logical);
        totals.allocated_bytes = totals.allocated_bytes.saturating_add(allocated);
    }

    fn finish(mut self, started_at_unix_ms: u64, duration: Duration) -> Tree {
        let nodes = &self.nodes;
        self.file_nodes.sort_unstable_by(|left, right| {
            let left = &nodes[*left as usize];
            let right = &nodes[*right as usize];
            right
                .actual_allocated_size
                .cmp(&left.actual_allocated_size)
                .then_with(|| right.actual_logical_size.cmp(&left.actual_logical_size))
                .then_with(|| right.modified_unix.cmp(&left.modified_unix))
        });
        self.file_nodes.truncate(self.options.top_files);

        let root = &mut self.nodes[ROOT_ID as usize];
        root.actual_logical_size = root.logical_size;
        root.actual_allocated_size = root.allocated_size;

        self.stats.errors = u64::try_from(self.issues.len()).unwrap_or(u64::MAX);

        Tree {
            root_path: self.root_path,
            nodes: self.nodes,
            top_files: self.file_nodes,
            extensions: self.extensions,
            issues: self.issues,
            stats: self.stats,
            started_at_unix_ms,
            duration_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        }
    }
}

fn node_kind(kind: FileKind) -> NodeKind {
    match kind {
        FileKind::Directory => NodeKind::Directory,
        FileKind::File => NodeKind::File,
        FileKind::Symlink => NodeKind::Symlink,
        FileKind::Other => NodeKind::Other,
    }
}
