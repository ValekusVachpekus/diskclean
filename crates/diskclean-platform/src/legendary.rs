//! Bounded offline Legendary installed-game records, also used by Heroic's Epic backend.
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer, de};

const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RECORDS: usize = 20_000;

#[derive(Debug, Deserialize)]
pub struct LegendaryGame {
    pub app_name: String,
    pub title: String,
    pub install_path: PathBuf,
    pub version: Option<String>,
    pub needs_verification: Option<bool>,
    pub is_preloaded: Option<bool>,
}

#[derive(Debug, Default)]
pub struct InstalledGames {
    pub games: Vec<LegendaryGame>,
    /// Invalid record IDs only, never authentication or arbitrary JSON contents.
    pub invalid_ids: Vec<String>,
}

/// Standalone Legendary follows LEGENDARY_CONFIG_PATH, XDG_CONFIG_HOME, ~/.config.
pub fn default_roots() -> Vec<PathBuf> {
    if let Some(root) = absolute_env("LEGENDARY_CONFIG_PATH") {
        return vec![root];
    }
    config_home()
        .map(|root| vec![root.join("legendary")])
        .unwrap_or_default()
}

/// Heroic's Epic backend. GOG/Amazon and third-party installs are separate providers.
pub fn heroic_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    let config = absolute_env("APPDATA");
    #[cfg(not(windows))]
    let config = config_home();
    if let Some(config) = config {
        roots.push(config.join("heroic/legendaryConfig/legendary"));
    }
    #[cfg(target_os = "linux")]
    if let Some(home) = crate::home_dir() {
        roots.push(
            home.join(
                ".var/app/com.heroicgameslauncher.hgl/config/heroic/legendaryConfig/legendary",
            ),
        );
    }
    roots
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}
fn config_home() -> Option<PathBuf> {
    absolute_env("XDG_CONFIG_HOME").or_else(|| crate::home_dir().map(|home| home.join(".config")))
}

// Retain malformed individual rows without dropping valid neighbours. Reject
// duplicate record IDs rather than silently overwriting evidence.
struct Records(BTreeMap<String, serde_json::Value>);
impl<'de> Deserialize<'de> for Records {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Records;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an installed-game object")
            }
            fn visit_map<M: de::MapAccess<'de>>(self, mut map: M) -> Result<Records, M::Error> {
                let mut rows = BTreeMap::new();
                while let Some(id) = map.next_key::<String>()? {
                    if rows.len() >= MAX_RECORDS || rows.contains_key(&id) {
                        return Err(de::Error::custom(
                            "too many or duplicate installed-game IDs",
                        ));
                    }
                    rows.insert(id, map.next_value()?);
                }
                Ok(Records(rows))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

pub fn read_installed(path: &Path) -> io::Result<InstalledGames> {
    let file = crate::open_regular_file(path)?;
    let mut data = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut data)?;
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid Legendary installed-game metadata",
        )
    };
    if data.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let records: Records = serde_json::from_slice(&data).map_err(|_| invalid())?;
    let mut result = InstalledGames::default();
    for (id, value) in records.0 {
        let Ok(game) = serde_json::from_value::<LegendaryGame>(value) else {
            result.invalid_ids.push(id);
            continue;
        };
        let path = &game.install_path;
        let valid = !id.trim().is_empty()
            && game.app_name == id
            && !game.title.trim().is_empty()
            && path.is_absolute()
            && path.parent().is_some()
            && !path.components().any(|part| part == Component::ParentDir)
            && (cfg!(windows) || !path.to_string_lossy().contains(['\\', ':']));
        if valid {
            result.games.push(game);
        } else {
            result.invalid_ids.push(id);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_records_keep_valid_neighbours_and_reject_bad_paths_ids_types() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("installed.json");
        let good = serde_json::json!({"app_name":"good", "title":"Game", "install_path":temp.path().join("Game"), "version":"v1", "needs_verification":false, "is_preloaded":false});
        let mut rows = serde_json::json!({"good": good, "broken": 42});
        for (id, location) in [
            ("relative", "relative"),
            ("escape", "/tmp/../escape"),
            ("root", "/"),
        ] {
            let mut bad = good.clone();
            bad["app_name"] = id.into();
            bad["install_path"] = location.into();
            rows[id] = bad;
        }
        rows["mismatched"] = good;
        std::fs::write(&path, rows.to_string()).expect("write");
        let report = read_installed(&path).expect("report");
        assert_eq!(report.games.len(), 1);
        assert_eq!(report.games[0].app_name, "good");
        assert_eq!(report.invalid_ids.len(), 5);
    }
    #[test]
    fn malformed_duplicate_oversized_and_deep_metadata_are_rejected() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("installed.json");
        for data in [
            "[]".to_owned(),
            "broken".into(),
            r#"{"a":{},"a":{}}"#.into(),
            format!("{{\"a\":{}0{}}}", "[".repeat(140), "]".repeat(140)),
            " ".repeat(MAX_BYTES as usize + 1),
        ] {
            std::fs::write(&path, data).expect("write");
            assert!(read_installed(&path).is_err());
        }
        let rows: BTreeMap<_, _> = (0..=MAX_RECORDS)
            .map(|i| (i.to_string(), serde_json::Value::Null))
            .collect();
        std::fs::write(&path, serde_json::to_vec(&rows).expect("json")).expect("write");
        assert!(read_installed(&path).is_err());
    }
}
