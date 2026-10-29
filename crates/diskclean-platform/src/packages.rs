//! Bounded local package metadata readers. Never execute package managers or scripts.
use std::{
    collections::{BTreeMap, HashSet},
    io::{self, Read},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RECORDS: usize = 20_000;
const MAX_LINE: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Dpkg,
    Pacman,
}
impl PackageManager {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dpkg => "dpkg",
            Self::Pacman => "pacman",
        }
    }
    pub fn default_roots(self) -> Vec<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            vec![PathBuf::from(match self {
                Self::Dpkg => "/var/lib/dpkg",
                Self::Pacman => "/var/lib/pacman/local",
            })]
        }
        #[cfg(not(target_os = "linux"))]
        {
            Vec::new()
        }
    }
}
#[derive(Debug, Clone)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Package metadata estimate, not allocated or reclaimable bytes.
    pub reported_size_bytes: Option<u64>,
}
#[derive(Debug, Default)]
pub struct PackageInventory {
    pub packages: Vec<Package>,
    pub invalid_records: usize,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid or excessive package metadata",
    )
}
fn cancelled(check: &impl Fn() -> bool) -> io::Result<()> {
    if check() {
        Err(io::ErrorKind::Interrupted.into())
    } else {
        Ok(())
    }
}
/// No-follow reads validate opened identity, size and precise mtime before/after.
/// The caller controls mount policy. Not a transaction across a package database.
fn read_text(path: &Path, limit: u64, check: &impl Fn() -> bool) -> io::Result<String> {
    cancelled(check)?;
    let before = crate::metadata(path)?;
    if before.kind != crate::FileKind::File || before.logical_size > limit {
        return Err(invalid());
    }
    let mut file = crate::open_regular_file(path)?;
    if crate::opened_identity(&file)? != (before.dev, before.ino) {
        return Err(invalid());
    }
    let mut data = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        cancelled(check)?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if data.len() as u64 + count as u64 > limit {
            return Err(invalid());
        }
        data.extend_from_slice(&buffer[..count]);
    }
    let after = crate::metadata(path)?;
    let opened = file.metadata()?;
    if (before.dev, before.ino, before.logical_size, before.modified)
        != (after.dev, after.ino, after.logical_size, after.modified)
        || data.len() as u64 != before.logical_size
        || opened.len() != before.logical_size
        || opened.modified().ok() != before.modified
    {
        return Err(invalid());
    }
    String::from_utf8(data).map_err(|_| invalid())
}
fn decimal(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        None
    } else {
        s.parse().ok()
    }
}
fn identity(s: &str, debian: bool) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+._-".contains(&b))
        && (!debian || (s.len() >= 2 && !s.bytes().any(|b| b.is_ascii_uppercase() || b == b'_')))
}
fn package(
    fields: &BTreeMap<String, String>,
    manager: PackageManager,
) -> io::Result<Option<Package>> {
    let (name, version, arch, size, factor) = match manager {
        PackageManager::Dpkg => {
            let status = fields.get("status").ok_or_else(invalid)?;
            let status: Vec<_> = status.split_whitespace().collect();
            if status.len() != 3 {
                return Err(invalid());
            }
            if status[1] != "ok" || status[2] != "installed" {
                return Ok(None);
            }
            ("package", "version", "architecture", "installed-size", 1024)
        }
        PackageManager::Pacman => (
            "NAME",
            "VERSION",
            "ARCH",
            if fields.contains_key("SIZE") {
                "SIZE"
            } else {
                "ISIZE"
            },
            1,
        ),
    };
    let name = fields
        .get(name)
        .filter(|s| identity(s, manager == PackageManager::Dpkg))
        .ok_or_else(invalid)?
        .clone();
    let version = fields
        .get(version)
        .filter(|s| !s.is_empty() && s.len() <= 1024 && !s.contains(char::is_whitespace))
        .ok_or_else(invalid)?
        .clone();
    let arch = fields
        .get(arch)
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
        .ok_or_else(invalid)?;
    let reported_size_bytes = fields
        .get(size)
        .and_then(|s| decimal(s))
        .and_then(|n| n.checked_mul(factor));
    Ok(Some(Package {
        id: format!("{name}:{arch}"),
        name,
        version,
        reported_size_bytes,
    }))
}

