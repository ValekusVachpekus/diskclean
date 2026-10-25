//! Synthetic scan benchmark.
//!
//! Run with:
//! `cargo run -p diskclean-core --example synthetic_scan --release`

use std::error::Error;
use std::fs;
use std::time::Instant;

use diskclean_core::{ScanControl, ScanOptions, scan};

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

    let control = ScanControl::new();
    let started = Instant::now();
    let tree = scan(root, &ScanOptions::default(), &control)?;
    let elapsed = started.elapsed();

    let files = tree.stats.files;
    let seconds = elapsed.as_secs_f64();
    let files_per_second = if seconds > 0.0 {
        files as f64 / seconds
    } else {
        0.0
    };

    println!(
        "synthetic scan: {} files, {} dirs, {} in {:.3}s ({:.0} files/s)",
        files,
        tree.stats.directories,
        diskclean_core::format_bytes(tree.stats.allocated_bytes),
        seconds,
        files_per_second
    );

    Ok(())
}
