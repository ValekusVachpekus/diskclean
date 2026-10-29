//! Bounded offline snapd facts. Never contact the daemon or mount/decompress snaps.
use serde::{Deserialize, Deserializer, de};
use std::{
    collections::HashSet,
    io::{self, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};

pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_RECORDS: usize = 20_000;

pub fn default_roots() -> Vec<PathBuf> {
    if cfg!(target_os = "linux") {
        vec!["/var/lib/snapd".into()]
    } else {
        Vec::new()
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid or excessive snapd metadata",
    )
}

pub fn valid_instance(value: &str) -> bool {
    let (name, key) = value
        .split_once('_')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    (2..=40).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name.bytes().any(|b| b.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && key.is_none_or(|key| {
            !key.is_empty()
                && key.len() <= 10
                && key
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}
pub fn valid_revision(value: &str) -> bool {
    let digits = value.strip_prefix('x').unwrap_or(value);
    !digits.is_empty()
        && !digits.starts_with('0')
        && digits.bytes().all(|b| b.is_ascii_digit())
        && digits.parse::<i64>().is_ok_and(|n| n > 0)
}
pub fn archive_identity(filename: &str) -> Option<(String, String)> {
    let (instance, revision) = filename.strip_suffix(".snap")?.rsplit_once('_')?;
    (valid_instance(instance) && valid_revision(revision))
        .then(|| (instance.into(), revision.into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionState {
    CurrentEnabled,
    CurrentDisabled,
    Retained,
}
#[derive(Debug, Clone)]
pub struct SnapRevision {
    pub instance: String,
    pub revision: String,
    pub state: RevisionState,
    pub shared_component: bool,
}
type Stamp = (u64, u64, u64, Option<SystemTime>);
fn stamp(meta: crate::FileMeta) -> Stamp {
    (meta.dev, meta.ino, meta.logical_size, meta.modified)
}
#[derive(Debug)]
pub struct StateInventory {
    pub revisions: Vec<SnapRevision>,
    pub invalid_records: usize,
    stamp: Stamp,
}
impl StateInventory {
    pub fn unchanged(&self, path: &Path) -> io::Result<bool> {
        crate::check_path_components(path)?;
        Ok(self.stamp == stamp(crate::metadata(path)?))
    }
}

// Only data.snaps is deserialized; authentication, tasks and unrelated state
// are skipped. Reject duplicate JSON keys rather than overwriting evidence.
#[derive(Debug)]
struct Strict(serde_json::Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Strict;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("bounded unique JSON")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Strict, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Strict(n.into()))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(serde_json::Value::Null))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Strict, A::Error> {
                let mut values = Vec::new();
                while let Some(Strict(value)) = seq.next_element()? {
                    if values.len() >= MAX_RECORDS {
                        return Err(de::Error::custom("sequence limit"));
                    }
                    values.push(value);
                }
                Ok(Strict(values.into()))
            }
            fn visit_map<M: de::MapAccess<'de>>(self, mut map: M) -> Result<Strict, M::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.len() >= MAX_RECORDS || values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate key or record limit"));
                    }
                    values.insert(key, map.next_value::<Strict>()?.0);
                }
                Ok(Strict(values.into()))
            }
        }
        d.deserialize_any(Visitor)
    }
}
#[derive(Deserialize)]
struct Envelope {
    data: Data,
}
#[derive(Deserialize)]
struct Data {
    #[serde(default)]
    snaps: Option<Strict>,
}
#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    active: bool,
    current: serde_json::Value,
    #[serde(default, rename = "instance-key")]
    instance_key: String,
    sequence: Vec<SideInfo>,
}
#[derive(Deserialize)]
struct SideInfo {
    #[serde(default)]
    name: String,
    revision: serde_json::Value,
}
fn revision(value: &serde_json::Value) -> Option<String> {
    let result = if let Some(value) = value.as_str() {
        value.to_owned()
    } else {
        let n = value.as_i64()?;
        if n < 0 {
            format!("x{}", n.checked_abs()?)
        } else {
            n.to_string()
        }
    };
    valid_revision(&result).then_some(result)
}