pub fn read_dpkg(path: &Path, check: impl Fn() -> bool) -> io::Result<PackageInventory> {
    let text = read_text(path, MAX_BYTES, &check)?;
    let mut inventory = PackageInventory::default();
    let mut fields = BTreeMap::new();
    let mut bad = false;
    let mut previous: Option<String> = None;
    let mut seen = HashSet::new();
    let mut records = 0;
    // Empty sentinel also flushes the final stanza without a newline.
    for line in text.lines().chain(Some("")) {
        cancelled(&check)?;
        if line.is_empty() {
            if fields.is_empty() && !bad {
                previous = None;
                continue;
            }
            records += 1;
            if records > MAX_RECORDS {
                return Err(invalid());
            }
            match if bad {
                Err(invalid())
            } else {
                package(&fields, PackageManager::Dpkg)
            } {
                Ok(Some(pkg)) if seen.insert(pkg.id.clone()) => {
                    if fields.contains_key("installed-size") && pkg.reported_size_bytes.is_none() {
                        inventory.invalid_records += 1;
                    }
                    inventory.packages.push(pkg);
                }
                Ok(None) => {}
                _ => inventory.invalid_records += 1,
            }
            fields.clear();
            bad = false;
            previous = None;
            continue;
        }
        if line.len() > MAX_LINE || line.contains('\0') {
            bad = true;
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if previous.as_deref().is_none_or(|key| {
                matches!(
                    key,
                    "package" | "status" | "version" | "architecture" | "installed-size"
                )
            }) {
                bad = true;
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            bad = true;
            continue;
        };
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            bad = true;
            continue;
        }
        previous = Some(key.to_ascii_lowercase());
        if fields
            .insert(key.to_ascii_lowercase(), value.trim().into())
            .is_some()
            || fields.len() > 256
        {
            bad = true;
        }
    }
    Ok(inventory)
}

