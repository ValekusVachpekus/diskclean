//! In-memory directory tree and its summaries.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use serde::Serialize;

use crate::error::{ScanIssue, serialize_path};

/// Node identifier inside a [`Tree`]. The root is always [`ROOT_ID`].
pub type NodeId = u32;

/// The root node id.
pub const ROOT_ID: NodeId = 0;

/// The kind of a filesystem entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Directory,
    File,
    Symlink,
    Other,
}

impl NodeKind {
    #[must_use]
    pub fn is_dir(self) -> bool {
        matches!(self, Self::Directory)
    }
}

/// A node in the scanned tree.
///
/// `logical_size` and `allocated_size` are the *counted* sizes: hardlink
/// duplicates contribute zero, so a directory's size is the unique disk usage
/// of its subtree. `actual_*` keeps the file's own size for display.
#[derive(Debug, Clone)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
    pub name: OsString,
    pub kind: NodeKind,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub actual_logical_size: u64,
    pub actual_allocated_size: u64,
    /// Regular files in this subtree. A file node counts itself as 1.
    pub file_count: u64,
    /// Directories in this subtree including this node. A file counts 0.
    pub dir_count: u64,
    pub modified_unix: i64,
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    pub is_hidden: bool,
    pub is_hardlink_duplicate: bool,
    pub depth: u16,
}

impl Node {
    #[must_use]
    pub fn display_name(&self) -> String {
        self.name.to_string_lossy().into_owned()
    }
}

/// Aggregate statistics for a completed scan.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ScanStats {
    pub directories: u64,
    pub files: u64,
    pub symlinks: u64,
    pub other: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub hardlink_duplicates: u64,
    pub skipped_mounts: u64,
    pub skipped_excluded: u64,
    pub skipped_depth: u64,
    pub errors: u64,
}

/// A single file in a top-N list.
#[derive(Debug, Clone, Serialize)]
pub struct FileSummary {
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    pub kind: NodeKind,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub modified_unix: i64,
    pub is_hardlink_duplicate: bool,
}

/// Totals for one filename extension.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExtensionTotals {
    pub files: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
}

/// A JSON-friendly extension summary.
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionSummary {
    pub extension: String,
    pub files: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
}

/// A completed read-only scan.
#[derive(Debug)]
pub struct Tree {
    pub root_path: PathBuf,
    pub nodes: Vec<Node>,
    pub top_files: Vec<NodeId>,
    pub extensions: BTreeMap<String, ExtensionTotals>,
    pub issues: Vec<ScanIssue>,
    pub stats: ScanStats,
    pub started_at_unix_ms: u64,
    pub duration_ms: u64,
}

impl Tree {
    #[must_use]
    pub fn root(&self) -> NodeId {
        ROOT_ID
    }

    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        debug_assert!((id as usize) < self.nodes.len());
        &self.nodes[id as usize]
    }

    #[must_use]
    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children {
            tree: self,
            next: self.node(id).first_child,
        }
    }

    /// Reconstruct the absolute path of a node.
    #[must_use]
    pub fn path(&self, id: NodeId) -> PathBuf {
        if id == ROOT_ID {
            return self.root_path.clone();
        }

        let mut parts = Vec::new();
        let mut current = Some(id);
        while let Some(node_id) = current {
            if node_id == ROOT_ID {
                break;
            }
            let node = self.node(node_id);
            parts.push(node.name.clone());
            current = node.parent;
        }

        let mut path = self.root_path.clone();
        for part in parts.iter().rev() {
            path.push(part);
        }
        path
    }

    /// The largest files, sorted by actual allocated size.
    #[must_use]
    pub fn top_file_summaries(&self) -> Vec<FileSummary> {
        self.top_files
            .iter()
            .map(|id| {
                let node = self.node(*id);
                FileSummary {
                    path: self.path(*id),
                    kind: node.kind,
                    logical_size: node.actual_logical_size,
                    allocated_size: node.actual_allocated_size,
                    modified_unix: node.modified_unix,
                    is_hardlink_duplicate: node.is_hardlink_duplicate,
                }
            })
            .collect()
    }

    /// Extension totals sorted by allocated size (largest first).
    #[must_use]
    pub fn extension_summaries(&self) -> Vec<ExtensionSummary> {
        let mut summaries: Vec<ExtensionSummary> = self
            .extensions
            .iter()
            .map(|(extension, totals)| ExtensionSummary {
                extension: extension.clone(),
                files: totals.files,
                logical_bytes: totals.logical_bytes,
                allocated_bytes: totals.allocated_bytes,
            })
            .collect();

        summaries.sort_unstable_by(|a, b| {
            b.allocated_bytes
                .cmp(&a.allocated_bytes)
                .then_with(|| b.logical_bytes.cmp(&a.logical_bytes))
                .then_with(|| a.extension.cmp(&b.extension))
        });
        summaries
    }
}

/// Iterator over a directory's direct children.
#[derive(Debug)]
pub struct Children<'a> {
    tree: &'a Tree,
    next: Option<NodeId>,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next?;
        self.next = self.tree.node(id).next_sibling;
        Some(id)
    }
}
