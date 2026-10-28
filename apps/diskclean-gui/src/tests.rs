//! Headless GUI regression tests using `egui_kittest`.
//!
//! These tests render the real application shell into an off-screen egui
//! context. They never open a window or touch the user's desktop. Filesystem
//! access is limited to disposable temporary directories created by a test.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use diskclean_cleanup::{CleanupItem, CleanupReport, Recommendation};
use diskclean_core::{
    DuplicateControl, DuplicateOptions, DuplicateReport, ExtensionTotals, Node, NodeId, NodeKind,
    ROOT_ID, ScanStats, Tree, find_duplicates,
};
use diskclean_rules::{ActionKind, Category, LocalizedText, Risk, Rule, SystemCommand};
use eframe::egui::{self, Key, Modifiers, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::{DiskCleanupApp, Tab, Workspace};
use crate::i18n::Language;
use crate::settings::Settings;
use crate::ui::theme::{self, ThemeMode};
use crate::views::cleanup::RiskFilter;

const GIB: u64 = 1024 * 1024 * 1024;

fn large_analysis_fixture(count: usize) -> Tree {
    let mut tree = fixture_tree();
    let template = tree
        .nodes
        .iter()
        .find(|node| node.kind == NodeKind::File)
        .expect("file")
        .clone();
    tree.nodes.truncate(1);
    tree.nodes[0].first_child = Some(1);
    tree.nodes[0].allocated_size = (1..=count as u64).sum();
    tree.nodes[0].file_count = count as u64;
    for id in 1..=count {
        let mut node = template.clone();
        node.parent = Some(0);
        node.first_child = None;
        node.next_sibling = (id < count).then_some(id as u32 + 1);
        node.name = format!("file-{id:06}.bin").into();
        node.allocated_size = id as u64;
        node.logical_size = id as u64;
        node.modified_unix = id as i64;
        tree.nodes.push(node);
    }
    tree.top_files = vec![count as u32];
    tree
}

#[test]
fn full_tree_queries_sort_and_find_files_outside_top_summary() {
    use crate::app::{SortKey, SortState};
    let tree = Arc::new(large_analysis_fixture(3_000));
    let mut app = DiskCleanupApp::new_for_test(english_settings(), Some(Arc::clone(&tree)));
    app.update_analysis(&tree);
    assert_eq!(app.analysis_view.files.len(), 3_000);
    assert_eq!(app.analysis_view.files[0], 3_000);
    app.file_sort = SortState {
        key: SortKey::Modified,
        descending: false,
    };
    app.update_analysis(&tree);
    assert_eq!(app.analysis_view.files[0], 1);
    app.filters.search = "file-000001".into();
    app.update_analysis(&tree);
    assert_eq!(&**app.analysis_view.files, &[1]);
    assert_eq!(app.analysis_view.extensions[0].files, 1);
    app.filters.min_size = "2".into();
    app.update_analysis(&tree);
    assert!(app.analysis_view.files.is_empty());
    assert!(app.analysis_view.extensions.is_empty());
}

#[test]
fn issue_queries_filter_paths_and_categories_independently_of_sizes() {
    let mut tree = fixture_tree();
    tree.issues = vec![
        diskclean_core::ScanIssue::new(
            "/blocked/one".into(),
            diskclean_core::IssueKind::PermissionDenied,
            "details",
        ),
        diskclean_core::ScanIssue::new(
            "/missing/two".into(),
            diskclean_core::IssueKind::NotFound,
            "details",
        ),
    ];
    let tree = Arc::new(tree);
    let mut app = DiskCleanupApp::new_for_test(english_settings(), Some(Arc::clone(&tree)));
    app.filters.min_size = "100G".into();
    app.filters.search = "blocked".into();
    app.update_analysis(&tree);
    assert_eq!(&**app.analysis_view.issues, &[0]);
    app.issue_kind = Some(diskclean_core::IssueKind::NotFound);
    app.update_analysis(&tree);
    assert!(app.analysis_view.issues.is_empty());
}

#[test]
fn hundred_thousand_node_queries_reuse_cached_rows_and_render() {
    let start = std::time::Instant::now();
    let tree = Arc::new(large_analysis_fixture(100_000));
    let mut app = DiskCleanupApp::new_for_test(english_settings(), Some(Arc::clone(&tree)));
    app.update_analysis(&tree);
    let rows = Arc::clone(&app.analysis_view.files);
    for _ in 0..100 {
        app.update_analysis(&tree);
    }
    assert!(Arc::ptr_eq(&rows, &app.analysis_view.files));
    assert_eq!(rows.len(), 100_000);
    app.tab = Tab::Files;
    let mut harness = harness(app, Vec2::new(1280.0, 820.0));
    harness.run();
    assert_eq!(harness.state().analysis_view.files.len(), 100_000);
    eprintln!("100k-node build/query/render: {:?}", start.elapsed());
}

fn node(
    parent: Option<NodeId>,
    first_child: Option<NodeId>,
    next_sibling: Option<NodeId>,
    name: &str,
    kind: NodeKind,
    size: u64,
    depth: u16,
) -> Node {
    let file_count = u64::from(kind == NodeKind::File);
    let dir_count = u64::from(kind == NodeKind::Directory);
    Node {
        parent,
        first_child,
        next_sibling,
        name: OsString::from(name),
        kind,
        logical_size: size,
        allocated_size: size,
        actual_logical_size: size,
        actual_allocated_size: size,
        file_count,
        dir_count,
        modified_unix: 1_700_000_000,
        dev: 1,
        ino: u64::from(depth) + 1,
        nlink: 1,
        is_hidden: false,
        is_hardlink_duplicate: false,
        depth,
    }
}

/// A small deterministic tree:
///
/// ```text
/// /virtual/fixture         10 GiB
/// ├── media                 6 GiB
/// │   ├── movie.mkv         4 GiB
/// │   └── photo.jpg         2 GiB
/// ├── cache                 3 GiB
/// │   └── blob.bin          3 GiB
/// └── notes.txt             1 GiB
/// ```
fn fixture_tree() -> Tree {
    let media = 1;
    let movie = 2;
    let photo = 3;
    let cache = 4;
    let blob = 5;
    let notes = 6;

    let mut extensions = BTreeMap::new();
    for (extension, size) in [
        ("mkv", 4 * GIB),
        ("jpg", 2 * GIB),
        ("bin", 3 * GIB),
        ("txt", GIB),
    ] {
        extensions.insert(
            extension.to_owned(),
            ExtensionTotals {
                files: 1,
                logical_bytes: size,
                allocated_bytes: size,
            },
        );
    }

    let mut tree = Tree {
        root_path: PathBuf::from("/virtual/fixture"),
        nodes: vec![
            node(
                None,
                Some(media),
                None,
                "fixture",
                NodeKind::Directory,
                10 * GIB,
                0,
            ),
            node(
                Some(ROOT_ID),
                Some(movie),
                Some(cache),
                "media",
                NodeKind::Directory,
                6 * GIB,
                1,
            ),
            node(
                Some(media),
                None,
                Some(photo),
                "movie.mkv",
                NodeKind::File,
                4 * GIB,
                2,
            ),
            node(
                Some(media),
                None,
                None,
                "photo.jpg",
                NodeKind::File,
                2 * GIB,
                2,
            ),
            node(
                Some(ROOT_ID),
                Some(blob),
                Some(notes),
                "cache",
                NodeKind::Directory,
                3 * GIB,
                1,
            ),
            node(
                Some(cache),
                None,
                None,
                "blob.bin",
                NodeKind::File,
                3 * GIB,
                2,
            ),
            node(
                Some(ROOT_ID),
                None,
                None,
                "notes.txt",
                NodeKind::File,
                GIB,
                1,
            ),
        ],
        top_files: vec![movie, blob, photo, notes],
        extensions,
        issues: Vec::new(),
        stats: ScanStats {
            directories: 3,
            files: 4,
            logical_bytes: 10 * GIB,
            allocated_bytes: 10 * GIB,
            ..ScanStats::default()
        },
        started_at_unix_ms: 1_700_000_000_000,
        duration_ms: 24,
    };
    tree.nodes[ROOT_ID as usize].file_count = 4;
    tree.nodes[ROOT_ID as usize].dir_count = 3;
    tree.nodes[1].file_count = 2;
    tree.nodes[1].dir_count = 1;
    tree.nodes[4].file_count = 1;
    tree.nodes[4].dir_count = 1;
    tree
}

fn english_settings() -> Settings {
    Settings {
        language: Language::English,
        ..Settings::default()
    }
}

fn test_app(settings: Settings, with_tree: bool) -> DiskCleanupApp {
    let tree = with_tree.then(|| Arc::new(fixture_tree()));
    DiskCleanupApp::new_for_test(settings, tree)
}

fn harness(app: DiskCleanupApp, size: Vec2) -> Harness<'static, DiskCleanupApp> {
    harness_at_scale(app, size, 1.0)
}

