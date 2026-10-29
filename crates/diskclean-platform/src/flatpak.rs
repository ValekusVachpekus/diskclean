//! Offline deployment metadata. Active links are read as text, never traversed.
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

pub const MAX_METADATA_BYTES: u64 = 1024 * 1024;

pub fn default_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let absolute_env = |key| {
            std::env::var_os(key)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        let mut roots =
            vec![absolute_env("FLATPAK_SYSTEM_DIR").unwrap_or_else(|| "/var/lib/flatpak".into())];
        if let Some(user) = absolute_env("FLATPAK_USER_DIR").or_else(|| {
            absolute_env("XDG_DATA_HOME")
                .or_else(|| crate::home_dir().map(|home| home.join(".local/share")))
                .map(|data| data.join("flatpak"))
        }) {
            roots.push(user);
        }
        roots
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Flatpak deployment metadata",
    )
}

/// A checksum with an optional subpath suffix (for example a locale checkout).
/// Restrict this to one filename, including on foreign-platform fixtures.
pub fn valid_deployment(value: &str) -> bool {
    value.len() >= 64
        && value.len() <= 255
        && value.as_bytes()[..64].iter().all(u8::is_ascii_hexdigit)
        && (value.len() == 64
            || (value.as_bytes()[64] == b'-'
                && value.len() > 65
                && value.as_bytes()[65..]
                    .iter()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c))))
}

pub fn read_active(branch: &Path) -> io::Result<String> {
    crate::check_path_components(branch)?;
    let path = branch.join("active");
    let before = crate::metadata(&path)?;
    if before.kind != crate::FileKind::Symlink {
        return Err(invalid());
    }
    let target = std::fs::read_link(&path)?;
    let name = target
        .to_str()
        .filter(|name| valid_deployment(name))
        .ok_or_else(invalid)?;
    let after = crate::metadata(&path)?;
    if (
        before.kind,
        before.dev,
        before.ino,
        before.modified,
        before.logical_size,
    ) != (
        after.kind,
        after.dev,
        after.ino,
        after.modified,
        after.logical_size,
    ) {
        return Err(invalid());
    }
    Ok(name.to_owned())
}

/// Read just the identity group. Other groups (permissions, commands, etc.) are
/// ignored; they never become filesystem paths or executable commands.
pub fn read_name(path: &Path, runtime: bool, cancelled: impl Fn() -> bool) -> io::Result<String> {
    let text = crate::packages::read_text(path, MAX_METADATA_BYTES, &cancelled)?;
    let expected = if runtime { "Runtime" } else { "Application" };
    let mut section = "";
    let mut groups = HashSet::new();
    let mut keys = HashSet::new();
    let mut name = None;
    for line in text.lines() {
        if cancelled() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if line.len() > 64 * 1024 || line.contains('\0') {
            return Err(invalid());
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(group) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if group.is_empty() || !groups.insert(group) {
                return Err(invalid());
            }
            section = group;
            keys.clear();
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(invalid)?;
        let key = key.trim();
        if section.is_empty() || key.is_empty() || !keys.insert(key) {
            return Err(invalid());
        }
        if section == expected && key == "name" {
            let value = value.trim();
            if value.len() > 255 || !valid_component(value) {
                return Err(invalid());
            }
            name = Some(value.to_owned());
        }
    }
    // A wrong-kind identity is not evidence of an installed application.
    if groups.contains(if runtime { "Application" } else { "Runtime" }) {
        return Err(invalid());
    }
    name.ok_or_else(invalid)
}

pub fn valid_component(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= 255
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_identity_reader_rejects_ambiguous_metadata() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("metadata");
        for (text, runtime, valid) in [
            (
                "[Application]\nname=org.example.App\nruntime=org.example.Platform/x86_64/stable\n[Context]\nshared=network;ipc;\n",
                false,
                true,
            ),
            (
                "[Runtime]\nname = org.example.Platform.Locale\n[ExtensionOf]\nref=runtime/org.example.Platform/x86_64/1\n",
                true,
                true,
            ),
            ("[Application]\nname=../../escape\n", false, false),
            (
                "[Application]\nname=org.example.App\nname=other\n",
                false,
                false,
            ),
            (
                "[Application]\nname=org.example.App\n[Application]\nname=other\n",
                false,
                false,
            ),
            ("[Runtime]\nname=org.example.Platform\n", false, false),
            ("[Application]\nname=org.example.App\0\n", false, false),
        ] {
            std::fs::write(&path, text).expect("write");
            assert_eq!(read_name(&path, runtime, || false).is_ok(), valid, "{text}");
        }
        assert_eq!(
            read_name(&path, false, || true).expect_err("cancel").kind(),
            io::ErrorKind::Interrupted
        );
        std::fs::write(&path, [255]).expect("utf8");
        assert!(read_name(&path, false, || false).is_err());
        std::fs::File::create(&path)
            .expect("file")
            .set_len(MAX_METADATA_BYTES + 1)
            .expect("length");
        assert!(read_name(&path, false, || false).is_err());
    }
    #[test]
    fn deployment_names_cannot_escape_branch() {
        let hash = "a".repeat(64);
        for value in [&hash, &format!("{hash}-ru"), &format!("{hash}-en_US")] {
            assert!(valid_deployment(value));
        }
        for value in [
            "../escape",
            "/absolute",
            "C:\\file",
            &format!("{hash}/files"),
            &format!("{hash}-"),
            &format!("{hash}-../escape"),
            "short",
        ] {
            assert!(!valid_deployment(value));
        }
    }
    #[cfg(unix)]
    #[test]
    fn active_link_is_text_evidence_and_metadata_links_are_rejected() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().expect("temp");
        let target = format!("{}-ru", "b".repeat(64));
        // No target exists: read_active does not traverse the link.
        symlink(&target, temp.path().join("active")).expect("link");
        assert_eq!(read_active(temp.path()).expect("active"), target);
        std::fs::write(
            temp.path().join("real"),
            "[Runtime]\nname=org.example.Runtime\n",
        )
        .expect("write");
        symlink("real", temp.path().join("metadata")).expect("link");
        assert!(read_name(&temp.path().join("metadata"), true, || false).is_err());
        symlink(temp.path(), temp.path().join("alias")).expect("alias");
        assert!(read_active(&temp.path().join("alias")).is_err());
    }
}
