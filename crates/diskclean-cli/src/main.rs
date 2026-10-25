use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use diskclean_core::{
    FileSummary, ProgressSnapshot, ScanControl, ScanOptions, Tree, format_bytes, parse_size, scan,
};
use diskclean_platform::list_volumes;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "diskclean",
    version,
    about = "Read-only disk usage analyzer and safe cleanup assistant"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Scan a directory and report sizes, largest files, and extensions.
    Scan(ScanArgs),
    /// Scan and print only the largest files.
    Largest(LargestArgs),
    /// List mounted volumes.
    Volumes(VolumesArgs),
}

#[derive(Debug, Args)]
struct ScanArgs {
    #[command(flatten)]
    base: ScanBase,

    /// Include the full tree in JSON output; implies --json.
    #[arg(long)]
    tree: bool,

    /// Number of largest files to keep in the report.
    #[arg(long, default_value_t = 100)]
    top: usize,
}

#[derive(Debug, Args)]
struct LargestArgs {
    #[command(flatten)]
    base: ScanBase,

    /// Number of largest files to print.
    #[arg(long, default_value_t = 50)]
    top: usize,

    /// Only show files at least this large (for example 100M or 1.5GiB).
    #[arg(long)]
    min_size: Option<String>,
}

#[derive(Debug, Args)]
struct ScanBase {
    /// Directory or volume to scan.
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Print machine-readable JSON to stdout.
    #[arg(long)]
    json: bool,

    /// Do not enter this path (repeatable).
    #[arg(long = "exclude", value_name = "PATH")]
    exclude: Vec<PathBuf>,

    /// Maximum directory depth; 0 scans only the root directory.
    #[arg(long)]
    max_depth: Option<u16>,

    /// Skip dotfiles and platform-hidden entries.
    #[arg(long)]
    skip_hidden: bool,

    /// Cross filesystem/mount boundaries.
    #[arg(long)]
    cross_filesystems: bool,

    /// Follow symlinks (not implemented yet; rejected explicitly).
    #[arg(long)]
    follow_symlinks: bool,

    /// Number of scanner worker threads.
    #[arg(long)]
    threads: Option<usize>,
}

#[derive(Debug, Args)]
struct VolumesArgs {
    /// Print machine-readable JSON to stdout.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Scan(args) => run_scan(args),
        Command::Largest(args) => run_largest(args),
        Command::Volumes(args) => run_volumes(args),
    }
}

fn run_scan(args: ScanArgs) -> Result<()> {
    configure_threads(args.base.threads)?;

    let control = ScanControl::new();
    install_cancel_handler(control.clone());
    let progress = ProgressReporter::start(control.clone());

    let options = scan_options(&args.base, args.top);
    let result = scan(&args.base.path, &options, &control);
    progress.stop();
    let tree = result.context("scan failed")?;

    if args.base.json || args.tree {
        write_json(&tree.report(args.tree))
    } else {
        print_scan_summary(&tree);
        Ok(())
    }
}

fn run_largest(args: LargestArgs) -> Result<()> {
    configure_threads(args.base.threads)?;

    let control = ScanControl::new();
    install_cancel_handler(control.clone());
    let progress = ProgressReporter::start(control.clone());

    let options = scan_options(&args.base, args.top);
    let result = scan(&args.base.path, &options, &control);
    progress.stop();
    let tree = result.context("scan failed")?;

    let min_size = match args.min_size.as_deref() {
        Some(value) => parse_size(value).map_err(anyhow::Error::msg)?,
        None => 0,
    };
    let files: Vec<FileSummary> = tree
        .top_file_summaries()
        .into_iter()
        .filter(|file| file.allocated_size >= min_size)
        .collect();

    if args.base.json {
        write_json(&files)
    } else {
        print_largest(&files);
        Ok(())
    }
}

fn run_volumes(args: VolumesArgs) -> Result<()> {
    let volumes = list_volumes();

    if args.json {
        write_json(&volumes)
    } else {
        println!(
            "{:<24} {:<12} {:>12} {:>12}  NAME",
            "MOUNT", "FILESYSTEM", "SIZE", "FREE"
        );
        for volume in volumes {
            println!(
                "{:<24} {:<12} {:>12} {:>12}  {}",
                volume.mount_point,
                volume.file_system,
                format_bytes(volume.total_space),
                format_bytes(volume.available_space),
                volume.name
            );
        }
        Ok(())
    }
}

