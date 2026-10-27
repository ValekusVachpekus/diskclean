//! Mount topology used by read-only traversal and cleanup snapshots.

use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Default)]
pub struct MountPoints(Vec<PathBuf>);

impl MountPoints {
    /// Linux mountinfo also identifies bind mounts on the same device.
    /// Other platforms additionally use device IDs and reparse-point checks.
    pub fn read() -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::ffi::OsStringExt;
            let data = std::fs::read("/proc/self/mountinfo")?;
            let mut paths = Vec::new();
            for line in data
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                let field = line.split(|byte| *byte == b' ').nth(4).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid mountinfo")
                })?;
                paths.push(PathBuf::from(std::ffi::OsString::from_vec(unescape(field))));
            }
            Ok(Self(paths))
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(Self::default())
        }
    }

    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.0.iter().any(|mount| mount == path)
    }
}

#[cfg(target_os = "linux")]
fn unescape(value: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < value.len() {
        if value[i] == b'\\'
            && i + 3 < value.len()
            && value[i + 1..i + 4]
                .iter()
                .all(|c| (b'0'..=b'7').contains(c))
        {
            result.push(
                ((value[i + 1] - b'0') * 64)
                    .wrapping_add((value[i + 2] - b'0') * 8)
                    .wrapping_add(value[i + 3] - b'0'),
            );
            i += 4;
        } else {
            result.push(value[i]);
            i += 1;
        }
    }
    result
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn mount_escapes_preserve_paths() {
        assert_eq!(super::unescape(br"/a\040b\134c"), b"/a b\\c");
        assert!(
            super::MountPoints::read()
                .expect("mountinfo")
                .contains(std::path::Path::new("/"))
        );
    }
}
