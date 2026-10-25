//! Mounted volume enumeration.

use std::path::PathBuf;

use serde::Serialize;
use sysinfo::Disks;

/// A mounted volume as reported by the operating system.
#[derive(Debug, Clone, Serialize)]
pub struct VolumeInfo {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub total_space: u64,
    pub available_space: u64,
    pub is_removable: bool,
    #[serde(skip)]
    pub mount_path: PathBuf,
}

/// Return all mounted volumes. The list is sorted by mount point.
pub fn list_volumes() -> Vec<VolumeInfo> {
    let disks = Disks::new_with_refreshed_list();
    let mut volumes: Vec<VolumeInfo> = disks
        .iter()
        .map(|disk| {
            let mount_path = disk.mount_point().to_path_buf();
            VolumeInfo {
                name: disk.name().to_string_lossy().into_owned(),
                mount_point: mount_path.to_string_lossy().into_owned(),
                file_system: disk.file_system().to_string_lossy().into_owned(),
                total_space: disk.total_space(),
                available_space: disk.available_space(),
                is_removable: disk.is_removable(),
                mount_path,
            }
        })
        .collect();

    volumes.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    volumes.dedup_by(|a, b| a.mount_point == b.mount_point);
    volumes
}