fn harness_at_scale(
    app: DiskCleanupApp,
    size: Vec2,
    pixels_per_point: f32,
) -> Harness<'static, DiskCleanupApp> {
    Harness::builder()
        .with_size(size)
        .with_pixels_per_point(pixels_per_point)
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            theme::install_fonts(ctx);
            theme::apply(ctx, ThemeMode::Dark);
        })
        .build_ui_state(|ui, app| app.show(ui), app)
}

#[test]
fn empty_overview_renders_without_a_scan() {
    let app = test_app(english_settings(), false);
    let harness = harness(app, Vec2::new(1280.0, 820.0));

    assert!(harness.query_by_label("Scan home folder").is_some());
    assert!(
        harness
            .query_by_label_contains("See what is using your disk")
            .is_some()
    );
    assert!(harness.query_by_label_contains("Disk Cleanup").is_some());
}

#[test]
fn analyze_shell_exposes_tabs_and_selection() {
    let mut app = test_app(english_settings(), true);
    app.tab = Tab::Folders;
    let harness = harness(app, Vec2::new(1280.0, 820.0));

    assert!(
        harness
            .query_all_by_label_contains("Folders")
            .next()
            .is_some()
    );
    assert!(harness.query_by_label_contains("Selection").is_some());
    assert!(
        harness
            .query_all_by_label_contains("media")
            .next()
            .is_some()
    );
    assert!(
        harness
            .query_all_by_label_contains("cache")
            .next()
            .is_some()
    );
}

