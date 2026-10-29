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

#[test]
fn applications_json_measures_steam_installation_and_reports_bad_metadata() {
    let temp = tempdir().expect("temp");
    let apps = temp.path().join("steamapps");
    write_file(&apps.join("common/Test Game/data.bin"), 123);
    fs::write(apps.join("appmanifest_10.acf"), r#""AppState" { "appid" "10" "name" "Test Game" "installdir" "Test Game" "StateFlags" "4" "SizeOnDisk" "99999999" }"#).expect("manifest");
    fs::write(apps.join("appmanifest_11.acf"), "malformed").expect("bad manifest");
    let output = diskclean()
        .arg("applications")
        .arg(temp.path())
        .args(["--json", "--threads", "1"])
        .output()
        .expect("run");
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(report["applications"].as_array().expect("apps").len(), 1);
    assert_eq!(report["applications"][0]["name"], "Test Game");
    assert_eq!(report["applications"][0]["logical_bytes"], 123);
    assert_eq!(report["applications"][0]["status"], "installed");
    assert_eq!(report["issues"].as_array().expect("issues").len(), 1);
    assert!(apps.join("common/Test Game/data.bin").exists());
}

#[test]
fn applications_epic_provider_measures_explicit_manifest_directory() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("Manifests");
    fs::create_dir_all(&root).expect("manifests");
    let install = temp.path().join("Game");
    write_file(&install.join("data.bin"), 456);
    fs::write(
        root.join("game.item"),
        serde_json::json!({
            "AppName": "epic-id", "DisplayName": "Epic Game", "InstallLocation": install,
            "bIsIncompleteInstall": false, "bNeedsValidation": false, "InstallSize": 999999
        })
        .to_string(),
    )
    .expect("manifest");
    let output = diskclean()
        .arg("applications")
        .arg(&root)
        .args(["--provider", "epic", "--json"])
        .output()
        .expect("run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(report["applications"].as_array().expect("apps").len(), 1);
    assert_eq!(report["applications"][0]["launcher"], "Epic Games");
    assert_eq!(report["applications"][0]["logical_bytes"], 456);
    assert_eq!(report["applications"][0]["status"], "installed");
    assert!(install.join("data.bin").exists());
    diskclean()
        .args(["applications", "--provider", "epic"])
        .assert()
        .failure();
}

#[test]
fn applications_legendary_and_heroic_use_only_installed_records() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("legendary");
    fs::create_dir_all(&root).expect("config");
    let install = temp.path().join("Game");
    write_file(&install.join("data.bin"), 222);
    fs::write(
        root.join("installed.json"),
        serde_json::json!({"id": {
        "app_name":"id", "title":"Legendary Game", "install_path":install,
        "needs_verification":false, "is_preloaded":false, "install_size":999999
    }, "bad": null})
        .to_string(),
    )
    .expect("metadata");
    fs::write(root.join("user.json"), "invalid and never read").expect("user");
    fs::create_dir_all(root.join("metadata")).expect("metadata dir");
    fs::write(
        root.join("metadata/uninstalled.json"),
        "invalid and never read",
    )
    .expect("catalog");
    for (provider, launcher) in [("legendary", "Legendary"), ("heroic", "Heroic (Epic)")] {
        let output = diskclean()
            .arg("applications")
            .arg(&root)
            .args(["--provider", provider, "--json"])
            .output()
            .expect("run");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("json");
        assert_eq!(report["applications"].as_array().expect("apps").len(), 1);
        assert_eq!(report["applications"][0]["launcher"], launcher);
        assert_eq!(report["applications"][0]["logical_bytes"], 222);
        assert_eq!(report["applications"][0]["status"], "installed");
        assert_eq!(report["issues"].as_array().expect("issues").len(), 1);
    }
    assert!(install.join("data.bin").exists());
}

