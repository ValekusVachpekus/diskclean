#![deny(unsafe_code)]

//! Disk Cleanup core: filesystem scanning, tree aggregation, and reports.
//!
//! The core is deliberately independent of any GUI. It performs read-only
//! scans and exposes a compact in-memory tree plus summaries suitable for a
//! treemap, tables, and JSON output.

pub mod applications;
pub mod duplicates;
pub mod error;
pub mod format;
pub mod parallel;
pub mod progress;
pub mod report;
pub mod scan;
pub mod tree;

pub use duplicates::{
    DuplicateControl, DuplicateError, DuplicateFile, DuplicateGroup, DuplicateIssue,
    DuplicateOptions, DuplicatePhase, DuplicateProgress, DuplicateReport, find_duplicates,
};
pub use error::{IssueKind, ScanError, ScanIssue};
pub use format::{format_bytes, parse_size};
pub use progress::{ProgressSnapshot, ScanControl};
pub use report::{ScanReport, TreeNode};
pub use scan::{ScanOptions, scan};
pub use tree::{
    Children, ExtensionSummary, ExtensionTotals, FileSummary, Node, NodeId, NodeKind, ROOT_ID,
    ScanStats, Tree,
};