#[test]
fn shell_stays_usable_at_minimum_window_size() {
    let app = test_app(Settings::default(), true);
    let _harness = harness(app, Vec2::new(900.0, 600.0));
}

#[test]
fn shell_reflows_at_high_dpi_in_both_themes_and_languages() {
    for (theme, pixels_per_point, language, scan, folders) in [
        (ThemeMode::Dark, 1.25, Language::English, "Scan", "Folders"),
        (
            ThemeMode::Light,
            2.0,
            Language::Russian,
            "Сканировать",
            "Папки",
        ),
    ] {
        let mut settings = english_settings();
        settings.theme = theme;
        settings.language = language;
        let app = test_app(settings, true);
        let harness = harness_at_scale(app, Vec2::new(900.0, 600.0), pixels_per_point);

        assert_eq!(harness.ctx.pixels_per_point(), pixels_per_point);
        assert_eq!(harness.state().palette.is_dark(), theme == ThemeMode::Dark);
        assert!(harness.query_by_label(scan).is_some());
        assert!(
            harness
                .query_all_by_label_contains(folders)
                .next()
                .is_some()
        );
    }
}

#[test]
fn language_switch_updates_shell_at_runtime() {
    let mut app = test_app(Settings::default(), false);
    app.settings.language = Language::English;
    let mut harness = harness(app, Vec2::new(1280.0, 820.0));

    assert!(harness.query_by_label("Scan").is_some());

    harness.state_mut().settings.language = Language::Russian;
    harness.run();

    assert!(harness.query_by_label("Сканировать").is_some());
    assert!(
        harness
            .query_by_label_contains("Посмотрите, что занимает диск")
            .is_some()
    );
}

#[test]
fn theme_switch_rebuilds_the_palette_at_runtime() {
    let mut app = test_app(Settings::default(), false);
    app.settings.theme = ThemeMode::Dark;
    let mut harness = harness(app, Vec2::new(1280.0, 820.0));

    assert!(harness.state().palette.is_dark());
    assert_eq!(harness.ctx.theme(), egui::Theme::Dark);

    harness.state_mut().settings.theme = ThemeMode::Light;
    harness.run();

    assert!(!harness.state().palette.is_dark());
    assert_eq!(harness.ctx.theme(), egui::Theme::Light);
}

