use std::fs;
use std::io::Write;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn diskclean() -> Command {
    Command::cargo_bin("diskclean").expect("binary exists")
}

fn write_file(path: &std::path::Path, size: usize) {
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
