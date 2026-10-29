use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use diskclean_cleanup::{
    ApplyControl, AuditLog, CleanupContext, CleanupItem, ProtectedPaths, SystemTrash,
    apply_with_control, default_audit_path, discover_default,
};
use diskclean_core::{
    DuplicateControl, DuplicateOptions, DuplicateProgress, FileSummary, ProgressSnapshot,
    ScanControl, ScanOptions, Tree, find_duplicates, format_bytes, parse_size, scan,
};
use diskclean_platform::list_volumes;
use diskclean_rules::{ActionKind, Risk};
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
    /// List safe cleanup recommendations. Read-only.
    Recommend(RecommendArgs),
    /// Dry-run or apply reviewed cleanup recommendations.
    Clean(CleanArgs),
    /// Find exact duplicate files. Read-only.
    Duplicates(DuplicatesArgs),
    /// Inventory launcher games/applications and measure installation folders. Read-only.
    Applications(ApplicationsArgs),
}

#[derive(Debug, Args)]
struct ApplicationsArgs {
    /// Provider for an explicit path (Steam by default).
    #[arg(long, value_parser = ["steam", "epic", "legendary", "heroic", "gog", "heroic-gog"], requires = "path")]
    provider: Option<String>,
    /// Launcher library/config folder (Heroic GOG takes its Heroic config root).
    path: Option<PathBuf>,
    #[arg(long)]
    json: bool,
    /// Follow libraries and game content across mount boundaries.
    #[arg(long)]
    cross_filesystems: bool,
    #[arg(long, value_parser = parse_threads)]
    threads: Option<usize>,
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

    /// Worker threads: 0 = automatic (up to 8), 1 = sequential, maximum 32.
    #[arg(long, value_parser = parse_threads)]
    threads: Option<usize>,
}

#[derive(Debug, Args)]
struct VolumesArgs {
    /// Print machine-readable JSON to stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RecommendArgs {
    /// Print machine-readable JSON to stdout.
    #[arg(long)]
    json: bool,

    /// Include high-risk candidates in human-readable output (they are always
    /// listed in JSON).
    #[arg(long)]
    include_high_risk: bool,
}

#[derive(Debug, Args)]
struct CleanArgs {
    /// Show what would be moved, but do not touch anything.
    #[arg(long)]
    dry_run: bool,

    /// Move selected items to the operating-system trash.
    #[arg(long)]
    apply: bool,

    /// Required with --apply. Confirms that the listed items may be trashed.
    #[arg(long)]
    confirm: bool,

    /// Print machine-readable JSON to stdout.
    #[arg(long)]
    json: bool,

    /// Include high-risk items (for example build artifact folders).
    #[arg(long)]
    include_high_risk: bool,

    /// Only consider this rule id (repeatable).
    #[arg(long = "rule", value_name = "ID")]
    rules: Vec<String>,

    /// Only consider this category (repeatable), for example app_cache.
    #[arg(long = "category", value_name = "CATEGORY")]
    categories: Vec<String>,

    /// Only consider items at least this large (for example 100M).
    #[arg(long)]
    min_size: Option<String>,

    /// Override the audit log path. The default lives under XDG state or
    /// LocalAppData.
    #[arg(long, value_name = "PATH")]
    audit_log: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct DuplicatesArgs {
    /// Worker threads: 0 = automatic (up to 8), 1 = sequential, maximum 32.
    #[arg(long, value_parser = parse_threads)]
    threads: Option<usize>,
    /// Directory or volume to search.
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Print machine-readable JSON to stdout.
    #[arg(long)]
    json: bool,

    /// Ignore files smaller than this size (for example 100M or 1.5GiB).
    #[arg(long, default_value = "1M")]
    min_size: String,

    /// Bytes hashed before a full-file BLAKE3 hash.
    #[arg(long, default_value_t = 16 * 1024)]
    prefix_bytes: usize,

    /// Show at most this many duplicate groups in human-readable output.
    #[arg(long, default_value_t = 100)]
    top: usize,

    /// Skip dotfiles and platform-hidden entries.
    #[arg(long)]
    skip_hidden: bool,

    /// Cross filesystem/mount boundaries.
    #[arg(long)]
    cross_filesystems: bool,
}

#[derive(Debug, Serialize)]
struct CleanDryRun {
    items: Vec<CleanupItem>,
    total_bytes: u64,
    total_items: usize,
    skipped_high_risk: usize,
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
        Command::Recommend(args) => run_recommend(args),
        Command::Clean(args) => run_clean(args),
        Command::Duplicates(args) => run_duplicates(args),
        Command::Applications(args) => run_applications(args),
    }
}

