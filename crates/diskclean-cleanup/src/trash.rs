//! Trash-only backend. There is intentionally no permanent-delete fallback.

use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrashError {
    #[error("the operating system trash rejected {path}: {message}")]
    Rejected { path: String, message: String },
    #[error("the cleanup was cancelled before this item was moved")]
    Cancelled,
}

/// A replaceable trash implementation. Tests use a fake backend; production
/// always uses [`SystemTrash`].
pub trait TrashBackend {
    fn move_to_trash(&mut self, path: &Path) -> Result<(), TrashError>;
}

#[derive(Debug, Default)]
pub struct SystemTrash;

impl TrashBackend for SystemTrash {
    fn move_to_trash(&mut self, path: &Path) -> Result<(), TrashError> {
        trash::delete(path).map_err(|error| TrashError::Rejected {
            path: path.display().to_string(),
            message: error.to_string(),
        })
    }
}
