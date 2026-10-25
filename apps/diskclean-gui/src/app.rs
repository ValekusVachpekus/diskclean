//! Application state, scan jobs, and top-level routing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use diskclean_core::{
    NodeId, NodeKind, ROOT_ID, ScanControl, ScanError, ScanOptions, Tree, parse_size, scan,
};
use diskclean_platform::{VolumeInfo, list_volumes};
use eframe::egui::{self, Id, Key, Margin, Stroke};
use egui::Frame;

use crate::i18n::{self, Key as TextKey, Language};
use crate::settings::Settings;
use crate::ui::theme::{self, Palette, ThemeMode};
use crate::ui::tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Map,
    Files,
    Folders,
    Types,
    Issues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortKey {
    Size,
    Name,
    Modified,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SortState {
    pub(crate) key: SortKey,
    pub(crate) descending: bool,
}

impl SortState {
    pub(crate) const SIZE: Self = Self {
        key: SortKey::Size,
        descending: true,
    };
}

#[derive(Debug, Clone)]
pub(crate) struct Filters {
    pub(crate) search: String,
    pub(crate) min_size: String,
    pub(crate) show_files: bool,
    pub(crate) show_dirs: bool,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            search: String::new(),
            min_size: "0".to_owned(),
            show_files: true,
            show_dirs: true,
        }
    }
}

#[derive(Debug)]
pub(crate) enum ScanFailure {
    Cancelled,
    Error(String),
}

pub(crate) enum ScanState {
    Idle,
    Running {
        control: ScanControl,
        receiver: Receiver<Result<Tree, ScanFailure>>,
        started: Instant,
        path: PathBuf,
    },
    Failed(String),
}

pub(crate) struct CachedScan {
    pub(crate) path: PathBuf,
    pub(crate) tree: Arc<Tree>,
    pub(crate) scanned_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastKind {
    Info,
    Error,
}

#[derive(Debug, Clone)]
pub(crate) struct Toast {
    pub(crate) text: String,
    pub(crate) kind: ToastKind,
    pub(crate) until: Instant,
}

pub(crate) struct DiskCleanupApp {
    pub(crate) settings: Settings,
    pub(crate) palette: Palette,
    pub(crate) path_input: String,
    pub(crate) volumes: Vec<VolumeInfo>,
    pub(crate) cache: Vec<CachedScan>,
    pub(crate) tree: Option<Arc<Tree>>,
    pub(crate) top_folders: Vec<NodeId>,
    pub(crate) selected: NodeId,
    pub(crate) state: ScanState,
    pub(crate) tab: Tab,
    pub(crate) filters: Filters,
    pub(crate) pending_scan: bool,
    pub(crate) settings_open: bool,
    pub(crate) help_open: bool,
    pub(crate) toast: Option<Toast>,
    pub(crate) focus_search: bool,
    pub(crate) file_sort: SortState,
    pub(crate) folder_sort: SortState,
    pub(crate) narrow_inspector: bool,
    pub(crate) wide: bool,
    applied_theme: ThemeMode,
    applied_system_theme: Option<egui::Theme>,
}

impl DiskCleanupApp {
    pub(crate) fn new(context: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&context.egui_ctx);

        let settings = Settings::load(context.storage);
        let applied_theme = settings.theme;
        theme::apply(&context.egui_ctx, settings.theme);
        let palette = theme::current_palette(&context.egui_ctx, settings.theme);
        let applied_system_theme = context.egui_ctx.system_theme();

