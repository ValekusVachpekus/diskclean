#![deny(unsafe_code)]

//! Low-level operating-system facts used by the Disk Cleanup core.
//!
//! This crate deliberately contains no cleanup policy. It only reports what the
//! operating system says about files, directories, and mounted volumes.

pub mod epic;
pub mod file_meta;
pub mod gog;
pub mod legendary;
pub mod lutris;
pub mod mounts;
pub mod registry;
pub mod steam;
pub mod user_dirs;
pub mod volumes;
pub use mounts::MountPoints;
pub use user_dirs::{downloads_dir, home_dir};

pub use file_meta::{
    FileKind, FileMeta, check_path_components, metadata, open_regular_file, opened_identity,
};
pub use volumes::{VolumeInfo, list_volumes};
