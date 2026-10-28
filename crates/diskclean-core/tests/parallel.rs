//! Compare complete results, safety exclusions and progress across worker counts.
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use diskclean_core::{
    DuplicateControl, DuplicateError, DuplicateOptions, ScanControl, ScanOptions, find_duplicates,
    scan,
};

fn fixture(root: &Path) {
    for directory in ["flat", "nested/a", "nested/b", ".hidden", "excluded"] {
        fs::create_dir_all(root.join(directory)).expect("directory");
    }
    // More than one metadata batch, several size groups, full and prefix-only
    // matches, prefix collisions, unique sizes, and a zero-length file.
    for index in 0..600 {
        let mut contents = vec![b'a' + (index % 5) as u8; 128 + index % 7];
        contents[127] = (index % 3) as u8;
        let directory = if index % 4 == 0 { "nested/a" } else { "flat" };
        fs::write(root.join(directory).join(index.to_string()), contents).expect("file");
    }
    fs::write(root.join("nested/b/empty"), []).expect("empty");
    fs::write(root.join(".hidden/file"), b"hidden").expect("hidden");
    fs::write(root.join("excluded/file"), b"excluded").expect("excluded");
    #[cfg(unix)]
    {
        fs::hard_link(root.join("flat/1"), root.join("nested/b/hardlink")).expect("hardlink");
        std::os::unix::fs::symlink(root.join("flat"), root.join("linked")).expect("symlink");
    }
}

#[test]
fn scan_results_and_progress_match_across_workers() {
    let temp = tempfile::tempdir().expect("fixture");
    fixture(temp.path());
    let mut expected = None;
    for threads in [1, 4, 2, 0] {
        let control = ScanControl::new();
        let tree = scan(
            temp.path(),
            &ScanOptions {
                threads,
                skip_hidden: true,
                exclude: vec!["excluded".into()],
                ..ScanOptions::default()
            },
            &control,
        )
        .expect("scan");
        assert_eq!(tree.stats.files, control.snapshot().files);
        assert!(tree.issues.is_empty());
        assert_eq!(tree.stats.files, if cfg!(unix) { 602 } else { 601 });
        let mut report = serde_json::to_value(tree.report(true)).expect("report");
        report
            .as_object_mut()
            .expect("object")
            .remove("started_at_unix_ms");
        // Only timing and generation fields may vary between identical scans.
        if let Some(stats) = report.get_mut("stats").and_then(|s| s.as_object_mut()) {
            stats.remove("duration_ms");
        }
        report
            .as_object_mut()
            .expect("object")
            .remove("duration_ms");
        if let Some(expected) = &expected {
            assert_eq!(&report, expected);
        } else {
            expected = Some(report);
        }
    }
}

#[test]
fn duplicate_results_limits_and_progress_match_across_workers() {
    let temp = tempfile::tempdir().expect("fixture");
    fixture(temp.path());
    for max_files in [usize::MAX, 300] {
        for prefix_bytes in [16, 1024] {
            let mut expected = None;
            for threads in [1, 4, 2] {
                let control = DuplicateControl::new();
                let report = find_duplicates(
                    temp.path(),
                    &DuplicateOptions {
                        threads,
                        prefix_bytes,
                        skip_hidden: true,
                        max_files,
                        ..DuplicateOptions::default()
                    },
                    &control,
                )
                .expect("duplicates");
                assert!(report.files_scanned <= max_files as u64);
                assert_eq!(report.bytes_hashed, control.snapshot().bytes_hashed);
                assert_eq!(report.group_count() as u64, control.snapshot().groups_found);
                let mut value = serde_json::to_value(report).expect("report");
                let object = value.as_object_mut().expect("object");
                object.remove("generated_at_unix_ms");
                object.remove("duration_ms");
                if let Some(expected) = &expected {
                    assert_eq!(&value, expected);
                } else {
                    expected = Some(value);
                }
            }
        }
    }
}

#[test]
fn cancellation_during_parallel_full_hashing_returns_no_partial_report() {
    let temp = tempfile::tempdir().expect("fixture");
    for index in 0..8 {
        fs::File::create(temp.path().join(index.to_string()))
            .expect("file")
            .set_len(128 * 1024 * 1024)
            .expect("sparse file");
    }
    let control = DuplicateControl::new();
    std::thread::scope(|scope| {
        let job = scope.spawn(|| {
            find_duplicates(
                temp.path(),
                &DuplicateOptions {
                    threads: 4,
                    ..DuplicateOptions::default()
                },
                &control,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while control.snapshot().bytes_hashed < 1024 * 1024 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        control.cancel();
        assert!(matches!(
            job.join().expect("worker"),
            Err(DuplicateError::Cancelled)
        ));
        assert!(
            control.snapshot().bytes_hashed >= 1024 * 1024,
            "must reach full hashing"
        );
        assert_eq!(control.snapshot().groups_found, 0);
    });
}