fn scan_options(base: &ScanBase, top: usize) -> ScanOptions {
    ScanOptions {
        cross_filesystems: base.cross_filesystems,
        max_depth: base.max_depth,
        exclude: base.exclude.clone(),
        skip_hidden: base.skip_hidden,
        top_files: top,
        follow_symlinks: base.follow_symlinks,
    }
}

fn configure_threads(threads: Option<usize>) -> Result<()> {
    let Some(threads) = threads else {
        return Ok(());
    };
    if threads == 0 {
        bail!("--threads must be at least 1");
    }

    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()
        .map_err(|error| anyhow::anyhow!("cannot configure scanner threads: {error}"))
}

fn install_cancel_handler(control: ScanControl) {
    if let Err(error) = ctrlc::set_handler(move || {
        eprintln!("\ninterrupt: cancelling scan...");
        control.cancel();
    }) {
        eprintln!("warning: cannot install Ctrl-C handler: {error}");
    }
}

fn write_json<T: Serialize>(value: &T) -> Result<()> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer_pretty(&mut lock, value).context("cannot serialize JSON")?;
    writeln!(lock).context("cannot write JSON")?;
    Ok(())
}

fn print_scan_summary(tree: &Tree) {
    println!("Root:        {}", tree.root_path.display());
    println!("Duration:    {}", format_duration(tree.duration_ms));
    println!("Directories: {}", tree.stats.directories);
    println!("Files:       {}", tree.stats.files);
    println!("Symlinks:    {}", tree.stats.symlinks);
    println!("Logical:     {}", format_bytes(tree.stats.logical_bytes));
    println!("On disk:     {}", format_bytes(tree.stats.allocated_bytes));

    if tree.stats.hardlink_duplicates > 0 {
        println!(
            "Hardlinks:   {} duplicate paths not counted again",
            tree.stats.hardlink_duplicates
        );
    }
    if tree.stats.skipped_mounts > 0
        || tree.stats.skipped_excluded > 0
        || tree.stats.skipped_depth > 0
    {
        println!(
            "Skipped:     {} mounts, {} excluded/hidden, {} by depth",
            tree.stats.skipped_mounts, tree.stats.skipped_excluded, tree.stats.skipped_depth
        );
    }

    let files = tree.top_file_summaries();
    if !files.is_empty() {
        println!("\nLargest files:");
        print_file_list(&files);
    }

    if !tree.issues.is_empty() {
        println!("\nIssues ({}):", tree.issues.len());
        for issue in tree.issues.iter().take(20) {
            println!(
                "  {}: {} ({:?})",
                issue.path.display(),
                issue.message,
                issue.kind
            );
        }
        if tree.issues.len() > 20 {
            println!("  ... and {} more", tree.issues.len() - 20);
        }
    }
}

fn print_largest(files: &[FileSummary]) {
    if files.is_empty() {
        println!("No files matched.");
        return;
    }

    println!("Largest files:");
    print_file_list(files);
}

fn print_file_list(files: &[FileSummary]) {
    for (index, file) in files.iter().enumerate() {
        let marker = if file.is_hardlink_duplicate {
            " [hardlink]"
        } else {
            ""
        };
        println!(
            "{:>4}. {:>12}  {}{}",
            index + 1,
            format_bytes(file.allocated_size),
            file.path.display(),
            marker
        );
    }
}

fn format_duration(milliseconds: u64) -> String {
    if milliseconds < 1000 {
        format!("{milliseconds} ms")
    } else {
        format!("{:.2} s", milliseconds as f64 / 1000.0)
    }
}

struct ProgressReporter {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ProgressReporter {
    fn start(control: ScanControl) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        if !io::stderr().is_terminal() {
            return Self { stop, handle: None };
        }

        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                let snapshot = control.snapshot();
                eprint!("\r\x1b[2K{}", progress_line(&snapshot));
                let _ = io::stderr().flush();
                thread::sleep(Duration::from_millis(200));
            }
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
            eprintln!();
        }
    }
}

fn progress_line(snapshot: &ProgressSnapshot) -> String {
    let current = snapshot
        .current_path
        .as_ref()
        .map_or_else(String::new, |path| shorten(&path.display().to_string(), 80));

    format!(
        "scanning: {} files, {} dirs, {} on disk, {} errors | {}",
        snapshot.files,
        snapshot.directories,
        format_bytes(snapshot.allocated_bytes),
        snapshot.errors,
        current
    )
}

fn shorten(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }

    let tail: String = value
        .chars()
        .rev()
        .take(max_chars.saturating_sub(3))
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("...{tail}")
}
