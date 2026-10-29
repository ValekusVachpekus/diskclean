//! Bounded, read-only access to Steam's text KeyValues metadata.
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

const MAX_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SteamManifest {
    pub app_id: u32,
    pub name: String,
    pub install_dir: String,
    pub state_flags: Option<u64>,
    pub build_id: Option<String>,
    pub last_played: Option<u64>,
}

/// Common native and Flatpak locations. Missing locations are expected.
pub fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = crate::home_dir() {
        #[cfg(target_os = "linux")]
        {
            let data = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(".local/share"));
            roots.push(data.join("Steam"));
            roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
            roots.push(home.join(".steam/steam"));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = home;
    }
    #[cfg(windows)]
    for key in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(path) = std::env::var_os(key) {
            roots.push(PathBuf::from(path).join("Steam"));
        }
    }
    roots
}

#[derive(Debug)]
enum Value {
    Text(String),
    Object(BTreeMap<String, Value>),
}
impl Value {
    fn object(&self) -> io::Result<&BTreeMap<String, Value>> {
        match self {
            Self::Object(value) => Ok(value),
            _ => Err(invalid()),
        }
    }
    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Steam metadata")
}

struct Parser<'a> {
    remaining: &'a str,
    entries: usize,
}
impl Parser<'_> {
    fn whitespace(&mut self) {
        loop {
            self.remaining = self.remaining.trim_start();
            if let Some(comment) = self.remaining.strip_prefix("//") {
                self.remaining = comment.split_once('\n').map_or("", |(_, rest)| rest);
            } else {
                break;
            }
        }
    }
    fn string(&mut self) -> io::Result<String> {
        self.whitespace();
        self.remaining = self.remaining.strip_prefix('"').ok_or_else(invalid)?;
        let mut value = String::new();
        let mut chars = self.remaining.char_indices();
        while let Some((index, ch)) = chars.next() {
            match ch {
                '"' => {
                    self.remaining = &self.remaining[index + 1..];
                    return Ok(value);
                }
                '\\' => {
                    let (_, next) = chars.next().ok_or_else(invalid)?;
                    match next {
                        '\\' | '"' => value.push(next),
                        _ => {
                            value.push('\\');
                            value.push(next);
                        }
                    }
                }
                _ => value.push(ch),
            }
        }
        Err(invalid())
    }
    fn object(&mut self, depth: usize) -> io::Result<BTreeMap<String, Value>> {
        if depth > 32 {
            return Err(invalid());
        }
        let mut values = BTreeMap::new();
        loop {
            self.whitespace();
            if self.remaining.is_empty() {
                return if depth == 0 {
                    Ok(values)
                } else {
                    Err(invalid())
                };
            }
            if let Some(rest) = self.remaining.strip_prefix('}') {
                if depth == 0 {
                    return Err(invalid());
                }
                self.remaining = rest;
                return Ok(values);
            }
            self.entries += 1;
            if self.entries > 100_000 {
                return Err(invalid());
            }
            let key = self.string()?.to_ascii_lowercase();
            self.whitespace();
            let value = if let Some(rest) = self.remaining.strip_prefix('{') {
                self.remaining = rest;
                Value::Object(self.object(depth + 1)?)
            } else {
                Value::Text(self.string()?)
            };
            if values.insert(key, value).is_some() {
                return Err(invalid());
            }
        }
    }
}
fn read(path: &Path) -> io::Result<BTreeMap<String, Value>> {
    let file = crate::open_regular_file(path)?;
    let mut text = String::new();
    file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    Parser {
        remaining: text.trim_start_matches('\u{feff}'),
        entries: 0,
    }
    .object(0)
}