#[test]
fn search_and_size_filters_apply_to_visible_children() {
    let mut app = test_app(Settings::default(), true);
    let tree = Arc::clone(app.tree.as_ref().expect("fixture tree"));

    assert_eq!(app.visible_children(&tree, ROOT_ID).len(), 3);

    app.filters.search = "mov".to_owned();
    assert_eq!(
        app.visible_children(&tree, ROOT_ID),
        Vec::<NodeId>::new(),
        "children of the root do not include nested matches"
    );
    let media = tree
        .nodes
        .iter()
        .position(|node| node.display_name() == "media")
        .expect("media node") as NodeId;
    assert_eq!(app.visible_children(&tree, media), vec![2]);

    app.filters.search.clear();
    app.filters.min_size = "4 GiB".to_owned();
    assert_eq!(app.visible_children(&tree, ROOT_ID), vec![1]);
    assert!(app.filters_active() >= 1);

    app.clear_filters();
    assert_eq!(app.visible_children(&tree, ROOT_ID).len(), 3);
}

#[test]
fn settings_round_trip_keeps_ui_state() {
    let mut settings = Settings {
        theme: ThemeMode::Light,
        language: Language::Russian,
        skip_hidden: true,
        cross_filesystems: true,
        inspector_open: false,
        last_path: "/virtual/fixture".to_owned(),
        recent_roots: vec!["/virtual/fixture".to_owned()],
        sidebar_width: 254.0,
        inspector_width: 320.0,
    };

    let mut storage = TestStorage::default();
    settings.save(&mut storage);
    let loaded = Settings::load(Some(&storage));

    assert_eq!(loaded.theme, ThemeMode::Light);
    assert_eq!(loaded.language, Language::Russian);
    assert!(loaded.skip_hidden);
    assert!(loaded.cross_filesystems);
    assert!(!loaded.inspector_open);
    assert_eq!(loaded.last_path, "/virtual/fixture");
    assert_eq!(loaded.recent_roots, vec!["/virtual/fixture"]);
    assert_eq!(loaded.sidebar_width, 254.0);
    assert_eq!(loaded.inspector_width, 320.0);

    settings.reset_preferences();
    assert_eq!(settings.theme, ThemeMode::System);
    assert_eq!(settings.language, Language::System);
    assert_eq!(settings.last_path, "/virtual/fixture");
    assert_eq!(settings.recent_roots, vec!["/virtual/fixture"]);
}

fn cleanup_fixture() -> (tempfile::TempDir, CleanupReport, CleanupItem, CleanupItem) {
    let temp = tempfile::tempdir().expect("tempdir");
    let safe_path = temp.path().join("cache.bin");
    let risky_path = temp.path().join("important.bin");
    fs::write(&safe_path, vec![b'a'; 1024]).expect("safe fixture");
    fs::write(&risky_path, vec![b'b'; 2048]).expect("high-risk fixture");

    let safe = CleanupItem::new(&safe_path, "test-cache", Category::Temporary, Risk::Low)
        .expect("safe snapshot");
    let high = CleanupItem::new(
        &risky_path,
        "test-important",
        Category::LargeOldFiles,
        Risk::High,
    )
    .expect("high-risk snapshot");

    let safe_rule = Rule {
        id: "test-cache".to_owned(),
        category: Category::Temporary,
        risk: Risk::Low,
        title: LocalizedText::english("Test cache"),
        description: LocalizedText::english("A safe test cache location."),
        action: ActionKind::Trash,
        locations: Vec::new(),
        commands: Vec::new(),
    };
    let high_rule = Rule {
        id: "test-important".to_owned(),
        category: Category::LargeOldFiles,
        risk: Risk::High,
        title: LocalizedText::english("Large old files"),
        description: LocalizedText::english("Review these files carefully."),
        action: ActionKind::Trash,
        locations: Vec::new(),
        commands: Vec::new(),
    };
    let command_rule = Rule {
        id: "test-command".to_owned(),
        category: Category::System,
        risk: Risk::SystemOnly,
        title: LocalizedText::english("System command"),
        description: LocalizedText::english("Recommendation only."),
        action: ActionKind::SystemCommand,
        locations: Vec::new(),
        commands: vec![SystemCommand {
            label: LocalizedText::english("Clean system cache"),
            command: "sudo systemctl clean".to_owned(),
            requires_admin: true,
            note: None,
        }],
    };

    let report = CleanupReport {
        generated_at_unix_ms: 1_700_000_000_000,
        recommendations: vec![
            Recommendation {
                rule: safe_rule,
                items: vec![safe.clone()],
            },
            Recommendation {
                rule: high_rule,
                items: vec![high.clone()],
            },
            Recommendation {
                rule: command_rule,
                items: Vec::new(),
            },
        ],
        issues: Vec::new(),
    };

    (temp, report, safe, high)
}