#[test]
fn applications_gog_and_heroic_gog_measure_local_installations() {
    let temp = tempdir().expect("temp");
    let game = temp.path().join("Game");
    write_file(&game.join("data.bin"), 333);
    let info = r#"{"gameId":"123","name":"GOG Test Game","buildId":"v1"}"#;
    fs::write(game.join("goggame-123.info"), info).expect("info");
    let config = temp.path().join("heroic");
    fs::create_dir_all(config.join("gog_store")).expect("store");
    fs::write(
        config.join("gog_store/installed.json"),
        serde_json::json!({"installed":[{"appName":"123","install_path":game}]}).to_string(),
    )
    .expect("store");
    for (root, provider, launcher) in [
        (&game, "gog", "GOG"),
        (&config, "heroic-gog", "Heroic (GOG)"),
    ] {
        let output = diskclean()
            .arg("applications")
            .arg(root)
            .args(["--provider", provider, "--json"])
            .output()
            .expect("run");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("json");
        assert_eq!(report["applications"].as_array().expect("apps").len(), 1);
        assert_eq!(report["applications"][0]["name"], "GOG Test Game");
        assert_eq!(report["applications"][0]["launcher"], launcher);
        assert_eq!(report["applications"][0]["logical_bytes"], 333 + info.len());
        assert_eq!(report["applications"][0]["status"], "installed");
    }
    assert!(game.join("data.bin").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn applications_lutris_queries_installed_rows_and_never_changes_source() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("lutris");
    fs::create_dir_all(&root).expect("root");
    let game = temp.path().join("Game");
    write_file(&game.join("data.bin"), 345);
    let path = root.join("pga.db");
    let db = rusqlite::Connection::open(&path).expect("db");
    db.execute_batch("CREATE TABLE games(id INTEGER,name TEXT,directory TEXT,lastplayed INTEGER,installed INTEGER);INSERT INTO games VALUES(2,'Catalog','/not-installed',0,0);").expect("schema");
    db.execute(
        "INSERT INTO games VALUES(1,'Lutris Test',?1,123,1)",
        [game.to_string_lossy().as_ref()],
    )
    .expect("row");
    drop(db);
    let before = fs::read(&path).expect("before");
    let output = diskclean()
        .arg("applications")
        .arg(&root)
        .args(["--provider", "lutris", "--json"])
        .output()
        .expect("run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(report["applications"].as_array().expect("apps").len(), 1);
    assert_eq!(report["applications"][0]["launcher"], "Lutris");
    assert_eq!(report["applications"][0]["logical_bytes"], 345);
    assert_eq!(report["applications"][0]["last_played_unix"], 123);
    assert_eq!(fs::read(&path).expect("after"), before);
    assert_eq!(fs::read_dir(root).expect("dir").count(), 1);
}

#[test]
fn registry_provider_rejects_paths_and_other_providers_require_them() {
    diskclean()
        .args(["applications", "unused", "--provider", "registry"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "does not accept a filesystem path",
        ));
    diskclean()
        .args(["applications", "--provider", "epic"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("requires a source path"));
    #[cfg(not(windows))]
    diskclean()
        .args(["applications", "--provider", "registry", "--json"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("only available on Windows"));
}

#[test]
fn package_inventories_keep_estimates_separate_from_measurements() {
    let temp = tempdir().expect("temp");
    let dpkg = temp.path().join("dpkg");
    fs::create_dir(&dpkg).expect("dpkg");
    let deb = "Package: example\nStatus: install ok installed\nVersion: 1\nArchitecture: amd64\nInstalled-Size: 2\n\nPackage: removed\nStatus: deinstall ok config-files\n";
    fs::write(dpkg.join("status"), deb).expect("status");
    let pacman = temp.path().join("local");
    fs::create_dir_all(pacman.join("example-1-1")).expect("local");
    let desc = "%NAME%\nexample\n%VERSION%\n1-1\n%ARCH%\nx86_64\n%SIZE%\n2048\n";
    fs::write(pacman.join("example-1-1/desc"), desc).expect("desc");
    for (root, provider) in [(&dpkg, "dpkg"), (&pacman, "pacman")] {
        let output = diskclean()
            .arg("applications")
            .arg(root)
            .args(["--provider", provider, "--json"])
            .output()
            .expect("run");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("json");
        assert_eq!(report["applications"].as_array().expect("apps").len(), 1);
        let row = &report["applications"][0];
        assert_eq!(row["launcher"], provider);
        assert_eq!(row["status"], "installed");
        assert_eq!(row["reported_size_bytes"], 2048);
        assert!(row["allocated_bytes"].is_null());
        assert!(row["logical_bytes"].is_null());
        assert_eq!(row["install_path"], "");
        diskclean()
            .arg("applications")
            .arg(root)
            .args(["--provider", provider])
            .assert()
            .success()
            .stdout(predicates::str::contains("package estimate"));
    }
    assert_eq!(fs::read_to_string(dpkg.join("status")).expect("after"), deb);
    assert_eq!(
        fs::read_to_string(pacman.join("example-1-1/desc")).expect("after"),
        desc
    );
}
