#![deny(unsafe_code)]

//! Cleanup rule schema and built-in rule packs.
//!
//! This crate is deliberately pure: it describes *what* may be recommended and
//! *why*, but it never reads or mutates the filesystem. Recommendation
//! discovery, protected-path checks, trash movement, and auditing belong to
//! `diskclean-cleanup`.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Version of the rule schema understood by this build.
pub const RULES_SCHEMA_VERSION: u32 = 1;

/// A translated string. GUI/CLI choose the field for their active language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizedText {
    pub en: String,
    pub ru: String,
}

impl LocalizedText {
    #[must_use]
    pub fn english(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            en: text.clone(),
            ru: text,
        }
    }

    #[must_use]
    pub fn localized(&self, russian: bool) -> &str {
        if russian { &self.ru } else { &self.en }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Temporary,
    AppCache,
    PackageCache,
    Trash,
    OldDownloads,
    LargeOldFiles,
    DevArtifacts,
    Duplicates,
    System,
}

impl Category {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Temporary => "temporary",
            Self::AppCache => "app_cache",
            Self::PackageCache => "package_cache",
            Self::Trash => "trash",
            Self::OldDownloads => "old_downloads",
            Self::LargeOldFiles => "large_old_files",
            Self::DevArtifacts => "dev_artifacts",
            Self::Duplicates => "duplicates",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Low,
    Medium,
    High,
    SystemOnly,
}

