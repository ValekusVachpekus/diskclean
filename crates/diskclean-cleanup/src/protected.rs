//! Protected-path policy, evaluated before anything enters the trash.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use crate::discovery::CleanupContext;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtectedPathError {
    #[error("path is not absolute: {path}")]
    NotAbsolute { path: PathBuf },
    #[error("path is protected and cannot be cleaned: {path}")]
    Protected { path: PathBuf },
}

/// Two kinds of roots:
///
/// - recursive roots protect themselves and every descendant (system and
///   sensitive directories);
/// - exact roots protect only the path itself, plus ancestors that could
///   remove it. This is how a user profile root is protected without making
///   `~/.cache` untouchable.
#[derive(Debug, Clone)]
pub struct ProtectedPaths {
    recursive: Vec<PathBuf>,
    exact: Vec<PathBuf>,
}

impl ProtectedPaths {
    /// Build the policy for the current process environment.
    #[must_use]
    pub fn from_environment() -> Self {
        Self::for_context(&CleanupContext::from_environment(), [])
    }

    /// Build the policy from a resolved context and optional extra roots
    /// (for example the application's audit directory).
    #[must_use]
    pub fn for_context(
        context: &CleanupContext,
        extra_recursive: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        let mut recursive = Vec::new();
        let mut exact = Vec::new();

        for path in platform_roots() {
            recursive.push(path);
        }

        if let Some(home) = &context.home {
            exact.push(home.clone());
            for relative in [
                ".ssh",
                ".gnupg",
                ".pki",
                ".local/share/keyrings",
                ".config/autostart",
            ] {
                recursive.push(home.join(relative));
            }
        }

        recursive.extend(extra_recursive);

        Self {
            recursive: dedupe_normalized(recursive),
            exact: dedupe_normalized(exact),
        }
    }

    /// Check a path. This rejects protected roots, their descendants, and any
    /// ancestor directory that contains a protected root.
    pub fn check(&self, path: &Path) -> Result<(), ProtectedPathError> {
        let path = normalize(path);
        if !path.is_absolute() {
            return Err(ProtectedPathError::NotAbsolute { path });
        }

        if is_root(&path) {
            return Err(ProtectedPathError::Protected { path });
        }
        #[cfg(windows)]
        if path.components().any(|component| match component {
            Component::Normal(name) => ["System32", "WinSxS", "WindowsApps"]
                .iter()
                .any(|protected| name.to_string_lossy().eq_ignore_ascii_case(protected)),
            _ => false,
        }) {
            return Err(ProtectedPathError::Protected { path });
        }

        for root in &self.recursive {
            if is_same_or_descendant(&path, root) {
                return Err(ProtectedPathError::Protected { path });
            }
            if is_same_or_descendant(root, &path) {
                return Err(ProtectedPathError::Protected { path });
            }
        }
        for root in &self.exact {
            if same_path(&path, root) || is_same_or_descendant(root, &path) {
                return Err(ProtectedPathError::Protected { path });
            }
        }

        Ok(())
    }

    #[must_use]
    pub fn is_protected(&self, path: &Path) -> bool {
        self.check(path).is_err()
    }
}

fn dedupe_normalized(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = Vec::new();
    for path in paths {
        let path = normalize(&path);
        if path.as_os_str().is_empty() {
            continue;
        }
        if !result.iter().any(|existing| same_path(existing, &path)) {
            result.push(path);
        }
    }
    result
}

#[cfg(unix)]
fn platform_roots() -> Vec<PathBuf> {
    [
        "/boot", "/etc", "/usr", "/var/lib", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/proc",
        "/sys", "/dev", "/run",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

#[cfg(windows)]
fn platform_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = [
        r"C:\Windows",
        r"C:\Program Files",
        r"C:\Program Files (x86)",
        r"C:\ProgramData",
        r"C:\Windows\System32",
        r"C:\Windows\WinSxS",
        r"C:\Program Files\WindowsApps",
        r"C:\Program Files (x86)\WindowsApps",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    roots.extend(diskclean_platform::user_dirs::system_directories());
    for variable in [
        "SystemRoot",
        "WINDIR",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "ProgramData",
    ] {
        if let Some(path) = std::env::var_os(variable).map(PathBuf::from)
            && path.is_absolute()
        {
            roots.push(path);
        }
    }
    roots
}

#[cfg(not(any(unix, windows)))]
fn platform_roots() -> Vec<PathBuf> {
    Vec::new()
}

fn normalize(path: &Path) -> PathBuf {
    #[cfg(windows)]
    let path = strip_verbatim(path);
    #[cfg(not(windows))]
    let path = path.to_path_buf();

    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    result.push("..");
                }
            }
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(Component::RootDir.as_os_str()),
            Component::Normal(part) => result.push(part),
        }
    }
    result
}

