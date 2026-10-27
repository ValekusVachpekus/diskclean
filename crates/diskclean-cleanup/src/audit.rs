//! Append-only JSONL audit log for every filesystem mutation.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use diskclean_rules::{Category, Risk};
use serde::Serialize;
use thiserror::Error;

use crate::snapshot::{CleanupItem, ItemKind, now_unix_ms, serialize_path};

const ROTATE_AFTER_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEvent {
    /// Written before the trash backend is called.
    Intent,
    /// Written after the backend returns, including failures.
    Result,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub timestamp_unix_ms: u64,
    pub batch_id: String,
    pub event: AuditEvent,
    pub rule_id: String,
    pub category: Category,
    pub risk: Risk,
    pub kind: ItemKind,
    pub size: u64,
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AuditEntry {
    #[must_use]
    pub fn intent(batch_id: &str, item: &CleanupItem) -> Self {
        Self::from_item(batch_id, AuditEvent::Intent, item)
    }

    #[must_use]
    pub fn result(
        batch_id: &str,
        item: &CleanupItem,
        outcome: impl Into<String>,
        error: Option<String>,
    ) -> Self {
        let mut entry = Self::from_item(batch_id, AuditEvent::Result, item);
        entry.outcome = Some(outcome.into());
        entry.error = error;
        entry
    }

    fn from_item(batch_id: &str, event: AuditEvent, item: &CleanupItem) -> Self {
        Self {
            timestamp_unix_ms: now_unix_ms(),
            batch_id: batch_id.to_owned(),
            event,
            rule_id: item.rule_id.clone(),
            category: item.category,
            risk: item.risk,
            kind: item.snapshot.kind,
            size: item.size(),
            path: item.path.clone(),
            outcome: None,
            error: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("cannot determine the audit log location")]
    NoLocation,
    #[error("cannot create audit log directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot open audit log {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot encode audit entry: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("cannot append to audit log {path}: {source}")]
    Append {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// An open audit log. Every entry is flushed to disk before the caller is
/// allowed to mutate the filesystem.
#[derive(Debug)]
pub struct AuditLog {
    path: PathBuf,
    file: File,
}

impl AuditLog {
    /// Open the platform default log, creating its directory.
    pub fn open_default() -> Result<Self, AuditError> {
        let path = default_audit_path().ok_or(AuditError::NoLocation)?;
        Self::open(path)
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AuditError> {
        let path = path.into();
        let Some(parent) = path.parent() else {
            return Err(AuditError::CreateDirectory {
                path,
                source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
            });
        };
        create_private_dir(parent)?;
        rotate_if_needed(&path);

        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|source| AuditError::Open {
            path: path.clone(),
            source,
        })?;

        Ok(Self { path, file })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one entry and make it durable before returning.
    pub fn append(&mut self, entry: &AuditEntry) -> Result<(), AuditError> {
        let mut line = serde_json::to_vec(entry)?;
        line.push(b'\n');
        self.file
            .write_all(&line)
            .and_then(|()| self.file.sync_data())
            .map_err(|source| AuditError::Append {
                path: self.path.clone(),
                source,
            })
    }
}

fn create_private_dir(path: &Path) -> Result<(), AuditError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|source| AuditError::CreateDirectory {
            path: path.to_path_buf(),
            source,
        })
}

fn rotate_if_needed(path: &Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() < ROTATE_AFTER_BYTES {
        return;
    }

    let stamp = now_unix_ms();
    let rotated = path.with_extension(format!("jsonl.{stamp}"));
    let _ = fs::rename(path, rotated);
}

/// XDG state directory on Linux, LocalAppData on Windows.
#[must_use]
pub fn default_audit_path() -> Option<PathBuf> {
    let state = if cfg!(windows) {
        env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
    }?;
    Some(state.join("diskclean/audit.jsonl"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{BufRead, BufReader};

    use diskclean_rules::{Category, Risk};

    use super::*;
    use crate::snapshot::CleanupItem;

    #[test]
    fn writes_intent_and_result_as_jsonl() {
        let temp = tempfile::tempdir().expect("tempdir");
        let payload = temp.path().join("payload.bin");
        fs::write(&payload, b"data").expect("write");
        let item =
            CleanupItem::new(&payload, "test-rule", Category::Temporary, Risk::Low).expect("item");

        let log_path = temp.path().join("state/audit.jsonl");
        let mut log = AuditLog::open(&log_path).expect("open log");
        log.append(&AuditEntry::intent("batch-1", &item))
            .expect("intent");
        log.append(&AuditEntry::result("batch-1", &item, "trashed", None))
            .expect("result");

        let log_file = fs::File::open(&log_path).expect("read log");
        let lines: Vec<String> = BufReader::new(log_file)
            .lines()
            .map(Result::unwrap)
            .collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(&lines[0]).expect("json");
        let second: serde_json::Value = serde_json::from_str(&lines[1]).expect("json");
        assert_eq!(first["event"], "intent");
        assert_eq!(first["path"], payload.to_string_lossy().as_ref());
        assert_eq!(second["event"], "result");
        assert_eq!(second["outcome"], "trashed");
    }

    #[cfg(unix)]
    #[test]
    fn audit_file_is_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("audit.jsonl");
        let _log = AuditLog::open(&path).expect("open");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn fails_cleanly_when_the_log_path_is_a_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("as-directory");
        fs::create_dir(&path).expect("mkdir");
        assert!(AuditLog::open(path).is_err());
    }
}
