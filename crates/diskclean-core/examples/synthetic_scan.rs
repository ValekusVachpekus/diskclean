//! Synthetic scan benchmark.
//!
//! Run with:
//! `cargo run -p diskclean-core --example synthetic_scan --release`

use std::error::Error;
use std::fs;
use std::time::Instant;

use diskclean_core::{
    DuplicateControl, DuplicateOptions, ScanControl, ScanOptions, find_duplicates, scan,
};

const DIRS: usize = 64;
const FILES_PER_DIR: usize = 64;
const FILE_SIZE: usize = 4096;

fn main() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let payload = vec![0_u8; FILE_SIZE];

    for dir_index in 0..DIRS {
        let dir = root.join(format!("dir-{dir_index:03}"));
        fs::create_dir_all(&dir)?;
        for file_index in 0..FILES_PER_DIR {
            let path = dir.join(format!("file-{file_index:03}.bin"));
            fs::write(path, &payload)?;
        }
    }

    // Include full-file hashing, not only files that fit in the prefix.
    let large_payload = vec![b'x'; 256 * 1024];
    let large = root.join("large");
    fs::create_dir(&large)?;
    for index in 0..64 {
        fs::write(large.join(index.to_string()), &large_payload)?;
    }

    println!(
        "Warm-cache synthetic benchmark; medians of 5 interleaved runs. Includes pool startup.
Storage/cache and workload determine speedup; this is not an HDD or cold-cache benchmark."
    );
    let configurations = [1, 4, 0];
    let mut scans = [Vec::new(), Vec::new(), Vec::new()];
    let mut duplicates = [Vec::new(), Vec::new(), Vec::new()];
    for round in 0..6 {
        for (index, threads) in configurations.into_iter().enumerate() {
            let started = Instant::now();
            let tree = scan(
                root,
                &ScanOptions {
                    threads,
                    ..ScanOptions::default()
                },
                &ScanControl::new(),
            )?;
            let scan_elapsed = started.elapsed();
            let started = Instant::now();
            let report = find_duplicates(
                root,
                &DuplicateOptions {
                    threads,
                    ..DuplicateOptions::default()
                },
                &DuplicateControl::new(),
            )?;
            let hash_elapsed = started.elapsed();
            assert_eq!(tree.stats.files, (DIRS * FILES_PER_DIR + 64) as u64);
            assert_eq!(report.group_count(), 2);
            if round > 0 {
                scans[index].push(scan_elapsed);
                duplicates[index].push(hash_elapsed);
            }
        }
    }
    for (index, threads) in configurations.into_iter().enumerate() {
        scans[index].sort();
        duplicates[index].sort();
        println!(
            "threads={threads} (effective {}): scan {:.3}s, duplicates {:.3}s",
            diskclean_core::parallel::worker_threads(threads),
            scans[index][2].as_secs_f64(),
            duplicates[index][2].as_secs_f64()
        );
    }
    Ok(())
}
