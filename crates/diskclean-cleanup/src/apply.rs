//! Revalidation and trash application after the user confirms the basket.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use diskclean_rules::Risk;
use serde::Serialize;

use crate::ApplyControl;
use crate::audit::{AuditEntry, AuditError, AuditLog};
use crate::protected::ProtectedPaths;
use crate::snapshot::{CleanupItem, FileSnapshot, now_unix_ms, serialize_path};
use crate::trash::{TrashBackend, TrashError};

/// Anything that can durably record an audit entry. Tests can inject failures.
pub trait AuditSink {
    fn append(&mut self, entry: &AuditEntry) -> Result<(), AuditError>;
}

impl AuditSink for AuditLog {
    fn append(&mut self, entry: &AuditEntry) -> Result<(), AuditError> {
        AuditLog::append(self, entry)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemResultKind {
    Trashed,
    Changed,
    Protected,
    Symlink,
    Missing,
    TrashFailed,
    AuditFailed,
    Cancelled,
    Duplicate,
    SystemOnly,
    MetadataError,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemResult {
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    pub rule_id: String,
    pub kind: ItemResultKind,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit_error: Option<String>,
}

impl ItemResult {
    fn from_item(item: &CleanupItem, kind: ItemResultKind, error: Option<String>) -> Self {
        Self {
            path: item.path.clone(),
            rule_id: item.rule_id.clone(),
            kind,
            size: item.size(),
            error,
            audit_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyReport {
    pub batch_id: String,
    pub results: Vec<ItemResult>,
    pub bytes_trashed: u64,
    pub trashed_count: usize,
    pub aborted_reason: Option<String>,
}

impl ApplyReport {
    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.results
            .iter()
            .filter(|result| result.kind != ItemResultKind::Trashed)
            .count()
    }
}

/// Apply a confirmed list of items.
///
/// Every item is checked against the protected-path policy and revalidated
/// first. An `intent` entry is flushed to the audit log before each backend
/// call. If the audit log cannot be written, the batch stops without touching
/// the remaining items.
pub fn apply(
    items: &[CleanupItem],
    protected: &ProtectedPaths,
    backend: &mut dyn TrashBackend,
    audit: &mut dyn AuditSink,
) -> ApplyReport {
    apply_with_control(items, protected, backend, audit, &ApplyControl::default())
}

/// Cancellation is checked during validation/hashing and between OS calls.
/// An OS trash call already in progress is allowed to finish and be audited.
pub fn apply_with_control(
    items: &[CleanupItem],
    protected: &ProtectedPaths,
    backend: &mut dyn TrashBackend,
    audit: &mut dyn AuditSink,
    control: &ApplyControl,
) -> ApplyReport {
    let batch_id = new_batch_id();
    let mut results = Vec::with_capacity(items.len());
    let mut seen = HashSet::new();
    let mut bytes_trashed = 0_u64;
    let mut trashed_count = 0_usize;
    let mut aborted_reason = None;

    control.update(0, items.len());
    for (index, item) in items.iter().enumerate() {
        control.update(index, items.len());
        if control.is_cancelled() {
            results.push(ItemResult::from_item(item, ItemResultKind::Cancelled, None));
            aborted_reason = Some("cleanup was cancelled".to_owned());
            break;
        }
        if items
            .iter()
            .filter_map(|selected| selected.duplicate.as_ref())
            .any(|evidence| evidence.keeper.starts_with(&item.path))
        {
            results.push(ItemResult::from_item(
                item,
                ItemResultKind::Changed,
                Some("item contains a keeper required by this batch".to_owned()),
            ));
            continue;
        }
        if let Some(evidence) = &item.duplicate
            && items
                .iter()
                .any(|selected| evidence.keeper.starts_with(&selected.path))
        {
            results.push(ItemResult::from_item(
                item,
                ItemResultKind::Changed,
                Some("keeper or its parent is selected for cleanup".to_owned()),
            ));
            continue;
        }
        if item.risk == Risk::SystemOnly {
            results.push(ItemResult::from_item(
                item,
                ItemResultKind::SystemOnly,
                Some("system-only recommendations are never executed".to_owned()),
            ));
            continue;
        }
        if !seen.insert(path_key(&item.path)) {
            results.push(ItemResult::from_item(
                item,
                ItemResultKind::Duplicate,
                Some("path appears more than once in this batch".to_owned()),
            ));
            continue;
        }
        if let Err(error) = protected.check(&item.path) {
            results.push(ItemResult::from_item(
                item,
                ItemResultKind::Protected,
                Some(error.to_string()),
            ));
            continue;
        }

        match revalidate(item, control) {
            Ok(()) => {}
            Err(RevalidateError::Cancelled) => {
                results.push(ItemResult::from_item(item, ItemResultKind::Cancelled, None));
                aborted_reason = Some("cleanup was cancelled".to_owned());
                break;
            }
            Err(RevalidateError::Missing(error)) => {
                results.push(ItemResult::from_item(
                    item,
                    ItemResultKind::Missing,
                    Some(error),
                ));
                continue;
            }
            Err(RevalidateError::Symlink) => {
                results.push(ItemResult::from_item(
                    item,
                    ItemResultKind::Symlink,
                    Some("path is now a symlink or reparse point".to_owned()),
                ));
                continue;
            }
            Err(RevalidateError::Changed) => {
                results.push(ItemResult::from_item(
                    item,
                    ItemResultKind::Changed,
                    Some("size, identity, mtime, or child count changed".to_owned()),
                ));
                continue;
            }
            Err(RevalidateError::Metadata(error)) => {
                results.push(ItemResult::from_item(
                    item,
                    ItemResultKind::MetadataError,
                    Some(error),
                ));
                continue;
            }
        }

        if let Err(error) = crate::duplicates::verify(item, &|| control.is_cancelled()) {
            if control.is_cancelled() {
                results.push(ItemResult::from_item(item, ItemResultKind::Cancelled, None));
                aborted_reason = Some("cleanup was cancelled".to_owned());
                break;
            }
            results.push(ItemResult::from_item(
                item,
                ItemResultKind::Changed,
                Some(error.to_string()),
            ));
            continue;
        }

        if let Err(error) = audit.append(&AuditEntry::intent(&batch_id, item)) {
            results.push(ItemResult::from_item(
                item,
                ItemResultKind::AuditFailed,
                Some(error.to_string()),
            ));
            aborted_reason = Some(format!(
                "audit intent could not be written; no further items were touched: {error}"
            ));
            break;
        }

        // Audit durability can take time. Revalidate again after the flush,
        // immediately before the path-based OS trash operation.
        if protected.check(&item.path).is_err()
            || revalidate(item, control).is_err()
            || control.is_cancelled()
        {
            let cancelled = control.is_cancelled();
            let mut result = ItemResult::from_item(
                item,
                if cancelled {
                    ItemResultKind::Cancelled
                } else {
                    ItemResultKind::Changed
                },
                Some("item changed after audit intent".to_owned()),
            );
            if let Err(error) = audit.append(&AuditEntry::result(
                &batch_id,
                item,
                if cancelled { "cancelled" } else { "changed" },
                result.error.clone(),
            )) {
                result.audit_error = Some(error.to_string());
                aborted_reason = Some("cannot record rejected cleanup result".to_owned());
            }
            results.push(result);
            if cancelled {
                aborted_reason = Some("cleanup was cancelled".to_owned());
            }
            if aborted_reason.is_some() {
                break;
            }
            continue;
        }

        let result = match backend.move_to_trash(&item.path) {
            Ok(()) => {
                bytes_trashed = bytes_trashed.saturating_add(item.size());
                trashed_count += 1;
                let mut result = ItemResult::from_item(item, ItemResultKind::Trashed, None);
                match audit.append(&AuditEntry::result(&batch_id, item, "trashed", None)) {
                    Ok(()) => {}
                    Err(error) => {
                        result.audit_error = Some(error.to_string());
                        aborted_reason = Some(format!(
                            "item was trashed but its audit result could not be written; stopping: {error}"
                        ));
                    }
                }
                result
            }
            Err(TrashError::Cancelled) => {
                let mut result = ItemResult::from_item(item, ItemResultKind::Cancelled, None);
                if let Err(error) =
                    audit.append(&AuditEntry::result(&batch_id, item, "cancelled", None))
                {
                    result.audit_error = Some(error.to_string());
                }
                results.push(result);
                aborted_reason = Some("cleanup was cancelled".to_owned());
                break;
            }
            Err(error) => {
                let message = error.to_string();
                let mut result =
                    ItemResult::from_item(item, ItemResultKind::TrashFailed, Some(message.clone()));
                if let Err(audit_error) = audit.append(&AuditEntry::result(
                    &batch_id,
                    item,
                    "trash_failed",
                    Some(message),
                )) {
                    result.audit_error = Some(audit_error.to_string());
                    aborted_reason = Some(format!(
                        "trash failed and the audit result could not be written; stopping: {audit_error}"
                    ));
                }
                result
            }
        };

        results.push(result);

        if aborted_reason.is_some() {
            break;
        }
    }

    control.update(results.len(), items.len());
    ApplyReport {
        batch_id,
        results,
        bytes_trashed,
        trashed_count,
        aborted_reason,
    }
}

enum RevalidateError {
    Cancelled,
    Missing(String),
    Symlink,
    Changed,
    Metadata(String),
}

fn revalidate(item: &CleanupItem, control: &ApplyControl) -> Result<(), RevalidateError> {
    let current = FileSnapshot::capture_with_cancel(&item.path, &|| control.is_cancelled())
        .map_err(|error| {
            if control.is_cancelled() {
                RevalidateError::Cancelled
            } else if error.kind() == std::io::ErrorKind::NotFound {
                RevalidateError::Missing(error.to_string())
            } else if error.kind() == std::io::ErrorKind::InvalidInput {
                RevalidateError::Symlink
            } else {
                RevalidateError::Metadata(error.to_string())
            }
        })?;

    if current != item.snapshot {
        return Err(RevalidateError::Changed);
    }
    if let Some(evidence) = &item.duplicate {
        let keeper = FileSnapshot::capture(&evidence.keeper)
            .map_err(|error| RevalidateError::Metadata(error.to_string()))?;
        if keeper != evidence.keeper_snapshot {
            return Err(RevalidateError::Changed);
        }
    }

    Ok(())
}

fn path_key(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

fn new_batch_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    format!(
        "{}-{}-{}",
        now_unix_ms(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use diskclean_rules::Category;

    use super::*;
    use crate::audit::AuditLog;
    use crate::snapshot::CleanupItem;

    #[derive(Default)]
    struct FakeTrash {
        calls: Vec<PathBuf>,
        fail: Option<String>,
        cancel_at: Option<usize>,
    }

    impl TrashBackend for FakeTrash {
        fn move_to_trash(&mut self, path: &Path) -> Result<(), TrashError> {
            if self.cancel_at == Some(self.calls.len()) {
                return Err(TrashError::Cancelled);
            }
            self.calls.push(path.to_path_buf());
            if let Some(message) = &self.fail {
                return Err(TrashError::Rejected {
                    path: path.display().to_string(),
                    message: message.clone(),
                });
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeAudit {
        entries: Vec<AuditEntry>,
        fail_at: Option<usize>,
    }

    impl AuditSink for FakeAudit {
        fn append(&mut self, entry: &AuditEntry) -> Result<(), AuditError> {
            if self.fail_at == Some(self.entries.len()) {
                return Err(AuditError::NoLocation);
            }
            self.entries.push(entry.clone());
            Ok(())
        }
    }

    fn setup() -> (tempfile::TempDir, ProtectedPaths, CleanupItem) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        fs::create_dir_all(home.join(".cache")).expect("cache");
        let file = home.join(".cache/payload.bin");
        fs::write(&file, b"payload").expect("write");
        let context = crate::CleanupContext {
            home: Some(home),
            cache: Some(temp.path().join("home/.cache")),
            ..crate::CleanupContext::default()
        };
        let protected = ProtectedPaths::for_context(&context, []);
        let item = CleanupItem::new(&file, "test", Category::AppCache, Risk::Low).expect("item");
        (temp, protected, item)
    }

    #[test]
    fn success_writes_intent_then_result() {
        let (_temp, protected, item) = setup();
        let mut trash = FakeTrash::default();
        let mut audit = FakeAudit::default();
        let report = apply(&[item], &protected, &mut trash, &mut audit);
        assert_eq!(report.trashed_count, 1);
        assert_eq!(report.failed_count(), 0);
        assert_eq!(trash.calls.len(), 1);
        assert_eq!(audit.entries.len(), 2);
        assert_eq!(audit.entries[0].event, crate::AuditEvent::Intent);
        assert_eq!(audit.entries[1].event, crate::AuditEvent::Result);
    }

    #[test]
    fn changed_file_is_not_touched() {
        let (_temp, protected, item) = setup();
        fs::write(&item.path, b"changed and larger").expect("modify");
        let mut trash = FakeTrash::default();
        let mut audit = FakeAudit::default();
        let report = apply(&[item], &protected, &mut trash, &mut audit);
        assert_eq!(report.results[0].kind, ItemResultKind::Changed);
        assert!(trash.calls.is_empty());
        assert!(audit.entries.is_empty());
    }

    #[test]
    fn protected_item_is_rejected_before_audit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        fs::create_dir_all(home.join(".ssh")).expect("ssh");
        let secret = home.join(".ssh/id_rsa");
        fs::write(&secret, b"secret").expect("write");
        let context = crate::CleanupContext {
            home: Some(home),
            ..crate::CleanupContext::default()
        };
        let protected = ProtectedPaths::for_context(&context, []);
        let item = CleanupItem::new(&secret, "ssh", Category::AppCache, Risk::Low).expect("item");
        let mut trash = FakeTrash::default();
        let mut audit = FakeAudit::default();
        let report = apply(&[item], &protected, &mut trash, &mut audit);
        assert_eq!(report.results[0].kind, ItemResultKind::Protected);
        assert!(trash.calls.is_empty());
        assert!(audit.entries.is_empty());
    }

    #[test]
    fn symlink_substitution_is_refused() {
        let (_temp, protected, item) = setup();
        let target = item.path.with_extension("target");
        fs::rename(&item.path, &target).expect("rename");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &item.path).expect("symlink");
        }
        #[cfg(not(unix))]
        {
            fs::write(&item.path, b"replacement").expect("replacement");
        }
        let mut trash = FakeTrash::default();
        let mut audit = FakeAudit::default();
        let report = apply(&[item], &protected, &mut trash, &mut audit);
        assert!(matches!(
            report.results[0].kind,
            ItemResultKind::Symlink | ItemResultKind::Changed
        ));
        assert!(trash.calls.is_empty());
    }

    #[test]
    fn trash_failure_is_audited_and_does_not_stop_other_items() {
        let (temp, protected, item) = setup();
        let second = temp.path().join("home/.cache/second.bin");
        fs::write(&second, b"second").expect("write");
        let second =
            CleanupItem::new(&second, "test", Category::AppCache, Risk::Low).expect("item");
        let mut trash = FakeTrash {
            fail: Some("simulated failure".to_owned()),
            ..FakeTrash::default()
        };
        let mut audit = FakeAudit::default();
        let report = apply(&[item, second], &protected, &mut trash, &mut audit);
        assert_eq!(report.results.len(), 2);
        assert!(
            report
                .results
                .iter()
                .all(|result| result.kind == ItemResultKind::TrashFailed)
        );
        assert_eq!(trash.calls.len(), 2);
        assert_eq!(audit.entries.len(), 4);
    }

    #[test]
    fn audit_intent_failure_aborts_without_mutation() {
        let (_temp, protected, item) = setup();
        let mut trash = FakeTrash::default();
        let mut audit = FakeAudit {
            fail_at: Some(0),
            ..FakeAudit::default()
        };
        let report = apply(&[item], &protected, &mut trash, &mut audit);
        assert_eq!(report.results[0].kind, ItemResultKind::AuditFailed);
        assert!(report.aborted_reason.is_some());
        assert!(trash.calls.is_empty());
    }

    #[test]
    fn audit_result_failure_stops_the_remaining_batch() {
        let (temp, protected, item) = setup();
        let second = temp.path().join("home/.cache/second.bin");
        fs::write(&second, b"second").expect("write");
        let second =
            CleanupItem::new(&second, "test", Category::AppCache, Risk::Low).expect("item");
        let mut trash = FakeTrash::default();
        let mut audit = FakeAudit {
            fail_at: Some(1),
            ..FakeAudit::default()
        };
        let report = apply(&[item, second], &protected, &mut trash, &mut audit);
        assert_eq!(report.trashed_count, 1);
        assert_eq!(report.results.len(), 1);
        assert!(report.results[0].audit_error.is_some());
        assert!(report.aborted_reason.is_some());
    }

    #[test]
    fn cancel_from_backend_stops_the_batch() {
        let (temp, protected, item) = setup();
        let second = temp.path().join("home/.cache/second.bin");
        fs::write(&second, b"second").expect("write");
        let second =
            CleanupItem::new(&second, "test", Category::AppCache, Risk::Low).expect("item");
        let mut trash = FakeTrash {
            cancel_at: Some(0),
            ..FakeTrash::default()
        };
        let mut audit = FakeAudit::default();
        let report = apply(&[item, second], &protected, &mut trash, &mut audit);
        assert_eq!(report.results[0].kind, ItemResultKind::Cancelled);
        assert!(trash.calls.is_empty());
        assert!(report.aborted_reason.is_some());
    }

    #[test]
    fn concrete_audit_log_accepts_the_sink_trait() {
        let (temp, protected, item) = setup();
        let mut log = AuditLog::open(temp.path().join("audit.jsonl")).expect("log");
        let mut trash = FakeTrash::default();
        let report = apply(&[item], &protected, &mut trash, &mut log);
        assert_eq!(report.trashed_count, 1);
    }

    #[test]
    fn duplicate_paths_are_rejected_once() {
        let (_temp, protected, item) = setup();
        let mut trash = FakeTrash::default();
        let mut audit = FakeAudit::default();
        let report = apply(&[item.clone(), item], &protected, &mut trash, &mut audit);
        assert_eq!(report.trashed_count, 1);
        assert_eq!(report.results[1].kind, ItemResultKind::Duplicate);
    }

    #[test]
    fn nested_changes_reject_the_entire_directory() {
        let (temp, protected, _) = setup();
        let dir = temp.path().join("home/.cache");
        fs::create_dir(dir.join("nested")).expect("nested");
        let item = CleanupItem::new(&dir, "test", Category::AppCache, Risk::Low).expect("snapshot");
        fs::write(dir.join("nested/new-important-file"), b"new data").expect("new file");
        let mut trash = FakeTrash::default();
        let report = apply(&[item], &protected, &mut trash, &mut FakeAudit::default());
        assert_eq!(report.results[0].kind, ItemResultKind::Changed);
        assert!(trash.calls.is_empty());
    }

    #[test]
    fn subsecond_changes_are_rejected() {
        use std::time::{Duration, UNIX_EPOCH};
        let (_temp, protected, item) = setup();
        let file = fs::File::options()
            .write(true)
            .open(&item.path)
            .expect("file");
        file.set_modified(UNIX_EPOCH + Duration::new(1_700_000_000, 100_000_000))
            .expect("mtime");
        let item =
            CleanupItem::new(&item.path, "test", Category::AppCache, Risk::Low).expect("snapshot");
        fs::write(&item.path, b"changed").expect("same size");
        file.set_modified(UNIX_EPOCH + Duration::new(1_700_000_000, 900_000_000))
            .expect("mtime");
        let mut trash = FakeTrash::default();
        let report = apply(&[item], &protected, &mut trash, &mut FakeAudit::default());
        assert_eq!(report.results[0].kind, ItemResultKind::Changed);
        assert!(trash.calls.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn parent_link_substitution_is_rejected_even_for_same_inode() {
        let (temp, protected, item) = setup();
        let original = temp.path().join("home/.cache");
        let moved = temp.path().join("home/moved-cache");
        fs::rename(&original, &moved).expect("rename parent");
        std::os::unix::fs::symlink(&moved, &original).expect("link parent");
        let mut trash = FakeTrash::default();
        let report = apply(&[item], &protected, &mut trash, &mut FakeAudit::default());
        assert_eq!(report.results[0].kind, ItemResultKind::Symlink);
        assert!(trash.calls.is_empty());
    }

    #[test]
    fn changes_during_audit_flush_are_rejected_and_recorded() {
        struct ChangingAudit {
            path: PathBuf,
            entries: Vec<AuditEntry>,
        }
        impl AuditSink for ChangingAudit {
            fn append(&mut self, entry: &AuditEntry) -> Result<(), AuditError> {
                if self.entries.is_empty() {
                    fs::write(&self.path, b"new data after intent").expect("change");
                }
                self.entries.push(entry.clone());
                Ok(())
            }
        }
        let (_temp, protected, item) = setup();
        let mut audit = ChangingAudit {
            path: item.path.clone(),
            entries: Vec::new(),
        };
        let mut trash = FakeTrash::default();
        let report = apply(&[item], &protected, &mut trash, &mut audit);
        assert_eq!(report.results[0].kind, ItemResultKind::Changed);
        assert!(trash.calls.is_empty());
        assert_eq!(audit.entries.len(), 2);
        assert_eq!(audit.entries[1].outcome.as_deref(), Some("changed"));
    }

    fn duplicate_fixture() -> (
        tempfile::TempDir,
        ProtectedPaths,
        diskclean_core::DuplicateGroup,
    ) {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home");
        for name in ["a", "b"] {
            fs::write(home.join(name), b"same").expect("file");
        }
        let policy = ProtectedPaths::for_context(
            &crate::CleanupContext {
                home: Some(home.clone()),
                ..Default::default()
            },
            [],
        );
        let report =
            diskclean_core::find_duplicates(&home, &Default::default(), &Default::default())
                .expect("scan");
        (temp, policy, report.groups[0].clone())
    }

    #[test]
    fn duplicate_evidence_is_not_refreshed_when_selected() {
        let (_temp, policy, group) = duplicate_fixture();
        fs::write(&group.files[1].path, b"unique new content").expect("change before selection");
        let item =
            CleanupItem::from_duplicate(&group.files[1], &group.files[0], &group.fingerprint)
                .expect("select");
        let mut trash = FakeTrash::default();
        let report = apply(&[item], &policy, &mut trash, &mut FakeAudit::default());
        assert_eq!(report.results[0].kind, ItemResultKind::Changed);
        assert!(trash.calls.is_empty());
    }

    #[test]
    fn missing_keeper_prevents_trashing_the_last_copy() {
        let (_temp, policy, group) = duplicate_fixture();
        let item =
            CleanupItem::from_duplicate(&group.files[1], &group.files[0], &group.fingerprint)
                .expect("select");
        fs::rename(
            &group.files[0].path,
            group.files[0].path.with_extension("moved"),
        )
        .expect("move keeper");
        let mut trash = FakeTrash::default();
        let report = apply(&[item], &policy, &mut trash, &mut FakeAudit::default());
        assert_ne!(report.results[0].kind, ItemResultKind::Trashed);
        assert!(trash.calls.is_empty());
    }

    #[test]
    fn content_changes_with_restored_metadata_are_rejected() {
        for change_keeper in [false, true] {
            let (_temp, policy, group) = duplicate_fixture();
            let item =
                CleanupItem::from_duplicate(&group.files[1], &group.files[0], &group.fingerprint)
                    .expect("select");
            let file = &group.files[usize::from(!change_keeper)];
            fs::write(&file.path, b"evil").expect("change");
            fs::File::options()
                .write(true)
                .open(&file.path)
                .expect("file")
                .set_modified(file.modified.expect("mtime"))
                .expect("restore mtime");
            let mut trash = FakeTrash::default();
            let report = apply(&[item], &policy, &mut trash, &mut FakeAudit::default());
            assert_eq!(report.results[0].kind, ItemResultKind::Changed);
            assert!(trash.calls.is_empty());
        }
    }

    #[test]
    fn valid_duplicates_are_trashed_but_keeper_cannot_be_in_batch() {
        let (_temp, policy, group) = duplicate_fixture();
        let item =
            CleanupItem::from_duplicate(&group.files[1], &group.files[0], &group.fingerprint)
                .expect("select");
        let mut trash = FakeTrash::default();
        let report = apply(
            std::slice::from_ref(&item),
            &policy,
            &mut trash,
            &mut FakeAudit::default(),
        );
        assert_eq!(report.trashed_count, 1);
        let keeper_item = CleanupItem::new(
            &group.files[0].path,
            "other-rule",
            Category::Temporary,
            Risk::Low,
        )
        .expect("keeper");
        let mut trash = FakeTrash::default();
        let report = apply(
            &[keeper_item, item],
            &policy,
            &mut trash,
            &mut FakeAudit::default(),
        );
        assert_eq!(report.trashed_count, 0);
        assert!(trash.calls.is_empty());
    }

    #[test]
    fn user_cancellation_stops_between_items_and_audits_completed_work() {
        struct CancellingTrash {
            control: ApplyControl,
            calls: usize,
        }
        impl TrashBackend for CancellingTrash {
            fn move_to_trash(&mut self, _: &Path) -> Result<(), TrashError> {
                self.calls += 1;
                self.control.cancel();
                Ok(())
            }
        }
        let (temp, policy, first) = setup();
        let second_path = temp.path().join("home/.cache/second");
        fs::write(&second_path, b"second").expect("second");
        let second =
            CleanupItem::new(second_path, "test", Category::Temporary, Risk::Low).expect("item");
        let control = ApplyControl::default();
        let mut trash = CancellingTrash {
            control: control.clone(),
            calls: 0,
        };
        let mut audit = FakeAudit::default();
        let report =
            apply_with_control(&[first, second], &policy, &mut trash, &mut audit, &control);
        assert_eq!(trash.calls, 1);
        assert_eq!(report.trashed_count, 1);
        assert_eq!(report.results[1].kind, ItemResultKind::Cancelled);
        assert_eq!(audit.entries.len(), 2);
        assert_eq!(audit.entries[1].outcome.as_deref(), Some("trashed"));
        assert!(report.aborted_reason.is_some());
        assert_eq!(control.progress(), (2, 2));
    }

    #[test]
    fn cancellation_before_apply_does_not_write_intent_or_call_backend() {
        let (_temp, policy, item) = setup();
        let control = ApplyControl::default();
        control.cancel();
        let mut trash = FakeTrash::default();
        let mut audit = FakeAudit::default();
        let report = apply_with_control(&[item], &policy, &mut trash, &mut audit, &control);
        assert_eq!(report.results[0].kind, ItemResultKind::Cancelled);
        assert!(trash.calls.is_empty());
        assert!(audit.entries.is_empty());
    }
}
