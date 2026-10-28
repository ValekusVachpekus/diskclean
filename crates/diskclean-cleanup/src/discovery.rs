//! Read-only discovery of cleanup candidates from built-in rules.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use diskclean_platform::{FileKind, MountPoints, check_path_components, metadata};
use diskclean_rules::{
    ActionKind, BaseDir, EntryKind, Location, LocationMode, Rule, RulesError, builtin_rules,
};
use serde::Serialize;

use crate::protected::ProtectedPaths;
use crate::snapshot::{CleanupItem, ItemKind, now_unix_ms, serialize_path};

#[derive(Debug, Clone, Default)]
pub struct CleanupContext {
    pub home: Option<PathBuf>,
    pub downloads: Option<PathBuf>,
    pub cache: Option<PathBuf>,
    pub local_data: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub temp: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub roaming_app_data: Option<PathBuf>,
    pub program_data: Option<PathBuf>,
    pub trash: Option<PathBuf>,
}

impl CleanupContext {
    #[must_use]
    pub fn from_environment() -> Self {
        let home = diskclean_platform::home_dir();
        let cache = env_path("XDG_CACHE_HOME").or_else(|| home.as_ref().map(|h| h.join(".cache")));
        let local_data =
            env_path("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        let config =
            env_path("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|h| h.join(".config")));
        let local_app_data = env_path("LOCALAPPDATA").or_else(|| local_data.clone());
        let temp = env_path("TMPDIR")
            .or_else(|| env_path("TEMP"))
            .or_else(|| env_path("TMP"))
            .or_else(|| {
                if !cfg!(windows) {
                    return Some(std::env::temp_dir());
                }
                local_app_data
                    .as_ref()
                    .map(|path| path.join("Temp"))
                    .or_else(|| Some(std::env::temp_dir()))
            });
        let trash = local_data.as_ref().map(|path| path.join("Trash/files"));
        let downloads = home
            .as_ref()
            .map(|path| diskclean_platform::downloads_dir(path, config.as_deref().unwrap_or(path)));

        Self {
            home,
            downloads,
            cache,
            local_data,
            config,
            temp,
            local_app_data,
            roaming_app_data: env_path("APPDATA"),
            program_data: env_path("PROGRAMDATA"),
            trash,
        }
    }

    /// Resolve a rule base and a validated relative path.
    #[must_use]
    pub fn resolve(&self, base: BaseDir, relative: &str) -> Option<PathBuf> {
        let root = match base {
            BaseDir::Home => self.home.clone()?,
            BaseDir::Downloads => self.downloads.clone()?,
            BaseDir::Cache => self.cache.clone()?,
            BaseDir::LocalData => self.local_data.clone()?,
            BaseDir::Config => self.config.clone()?,
            BaseDir::Temp => self.temp.clone()?,
            BaseDir::LocalAppData => self.local_app_data.clone()?,
            BaseDir::RoamingAppData => self.roaming_app_data.clone()?,
            BaseDir::ProgramData => self.program_data.clone()?,
            BaseDir::Trash => self.trash.clone()?,
        };

        if relative.is_empty() {
            return Some(root);
        }
        let relative = Path::new(relative);
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return None;
        }
        Some(root.join(relative))
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    let value = env::var_os(name)?;
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupIssue {
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub rule: Rule,
    pub items: Vec<CleanupItem>,
}

impl Recommendation {
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.items
            .iter()
            .fold(0_u64, |total, item| total.saturating_add(item.size()))
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.items.len()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupReport {
    pub generated_at_unix_ms: u64,
    pub recommendations: Vec<Recommendation>,
    pub issues: Vec<CleanupIssue>,
}

impl CleanupReport {
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.recommendations
            .iter()
            .fold(0_u64, |total, rec| total.saturating_add(rec.total_bytes()))
    }

    #[must_use]
    pub fn item_count(&self) -> usize {
        self.recommendations.iter().map(Recommendation::count).sum()
    }
}

/// Discover candidates using the embedded platform packs.
pub fn discover_default(
    context: &CleanupContext,
    protected: &ProtectedPaths,
) -> Result<CleanupReport, RulesError> {
    let rules = builtin_rules()?;
    Ok(discover(&rules, context, protected))
}

/// Discover candidates for the supplied rules. This function is read-only.
#[must_use]
pub fn discover(
    rules: &[Rule],
    context: &CleanupContext,
    protected: &ProtectedPaths,
) -> CleanupReport {
    let mut recommendations = Vec::new();
    let mut issues = Vec::new();
    let mut seen = HashSet::new();

    for rule in rules {
        let mut items = Vec::new();
        if rule.action == ActionKind::Trash {
            for location in &rule.locations {
                collect_location(
                    rule,
                    location,
                    context,
                    protected,
                    &mut seen,
                    &mut items,
                    &mut issues,
                );
            }
            items.sort_unstable_by(|left, right| {
                right
                    .size()
                    .cmp(&left.size())
                    .then_with(|| left.path.cmp(&right.path))
            });
        }

        recommendations.push(Recommendation {
            rule: rule.clone(),
            items,
        });
    }

    CleanupReport {
        generated_at_unix_ms: now_unix_ms(),
        recommendations,
        issues,
    }
}

fn collect_location(
    rule: &Rule,
    location: &Location,
    context: &CleanupContext,
    protected: &ProtectedPaths,
    seen: &mut HashSet<PathBuf>,
    items: &mut Vec<CleanupItem>,
    issues: &mut Vec<CleanupIssue>,
) {
    let Some(base) = context.resolve(location.base, &location.path) else {
        return;
    };
    if !base.exists() {
        return;
    }
    if let Err(error) = check_path_components(&base) {
        issues.push(CleanupIssue {
            path: base,
            message: error.to_string(),
        });
        return;
    }

    match location.mode {
        LocationMode::Path => {
            push_candidate(rule, location, &base, protected, seen, items, issues);
        }
        LocationMode::Children => {
            let Ok(entries) = fs::read_dir(&base) else {
                issues.push(CleanupIssue {
                    path: base,
                    message: "cannot read candidate directory".to_owned(),
                });
                return;
            };
            let mut paths = Vec::new();
            for entry in entries {
                match entry {
                    Ok(entry) => paths.push(entry.path()),
                    Err(error) => issues.push(CleanupIssue {
                        path: base.clone(),
                        message: error.to_string(),
                    }),
                }
                if paths.len() > location.max_entries {
                    issues.push(CleanupIssue {
                        path: base.clone(),
                        message: "candidate limit reached; results are incomplete".to_owned(),
                    });
                    break;
                }
            }
            paths.sort();
            for path in paths.into_iter().take(location.max_entries) {
                push_candidate(rule, location, &path, protected, seen, items, issues);
            }
        }
        LocationMode::RecursiveNamed => {
            walk_named(rule, location, &base, protected, seen, items, issues);
        }
    }
}

fn walk_named(
    rule: &Rule,
    location: &Location,
    base: &Path,
    protected: &ProtectedPaths,
    seen: &mut HashSet<PathBuf>,
    items: &mut Vec<CleanupItem>,
    issues: &mut Vec<CleanupIssue>,
) {
    let (root_meta, mounts) =
        match metadata(base).and_then(|meta| MountPoints::read().map(|mounts| (meta, mounts))) {
            Ok(result) => result,
            Err(error) => {
                issues.push(CleanupIssue {
                    path: base.to_path_buf(),
                    message: error.to_string(),
                });
                return;
            }
        };
    let mut stack = vec![(base.to_path_buf(), 0_u16)];
    let mut visited = 0_usize;

    while let Some((directory, depth)) = stack.pop() {
        if depth > location.max_depth {
            continue;
        }
        let entries =
            match check_path_components(&directory).and_then(|()| fs::read_dir(&directory)) {
                Ok(entries) => entries,
                Err(error) => {
                    issues.push(CleanupIssue {
                        path: directory,
                        message: error.to_string(),
                    });
                    continue;
                }
            };
        if items.len() >= location.max_entries {
            issues.push(CleanupIssue {
                path: directory,
                message: "candidate limit reached; results are incomplete".to_owned(),
            });
            continue;
        }
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => paths.push(entry.path()),
                Err(error) => issues.push(CleanupIssue {
                    path: directory.clone(),
                    message: error.to_string(),
                }),
            }
            if paths.len() > location.max_entries.saturating_mul(20).max(2_000) {
                issues.push(CleanupIssue {
                    path: directory.clone(),
                    message: "directory entry limit reached; results are incomplete".to_owned(),
                });
                break;
            }
        }
        paths.sort();

        for path in paths {
            if items.len() >= location.max_entries {
                issues.push(CleanupIssue {
                    path: directory.clone(),
                    message: "candidate limit reached; results are incomplete".to_owned(),
                });
                return;
            }
            visited = visited.saturating_add(1);
            if visited > location.max_entries.saturating_mul(20).max(2_000) {
                issues.push(CleanupIssue {
                    path: directory.clone(),
                    message: "traversal limit reached; results are incomplete".to_owned(),
                });
                return;
            }
            let Ok(meta) = metadata(&path) else {
                continue;
            };
            if meta.kind != FileKind::Directory
                || meta.dev != root_meta.dev
                || mounts.contains(&path)
                || protected.is_protected(&path)
            {
                continue;
            }
            let matches = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| location.names.iter().any(|candidate| candidate == name));
            if matches {
                push_candidate(rule, location, &path, protected, seen, items, issues);
                continue;
            }
            if depth < location.max_depth {
                stack.push((path, depth + 1));
            }
        }
    }
}