        let argument = std::env::args_os().nth(1).map(PathBuf::from);
        let pending_scan = argument.is_some();
        let initial = argument
            .or_else(|| {
                let last = settings.last_path.trim();
                (!last.is_empty()).then(|| PathBuf::from(last))
            })
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));

        Self {
            settings,
            palette,
            path_input: initial.display().to_string(),
            volumes: list_volumes(),
            cache: Vec::new(),
            tree: None,
            top_folders: Vec::new(),
            selected: ROOT_ID,
            state: ScanState::Idle,
            tab: Tab::Map,
            filters: Filters::default(),
            pending_scan,
            settings_open: false,
            help_open: false,
            toast: None,
            focus_search: false,
            file_sort: SortState::SIZE,
            folder_sort: SortState::SIZE,
            narrow_inspector: false,
            wide: true,
            applied_theme,
            applied_system_theme,
        }
    }

    pub(crate) fn text(&self, key: TextKey) -> &'static str {
        self.settings.language.text(key)
    }

    pub(crate) fn language(&self) -> Language {
        self.settings.language
    }

    pub(crate) fn is_running(&self) -> bool {
        matches!(&self.state, ScanState::Running { .. })
    }

    pub(crate) fn start_scan(&mut self) {
        if self.is_running() {
            return;
        }

        let path_text = self.path_input.trim();
        if path_text.is_empty() {
            self.state = ScanState::Failed(self.text(TextKey::ChooseFolder).to_owned());
            return;
        }

        let path = PathBuf::from(path_text);
        let options = ScanOptions {
            top_files: 2_000,
            skip_hidden: self.settings.skip_hidden,
            cross_filesystems: self.settings.cross_filesystems,
            ..ScanOptions::default()
        };
        let control = ScanControl::new();
        let thread_control = control.clone();
        let scan_path = path.clone();
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let result = match scan(&scan_path, &options, &thread_control) {
                Ok(tree) => Ok(tree),
                Err(ScanError::Cancelled) => Err(ScanFailure::Cancelled),
                Err(error) => Err(ScanFailure::Error(error.to_string())),
            };
            let _ = sender.send(result);
        });

        self.toast = None;
        self.state = ScanState::Running {
            control,
            receiver,
            started: Instant::now(),
            path,
        };
    }

    pub(crate) fn cancel_scan(&self) {
        if let ScanState::Running { control, .. } = &self.state {
            control.cancel();
        }
    }

    pub(crate) fn poll_scan(&mut self, context: &egui::Context) {
        let received = match &self.state {
            ScanState::Running { receiver, .. } => Some(receiver.try_recv()),
            _ => None,
        };

        match received {
            Some(Ok(Ok(tree))) => {
                let root = tree.root_path.clone();
                let top_folders = compute_top_folders(&tree);
                let tree = Arc::new(tree);

                self.path_input = root.display().to_string();
                self.settings.last_path = self.path_input.clone();
                self.settings.remember_root(&root);
                self.cache.retain(|entry| entry.path != root);
                self.cache.insert(
                    0,
                    CachedScan {
                        path: root,
                        tree: Arc::clone(&tree),
                        scanned_at: Instant::now(),
                    },
                );
                self.cache.truncate(3);

                self.tree = Some(tree);
                self.top_folders = top_folders;
                self.selected = ROOT_ID;
                self.tab = Tab::Map;
                self.narrow_inspector = false;
                self.state = ScanState::Idle;
            }
            Some(Ok(Err(ScanFailure::Cancelled))) => {
                self.state = ScanState::Idle;
                self.notify(
                    self.text(TextKey::ScanCancelled).to_owned(),
                    ToastKind::Info,
                );
            }
            Some(Ok(Err(ScanFailure::Error(error)))) => {
                self.state = ScanState::Failed(error.clone());
                self.notify(error, ToastKind::Error);
            }
            Some(Err(TryRecvError::Disconnected)) => {
                let message = "scanner thread stopped unexpectedly".to_owned();
                self.state = ScanState::Failed(message.clone());
                self.notify(message, ToastKind::Error);
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        if self.is_running() {
            context.request_repaint_after(Duration::from_millis(100));
        }
    }

    pub(crate) fn load_cached(&mut self, path: PathBuf, tree: Arc<Tree>) {
        self.path_input = path.display().to_string();
        self.settings.last_path = self.path_input.clone();
        self.settings.remember_root(&path);
        self.top_folders = compute_top_folders(&tree);
        self.tree = Some(tree);
        self.selected = ROOT_ID;
        self.tab = Tab::Map;
        self.narrow_inspector = false;
        self.state = ScanState::Idle;
    }

    pub(crate) fn min_size(&self) -> u64 {
        let text = self.filters.min_size.trim();
        if text.is_empty() {
            0
        } else {
            parse_size(text).unwrap_or(0)
        }
    }

    pub(crate) fn filters_active(&self) -> usize {
        let mut count = 0;
        if !self.filters.search.trim().is_empty() {
            count += 1;
        }
        if self.min_size() > 0 {
            count += 1;
        }
        if !self.filters.show_files {
            count += 1;
        }
        if !self.filters.show_dirs {
            count += 1;
        }
        count
    }

    pub(crate) fn clear_filters(&mut self) {
        self.filters = Filters::default();
        self.focus_search = false;
    }

    pub(crate) fn visible_children(&self, tree: &Tree, id: NodeId) -> Vec<NodeId> {
        let needle = self.filters.search.trim().to_lowercase();
        let min_size = self.min_size();

        tree.children(id)
            .filter(|child| {
                let node = tree.node(*child);
                let kind_matches = if node.kind.is_dir() {
                    self.filters.show_dirs
                } else {
                    self.filters.show_files
                };
                let search_matches = needle.is_empty()
                    || node.display_name().to_lowercase().contains(&needle)
                    || tree
                        .path(*child)
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&needle);
                let size_matches = node.allocated_size >= min_size;
                kind_matches && search_matches && size_matches
            })
            .collect()
    }

    pub(crate) fn notify(&mut self, text: String, kind: ToastKind) {
        self.toast = Some(Toast {
            text,
            kind,
            until: Instant::now() + Duration::from_secs(5),
        });
    }

    pub(crate) fn copy_path(&mut self, context: &egui::Context, path: &Path) {
        context.copy_text(path.display().to_string());
        self.notify(self.text(TextKey::CopiedPath).to_owned(), ToastKind::Info);
    }

    pub(crate) fn open_path(&mut self, path: &Path) {
        let target = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .map_or_else(|| path.to_path_buf(), Path::to_path_buf)
        };
        self.open_target(&target);
    }

    pub(crate) fn reveal_path(&mut self, path: &Path) {
        let target = path
            .parent()
            .map_or_else(|| path.to_path_buf(), Path::to_path_buf);
        self.open_target(&target);
    }

    fn open_target(&mut self, target: &Path) {
        let result = open_in_file_manager(target);
        if let Err(error) = result {
            self.notify(
                format!("{}: {error}", self.text(TextKey::RevealFailed)),
                ToastKind::Error,
            );
        }
    }

    fn handle_shortcuts(&mut self, context: &egui::Context) {
        if self.settings_open || self.help_open {
            return;
        }

        let mut browse = false;
        let mut scan = false;
        let mut settings = false;
        let mut focus_search = false;
        let mut tab = None;
        let mut escape = false;
        let mut up = false;

        context.input_mut(|input| {
            if input.modifiers.command && input.key_pressed(Key::O) {
                browse = true;
            }
            if (input.modifiers.command && input.key_pressed(Key::R)) || input.key_pressed(Key::F5)
            {
                scan = true;
            }
            if input.modifiers.command && input.key_pressed(Key::F) {
                focus_search = true;
            }
            if input.modifiers.command && input.key_pressed(Key::Comma) {
                settings = true;
            }
            if input.modifiers.command && input.key_pressed(Key::Num1) {
                tab = Some(Tab::Map);
            }
            if input.modifiers.command && input.key_pressed(Key::Num2) {
                tab = Some(Tab::Files);
            }
            if input.modifiers.command && input.key_pressed(Key::Num3) {
                tab = Some(Tab::Folders);
            }
            if input.modifiers.command && input.key_pressed(Key::Num4) {
                tab = Some(Tab::Types);
            }
            if input.modifiers.command && input.key_pressed(Key::Num5) {
                tab = Some(Tab::Issues);
            }
            if input.key_pressed(Key::Escape) {
                escape = true;
            }
            if input.key_pressed(Key::Backspace) && !input.modifiers.command {
                up = true;
            }
        });

        if browse
            && !self.is_running()
            && let Some(folder) = rfd::FileDialog::new().pick_folder()
        {
            self.path_input = folder.display().to_string();
            self.start_scan();
        }
        if scan {
            self.start_scan();
        }
        if settings {
            self.settings_open = true;
        }
        if focus_search && self.tree.is_some() {
            self.focus_search = true;
        }
        if let Some(tab) = tab {
            self.tab = tab;
        }
        if escape && !context.egui_wants_keyboard_input() {
            if !self.filters.search.is_empty() || self.filters_active() > 0 {
                self.clear_filters();
            } else if let Some(tree) = self.tree.clone()
                && let Some(parent) = tree.node(self.selected).parent
            {
                self.selected = parent;
            }
        }
        if up
            && !context.egui_wants_keyboard_input()
            && let Some(tree) = self.tree.clone()
            && let Some(parent) = tree.node(self.selected).parent
        {
            self.selected = parent;
        }
    }

    fn apply_theme_if_needed(&mut self, context: &egui::Context) {
        let system = context.system_theme();
        if self.settings.theme != self.applied_theme || system != self.applied_system_theme {
            theme::apply(context, self.settings.theme);
            self.applied_theme = self.settings.theme;
            self.applied_system_theme = system;
        }
        self.palette = theme::current_palette(context, self.settings.theme);
    }
}

