//! Fatal scan errors and non-fatal scan issues.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};

/// A fatal error that prevents a scan from producing a usable tree.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("cannot access root '{path}': {source}")]
    Root {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("root path is not a directory: {0}")]
    NotDirectory(PathBuf),

    #[error("scan cancelled by user")]
    Cancelled,

    #[error("following symlinks is not supported yet")]
    FollowSymlinksUnsupported,
}

/// A non-fatal problem encountered while walking the filesystem.
#[derive(Debug, Clone, Serialize)]
pub struct ScanIssue {
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    pub kind: IssueKind,
    pub message: String,
}

impl ScanIssue {
    pub fn new(path: PathBuf, kind: IssueKind, message: impl Into<String>) -> Self {
        Self {
            path,
            kind,
            message: message.into(),
        }
    }

    pub fn from_io(path: PathBuf, action: &str, error: &io::Error) -> Self {
        let kind = match IssueKind::from(error.kind()) {
            IssueKind::Other if action.starts_with("read directory") => IssueKind::ReadDir,
            IssueKind::Other if action == "read metadata" => IssueKind::Metadata,
            kind => kind,
        };
        Self::new(path, kind, format!("{action}: {error}"))
    }
}

/// A coarse classification for display and filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    ReadDir,
    Metadata,
    PermissionDenied,
    NotFound,
    Other,
}

impl From<io::ErrorKind> for IssueKind {
    fn from(kind: io::ErrorKind) -> Self {
        match kind {
            io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            io::ErrorKind::NotFound => Self::NotFound,
            _ => Self::Other,
        }
    }
}

pub(crate) fn serialize_path<S>(path: &Path, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&path.to_string_lossy())
}

pub(crate) fn serialize_optional_path<S>(
    path: &Option<PathBuf>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match path {
        Some(path) => serializer.serialize_some(&path.to_string_lossy()),
        None => serializer.serialize_none(),
    }
}
