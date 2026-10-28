use std::fs;
use std::io::Write;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn diskclean() -> Command {
    Command::cargo_bin("diskclean").expect("binary exists")
}

fn isolated_environment(temp: &std::path::Path) -> Command {
    let mut command = diskclean();
    // HOME isolates Unix discovery. Windows correctly uses Known Folders, so
    // candidate assertions below use the common thumbnail rule whose cache
    // base is explicitly configurable on both platforms.
    command
        .env("HOME", temp)
        .env("XDG_CACHE_HOME", temp.join(".cache"))
        .env("XDG_DATA_HOME", temp.join(".local/share"))
        .env("XDG_CONFIG_HOME", temp.join(".config"))
        .env("XDG_STATE_HOME", temp.join(".local/state"))
        .env("LOCALAPPDATA", temp.join(".local/share"))
        .env("APPDATA", temp.join(".config"))
        .env("PROGRAMDATA", temp.join("program-data"))
        .env("TMPDIR", temp.join("tmp"))
        .env("TEMP", temp.join("tmp"))
        .env("TMP", temp.join("tmp"));
    command
}

fn write_file(path: &std::path::Path, size: usize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    let mut file = fs::File::create(path).expect("create file");
    file.write_all(&vec![b'x'; size]).expect("write file");
}

#[test]
fn volumes_json_is_an_array() {
    let output = diskclean()
        .args(["volumes", "--json"])
        .output()
        .expect("run volumes");

    assert!(output.status.success());
    let volumes: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(volumes.is_array());
}

#[test]
fn scan_json_reports_files() {
    let temp = tempdir().expect("temp dir");
    let file = temp.path().join("data.bin");
    write_file(&file, 123);

    let output = diskclean()
        .args([
            "scan",
            temp.path().to_str().expect("utf-8 temp path"),
            "--json",
            "--top",
            "1",
        ])
        .output()
        .expect("run scan");

    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(report["stats"]["files"], 1);
    assert_eq!(report["top_files"][0]["logical_size"], 123);
    assert_eq!(report["root"], temp.path().to_string_lossy().as_ref());
}

#[test]
fn tree_flag_implies_json() {
    let temp = tempdir().expect("temp dir");
    write_file(&temp.path().join("data.bin"), 10);

    let output = diskclean()
        .args([
            "scan",
            temp.path().to_str().expect("utf-8 temp path"),
            "--tree",
        ])
        .output()
        .expect("run scan");

    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(report["tree"].is_object());
}

#[test]
fn largest_json_honors_min_size() {
    let temp = tempdir().expect("temp dir");
    write_file(&temp.path().join("small.bin"), 10);
    write_file(&temp.path().join("large.bin"), 1024 * 1024);

    let output = diskclean()
        .args([
            "largest",
            temp.path().to_str().expect("utf-8 temp path"),
            "--json",
            "--min-size",
            "10000",
        ])
        .output()
        .expect("run largest");

    assert!(output.status.success());
    let files: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(files.as_array().expect("array").len(), 1);
    assert_eq!(files[0]["logical_size"], 1024 * 1024);
}

#[test]
fn invalid_size_is_rejected() {
    diskclean()
        .args(["largest", ".", "--min-size", "12XB"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown size suffix"));
}

#[test]
fn recommend_json_lists_rules_and_candidates() {
    let temp = tempdir().expect("temp dir");
    write_file(&temp.path().join(".cache/thumbnails/image.png"), 1024);
    let output = isolated_environment(temp.path())
        .args(["recommend", "--json"])
        .output()
        .expect("run recommend");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    let recommendations = report["recommendations"].as_array().expect("array");
    let thumbnails = recommendations
        .iter()
        .find(|recommendation| recommendation["rule"]["id"] == "thumbnail-cache")
        .expect("thumbnail rule");
    let items = thumbnails["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(
        std::path::Path::new(items[0]["path"].as_str().expect("candidate path")),
        temp.path().join(".cache/thumbnails")
    );
    assert_eq!(items[0]["rule_id"], "thumbnail-cache");
}

#[test]
fn recommend_never_mutates_files() {
    let temp = tempdir().expect("temp dir");
    let candidate = temp.path().join(".cache/thumbnails/image.png");
    write_file(&candidate, 1024);
    isolated_environment(temp.path())
        .args(["recommend"])
        .assert()
        .success()
        .stdout(predicate::str::contains("recommendation only"));
    assert!(candidate.exists());
}

#[test]
fn clean_refuses_apply_without_confirmation() {
    isolated_environment(std::path::Path::new("/"))
        .args(["clean", "--apply"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--confirm"));
}

#[test]
fn clean_dry_run_never_touches_the_candidate() {
    let temp = tempdir().expect("temp dir");
    let candidate = temp.path().join(".cache/thumbnails/image.png");
    write_file(&candidate, 1024);
    isolated_environment(temp.path())
        .args(["clean", "--dry-run", "--rule", "thumbnail-cache"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing was changed"));
    assert!(candidate.exists());
}

#[test]
fn duplicates_json_finds_groups_without_mutating_files() {
    let temp = tempdir().expect("temp dir");
    let first = temp.path().join("first.bin");
    let second = temp.path().join("nested/second.bin");
    write_file(&first, 4096);
    write_file(&second, 4096);

    let output = diskclean()
        .args([
            "duplicates",
            temp.path().to_str().expect("utf-8 temp path"),
            "--json",
            "--min-size",
            "0",
        ])
        .output()
        .expect("run duplicates");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(report["groups"].as_array().expect("groups").len(), 1);
    assert_eq!(
        report["groups"][0]["files"]
            .as_array()
            .expect("files")
            .len(),
        2
    );
    assert!(first.exists());
    assert!(second.exists());
}

#[test]
fn clean_requires_a_mode() {
    isolated_environment(std::path::Path::new("/"))
        .args(["clean"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--dry-run"));
}

#[test]
fn thread_options_apply_to_scans_and_duplicates_and_reject_excessive_counts() {
    let temp = tempdir().expect("fixture");
    write_file(&temp.path().join("a"), 100);
    write_file(&temp.path().join("b"), 100);
    for command in ["scan", "largest", "duplicates"] {
        for threads in ["0", "1", "4"] {
            diskclean()
                .arg(command)
                .arg(temp.path())
                .args(["--threads", threads, "--json"])
                .assert()
                .success();
        }
        diskclean()
            .arg(command)
            .arg(temp.path())
            .args(["--threads", "33"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("thread count from 0 to 32"));
    }
}