impl eframe::App for DiskCleanupApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.apply_theme_if_needed(&context);

        if self.pending_scan {
            self.pending_scan = false;
            self.start_scan();
        }
        self.handle_shortcuts(&context);
        self.poll_scan(&context);

        self.wide = ui.available_width() >= 1_180.0;
        let show_inspector = self.tree.is_some()
            && self.settings.inspector_open
            && (self.wide || self.narrow_inspector);

        let panel_fill = self.palette.panel;
        let bg = self.palette.bg;
        let border = self.palette.border;

        egui::Panel::top("command_bar")
            .frame(
                Frame::new()
                    .fill(panel_fill)
                    .inner_margin(Margin::symmetric(14, 8)),
            )
            .show(ui, |ui| self.top_bar(ui));

        let navigation = egui::Panel::left("navigation")
            .resizable(true)
            .default_size(self.settings.sidebar_width)
            .min_size(196.0)
            .max_size(320.0)
            .frame(
                Frame::new()
                    .fill(panel_fill)
                    .inner_margin(Margin::symmetric(10, 10)),
            )
            .show(ui, |ui| self.navigation(ui));
        self.settings.sidebar_width = navigation.response.rect.width();

        if show_inspector {
            let response = egui::Panel::right("inspector")
                .resizable(true)
                .default_size(self.settings.inspector_width)
                .min_size(248.0)
                .max_size(440.0)
                .frame(
                    Frame::new()
                        .fill(panel_fill)
                        .inner_margin(Margin::symmetric(12, 10)),
                )
                .show(ui, |ui| self.inspector(ui));
            self.settings.inspector_width = response.response.rect.width();
        }

        egui::Panel::bottom("status_bar")
            .exact_size(tokens::STATUS_BAR_HEIGHT)
            .frame(
                Frame::new()
                    .fill(panel_fill)
                    .inner_margin(Margin::symmetric(12, 3))
                    .stroke(Stroke::new(1.0, border)),
            )
            .show(ui, |ui| self.status_bar(ui));

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(bg)
                    .inner_margin(Margin::symmetric(18, 14)),
            )
            .show(ui, |ui| self.central(ui));

        self.settings_dialog(&context);
        self.help_dialog(&context);
        self.show_toast(&context);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.last_path = self.path_input.clone();
        self.settings.save(storage);
    }
}