#[test]
fn cleanup_workspace_renders_and_never_preselects() {
    let (_temp, report, safe, high) = cleanup_fixture();
    let mut app = test_app(english_settings(), false);
    app.workspace = Workspace::Cleanup;
    app.cleanup.report = Some(report.clone());

    assert!(app.cleanup.selected.is_empty());
    app.cleanup.select_safe(&report);
    assert_eq!(app.cleanup.selected.len(), 1);
    assert!(app.cleanup.selected.contains_key(&safe.path));
    assert!(!app.cleanup.selected.contains_key(&high.path));
    assert!(!app.cleanup.selected_high_risk());

    app.cleanup.selected.insert(high.path.clone(), high);
    assert!(app.cleanup.selected_high_risk());
    assert!(
        !app.cleanup.can_apply(),
        "high-risk items require explicit acknowledgement"
    );
    app.cleanup.acknowledge_high_risk = true;
    assert!(app.cleanup.can_apply());

    app.cleanup.clear_selection();
    app.cleanup.select_safe(&report);
    let harness = harness(app, Vec2::new(1280.0, 820.0));

    assert!(harness.query_by_label("Test cache").is_some());
    assert!(
        harness
            .query_all_by_label_contains("Review basket")
            .next()
            .is_some()
    );
    assert!(
        harness
            .query_all_by_label_contains("System command")
            .next()
            .is_some()
    );
}

#[test]
fn cleanup_commands_filter_does_not_claim_there_are_no_recommendations() {
    let (_temp, mut report, _, _) = cleanup_fixture();
    report
        .recommendations
        .retain(|recommendation| recommendation.rule.action == ActionKind::SystemCommand);

    let mut app = test_app(english_settings(), false);
    app.workspace = Workspace::Cleanup;
    app.cleanup.report = Some(report);
    app.cleanup.risk_filter = RiskFilter::Commands;
    let harness = harness(app, Vec2::new(1280.0, 820.0));

    assert!(
        harness
            .query_all_by_label_contains("System command")
            .next()
            .is_some()
    );
    assert!(
        harness
            .query_all_by_label_contains("No cleanup candidates")
            .next()
            .is_none()
    );
}

#[test]
fn workspace_shortcuts_switch_between_analyze_cleanup_and_duplicates() {
    let mut harness = harness(
        test_app(english_settings(), false),
        Vec2::new(1280.0, 820.0),
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    harness.run();
    assert_eq!(harness.state().workspace, Workspace::Cleanup);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Num3);
    harness.run();
    assert_eq!(harness.state().workspace, Workspace::Duplicates);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Num1);
    harness.run();
    assert_eq!(harness.state().workspace, Workspace::Analyze);
}

#[test]
fn loading_a_different_root_invalidates_duplicate_results() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("copy.bin");
    fs::write(&path, b"duplicate fixture").expect("fixture file");
    fs::write(temp.path().join("keeper.bin"), b"duplicate fixture").expect("keeper");
    let scan = find_duplicates(
        temp.path(),
        &DuplicateOptions::default(),
        &DuplicateControl::new(),
    )
    .expect("duplicates");
    let group = &scan.groups[0];
    let item = CleanupItem::from_duplicate(&group.files[0], &group.files[1], &group.fingerprint)
        .expect("item evidence");
    let empty_report = DuplicateReport {
        root_path: PathBuf::from("/virtual/fixture"),
        generated_at_unix_ms: 0,
        groups: Vec::new(),
        issues: Vec::new(),
        files_scanned: 0,
        hardlink_duplicates: 0,
        bytes_hashed: 0,
        duration_ms: 0,
    };

    let mut app = test_app(english_settings(), true);
    app.duplicates.report = Some(empty_report.clone());
    app.duplicates.selected.insert(path.clone(), item);
    app.duplicates
        .keepers
        .insert("fingerprint".to_owned(), path.clone());

    app.load_cached(PathBuf::from("/virtual/fixture"), Arc::new(fixture_tree()));
    assert!(
        app.duplicates.report.is_some(),
        "reopening the same cached root keeps valid duplicate results"
    );

    app.load_cached(PathBuf::from("/virtual/other"), Arc::new(fixture_tree()));
    assert!(app.duplicates.report.is_none());
    assert!(app.duplicates.selected.is_empty());
    assert!(app.duplicates.keepers.is_empty());
}