/// Supports numbered legacy path strings and current numbered objects.
pub fn read_libraries(path: &Path) -> io::Result<Vec<PathBuf>> {
    let data = read(path)?;
    let libraries = data.get("libraryfolders").ok_or_else(invalid)?.object()?;
    let mut paths = Vec::new();
    for (key, value) in libraries {
        if key.parse::<u32>().is_err() {
            continue;
        }
        let path = match value {
            Value::Text(path) => path.as_str(),
            Value::Object(fields) => fields
                .get("path")
                .and_then(Value::text)
                .ok_or_else(invalid)?,
        };
        let path = PathBuf::from(path);
        if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
            return Err(invalid());
        }
        paths.push(path);
    }
    Ok(paths)
}

pub fn read_manifest(path: &Path) -> io::Result<SteamManifest> {
    let data = read(path)?;
    let state = data.get("appstate").ok_or_else(invalid)?.object()?;
    let field = |key| state.get(key).and_then(Value::text);
    let app_id = field("appid")
        .and_then(|id| id.parse::<u32>().ok())
        .filter(|id| *id > 0)
        .ok_or_else(invalid)?;
    let name = field("name")
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(invalid)?
        .to_owned();
    let install_dir = field("installdir").ok_or_else(invalid)?.to_owned();
    // Apply both OS path syntaxes even when testing Windows fixtures on Linux.
    if install_dir.is_empty()
        || install_dir == "."
        || install_dir == ".."
        || install_dir.contains(['/', '\\', ':'])
    {
        return Err(invalid());
    }
    let expected = format!("appmanifest_{app_id}.acf");
    if path.file_name().and_then(|name| name.to_str()) != Some(expected.as_str()) {
        return Err(invalid());
    }
    Ok(SteamManifest {
        app_id,
        name,
        install_dir,
        state_flags: field("stateflags").and_then(|value| value.parse().ok()),
        build_id: field("buildid").map(str::to_owned),
        last_played: field("lastplayed")
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_handles_escapes_comments_case_and_nesting() {
        let mut parser = Parser {
            remaining: r#"// comment
        "LibraryFolders" { "0" { "path" "C:\\Steam" "apps" { "10" "20" } } }"#,
            entries: 0,
        };
        let data = parser.object(0).expect("parse");
        let libraries = data["libraryfolders"].object().expect("libraries");
        assert_eq!(
            libraries["0"].object().expect("entry")["path"].text(),
            Some("C:\\Steam")
        );
    }
    #[test]
    fn malformed_and_excessive_nesting_are_rejected() {
        for input in [
            "\"a\" {",
            "}",
            "\"a\" \"b\" \"A\" \"c\"",
            "\"a\" \"unterminated",
        ] {
            assert!(
                Parser {
                    remaining: input,
                    entries: 0
                }
                .object(0)
                .is_err()
            );
        }
        let deep = format!("{}{}", "\"a\" {".repeat(34), "}".repeat(34));
        assert!(
            Parser {
                remaining: &deep,
                entries: 0
            }
            .object(0)
            .is_err()
        );
    }
    #[test]
    fn metadata_size_limit_and_parent_library_paths_are_rejected() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("libraryfolders.vdf");
        std::fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).expect("large file");
        assert!(read_libraries(&path).is_err());
        for directory in ["relative", "/tmp/../outside"] {
            std::fs::write(
                &path,
                format!(r#""libraryfolders" {{ "0" "{directory}" }}"#),
            )
            .expect("paths");
            assert!(read_libraries(&path).is_err());
        }
    }

    #[test]
    fn manifest_rejects_path_escape_and_mismatched_id() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("appmanifest_10.acf");
        for install in ["../escape", "..\\escape", "C:\\escape", "..", ".", ""] {
            std::fs::write(
                &path,
                format!(r#""AppState" {{ "appid" "10" "name" "Game" "installdir" "{install}" }}"#),
            )
            .expect("fixture");
            assert!(read_manifest(&path).is_err());
        }
        std::fs::write(
            &path,
            r#""AppState" { "appid" "11" "name" "Game" "installdir" "Game" }"#,
        )
        .expect("fixture");
        assert!(read_manifest(&path).is_err());
    }
}