pub(crate) fn compute_top_folders(tree: &Tree) -> Vec<NodeId> {
    let mut folders: Vec<NodeId> = tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(index, node)| *index != ROOT_ID as usize && node.kind.is_dir())
        .map(|(index, _)| index as NodeId)
        .collect();

    folders.sort_unstable_by(|left, right| {
        tree.node(*right)
            .allocated_size
            .cmp(&tree.node(*left).allocated_size)
            .then_with(|| {
                tree.node(*left)
                    .display_name()
                    .cmp(&tree.node(*right).display_name())
            })
    });
    folders.truncate(2_000);
    folders
}

pub(crate) fn sort_children_by_size(tree: &Tree, children: &mut [NodeId]) {
    children.sort_unstable_by(|left, right| {
        tree.node(*right)
            .allocated_size
            .cmp(&tree.node(*left).allocated_size)
            .then_with(|| {
                tree.node(*left)
                    .display_name()
                    .cmp(&tree.node(*right).display_name())
            })
    });
}

pub(crate) fn sort_ids(tree: &Tree, ids: &mut [NodeId], sort: SortState) {
    ids.sort_unstable_by(|left, right| {
        let left_node = tree.node(*left);
        let right_node = tree.node(*right);
        let order = match sort.key {
            SortKey::Size => right_node
                .allocated_size
                .cmp(&left_node.allocated_size)
                .then_with(|| left_node.display_name().cmp(&right_node.display_name())),
            SortKey::Name => left_node
                .display_name()
                .cmp(&right_node.display_name())
                .then_with(|| right_node.allocated_size.cmp(&left_node.allocated_size)),
            SortKey::Modified => right_node
                .modified_unix
                .cmp(&left_node.modified_unix)
                .then_with(|| right_node.allocated_size.cmp(&left_node.allocated_size)),
        };
        if sort.descending {
            order.reverse()
        } else {
            order
        }
    });
}

pub(crate) fn kind_label(language: Language, kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Directory => language.text(TextKey::Folder),
        NodeKind::File => language.text(TextKey::File),
        NodeKind::Symlink => language.text(TextKey::Symlink),
        NodeKind::Other => language.text(TextKey::Other),
    }
}

pub(crate) fn kind_icon(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Directory => crate::ui::icons::FOLDER,
        NodeKind::File => crate::ui::icons::FILE,
        NodeKind::Symlink => crate::ui::icons::FILE,
        NodeKind::Other => crate::ui::icons::FILE,
    }
}

pub(crate) fn files_label(language: Language, count: u64) -> String {
    i18n::files_text(language, count)
}

pub(crate) fn folders_label(language: Language, count: u64) -> String {
    i18n::folders_text(language, count)
}

pub(crate) fn issues_label(language: Language, count: u64) -> String {
    i18n::issues_text(language, count)
}

fn open_in_file_manager(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "opening the file manager is not supported on this platform",
        ))
    }
}

pub(crate) fn search_field_id() -> Id {
    Id::unique("workspace_search")
}

pub(crate) fn path_field_id() -> Id {
    Id::unique("command_bar_path")
}