#[test]
fn duplicate_workspace_selects_rest_without_the_keeper() {
    let temp = tempfile::tempdir().expect("tempdir");
    for name in ["a.bin", "b.bin", "c.bin"] {
        fs::write(temp.path().join(name), vec![b'x'; 4096]).expect("duplicate fixture");
    }
    let control = DuplicateControl::new();
    let report = find_duplicates(
        temp.path(),
        &DuplicateOptions {
            min_size: 0,
            ..DuplicateOptions::default()
        },
        &control,
    )
    .expect("duplicate report");
    assert_eq!(report.group_count(), 1);
    let group = report.groups[0].clone();
    let keeper = group.files[0].path.clone();

    let mut app = test_app(english_settings(), false);
    app.workspace = Workspace::Duplicates;
    app.duplicates.report = Some(report);
    app.duplicates
        .keepers
        .insert(group.fingerprint.clone(), keeper.clone());
    app.duplicates.select_rest(&group).expect("select rest");

    assert_eq!(app.duplicates.selected.len(), 2);
    assert!(!app.duplicates.selected.contains_key(&keeper));
    assert!(app.duplicates.can_apply());

    let mut harness = harness(app, Vec2::new(1280.0, 820.0));
    assert!(harness.query_by_label("Select the rest").is_none());
    harness
        .query_by_label_contains("a.bin")
        .expect("group header")
        .click();
    harness.run();
    assert!(harness.query_by_label("Select the rest").is_some());
    assert!(
        harness
            .query_all_by_label_contains("a.bin")
            .next()
            .is_some()
    );
    assert!(
        harness
            .query_all_by_label_contains("Review basket")
            .next()
            .is_some()
    );
}

#[test]
fn duplicate_workspace_explains_that_a_scan_is_required() {
    let mut app = test_app(english_settings(), false);
    app.workspace = Workspace::Duplicates;
    let harness = harness(app, Vec2::new(1280.0, 820.0));
    assert!(
        harness
            .query_by_label_contains("Scan a folder first")
            .is_some()
    );
}

#[derive(Default)]
struct TestStorage {
    values: HashMap<String, String>,
}

impl eframe::Storage for TestStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.values.insert(key.to_owned(), value);
    }

    fn remove_string(&mut self, key: &str) {
        self.values.remove(key);
    }

    fn flush(&mut self) {}
}

#[test]
fn fixture_tree_is_consistent() {
    let tree = fixture_tree();
    let media = tree
        .nodes
        .iter()
        .position(|node| node.display_name() == "media")
        .expect("media node") as NodeId;

    assert_eq!(
        tree.path(2),
        PathBuf::from("/virtual/fixture/media/movie.mkv")
    );
    assert_eq!(tree.children(ROOT_ID).count(), 3);
    assert_eq!(tree.node(media).allocated_size, 6 * GIB);
    assert_eq!(tree.node(ROOT_ID).file_count, 4);
    assert_eq!(tree.node(ROOT_ID).dir_count, 3);
    assert!(SystemTime::now().duration_since(UNIX_EPOCH).is_ok());
}

