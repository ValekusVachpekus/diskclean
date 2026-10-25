//! JSON-friendly scan reports.

use std::path::PathBuf;

use serde::Serialize;

use crate::error::{ScanIssue, serialize_path};
use crate::tree::{ExtensionSummary, FileSummary, NodeId, NodeKind, ROOT_ID, ScanStats, Tree};

/// Top-level report produced by the CLI.
#[derive(Debug, Serialize)]
pub struct ScanReport {
    pub version: u32,
    #[serde(serialize_with = "serialize_path")]
    pub root: PathBuf,
    pub started_at_unix_ms: u64,
    pub duration_ms: u64,
    pub stats: ScanStats,
    pub top_files: Vec<FileSummary>,
    pub extensions: Vec<ExtensionSummary>,
    pub issues: Vec<ScanIssue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<TreeNode>,
}

/// A recursive tree node for `--tree` JSON output.
#[derive(Debug, Serialize)]
pub struct TreeNode {
    pub name: String,
    pub kind: NodeKind,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub actual_logical_size: u64,
    pub actual_allocated_size: u64,
    pub file_count: u64,
    pub dir_count: u64,
    pub modified_unix: i64,
    pub is_hidden: bool,
    pub is_hardlink_duplicate: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeNode>,
}

impl Tree {
    /// Build a report. The full tree is only materialized when requested.
    #[must_use]
    pub fn report(&self, include_tree: bool) -> ScanReport {
        ScanReport {
            version: 1,
            root: self.root_path.clone(),
            started_at_unix_ms: self.started_at_unix_ms,
            duration_ms: self.duration_ms,
            stats: self.stats,
            top_files: self.top_file_summaries(),
            extensions: self.extension_summaries(),
            issues: self.issues.clone(),
            tree: include_tree.then(|| self.tree_node(ROOT_ID)),
        }
    }

    fn tree_node(&self, id: NodeId) -> TreeNode {
        let node = self.node(id);
        TreeNode {
            name: node.display_name(),
            kind: node.kind,
            logical_size: node.logical_size,
            allocated_size: node.allocated_size,
            actual_logical_size: node.actual_logical_size,
            actual_allocated_size: node.actual_allocated_size,
            file_count: node.file_count,
            dir_count: node.dir_count,
            modified_unix: node.modified_unix,
            is_hidden: node.is_hidden,
            is_hardlink_duplicate: node.is_hardlink_duplicate,
            children: self
                .children(id)
                .map(|child| self.tree_node(child))
                .collect(),
        }
    }
}
