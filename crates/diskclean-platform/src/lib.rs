#![deny(unsafe_code)]

//! Low-level operating-system facts used by the Disk Cleanup core.
//!
//! This crate deliberately contains no cleanup policy. It only reports what the
//! operating system says about files, directories, and mounted volumes.

pub mod file_meta;
pub mod mounts;
pub mod volumes;
pub use mounts::MountPoints;

pub use file_meta::{
    FileKind, FileMeta, check_path_components, metadata, open_regular_file, opened_identity,
};
pub use volumes::{VolumeInfo, list_volumes};
