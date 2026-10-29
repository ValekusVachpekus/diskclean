//! Offline GOG metadata and Heroic's installed GOG store. No launcher execution.
use serde::Deserialize;
use std::collections::HashSet;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RECORDS: usize = 20_000;

#[derive(Debug, Deserialize)]
pub struct GogInfo {
    #[serde(rename = "gameId")]
    pub id: String,
    pub name: String,
    #[serde(rename = "buildId")]
    pub build: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HeroicGame {
    #[serde(rename = "appName")]
    pub id: String,
    pub install_path: PathBuf,
    pub version: Option<String>,
    #[serde(rename = "buildId")]
    pub build: Option<String>,
}
#[derive(Debug, Default)]
pub struct InstalledGames {
    pub games: Vec<HeroicGame>,
    pub invalid_records: Vec<usize>,
}

/// Config directories containing gog_store/installed.json, not gogdl manifests.
pub fn heroic_roots() -> Vec<PathBuf> {
    // Reuse the native/Flatpak Heroic locations; remove its Epic-specific suffix.
    crate::legendary::heroic_roots()
        .into_iter()
        .filter_map(|root| root.parent()?.parent().map(Path::to_path_buf))
        .collect()
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid GOG metadata")
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    let file = crate::open_regular_file(path)?;
    let mut data = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    serde_json::from_slice(&data).map_err(|_| invalid())
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn read_info(path: &Path) -> io::Result<GogInfo> {
    let info: GogInfo = read_json(path)?;
    let expected = format!("goggame-{}.info", info.id);
    if !valid_id(&info.id)
        || info.name.trim().is_empty()
        || path.file_name().and_then(|name| name.to_str()) != Some(expected.as_str())
    {
        return Err(invalid());
    }
    Ok(info)
}

pub fn read_heroic(path: &Path) -> io::Result<InstalledGames> {
    #[derive(Deserialize)]
    struct Store {
        installed: Vec<serde_json::Value>,
    }
    let store: Store = read_json(path)?;
    if store.installed.len() > MAX_RECORDS {
        return Err(invalid());
    }
    let mut result = InstalledGames::default();
    let mut ids = HashSet::new();
    for (index, row) in store.installed.into_iter().enumerate() {
        let Ok(game) = serde_json::from_value::<HeroicGame>(row) else {
            result.invalid_records.push(index);
            continue;
        };
        let path = &game.install_path;
        let valid = valid_id(&game.id)
            && path.is_absolute()
            && path.parent().is_some()
            && !path.components().any(|part| part == Component::ParentDir)
            && (cfg!(windows) || !path.to_string_lossy().contains(['\\', ':']));
        if valid && ids.insert(game.id.clone()) {
            result.games.push(game);
        } else {
            result.invalid_records.push(index);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn info_matches_filename_and_heroic_keeps_valid_rows() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("goggame-123.info");
        std::fs::write(&path, r#"{"gameId":"123","name":"Game","buildId":"42","playTasks":[{"path":"never executed"}]}"#).expect("info");
        assert_eq!(read_info(&path).expect("read").name, "Game");
        std::fs::write(&path, r#"{"gameId":"124","name":"Game"}"#).expect("bad");
        assert!(read_info(&path).is_err());
        let row = serde_json::json!({"appName":"123", "install_path":temp.path().join("Game"), "version":"v1"});
        let mut traversal = row.clone();
        traversal["install_path"] = "/tmp/../escape".into();
        let mut invalid_id = row.clone();
        invalid_id["appName"] = "../123".into();
        std::fs::write(
            &path,
            serde_json::json!({"installed":[row.clone(),null,row,traversal,invalid_id]})
                .to_string(),
        )
        .expect("store");
        let parsed = read_heroic(&path).expect("read");
        assert_eq!(parsed.games.len(), 1);
        assert_eq!(parsed.invalid_records, vec![1, 2, 3, 4]);
    }
    #[test]
    fn bounds_types_roots_and_malformed_metadata_are_rejected() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("installed.json");
        for data in [
            "[]".into(),
            "{}".into(),
            "not json".into(),
            " ".repeat(MAX_BYTES as usize + 1),
            format!(
                "{{\"installed\":[{}0{}]}}",
                "[".repeat(140),
                "]".repeat(140)
            ),
            serde_json::json!({"installed":vec![serde_json::Value::Null;MAX_RECORDS+1]})
                .to_string(),
        ] {
            std::fs::write(&path, data).expect("write");
            assert!(read_heroic(&path).is_err());
        }
        for location in ["relative", "/", ""] {
            std::fs::write(
                &path,
                serde_json::json!({"installed":[{"appName":"123","install_path":location}]})
                    .to_string(),
            )
            .expect("write");
            assert!(read_heroic(&path).expect("read").games.is_empty());
        }
    }
}