pub fn read_state(path: &Path, cancelled: impl Fn() -> bool) -> io::Result<StateInventory> {
    let before = crate::metadata(path)?;
    let text = crate::packages::read_text(path, MAX_BYTES, &cancelled)?;
    if stamp(before) != stamp(crate::metadata(path)?) {
        return Err(invalid());
    }
    let envelope: Envelope = serde_json::from_str(&text).map_err(|_| invalid())?;
    let rows = match envelope.data.snaps {
        Some(Strict(serde_json::Value::Object(rows))) => rows,
        None => serde_json::Map::new(),
        _ => return Err(invalid()),
    };
    let mut inventory = StateInventory {
        revisions: Vec::new(),
        invalid_records: 0,
        stamp: stamp(before),
    };
    for (instance, value) in rows {
        if cancelled() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let parsed = (|| -> Option<Vec<SnapRevision>> {
            if !valid_instance(&instance) {
                return None;
            }
            let entry: Entry = serde_json::from_value(value).ok()?;
            let current = revision(&entry.current)?;
            let (name, key) = instance.split_once('_').unwrap_or((&instance, ""));
            if key != entry.instance_key {
                return None;
            }
            let mut revisions = Vec::new();
            let mut seen = HashSet::new();
            for side in entry.sequence {
                if !side.name.is_empty() && side.name != name {
                    return None;
                }
                let rev = revision(&side.revision)?;
                if !seen.insert(rev.clone()) {
                    return None;
                }
                let state = if rev != current {
                    RevisionState::Retained
                } else if entry.active {
                    RevisionState::CurrentEnabled
                } else {
                    RevisionState::CurrentDisabled
                };
                revisions.push(SnapRevision {
                    instance: instance.clone(),
                    revision: rev,
                    state,
                    shared_component: matches!(
                        entry.kind.as_str(),
                        "base" | "os" | "snapd" | "kernel" | "gadget"
                    ),
                });
            }
            if !seen.contains(&current) {
                return None;
            }
            Some(revisions)
        })();
        match parsed {
            Some(revisions) if revisions.len() <= MAX_RECORDS - inventory.revisions.len() => {
                inventory.revisions.extend(revisions)
            }
            Some(_) => return Err(invalid()),
            None => inventory.invalid_records += 1,
        }
    }
    if cancelled() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    Ok(inventory)
}

