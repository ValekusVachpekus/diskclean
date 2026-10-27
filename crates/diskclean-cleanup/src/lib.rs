#![deny(unsafe_code)]

//! Safe cleanup primitives for Disk Cleanup.
//!
//! This crate is the only place allowed to mutate the filesystem. It provides:
//! read-only recommendation discovery, a protected-path policy, metadata
//! revalidation, a trash-only backend, and an append-only JSONL audit log.
//!
//! There is no permanent-delete fallback and no elevation helper.

mod apply;
mod audit;
mod control;
mod discovery;
mod duplicates;
mod protected;
mod snapshot;
mod trash;

pub use apply::{ApplyReport, AuditSink, ItemResult, ItemResultKind, apply, apply_with_control};
pub use audit::{AuditEntry, AuditError, AuditEvent, AuditLog, default_audit_path};
pub use control::ApplyControl;
pub use discovery::{
    CleanupContext, CleanupIssue, CleanupReport, Recommendation, discover, discover_default,
};
pub use protected::{ProtectedPathError, ProtectedPaths};
pub use snapshot::{
    CleanupItem, DescendantSnapshot, DuplicateEvidence, FileSnapshot, ItemKind, snapshot,
};
pub use trash::{SystemTrash, TrashBackend, TrashError};