fn run_applications(args: ApplicationsArgs) -> Result<()> {
    let roots = args.path.map(|path| vec![path]);
    let control = ScanControl::new();
    install_cancel_handler(control.clone());
    let report = diskclean_core::applications::discover_applications(
        roots.as_deref(),
        match args.provider.as_deref() {
            Some("gog") => diskclean_core::applications::ApplicationSource::Gog,
            Some("heroic-gog") => diskclean_core::applications::ApplicationSource::HeroicGog,
            Some("epic") => diskclean_core::applications::ApplicationSource::Epic,
            Some("legendary") => diskclean_core::applications::ApplicationSource::Legendary,
            Some("heroic") => diskclean_core::applications::ApplicationSource::Heroic,
            _ => diskclean_core::applications::ApplicationSource::Steam,
        },
        &ScanOptions {
            threads: args.threads.unwrap_or(0),
            cross_filesystems: args.cross_filesystems,
            ..Default::default()
        },
        &control,
    )?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        for app in &report.applications {
            println!(
                "{} · {} · {:?} · {}\n  {}",
                app.launcher,
                app.name,
                app.status,
                app.allocated_bytes
                    .map(format_bytes)
                    .unwrap_or_else(|| "unknown size".into()),
                app.install_path.display()
            );
        }
        for issue in &report.issues {
            eprintln!("{}: {}", issue.path.display(), issue.message);
        }
    }
    Ok(())
}