/// Inspect only a SquashFS v4 header and stable file metadata. This is not a
/// signature/content verification and reads no mounted/decompressed content.
pub fn archive_metadata(path: &Path) -> io::Result<crate::FileMeta> {
    let before = crate::metadata(path)?;
    if before.kind != crate::FileKind::File || before.logical_size < 96 {
        return Err(invalid());
    }
    let mut file = crate::open_regular_file(path)?;
    if crate::opened_identity(&file)? != (before.dev, before.ino) {
        return Err(invalid());
    }
    let mut header = [0; 96];
    file.read_exact(&mut header)?;
    let block = u32::from_le_bytes(header[12..16].try_into().map_err(|_| invalid())?);
    let used = u64::from_le_bytes(header[40..48].try_into().map_err(|_| invalid())?);
    if &header[..4] != b"hsqs"
        || header[28..32] != [4, 0, 0, 0]
        || !(4096..=1048576).contains(&block)
        || !block.is_power_of_two()
        || !(96..=before.logical_size).contains(&used)
    {
        return Err(invalid());
    }
    let after = crate::metadata(path)?;
    let opened = file.metadata()?;
    if stamp(before) != stamp(after)
        || before.kind != after.kind
        || opened.len() != before.logical_size
        || opened.modified().ok() != before.modified
    {
        return Err(invalid());
    }
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header() -> [u8; 96] {
        let mut bytes = [0; 96];
        bytes[..4].copy_from_slice(b"hsqs");
        bytes[12..16].copy_from_slice(&131072u32.to_le_bytes());
        bytes[28] = 4;
        bytes[40..48].copy_from_slice(&96u64.to_le_bytes());
        bytes
    }
    #[test]
    fn archive_names_revisions_and_instances_are_bounded_path_components() {
        assert_eq!(
            archive_identity("hello-world_test1_x2.snap"),
            Some(("hello-world_test1".into(), "x2".into()))
        );
        for bad in [
            "../../x_1.snap",
            "ab_../x_1.snap",
            "ab_0.snap",
            "ab_01.snap",
            "ab_x0.snap",
            "ab_-1.snap",
            "ab_1.comp",
            "1_1.snap",
            "a--b_1.snap",
            "ab_keytoolong12_1.snap",
            "ab_9223372036854775808.snap",
        ] {
            assert!(archive_identity(bad).is_none(), "{bad}");
        }
    }
    #[test]
    fn state_retains_enabled_disabled_old_local_and_parallel_instance_evidence() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("state.json");
        let text = r#"{"data":{"auth":{"secret":"never in report"},"snaps":{
            "example":{"type":"app","active":true,"current":"2","sequence":[{"name":"example","revision":"1"},{"name":"example","revision":"2"}]},
            "core22":{"type":"base","current":10,"sequence":[{"name":"core22","revision":10}]},
            "local_test":{"type":"app","instance-key":"test","active":true,"current":"x1","sequence":[{"revision":-1}]},
            "bad":{"current":"1","sequence":[{"name":"other","revision":"1"}]}
        }},"tasks":{}}"#;
        std::fs::write(&path, text).expect("state");
        let state = read_state(&path, || false).expect("state");
        assert_eq!(state.revisions.len(), 4);
        assert_eq!(state.invalid_records, 1);
        let find = |instance: &str, revision: &str| {
            state
                .revisions
                .iter()
                .find(|r| r.instance == instance && r.revision == revision)
                .expect("revision")
        };
        assert_eq!(find("example", "1").state, RevisionState::Retained);
        assert_eq!(find("example", "2").state, RevisionState::CurrentEnabled);
        assert_eq!(find("core22", "10").state, RevisionState::CurrentDisabled);
        assert!(find("core22", "10").shared_component);
        assert_eq!(
            find("local_test", "x1").state,
            RevisionState::CurrentEnabled
        );
        assert!(state.unchanged(&path).expect("unchanged"));
        assert!(!format!("{state:?}").contains("never in report"));
        assert_eq!(std::fs::read_to_string(&path).expect("unchanged"), text);
        std::fs::write(&path, "{}").expect("change");
        assert!(!state.unchanged(&path).expect("changed"));
    }
    #[test]
    fn ambiguous_corrupt_excessive_and_cancelled_state_is_rejected() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("state.json");
        for text in [
            r#"{"data":{"snaps":{"example":{},"example":{}}}}"#,
            r#"{"data":{"snaps":{"example":{"current":"1","current":"2"}}}}"#,
            r#"{"data":{"snaps":[]}}"#,
            "broken",
        ] {
            std::fs::write(&path, text).expect("write");
            assert!(read_state(&path, || false).is_err());
        }
        assert_eq!(
            read_state(&path, || true).expect_err("cancel").kind(),
            io::ErrorKind::Interrupted
        );
        std::fs::write(&path, [255]).expect("utf8");
        assert!(read_state(&path, || false).is_err());
        std::fs::File::create(&path)
            .expect("file")
            .set_len(MAX_BYTES + 1)
            .expect("len");
        assert!(read_state(&path, || false).is_err());
        let sequence = vec![serde_json::json!({"revision":"1"}); MAX_RECORDS + 1];
        std::fs::write(
            &path,
            serde_json::json!({"data":{"snaps":{"example":{"current":"1","sequence":sequence}}}})
                .to_string(),
        )
        .expect("many");
        assert!(read_state(&path, || false).is_err());
    }
    #[test]
    fn archives_measure_allocated_compressed_files_and_reject_invalid_headers() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("example_1.snap");
        let good = header();
        std::fs::write(&path, good).expect("archive");
        let meta = archive_metadata(&path).expect("metadata");
        assert_eq!(meta.logical_size, 96);
        // Padding/sparse extents belong to the archive file, not bytes_used in
        // the SquashFS header. No payload read or decompression is required.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open")
            .set_len(8 * 1024 * 1024)
            .expect("sparse padding");
        let padded = archive_metadata(&path).expect("padded archive");
        assert_eq!(padded.logical_size, 8 * 1024 * 1024);
        assert_eq!(
            padded.allocated_size,
            crate::metadata(&path).expect("allocation").allocated_size
        );
        for offset in [0, 12, 28, 40] {
            let mut bad = good;
            bad[offset] = if offset == 12 { 1 } else { 255 };
            std::fs::write(&path, bad).expect("write");
            assert!(archive_metadata(&path).is_err(), "offset {offset}");
        }
        std::fs::write(&path, b"hsqs").expect("truncated");
        assert!(archive_metadata(&path).is_err());
        assert!(archive_metadata(temp.path()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn archive_and_state_links_and_linked_ancestors_are_rejected() {
        let temp = tempfile::tempdir().expect("temp");
        let archive = temp.path().join("real.snap");
        std::fs::write(&archive, header()).expect("archive");
        let state = temp.path().join("real.json");
        std::fs::write(&state, r#"{"data":{}}"#).expect("state");
        std::os::unix::fs::symlink(&archive, temp.path().join("linked.snap")).expect("link");
        std::os::unix::fs::symlink(&state, temp.path().join("state.json")).expect("link");
        std::os::unix::fs::symlink(temp.path(), temp.path().join("alias")).expect("alias");
        assert!(archive_metadata(&temp.path().join("linked.snap")).is_err());
        assert!(archive_metadata(&temp.path().join("alias/real.snap")).is_err());
        assert!(read_state(&temp.path().join("state.json"), || false).is_err());
        assert!(read_state(&temp.path().join("alias/real.json"), || false).is_err());
    }
}