#[test]
fn overview_volume_grid_fits_viewport_with_long_mounts_in_both_languages() {
    for language in [Language::English, Language::Russian] {
        for width in [480.0, 760.0, 1100.0] {
            let mut app = test_app(english_settings(), false);
            app.settings.language = language;
            app.volumes = [
                "/".to_owned(),
                "/boot/efi".to_owned(),
                format!(
                    "/var/lib/docker/rootfs/overlayfs/{}",
                    "abcdef0123456789".repeat(6)
                ),
            ]
            .into_iter()
            .map(|mount_point| diskclean_platform::VolumeInfo {
                mount_path: PathBuf::from(&mount_point),
                mount_point,
                name: "/dev/nvme0n1p5".to_owned(),
                file_system: "ext4".to_owned(),
                total_space: 324 * GIB,
                available_space: 120 * GIB,
                is_removable: false,
            })
            .collect();
            app.settings.recent_roots =
                vec![format!("/home/user/{}", "long-directory/".repeat(20))];
            let mut harness = Harness::builder()
                .with_size(Vec2::new(width, 900.0))
                .with_accessibility_check(false)
                .with_context_setup(|ctx| {
                    theme::install_fonts(ctx);
                    theme::apply(ctx, ThemeMode::Dark);
                })
                .build_ui_state(
                    |ui, state: &mut (DiskCleanupApp, f32)| {
                        let right = ui.max_rect().right();
                        state.0.overview(ui);
                        state.1 = ui.min_rect().right() - right;
                    },
                    (app, 0.0),
                );
            harness.run();
            assert!(
                harness.state().1 <= 1.0,
                "overview overflow at {width}: {}",
                harness.state().1
            );
        }
    }
}

#[test]
fn scan_bar_shows_measured_counts_and_estimated_percent() {
    use crate::app::ScanState;
    use diskclean_core::{ScanControl, ScanOptions, scan};
    let temp = tempfile::tempdir().expect("fixture");
    fs::write(temp.path().join("file.bin"), [1; 4096]).expect("file");
    let control = ScanControl::new();
    let mut app = test_app(english_settings(), false);
    let (sender, receiver) = std::sync::mpsc::channel();
    app.state = ScanState::Running {
        estimated_bytes: Some(8192),
        control: control.clone(),
        receiver,
        started: std::time::Instant::now(),
        path: temp.path().to_path_buf(),
    };
    let mut harness = Harness::builder()
        .with_size(Vec2::new(1000.0, 180.0))
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            theme::install_fonts(ctx);
            theme::apply(ctx, ThemeMode::Dark);
        })
        .build_ui_state(|ui, app: &mut DiskCleanupApp| app.top_bar(ui), app);
    assert!(harness.query_by_label("≈ 0%").is_some());
    scan(temp.path(), &ScanOptions::default(), &control).expect("scan");
    harness.run();
    let expected = (control.snapshot().allocated_bytes as f64 / 8192.0 * 100.0)
        .clamp(0.0, 99.0)
        .floor();
    assert!(
        harness
            .query_by_label(&format!("≈ {expected:.0}%"))
            .is_some()
    );
    assert!(harness.query_by_label_contains("1 file").is_some());
    drop(sender);
}

#[test]
fn scan_progress_uses_occupied_bytes_of_the_enclosing_volume() {
    let temp = tempfile::tempdir().expect("fixture");
    let nested = temp.path().join("data");
    let mut app = test_app(english_settings(), false);
    app.volumes = [
        (temp.path().to_path_buf(), 1000, 200),
        (nested.clone(), 500, 400),
    ]
    .into_iter()
    .map(
        |(mount_path, total_space, available_space)| diskclean_platform::VolumeInfo {
            mount_point: mount_path.display().to_string(),
            mount_path,
            name: "fixture".to_owned(),
            file_system: "test".to_owned(),
            total_space,
            available_space,
            is_removable: false,
        },
    )
    .collect();
    assert_eq!(app.scan_volume_bytes(&nested.join("folder")), Some(100));
    assert_eq!(
        app.scan_volume_bytes(&temp.path().join("database")),
        Some(800)
    );
    app.volumes[1].available_space = 500;
    assert_eq!(app.scan_volume_bytes(&nested), None);
    app.volumes.clear();
    assert_eq!(app.scan_volume_bytes(temp.path()), None);
}

fn duplicate_ui_fixture() -> DuplicateReport {
    let temp = tempfile::tempdir().expect("tempdir");
    for name in ["a.bin", "b.bin", "c.bin"] {
        fs::write(temp.path().join(name), [42; 4096]).expect("fixture");
    }
    find_duplicates(
        temp.path(),
        &DuplicateOptions {
            min_size: 0,
            ..Default::default()
        },
        &DuplicateControl::new(),
    )
    .expect("report")
}