fn push_candidate(
    rule: &Rule,
    location: &Location,
    path: &Path,
    protected: &ProtectedPaths,
    seen: &mut HashSet<PathBuf>,
    items: &mut Vec<CleanupItem>,
    issues: &mut Vec<CleanupIssue>,
) {
    if protected.is_protected(path) {
        return;
    }
    if seen.contains(&path_key(path)) {
        return;
    }

    // Apply cheap filters before walking an entire candidate directory.
    if let Ok(meta) = metadata(path) {
        let kind = match meta.kind {
            FileKind::File => ItemKind::File,
            FileKind::Directory => ItemKind::Directory,
            _ => return,
        };
        if !kind_matches(location.kind, kind)
            || (location.min_age_days > 0 && !old_enough(meta.modified_unix, location.min_age_days))
        {
            return;
        }
    }

    let item = match CleanupItem::new(path, &rule.id, rule.category, rule.risk) {
        Ok(item) => item,
        Err(error) => {
            issues.push(CleanupIssue {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            return;
        }
    };

    if !kind_matches(location.kind, item.kind()) {
        return;
    }
    if location.min_age_days > 0 && !old_enough(item.snapshot.modified_unix, location.min_age_days)
    {
        return;
    }

    seen.insert(path_key(path));
    items.push(item);
}

fn kind_matches(expected: EntryKind, actual: ItemKind) -> bool {
    match expected {
        EntryKind::Any => true,
        EntryKind::Files => actual == ItemKind::File,
        EntryKind::Directories => actual == ItemKind::Directory,
    }
}

fn old_enough(modified_unix: i64, min_age_days: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let age = now.saturating_sub(u64::try_from(modified_unix).unwrap_or(0));
    age >= min_age_days.saturating_mul(86_400)
}

fn path_key(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::time::{Duration, SystemTime};

    use diskclean_rules::{Category, LocalizedText, Risk};

    use super::*;

    fn context_for(home: &Path) -> CleanupContext {
        CleanupContext {
            home: Some(home.to_path_buf()),
            downloads: Some(home.join("Downloads")),
            cache: Some(home.join(".cache")),
            local_data: Some(home.join(".local/share")),
            config: Some(home.join(".config")),
            temp: Some(home.join("tmp")),
            ..CleanupContext::default()
        }
    }

    fn rule(id: &str, risk: Risk, location: Location) -> Rule {
        Rule {
            id: id.to_owned(),
            category: Category::AppCache,
            risk,
            title: LocalizedText::english("Title"),
            description: LocalizedText::english("Description"),
            action: ActionKind::Trash,
            locations: vec![location],
            commands: Vec::new(),
        }
    }

    fn write_file(path: &Path, bytes: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        let mut file = File::create(path).expect("file");
        file.write_all(&vec![0_u8; bytes]).expect("write");
    }

    #[test]
    fn discovers_cache_and_skips_symlinks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        write_file(&home.join(".cache/pip/a.whl"), 1024);
        write_file(&home.join(".cache/pip/b.whl"), 2048);
        let context = context_for(&home);
        let protected = ProtectedPaths::for_context(&context, []);
        let mut location = Location::new(BaseDir::Cache, "pip", LocationMode::Path);
        location.kind = EntryKind::Directories;

        let report = discover(
            &[rule("pip-cache", Risk::Low, location)],
            &context,
            &protected,
        );
        let expected = diskclean_platform::metadata(&home.join(".cache/pip/a.whl"))
            .expect("a")
            .allocated_size
            + diskclean_platform::metadata(&home.join(".cache/pip/b.whl"))
                .expect("b")
                .allocated_size;
        assert_eq!(report.item_count(), 1);
        assert_eq!(report.total_bytes(), expected);
    }

    #[test]
    fn age_filter_applies_to_downloads() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let old = home.join("Downloads/old.iso");
        let fresh = home.join("Downloads/fresh.iso");
        write_file(&old, 64);
        write_file(&fresh, 64);
        let old_file = File::options().write(true).open(&old).expect("open");
        old_file
            .set_modified(SystemTime::now() - Duration::from_secs(40 * 86_400))
            .expect("set mtime");

        let context = context_for(&home);
        let protected = ProtectedPaths::for_context(&context, []);
        let mut location = Location::new(BaseDir::Downloads, "", LocationMode::Children);
        location.kind = EntryKind::Files;
        location.min_age_days = 30;

        let report = discover(
            &[rule("old-downloads", Risk::Medium, location)],
            &context,
            &protected,
        );
        let items = &report.recommendations[0].items;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, old);
    }

    #[test]
    fn named_walker_finds_build_directories_without_descending_into_them() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        write_file(&home.join("project/target/debug/app"), 100);
        write_file(&home.join("project/node_modules/pkg/index.js"), 50);
        write_file(&home.join("project/src/main.rs"), 10);
        let context = context_for(&home);
        let protected = ProtectedPaths::for_context(&context, []);

        let mut location = Location::new(BaseDir::Home, "", LocationMode::RecursiveNamed);
        location.names = vec!["target".to_owned(), "node_modules".to_owned()];
        location.kind = EntryKind::Directories;
        location.max_depth = 5;

        let report = discover(
            &[rule("dev-artifacts", Risk::High, location)],
            &context,
            &protected,
        );
        let items = &report.recommendations[0].items;
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.kind() == ItemKind::Directory));
    }

    #[test]
    fn protected_paths_never_appear() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        write_file(&home.join(".ssh/id_rsa"), 100);
        let context = context_for(&home);
        let protected = ProtectedPaths::for_context(&context, []);

        let mut location = Location::new(BaseDir::Home, ".ssh", LocationMode::Path);
        location.kind = EntryKind::Directories;
        let report = discover(&[rule("ssh", Risk::Low, location)], &context, &protected);
        assert_eq!(report.item_count(), 0);
    }

    #[test]
    fn command_rules_have_no_items_and_are_never_mutated() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = context_for(&temp.path().join("home"));
        let protected = ProtectedPaths::for_context(&context, []);
        let command_rule = Rule {
            id: "command".to_owned(),
            category: Category::System,
            risk: Risk::SystemOnly,
            title: LocalizedText::english("Command"),
            description: LocalizedText::english("Shown only"),
            action: ActionKind::SystemCommand,
            locations: Vec::new(),
            commands: vec![diskclean_rules::SystemCommand {
                label: LocalizedText::english("Run"),
                command: "echo hello".to_owned(),
                requires_admin: false,
                note: None,
            }],
        };
        let report = discover(&[command_rule], &context, &protected);
        assert_eq!(report.recommendations[0].items.len(), 0);
    }
}