impl Risk {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::SystemOnly => "system_only",
        }
    }

    /// Risks that "select safe" may include. High-risk and system-only items
    /// always require an explicit per-item choice.
    #[must_use]
    pub fn safe_to_select(self) -> bool {
        matches!(self, Self::Low | Self::Medium)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Move the matched entries to the operating-system trash.
    Trash,
    /// Show commands for the user to run; never execute them.
    SystemCommand,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    #[default]
    Any,
    Files,
    Directories,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseDir {
    Home,
    Downloads,
    Cache,
    LocalData,
    Config,
    Temp,
    LocalAppData,
    RoamingAppData,
    ProgramData,
    Trash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationMode {
    /// The resolved path itself.
    Path,
    /// Direct children of the resolved path.
    Children,
    /// Walk below the resolved path and match directory/file names exactly.
    RecursiveNamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub base: BaseDir,
    #[serde(default)]
    pub path: String,
    pub mode: LocationMode,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub kind: EntryKind,
    #[serde(default)]
    pub min_age_days: u64,
    #[serde(default = "default_max_depth")]
    pub max_depth: u16,
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
}

fn default_max_depth() -> u16 {
    6
}

fn default_max_entries() -> usize {
    500
}

impl Location {
    #[must_use]
    pub fn new(base: BaseDir, path: impl Into<String>, mode: LocationMode) -> Self {
        Self {
            base,
            path: path.into(),
            mode,
            names: Vec::new(),
            kind: EntryKind::Any,
            min_age_days: 0,
            max_depth: default_max_depth(),
            max_entries: default_max_entries(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemCommand {
    pub label: LocalizedText,
    pub command: String,
    #[serde(default)]
    pub requires_admin: bool,
    #[serde(default)]
    pub note: Option<LocalizedText>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub category: Category,
    pub risk: Risk,
    pub title: LocalizedText,
    pub description: LocalizedText,
    pub action: ActionKind,
    #[serde(default)]
    pub locations: Vec<Location>,
    #[serde(default)]
    pub commands: Vec<SystemCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulePack {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Error)]
pub enum RulesError {
    #[error("cannot parse rule pack {pack}: {source}")]
    Parse {
        pack: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("unsupported rule schema {version} in pack {pack}")]
    Schema { pack: String, version: u32 },
    #[error("invalid rule pack {pack}: {message}")]
    Invalid { pack: String, message: String },
}

impl RulePack {
    /// Parse and validate one TOML pack.
    pub fn parse(source_name: &str, text: &str) -> Result<Self, RulesError> {
        let pack: Self = toml::from_str(text).map_err(|source| RulesError::Parse {
            pack: source_name.to_owned(),
            source,
        })?;
        pack.validate()?;
        Ok(pack)
    }

    /// Validate schema and safety invariants.
    pub fn validate(&self) -> Result<(), RulesError> {
        if self.schema_version != RULES_SCHEMA_VERSION {
            return Err(RulesError::Schema {
                pack: self.id.clone(),
                version: self.schema_version,
            });
        }
        if self.id.trim().is_empty() {
            return Err(self.invalid("pack id is empty"));
        }

        let mut ids = HashSet::new();
        for rule in &self.rules {
            if rule.id.trim().is_empty() {
                return Err(self.invalid("rule id is empty"));
            }
            if !ids.insert(rule.id.as_str()) {
                return Err(self.invalid(format!("duplicate rule id {}", rule.id)));
            }
            if rule.title.en.trim().is_empty() || rule.title.ru.trim().is_empty() {
                return Err(self.invalid(format!("rule {} has an empty title", rule.id)));
            }
            if rule.description.en.trim().is_empty() || rule.description.ru.trim().is_empty() {
                return Err(self.invalid(format!("rule {} has an empty description", rule.id)));
            }

            match rule.action {
                ActionKind::Trash => {
                    if rule.locations.is_empty() {
                        return Err(
                            self.invalid(format!("trash rule {} has no locations", rule.id))
                        );
                    }
                    if !rule.commands.is_empty() {
                        return Err(
                            self.invalid(format!("trash rule {} must not carry commands", rule.id))
                        );
                    }
                    if rule.risk == Risk::SystemOnly {
                        return Err(
                            self.invalid(format!("trash rule {} cannot be system-only", rule.id))
                        );
                    }
                }
                ActionKind::SystemCommand => {
                    if rule.commands.is_empty() {
                        return Err(
                            self.invalid(format!("system rule {} has no commands", rule.id))
                        );
                    }
                    if !rule.locations.is_empty() {
                        return Err(self.invalid(format!(
                            "system rule {} must not carry trash locations",
                            rule.id
                        )));
                    }
                }
            }

            for location in &rule.locations {
                if location.mode == LocationMode::RecursiveNamed && location.names.is_empty() {
                    return Err(
                        self.invalid(format!("rule {} has a named walker without names", rule.id))
                    );
                }
                if location.mode != LocationMode::RecursiveNamed && !location.names.is_empty() {
                    return Err(self.invalid(format!(
                        "rule {} has names but is not a named walker",
                        rule.id
                    )));
                }
                if location.max_entries == 0 {
                    return Err(self.invalid(format!("rule {} has max_entries = 0", rule.id)));
                }
                if location.mode != LocationMode::Path && location.max_depth == 0 {
                    return Err(self.invalid(format!(
                        "rule {} has max_depth = 0 for a non-path matcher",
                        rule.id
                    )));
                }
                validate_relative_path(&location.path).map_err(|message| {
                    self.invalid(format!("rule {} location: {message}", rule.id))
                })?;
                for name in &location.names {
                    if name.is_empty() || name.contains(['/', '\\']) || name == ".." || name == "."
                    {
                        return Err(self.invalid(format!(
                            "rule {} has an invalid entry name {name:?}",
                            rule.id
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    fn invalid(&self, message: impl Into<String>) -> RulesError {
        RulesError::Invalid {
            pack: self.id.clone(),
            message: message.into(),
        }
    }
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Ok(());
    }
    if path.starts_with(['/', '\\']) || path.get(1..2) == Some(":") {
        return Err("location path must be relative to its base directory".to_owned());
    }
    for component in path.split(['/', '\\']) {
        if component == ".." || component == "." {
            return Err(format!("location path contains {component:?}"));
        }
    }
    Ok(())
}

/// Load the built-in packs for the current platform.
///
/// All packs are embedded at compile time. There is no network or custom-rule
/// execution in this phase.
pub fn builtin_packs() -> Result<Vec<RulePack>, RulesError> {
    let mut packs = vec![RulePack::parse(
        "rules/common.toml",
        include_str!("../../../rules/common.toml"),
    )?];

    #[cfg(target_os = "linux")]
    packs.push(RulePack::parse(
        "rules/linux.toml",
        include_str!("../../../rules/linux.toml"),
    )?);

    #[cfg(target_os = "windows")]
    packs.push(RulePack::parse(
        "rules/windows.toml",
        include_str!("../../../rules/windows.toml"),
    )?);

    let mut ids = HashSet::new();
    for pack in &packs {
        for rule in &pack.rules {
            if !ids.insert(rule.id.as_str()) {
                return Err(RulesError::Invalid {
                    pack: pack.id.clone(),
                    message: format!("duplicate rule id {} across packs", rule.id),
                });
            }
        }
    }

    Ok(packs)
}

/// Flat list of built-in rules for engines and CLI output.
pub fn builtin_rules() -> Result<Vec<Rule>, RulesError> {
    Ok(builtin_packs()?
        .into_iter()
        .flat_map(|pack| pack.rules)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_packs_parse_and_have_unique_rule_ids() {
        let packs = builtin_packs().expect("built-in packs must parse");
        assert!(!packs.is_empty());

        let mut ids = HashSet::new();
        for pack in &packs {
            for rule in &pack.rules {
                assert!(ids.insert(rule.id.clone()), "duplicate {}", rule.id);
            }
        }
    }

    #[test]
    fn all_platform_packs_parse() {
        for (name, text) in [
            ("common", include_str!("../../../rules/common.toml")),
            ("linux", include_str!("../../../rules/linux.toml")),
            ("windows", include_str!("../../../rules/windows.toml")),
        ] {
            RulePack::parse(name, text).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }

    #[test]
    fn rejects_path_traversal_in_locations() {
        let source = r#"
            schema_version = 1
            id = "bad"
            [[rules]]
            id = "bad-rule"
            category = "temporary"
            risk = "low"
            title = { en = "Bad", ru = "Плохо" }
            description = { en = "Bad path", ru = "Плохой путь" }
            action = "trash"
            [[rules.locations]]
            base = "home"
            path = "../escape"
            mode = "children"
        "#;
        let error = RulePack::parse("bad", source).expect_err("traversal must fail");
        assert!(error.to_string().contains(".."));
    }

    #[test]
    fn rejects_system_rule_with_trash_location() {
        let source = r#"
            schema_version = 1
            id = "bad"
            [[rules]]
            id = "bad-rule"
            category = "system"
            risk = "system_only"
            title = { en = "Bad", ru = "Плохо" }
            description = { en = "Mixed", ru = "Смешано" }
            action = "system_command"
            [[rules.locations]]
            base = "home"
            path = ""
            mode = "children"
            [[rules.commands]]
            label = { en = "Run", ru = "Запустить" }
            command = "echo hi"
        "#;
        let error = RulePack::parse("bad", source).expect_err("mixed rule must fail");
        assert!(error.to_string().contains("must not carry"));
    }
}