#[cfg(windows)]
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn is_root(path: &Path) -> bool {
    path.has_root()
        && !path
            .components()
            .any(|component| matches!(component, Component::Normal(_) | Component::ParentDir))
}

fn path_key(path: &Path) -> String {
    if cfg!(windows) {
        path.to_string_lossy().replace('/', "\\").to_lowercase()
    } else {
        path.to_string_lossy().into_owned()
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    if cfg!(windows) {
        path_key(left) == path_key(right)
    } else {
        left == right
    }
}

/// `path` equals `root` or is below it. Components are compared as text with
/// a separator check, so `/etc2` is not treated as a child of `/etc`.
fn is_same_or_descendant(path: &Path, root: &Path) -> bool {
    if !cfg!(windows) {
        return path.starts_with(root);
    }
    let path = path_key(path);
    let root = path_key(root);
    if path == root {
        return true;
    }
    let separator = if cfg!(windows) { '\\' } else { '/' };
    if root.ends_with(separator) {
        return path.starts_with(&root);
    }
    path.strip_prefix(&root)
        .is_some_and(|rest| rest.starts_with(separator))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(home: &Path) -> CleanupContext {
        CleanupContext {
            home: Some(home.to_path_buf()),
            ..CleanupContext::default()
        }
    }

    #[test]
    fn protects_system_paths_and_the_profile_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let policy = ProtectedPaths::for_context(&context(&home), []);

        #[cfg(unix)]
        {
            assert!(policy.is_protected(Path::new("/etc")));
            assert!(policy.is_protected(Path::new("/etc/passwd")));
            assert!(policy.is_protected(Path::new("/")));
        }

        #[cfg(windows)]
        {
            assert!(policy.is_protected(Path::new(r"C:\Windows")));
            assert!(policy.is_protected(Path::new(r"C:\Windows\System32\drivers")));
            assert!(policy.is_protected(Path::new(r"C:\")));
        }

        assert!(policy.is_protected(&home));
        assert!(policy.is_protected(temp.path()), "ancestor of home");
        assert!(policy.is_protected(&home.join(".ssh/id_ed25519")));
    }

    #[test]
    fn cache_below_the_profile_root_is_allowed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let policy = ProtectedPaths::for_context(&context(&home), []);
        assert!(!policy.is_protected(&home.join(".cache/pip")));
        assert!(!policy.is_protected(&home.join("Downloads/old.iso")));
    }

    #[test]
    fn dot_dot_is_resolved_before_the_check() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let policy = ProtectedPaths::for_context(&context(&home), []);
        let path = home.join("Downloads/../.ssh/id_ed25519");
        let error = policy
            .check(&path)
            .expect_err("normalized path is protected");
        assert!(matches!(error, ProtectedPathError::Protected { .. }));
    }

    #[test]
    fn relative_paths_are_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let policy = ProtectedPaths::for_context(&context(&home), []);
        assert!(matches!(
            policy.check(Path::new("Downloads/file")),
            Err(ProtectedPathError::NotAbsolute { .. })
        ));
    }

    #[test]
    fn sibling_with_a_common_prefix_is_not_protected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let policy = ProtectedPaths::for_context(&context(&home), []);

        assert!(!policy.is_protected(&temp.path().join("home2")));

        #[cfg(unix)]
        {
            assert!(!policy.is_protected(Path::new("/etc2")));
            assert!(!policy.is_protected(Path::new("/usr-local")));
        }

        #[cfg(windows)]
        {
            assert!(!policy.is_protected(Path::new(r"C:\Windows2")));
            assert!(!policy.is_protected(Path::new(r"C:\Windows.old\Temp")));
            assert!(!policy.is_protected(Path::new(r"C:\ProgramData2\file.txt")));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_are_case_insensitive_and_verbatim_safe() {
        let home = PathBuf::from(r"C:\Users\test");
        let policy = ProtectedPaths::for_context(&context(&home), []);
        assert!(policy.is_protected(Path::new(r"c:\windows\system32")));
        assert!(policy.is_protected(Path::new(r"\\?\C:\Windows\Temp")));
        assert!(!policy.is_protected(Path::new(r"C:\Users\test\AppData\Local\Temp")));
        assert!(policy.is_protected(Path::new(r"D:\")));
        assert!(policy.is_protected(Path::new(r"\\server\share\")));
        assert!(policy.is_protected(Path::new(r"\\?\D:\")));
    }
}