fn run_scan(args: ScanArgs) -> Result<()> {
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

fn run_recommend(args: RecommendArgs) -> Result<()> {
    let context = CleanupContext::from_environment();
    let audit_path = default_audit_path();
    let protected = protected_paths(&context, audit_path.as_deref());
    let report = discover_default(&context, &protected).context("cannot load cleanup rules")?;

    if args.json {
        return write_json(&report);
    }

    println!(
        "Cleanup recommendations: {} items, {} total",
        report.item_count(),
        format_bytes(report.total_bytes())
    );
    println!();

    for recommendation in &report.recommendations {
        let rule = &recommendation.rule;
        let risk = rule.risk.as_str();
        if rule.action == ActionKind::Trash {
            if rule.risk == Risk::High && !args.include_high_risk {
                println!(
                    "[high, hidden] {} — {} item(s), {} (use --include-high-risk)",
                    rule.title.en,
                    recommendation.count(),
                    format_bytes(recommendation.total_bytes())
                );
                continue;
            }
            println!(
                "[{risk}] {} — {} item(s), {}",
                rule.title.en,
                recommendation.count(),
                format_bytes(recommendation.total_bytes())
            );
            println!("  {}", rule.description.en);
            for item in recommendation.items.iter().take(50) {
                println!(
                    "  {:>10}  {}",
                    format_bytes(item.size()),
                    item.path.display()
                );
            }
            if recommendation.items.len() > 50 {
                println!("  ... and {} more", recommendation.items.len() - 50);
            }
        } else {
            println!("[{risk}] {} — recommendation only", rule.title.en);
            println!("  {}", rule.description.en);
            for command in &rule.commands {
                println!("  $ {}", command.command);
                if command.requires_admin {
                    println!("    requires administrator rights");
                }
            }
        }
        println!();
    }

    if !report.issues.is_empty() {
        println!("Skipped paths ({}):", report.issues.len());
        for issue in report.issues.iter().take(20) {
            println!("  {}: {}", issue.path.display(), issue.message);
        }
    }

    Ok(())
}

fn run_clean(args: CleanArgs) -> Result<()> {
    if args.dry_run == args.apply {
        bail!("choose exactly one of --dry-run or --apply");
    }
    if args.apply && !args.confirm {
        bail!("refusing to move files to trash without --confirm");
    }

    let min_size = match args.min_size.as_deref() {
        Some(value) => parse_size(value).map_err(anyhow::Error::msg)?,
        None => 0,
    };
    let context = CleanupContext::from_environment();
    let audit_path = args
        .audit_log
        .clone()
        .or_else(default_audit_path)
        .ok_or_else(|| anyhow::anyhow!("cannot determine the audit log path"))?;
    let protected = protected_paths(&context, Some(&audit_path));
    let report = discover_default(&context, &protected).context("cannot load cleanup rules")?;

    let mut skipped_high_risk = 0_usize;
    let selected = select_items(
        &report,
        &args.rules,
        &args.categories,
        min_size,
        args.include_high_risk,
        &mut skipped_high_risk,
    );

    if args.dry_run {
        let summary = CleanDryRun {
            total_bytes: selected
                .iter()
                .fold(0_u64, |total, item| total.saturating_add(item.size())),
            total_items: selected.len(),
            skipped_high_risk,
            items: selected,
        };
        if args.json {
            return write_json(&summary);
        }
        if summary.items.is_empty() {
            println!("Nothing selected for cleanup.");
            return Ok(());
        }
        println!(
            "Dry run: {} item(s), {} would move to trash",
            summary.total_items,
            format_bytes(summary.total_bytes)
        );
        for item in &summary.items {
            println!(
                "  {:>10}  [{:?}]  {}",
                format_bytes(item.size()),
                item.risk,
                item.path.display()
            );
        }
        if summary.skipped_high_risk > 0 {
            println!(
                "\n{} high-risk item(s) were skipped. Use --include-high-risk only after reviewing them.",
                summary.skipped_high_risk
            );
        }
        println!(
            "\nNothing was changed. Re-run with --apply --confirm to move these items to trash."
        );
        return Ok(());
    }

    if selected.is_empty() {
        println!("Nothing selected for cleanup.");
        return Ok(());
    }

    let mut audit = AuditLog::open(&audit_path)
        .with_context(|| format!("cannot open audit log {}", audit_path.display()))?;
    let mut backend = SystemTrash;
    let control = ApplyControl::default();
    let handler_control = control.clone();
    ctrlc::set_handler(move || handler_control.cancel())
        .context("cannot install cleanup cancellation handler")?;
    let result = apply_with_control(&selected, &protected, &mut backend, &mut audit, &control);

    if args.json {
        write_json(&result)?;
    } else {
        println!(
            "Moved {} item(s), {}, to the trash.",
            result.trashed_count,
            format_bytes(result.bytes_trashed)
        );
        for item in &result.results {
            if item.kind == diskclean_cleanup::ItemResultKind::Trashed {
                println!("  trashed  {}", item.path.display());
            } else {
                println!(
                    "  {:?}  {}  {}",
                    item.kind,
                    item.path.display(),
                    item.error.as_deref().unwrap_or("")
                );
            }
        }
        println!("Audit log: {}", audit_path.display());
        if let Some(reason) = &result.aborted_reason {
            println!("Stopped: {reason}");
        }
    }

    if result.aborted_reason.is_some() || result.failed_count() > 0 {
        bail!(
            "cleanup finished with {} failure(s); see {}",
            result.failed_count(),
            audit_path.display()
        );
    }
    Ok(())
}

fn run_duplicates(args: DuplicatesArgs) -> Result<()> {
    let min_size = parse_size(&args.min_size).map_err(anyhow::Error::msg)?;
    let options = DuplicateOptions {
        threads: args.threads.unwrap_or(0),
        min_size,
        prefix_bytes: args.prefix_bytes,
        skip_hidden: args.skip_hidden,
        cross_filesystems: args.cross_filesystems,
        ..DuplicateOptions::default()
    };

    let control = DuplicateControl::new();
    install_duplicate_cancel_handler(control.clone());
    let progress = HashProgressReporter::start(control.clone());
    let result = find_duplicates(&args.path, &options, &control);
    progress.stop();
    let report = result.context("duplicate search failed")?;

    if args.json {
        return write_json(&report);
    }

    println!(
        "Exact duplicates: {} groups, {} files, {} potentially reclaimable",
        report.group_count(),
        report.duplicate_file_count(),
        format_bytes(report.total_reclaimable())
    );
    println!("Root: {}", report.root_path.display());
    if report.hardlink_duplicates > 0 {
        println!(
            "Hardlinks: {} alias path(s) skipped (removing them would not free space)",
            report.hardlink_duplicates
        );
    }
    println!();

    for (index, group) in report.groups.iter().take(args.top).enumerate() {
        println!(
            "{:>4}. {} × {} — up to {} reclaimable",
            index + 1,
            group.copies(),
            format_bytes(group.logical_size),
            format_bytes(group.reclaimable_bytes)
        );
        for file in &group.files {
            println!(
                "      {:>10}  {}",
                format_bytes(file.size()),
                file.path.display()
            );
        }
        println!();
    }
    if report.groups.len() > args.top {
        println!("... and {} more groups", report.groups.len() - args.top);
    }

    if !report.issues.is_empty() {
        println!("Skipped paths ({}):", report.issues.len());
        for issue in report.issues.iter().take(20) {
            println!("  {}: {}", issue.path.display(), issue.message);
        }
        if report.issues.len() > 20 {
            println!("  ... and {} more", report.issues.len() - 20);
        }
    }

    Ok(())
}

fn select_items(
    report: &diskclean_cleanup::CleanupReport,
    rules: &[String],
    categories: &[String],
    min_size: u64,
    include_high_risk: bool,
    skipped_high_risk: &mut usize,
) -> Vec<CleanupItem> {
    let mut selected = Vec::new();
    for recommendation in &report.recommendations {
        let rule = &recommendation.rule;
        if rule.action != ActionKind::Trash {
            continue;
        }
        if !rules.is_empty() && !rules.iter().any(|id| id == &rule.id) {
            continue;
        }
        if !categories.is_empty()
            && !categories
                .iter()
                .any(|category| category == rule.category.as_str())
        {
            continue;
        }

        for item in &recommendation.items {
            if item.size() < min_size {
                continue;
            }
            if item.risk == Risk::High && !include_high_risk {
                *skipped_high_risk += 1;
                continue;
            }
            if item.risk == Risk::SystemOnly {
                continue;
            }
            selected.push(item.clone());
        }
    }
    selected
}

fn protected_paths(
    context: &CleanupContext,
    audit_path: Option<&std::path::Path>,
) -> ProtectedPaths {
    let audit_root = audit_path
        .and_then(std::path::Path::parent)
        .map(std::path::Path::to_path_buf);
    ProtectedPaths::for_context(context, audit_root)
}

fn scan_options(base: &ScanBase, top: usize) -> ScanOptions {
    ScanOptions {
        threads: base.threads.unwrap_or(0),
        cross_filesystems: base.cross_filesystems,
        max_depth: base.max_depth,
        exclude: base.exclude.clone(),
        skip_hidden: base.skip_hidden,
        top_files: top,
        follow_symlinks: base.follow_symlinks,
    }
}

fn install_cancel_handler(control: ScanControl) {
    if let Err(error) = ctrlc::set_handler(move || {
        eprintln!("\ninterrupt: cancelling scan...");
        control.cancel();
    }) {
        eprintln!("warning: cannot install Ctrl-C handler: {error}");
    }
}

fn install_duplicate_cancel_handler(control: DuplicateControl) {
    if let Err(error) = ctrlc::set_handler(move || {
        eprintln!("\ninterrupt: cancelling duplicate search...");
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

struct HashProgressReporter {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl HashProgressReporter {
    fn start(control: DuplicateControl) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        if !io::stderr().is_terminal() {
            return Self { stop, handle: None };
        }

        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                let snapshot = control.snapshot();
                eprint!("\r\x1b[2K{}", hash_progress_line(&snapshot));
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

fn hash_progress_line(snapshot: &DuplicateProgress) -> String {
    let current = snapshot
        .current_path
        .as_ref()
        .map_or_else(String::new, |path| shorten(&path.display().to_string(), 80));
    format!(
        "duplicates: {:?}, {} scanned, {} hashed, {} read, {} groups | {}",
        snapshot.phase,
        snapshot.files_scanned,
        snapshot.files_hashed,
        format_bytes(snapshot.bytes_hashed),
        snapshot.groups_found,
        current
    )
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

fn parse_threads(value: &str) -> Result<usize, String> {
    let count: usize = value
        .parse()
        .map_err(|_| "expected a thread count from 0 to 32".to_owned())?;
    if count > diskclean_core::parallel::MAX_WORKER_THREADS {
        return Err("expected a thread count from 0 to 32".to_owned());
    }
    Ok(count)
}