/// One desc file from the local installed database (not a sync/repository DB).
/// SIZE is the current format; legacy pacman local databases used ISIZE.
pub fn read_pacman(path: &Path, check: impl Fn() -> bool) -> io::Result<PackageInventory> {
    let text = read_text(path, 1024 * 1024, &check)?;
    let mut fields = BTreeMap::new();
    let mut section: Option<String> = None;
    for line in text.lines() {
        cancelled(&check)?;
        if line.len() > MAX_LINE || line.contains('\0') {
            return Err(invalid());
        }
        if line.is_empty() {
            section = None;
            continue;
        }
        if line.starts_with('%') && line.ends_with('%') {
            let key = line
                .strip_prefix('%')
                .and_then(|s| s.strip_suffix('%'))
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
                .ok_or_else(invalid)?;
            if fields.insert(key.into(), String::new()).is_some() || fields.len() > 256 {
                return Err(invalid());
            }
            section = Some(key.into());
        } else if let Some(key) = &section {
            let value = fields.get_mut(key).ok_or_else(invalid)?;
            if !value.is_empty() {
                if ["NAME", "VERSION", "ARCH", "SIZE", "ISIZE"].contains(&key.as_str()) {
                    return Err(invalid());
                }
                continue; // Other sections may have multiple dependency/license values.
            }
            *value = line.into();
        } else {
            return Err(invalid());
        }
    }
    if fields.contains_key("SIZE")
        && fields.contains_key("ISIZE")
        && fields["SIZE"] != fields["ISIZE"]
    {
        return Err(invalid());
    }
    // Repository desc files are not evidence of an installed package.
    if fields.contains_key("FILENAME") || fields.contains_key("CSIZE") {
        return Err(invalid());
    }
    let mut inventory = PackageInventory::default();
    if let Some(pkg) = package(&fields, PackageManager::Pacman)? {
        if (fields.contains_key("SIZE") || fields.contains_key("ISIZE"))
            && pkg.reported_size_bytes.is_none()
        {
            inventory.invalid_records += 1;
        }
        inventory.packages.push(pkg);
    }
    Ok(inventory)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse_deb(text: &str) -> PackageInventory {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("status");
        std::fs::write(&path, text).expect("write");
        read_dpkg(&path, || false).expect("parse")
    }
    #[test]
    fn dpkg_installed_states_multiarch_continuations_and_reported_sizes() {
        let one = "Package: example\nStatus: hold ok installed\nVersion: 1:2.3-4\nArchitecture: amd64\nInstalled-Size: 12\nDescription: readable\n long detail\n";
        let text = format!(
            "{one}\n{}\nPackage: removed\nStatus: deinstall ok config-files\n\nPackage: broken\nStatus: install reinstreq installed\n",
            one.replace("amd64", "arm64")
        );
        let report = parse_deb(&text);
        assert_eq!(report.packages.len(), 2);
        assert_eq!(report.invalid_records, 0);
        assert_eq!(report.packages[0].reported_size_bytes, Some(12288));
        assert_eq!(report.packages[1].id, "example:arm64");
    }
    #[test]
    fn dpkg_invalid_neighbours_duplicates_size_overflow_and_missing_size() {
        let one = "Package: example\nStatus: install ok installed\nVersion: 1\nArchitecture: all\n";
        let report = parse_deb(&format!(
            "{one}\n{one}\n{one}Package: collision\n\n{}Installed-Size: 18446744073709551615\n\n{}",
            one.replace("example", "overflow"),
            one.replace("example", "no-size")
        ));
        assert_eq!(report.packages.len(), 3);
        assert_eq!(report.invalid_records, 3);
        assert!(
            report
                .packages
                .iter()
                .all(|p| p.reported_size_bytes.is_none())
        );
    }
    #[test]
    fn pacman_current_legacy_sections_and_invalid_values() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("desc");
        let base =
            "%NAME%\nexample\n\n%VERSION%\n1.2-3\n\n%ARCH%\nx86_64\n\n%DEPENDS%\none\ntwo\n\n";
        for key in ["SIZE", "ISIZE"] {
            std::fs::write(&path, format!("{base}%{key}%\n123\n")).expect("write");
            let r = read_pacman(&path, || false).expect("parse");
            assert_eq!(r.packages[0].reported_size_bytes, Some(123));
        }
        for tail in [
            "%NAME%\nrepeat\n",
            "%FILENAME%\nexample.pkg.tar.zst\n",
            "%SIZE%\n1\n2\n",
            "%SIZE%\n1\n\n%ISIZE%\n2\n",
        ] {
            std::fs::write(&path, format!("{base}{tail}")).expect("write");
            assert!(read_pacman(&path, || false).is_err());
        }
        std::fs::write(&path, format!("{base}%SIZE%\n-1\n")).expect("write");
        let r = read_pacman(&path, || false).expect("parse");
        assert_eq!(r.invalid_records, 1);
        assert_eq!(r.packages[0].reported_size_bytes, None);
    }
    #[test]
    fn readers_cancel_reject_non_utf8_oversize_and_source_changes() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("status");
        std::fs::write(&path, b"data").expect("write");
        assert_eq!(
            read_dpkg(&path, || true).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        std::fs::write(
            &path,
            "Package: removed\nStatus: deinstall ok config-files\n\n".repeat(MAX_RECORDS + 1),
        )
        .expect("write");
        assert!(read_dpkg(&path, || false).is_err());
        std::fs::write(&path, format!("Description: {}\n", "x".repeat(MAX_LINE))).expect("write");
        assert_eq!(
            read_dpkg(&path, || false)
                .expect("invalid row")
                .invalid_records,
            1
        );
        std::fs::write(&path, [0xff]).expect("write");
        assert!(read_dpkg(&path, || false).is_err());
        let file = std::fs::File::create(&path).expect("file");
        file.set_len(MAX_BYTES + 1).expect("len");
        assert!(read_dpkg(&path, || false).is_err());
        drop(file);
        std::fs::write(&path, b"data").expect("write");
        let count = std::cell::Cell::new(0);
        assert!(
            read_dpkg(&path, || {
                count.set(count.get() + 1);
                if count.get() == 2 {
                    std::fs::write(&path, b"changed size").expect("change");
                }
                false
            })
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn linked_package_metadata_and_ancestors_are_rejected() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("status");
        std::fs::write(&path, b"data").expect("write");
        let link = temp.path().join("alias");
        std::os::unix::fs::symlink(&path, &link).expect("link");
        assert!(read_dpkg(&link, || false).is_err());
        let parent = temp.path().join("parent");
        std::os::unix::fs::symlink(temp.path(), &parent).expect("parent");
        assert!(read_pacman(&parent.join("status"), || false).is_err());
    }
}