#[test]
fn duplicate_groups_toggle_and_bulk_selection_uses_displayed_report() {
    let mut app = test_app(english_settings(), false);
    app.workspace = Workspace::Duplicates;
    app.duplicates.report = Some(duplicate_ui_fixture());
    let mut harness = harness(app, Vec2::new(1280.0, 820.0));
    assert!(harness.query_by_label("Select the rest").is_none());
    harness
        .query_by_label_contains("a.bin")
        .expect("header")
        .click();
    harness.run();
    assert!(harness.query_by_label("Select the rest").is_some());
    harness
        .query_by_label(&format!("{} a.bin", crate::ui::icons::CARET_DOWN))
        .expect("header")
        .click();
    harness.run();
    assert!(harness.query_by_label("Select the rest").is_none());
    harness
        .query_by_label("Select all except keepers")
        .expect("bulk select")
        .click();
    harness.run();
    assert_eq!(harness.state().duplicates.selected.len(), 2);
}

#[test]
fn duplicate_workspace_virtualizes_a_hundred_thousand_files() {
    let mut report = duplicate_ui_fixture();
    let template = report.groups[0].files[0].clone();
    report.groups[0].files = (0..100_000)
        .map(|index| {
            let mut file = template.clone();
            file.path = PathBuf::from(format!("/fixture/copy-{index:06}.bin"));
            file
        })
        .collect();
    let fingerprint = report.groups[0].fingerprint.clone();
    let mut app = test_app(english_settings(), false);
    app.workspace = Workspace::Duplicates;
    app.duplicates.report = Some(report);
    app.duplicates.expanded_groups.insert(fingerprint);
    let mut harness = harness(app, Vec2::new(1280.0, 820.0));
    let visible = harness
        .query_all_by_label_contains("Keep /fixture/")
        .count();
    assert!(visible > 0 && visible < 40, "rendered {visible} rows");
    assert!(harness.query_by_label_contains("copy-099999.bin").is_none());
    let first = harness
        .query_by_label_contains("Keep /fixture/copy-000000.bin")
        .expect("first row")
        .rect();
    harness.hover_at(first.center());
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -2600.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(12);
    assert!(
        harness
            .query_by_label_contains("Keep /fixture/copy-000000.bin")
            .is_none()
    );
    let visible = harness
        .query_all_by_label_contains("Keep /fixture/")
        .count();
    assert!(
        visible > 0 && visible < 40,
        "rendered {visible} rows after scrolling"
    );
}

#[test]
fn duplicate_summary_and_list_fit_both_languages_and_themes() {
    for language in [Language::English, Language::Russian] {
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            for width in [480.0, 760.0, 1100.0] {
                let mut app = test_app(english_settings(), false);
                app.settings.language = language;
                app.settings.theme = mode;
                app.palette = theme::Palette::for_theme(mode, None);
                app.duplicates.report = Some(duplicate_ui_fixture());
                let mut harness = Harness::builder()
                    .with_size(Vec2::new(width, 900.0))
                    .with_accessibility_check(false)
                    .with_context_setup(move |ctx| {
                        theme::install_fonts(ctx);
                        theme::apply(ctx, mode);
                    })
                    .build_ui_state(
                        |ui, state: &mut (DiskCleanupApp, f32)| {
                            let right = ui.max_rect().right();
                            state.0.duplicates_workspace(ui);
                            state.1 = ui.min_rect().right() - right;
                        },
                        (app, 0.0),
                    );
                harness.run();
                assert!(
                    harness.state().1 <= 1.0,
                    "overflow at {width}: {}",
                    harness.state().1
                );
                let title = harness
                    .query_by_label(if language == Language::English {
                        "DUPLICATE GROUPS"
                    } else {
                        "ГРУППЫ ДУБЛИКАТОВ"
                    })
                    .expect("stat title")
                    .rect();
                let value = harness
                    .query_by_label(if language == Language::English {
                        "1 group"
                    } else {
                        "1 группа"
                    })
                    .expect("stat value")
                    .rect();
                assert!(
                    value.top() >= title.bottom(),
                    "stat text must stack vertically"
                );
            }
        }
    }
}
