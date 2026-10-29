//! Offline Epic Games Launcher `.item` metadata; never executes launch commands.
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

const MAX_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct EpicManifest {
    pub app_name: String,
    pub display_name: String,
    pub install_location: PathBuf,
    pub app_version_string: Option<String>,
    #[serde(rename = "bIsIncompleteInstall")]
    pub incomplete: Option<bool>,
    #[serde(rename = "bNeedsValidation")]
    pub needs_validation: Option<bool>,
}

/// Native Windows location. Wine prefixes on Linux must be selected explicitly.
pub fn default_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| vec![path.join("Epic/EpicGamesLauncher/Data/Manifests")])
            .unwrap_or_default()
    }
    #[cfg(not(windows))]
    Vec::new()
}

pub fn read_manifest(path: &Path) -> io::Result<EpicManifest> {
    let file = crate::open_regular_file(path)?;
    let mut data = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut data)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid Epic manifest");
    if data.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let manifest: EpicManifest = serde_json::from_slice(&data).map_err(|_| invalid())?;
    if manifest.app_name.trim().is_empty()
        || manifest.display_name.trim().is_empty()
        || !manifest.install_location.is_absolute()
        || manifest.install_location.parent().is_none()
        || manifest
            .install_location
            .components()
            .any(|part| part == Component::ParentDir)
    {
        return Err(invalid());
    }
    // A Windows path in a Wine manifest needs explicit translation, never guessing.
    #[cfg(not(windows))]
    if manifest
        .install_location
        .to_string_lossy()
        .contains(['\\', ':'])
    {
        return Err(invalid());
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_metadata_fields_paths_and_bounds() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("game.item");
        let data = serde_json::json!({
            "AppName": "id", "DisplayName": "Game", "InstallLocation": temp.path().join("Game"),
            "bIsIncompleteInstall": false, "bNeedsValidation": true,
            "LaunchCommand": "ignored", "InstallSize": 999999
        });
        std::fs::write(&path, data.to_string()).expect("write");
        let parsed = read_manifest(&path).expect("parse");
        assert_eq!(parsed.app_name, "id");
        assert_eq!(parsed.needs_validation, Some(true));
        for location in ["relative", "", "/", "/tmp/../outside"] {
            let mut bad = data.clone();
            bad["InstallLocation"] = location.into();
            std::fs::write(&path, bad.to_string()).expect("write");
            assert!(read_manifest(&path).is_err(), "{location}");
        }
        for bad in [
            r#"{"AppName":"a","AppName":"b"}"#,
            r#"{"AppName":42}"#,
            "not json",
        ] {
            std::fs::write(&path, bad).expect("write");
            assert!(read_manifest(&path).is_err());
        }
        std::fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).expect("write");
        assert!(read_manifest(&path).is_err());
    }
}
