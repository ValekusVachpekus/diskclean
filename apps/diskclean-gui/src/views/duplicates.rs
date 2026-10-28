//! Exact duplicate workspace: hashing progress, keeper selection, review basket.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use diskclean_cleanup::{
    ApplyReport, CleanupContext, CleanupItem, ItemResultKind, ProtectedPaths, default_audit_path,
};
use diskclean_core::{
    DuplicateControl, DuplicateFile, DuplicateOptions, DuplicatePhase, DuplicateReport,
    find_duplicates,
};
use eframe::egui::{
    self, Align, Button, Checkbox, Color32, Context, FontId, Frame, Id, Layout, Margin, RichText,
    ScrollArea, Stroke, TextEdit, vec2,
};

use crate::app::{DiskCleanupApp, ToastKind, Workspace};
use crate::i18n::{self, Key};
use crate::jobs::CleanupApplyJob as DuplicateApplyJob;
use crate::ui::{components, format, icons, theme, tokens};

pub(crate) enum DuplicateRunError {
    Cancelled,
    Error(String),
}

#[derive(Default)]
pub(crate) enum DuplicateJob {
    #[default]
    Idle,
    Running {
        receiver: Receiver<Result<DuplicateReport, DuplicateRunError>>,
        control: DuplicateControl,
        started: Instant,
    },
    Failed,
}

#[derive(Default)]
struct DuplicateSummary {
    copies: usize,
    reclaimable: u64,
    group_savings: Vec<u64>,
}

pub(crate) struct DuplicateWorkspace {
    summary: Option<DuplicateSummary>,
    pub(crate) report: Option<DuplicateReport>,
    pub(crate) job: DuplicateJob,
    pub(crate) apply_job: DuplicateApplyJob,
    pub(crate) min_size: String,
    pub(crate) selected: BTreeMap<PathBuf, CleanupItem>,
    pub(crate) keepers: BTreeMap<String, PathBuf>,
    pub(crate) expanded_groups: BTreeSet<String>,
    pub(crate) confirm_open: bool,
    pub(crate) confirm_focus_pending: bool,
    pub(crate) last_apply: Option<ApplyReport>,
    pub(crate) last_apply_audit: Option<PathBuf>,
    pub(crate) context: Option<CleanupContext>,
    pub(crate) protected: Option<ProtectedPaths>,
    pub(crate) audit_path: Option<PathBuf>,
    pub(crate) error: Option<String>,
}

impl Default for DuplicateWorkspace {
    fn default() -> Self {
        Self {
            summary: None,
            report: None,
            job: DuplicateJob::Idle,
            apply_job: DuplicateApplyJob::Idle,
            min_size: "1M".to_owned(),
            selected: BTreeMap::new(),
            keepers: BTreeMap::new(),
            expanded_groups: BTreeSet::new(),
            confirm_open: false,
            confirm_focus_pending: false,
            last_apply: None,
            last_apply_audit: None,
            context: None,
            protected: None,
            audit_path: None,
            error: None,
        }
    }
}

impl DuplicateWorkspace {
    pub(crate) fn is_scanning(&self) -> bool {
        matches!(self.job, DuplicateJob::Running { .. })
    }

    pub(crate) fn is_applying(&self) -> bool {
        matches!(self.apply_job, DuplicateApplyJob::Running { .. })
    }

    pub(crate) fn progress(&self) -> Option<diskclean_core::DuplicateProgress> {
        match &self.job {
            DuplicateJob::Running { control, .. } => Some(control.snapshot()),
            _ => None,
        }
    }

    pub(crate) fn elapsed(&self) -> Option<Duration> {
        match &self.job {
            DuplicateJob::Running { started, .. } => Some(started.elapsed()),
            _ => None,
        }
    }

    pub(crate) fn selected_size(&self) -> u64 {
        self.selected
            .values()
            .fold(0_u64, |total, item| total.saturating_add(item.size()))
    }

    pub(crate) fn can_apply(&self) -> bool {
        !self.selected.is_empty() && !self.is_scanning() && !self.is_applying()
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selected.clear();
    }

    pub(crate) fn keeper_for<'a>(&'a self, group: &'a diskclean_core::DuplicateGroup) -> &'a Path {
        if let Some(path) = self.keepers.get(&group.fingerprint) {
            path
        } else if let Some(file) = group.files.first() {
            &file.path
        } else {
            Path::new("")
        }
    }

    pub(crate) fn total_reclaimable(&self, report: &DuplicateReport) -> u64 {
        report.groups.iter().fold(0_u64, |total, group| {
            total.saturating_add(group.savings_keeping(self.keeper_for(group)))
        })
    }

    pub(crate) fn set_keeper(&mut self, group: &diskclean_core::DuplicateGroup, path: PathBuf) {
        let Some(keeper) = group.files.iter().find(|file| file.path == path) else {
            return;
        };
        self.summary = None;
        self.keepers.insert(group.fingerprint.clone(), path.clone());
        self.selected.remove(&path);
        for file in &group.files {
            if self.selected.contains_key(&file.path) {
                match CleanupItem::from_duplicate(file, keeper, &group.fingerprint) {
                    Ok(item) => {
                        self.selected.insert(file.path.clone(), item);
                    }
                    Err(_) => {
                        self.selected.remove(&file.path);
                    }
                }
            }
        }
    }

    /// Drops results that belong to a previous analysis root.
    ///
    /// A hashing job for the old root is cancelled. A cleanup job that is
    /// already applying is left alone because it revalidates every path before
    /// moving anything, but its report cannot be merged into the new root.
    pub(crate) fn reset_for_new_root(&mut self) {
        if let DuplicateJob::Running { control, .. } = &self.job {
            control.cancel();
        }
        self.job = DuplicateJob::Idle;
        self.report = None;
        self.summary = None;
        self.selected.clear();
        self.keepers.clear();
        self.expanded_groups.clear();
        self.confirm_open = false;
        self.confirm_focus_pending = false;
        self.error = None;
        self.last_apply = None;
        self.last_apply_audit = None;
    }

    pub(crate) fn toggle_copy(
        &mut self,
        file: &DuplicateFile,
        checked: bool,
        keeper: &Path,
        group: &diskclean_core::DuplicateGroup,
    ) -> Result<(), String> {
        if file.path == keeper {
            self.selected.remove(&file.path);
            return Ok(());
        }
        if checked {
            let keeper = group
                .files
                .iter()
                .find(|file| file.path == keeper)
                .ok_or_else(|| "keeper is not in the scanned group".to_owned())?;
            let item = CleanupItem::from_duplicate(file, keeper, &group.fingerprint)
                .map_err(|error| error.to_string())?;
            self.selected.insert(file.path.clone(), item);
        } else {
            self.selected.remove(&file.path);
        }
        Ok(())
    }

    pub(crate) fn select_rest(
        &mut self,
        group: &diskclean_core::DuplicateGroup,
    ) -> Result<(), String> {
        let keeper_path = self.keeper_for(group);
        let Some(keeper) = group.files.iter().find(|file| file.path == keeper_path) else {
            return Err("keeper is not in the scanned group".to_owned());
        };
        // Resolve the keeper once: a large group must not search all its files per copy.
        for file in &group.files {
            if file.path != keeper.path {
                let item = CleanupItem::from_duplicate(file, keeper, &group.fingerprint)
                    .map_err(|error| error.to_string())?;
                self.selected.insert(file.path.clone(), item);
            }
        }
        Ok(())
    }

    pub(crate) fn select_all_except_keepers(
        &mut self,
        report: &DuplicateReport,
    ) -> Result<(), String> {
        for group in &report.groups {
            self.select_rest(group)?;
        }
        Ok(())
    }

    pub(crate) fn remove_trashed(&mut self, report: &ApplyReport) {
        self.summary = None;
        let trashed: std::collections::HashSet<PathBuf> = report
            .results
            .iter()
            .filter(|result| result.kind == ItemResultKind::Trashed)
            .map(|result| result.path.clone())
            .collect();
        self.selected.retain(|path, _| !trashed.contains(path));
        let Some(duplicates) = self.report.as_mut() else {
            return;
        };
        for group in &mut duplicates.groups {
            group.files.retain(|file| !trashed.contains(&file.path));
        }
        duplicates.groups.retain(|group| group.files.len() >= 2);
        for group in &mut duplicates.groups {
            group.reclaimable_bytes = group
                .files
                .first()
                .map_or(0, |file| group.savings_keeping(&file.path));
            if let Some(keeper) = self.keepers.get(&group.fingerprint)
                && !group.files.iter().any(|file| &file.path == keeper)
                && let Some(first) = group.files.first()
            {
                self.keepers
                    .insert(group.fingerprint.clone(), first.path.clone());
            }
        }
        let live: BTreeSet<String> = duplicates
            .groups
            .iter()
            .map(|group| group.fingerprint.clone())
            .collect();
        self.keepers
            .retain(|fingerprint, _| live.contains(fingerprint));
        self.expanded_groups
            .retain(|fingerprint| live.contains(fingerprint));
    }
}

impl DiskCleanupApp {
    pub(crate) fn start_duplicate_search(&mut self) {
        if self.duplicates.is_scanning() || self.duplicates.is_applying() {
            return;
        }
        let Some(tree) = self.tree.clone() else {
            return;
        };
        let min_size = match diskclean_core::parse_size(&self.duplicates.min_size) {
            Ok(size) => size,
            Err(error) => {
                self.notify(
                    format!("{}: {error}", self.text(Key::InvalidSize)),
                    ToastKind::Error,
                );
                return;
            }
        };

        let root = tree.root_path.clone();
        if self
            .duplicates
            .report
            .as_ref()
            .is_some_and(|report| report.root_path != root)
        {
            self.duplicates.reset_for_new_root();
        }
        let options = DuplicateOptions {
            min_size,
            prefix_bytes: 16 * 1024,
            skip_hidden: self.settings.skip_hidden,
            cross_filesystems: self.settings.cross_filesystems,
            ..DuplicateOptions::default()
        };
        let context = CleanupContext::from_environment();
        let audit_path = default_audit_path();
        let audit_root = audit_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let protected = ProtectedPaths::for_context(&context, audit_root);

        let control = DuplicateControl::new();
        let thread_control = control.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = match find_duplicates(&root, &options, &thread_control) {
                Ok(report) => Ok(report),
                Err(diskclean_core::DuplicateError::Cancelled) => Err(DuplicateRunError::Cancelled),
                Err(error) => Err(DuplicateRunError::Error(error.to_string())),
            };
            let _ = sender.send(result);
        });

        self.duplicates.context = Some(context);
        self.duplicates.protected = Some(protected);
        self.duplicates.audit_path = audit_path;
        self.duplicates.selected.clear();
        self.duplicates.confirm_open = false;
        self.duplicates.error = None;
        self.duplicates.job = DuplicateJob::Running {
            receiver,
            control,
            started: Instant::now(),
        };
    }

    pub(crate) fn cancel_duplicate_search(&mut self) {
        if let DuplicateJob::Running { control, .. } = &self.duplicates.job {
            control.cancel();
        }
    }

    pub(crate) fn poll_duplicates(&mut self, context: &egui::Context) {
        let received = match &self.duplicates.job {
            DuplicateJob::Running { receiver, .. } => Some(receiver.try_recv()),
            _ => None,
        };
        match received {
            Some(Ok(Ok(report))) => {
                let mut keepers = BTreeMap::new();
                for group in &report.groups {
                    if let Some(first) = group.files.first() {
                        keepers.insert(group.fingerprint.clone(), first.path.clone());
                    }
                }
                self.duplicates.summary = None;
                self.duplicates.report = Some(report);
                self.duplicates.keepers = keepers;
                self.duplicates.selected.clear();
                self.duplicates.expanded_groups.clear();
                self.duplicates.last_apply = None;
                self.duplicates.last_apply_audit = None;
                self.duplicates.job = DuplicateJob::Idle;
                self.duplicates.error = None;
            }
            Some(Ok(Err(DuplicateRunError::Cancelled))) => {
                self.duplicates.job = DuplicateJob::Idle;
                self.duplicates.error = None;
                self.notify(self.text(Key::ScanCancelled).to_owned(), ToastKind::Info);
            }
            Some(Ok(Err(DuplicateRunError::Error(error)))) => {
                self.duplicates.error = Some(error.clone());
                self.duplicates.job = DuplicateJob::Failed;
                self.notify(
                    format!("{}: {error}", self.text(Key::DuplicatesFailed)),
                    ToastKind::Error,
                );
            }
            Some(Err(TryRecvError::Disconnected)) => {
                let error = self.text(Key::DuplicateSearchStopped).to_owned();
                self.duplicates.error = Some(error.clone());
                self.duplicates.job = DuplicateJob::Failed;
                self.notify(
                    format!("{}: {error}", self.text(Key::DuplicatesFailed)),
                    ToastKind::Error,
                );
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        let apply_received = match &self.duplicates.apply_job {
            DuplicateApplyJob::Running { receiver, .. } => Some(receiver.try_recv()),
            _ => None,
        };
        match apply_received {
            Some(Ok(Ok(report))) => {
                let failed = report.failed_count();
                let trashed_count = report.trashed_count;
                let bytes = report.bytes_trashed;
                self.duplicates.remove_trashed(&report);
                self.duplicates.last_apply_audit = self.duplicates.audit_path.clone();
                self.duplicates.last_apply = Some(report);
                self.duplicates.apply_job = DuplicateApplyJob::Idle;
                self.duplicates.error = None;
                let message = if failed == 0 {
                    format!(
                        "{}: {} · {}",
                        self.text(Key::ApplyComplete),
                        i18n::items_text(self.language(), trashed_count as u64),
                        format::size(self.language(), bytes)
                    )
                } else {
                    format!(
                        "{}: {} / {}",
                        self.text(Key::ApplyPartial),
                        trashed_count,
                        trashed_count + failed
                    )
                };
                self.notify(
                    message,
                    if failed == 0 {
                        ToastKind::Info
                    } else {
                        ToastKind::Error
                    },
                );
            }
            Some(Ok(Err(error))) => {
                self.duplicates.apply_job = DuplicateApplyJob::Idle;
                self.duplicates.confirm_open = false;
                self.duplicates.error = Some(error.clone());
                self.notify(
                    format!("{}: {error}", self.text(Key::ApplyFailed)),
                    ToastKind::Error,
                );
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.duplicates.apply_job = DuplicateApplyJob::Idle;
                self.duplicates.confirm_open = false;
                let error = self.text(Key::DuplicateWorkerStopped).to_owned();
                self.duplicates.error = Some(error.clone());
                self.notify(
                    format!("{}: {error}", self.text(Key::ApplyFailed)),
                    ToastKind::Error,
                );
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        if self.duplicates.is_scanning() || self.duplicates.is_applying() {
            context.request_repaint_after(Duration::from_millis(100));
        }
    }

    pub(crate) fn start_duplicate_apply(&mut self) {
        if !self.duplicates.can_apply() || self.cleanup.is_applying() {
            return;
        }
        let Some(protected) = self.duplicates.protected.clone() else {
            self.duplicates.error = Some(self.text(Key::AuditUnavailable).to_owned());
            return;
        };
        let Some(audit_path) = self.duplicates.audit_path.clone() else {
            self.duplicates.error = Some(self.text(Key::AuditUnavailable).to_owned());
            return;
        };
        let items: Vec<CleanupItem> = self.duplicates.selected.values().cloned().collect();
        self.duplicates.apply_job = DuplicateApplyJob::start(items, protected, audit_path);
        self.duplicates.confirm_open = false;
        self.duplicates.error = None;
    }

    pub(crate) fn duplicates_workspace(&mut self, ui: &mut egui::Ui) {
        let report = self.duplicates.report.take();
        self.duplicates_header(ui, report.is_some());
        self.duplicates_banners(ui);

        match self.tree.as_ref() {
            None if report.is_none() => self.duplicates_need_scan(ui),
            _ => {
                if let Some(report) = report.as_ref() {
                    if self.duplicates.summary.is_none() {
                        self.duplicates.summary = Some(DuplicateSummary {
                            copies: report.duplicate_file_count(),
                            reclaimable: self.duplicates.total_reclaimable(report),
                            group_savings: report
                                .groups
                                .iter()
                                .map(|group| {
                                    group.savings_keeping(self.duplicates.keeper_for(group))
                                })
                                .collect(),
                        });
                    }
                    self.duplicates_summary(ui, report);
                    self.duplicates_basket_panel(ui);
                    self.duplicates_body(ui, report);
                } else {
                    self.duplicates_empty(ui);
                }
            }
        }

        self.duplicates.report = report;
    }

    fn duplicates_header(&mut self, ui: &mut egui::Ui, has_report: bool) {
        let palette = self.palette;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icons::DUPLICATES)
                    .font(FontId::proportional(23.0))
                    .color(palette.accent),
            );
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(self.text(Key::Duplicates))
                        .font(theme::semibold(21.0))
                        .color(palette.text),
                );
                ui.label(
                    RichText::new(self.text(Key::DuplicatesSubtitle))
                        .color(palette.muted)
                        .size(11.0),
                );
            });
        });

        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.text(Key::MinSize))
                    .color(palette.muted)
                    .size(10.5),
            );
            let min_size_hint = self.text(Key::MinSizeHint);
            ui.add(
                TextEdit::singleline(&mut self.duplicates.min_size)
                    .desired_width(84.0)
                    .hint_text(min_size_hint)
                    .margin(Margin::symmetric(7, 4)),
            );
            if diskclean_core::parse_size(&self.duplicates.min_size).is_err() {
                ui.label(
                    RichText::new(self.text(Key::InvalidSize))
                        .color(palette.danger)
                        .size(9.5),
                );
            }
            let busy = self.duplicates.is_scanning() || self.duplicates.is_applying();
            let can_search = self.tree.is_some() && !busy;
            if ui
                .add_enabled(
                    can_search,
                    Button::new(
                        RichText::new(self.text(if has_report {
                            Key::ScanAgain
                        } else {
                            Key::FindDuplicates
                        }))
                        .color(palette.on_accent)
                        .font(theme::medium(11.0)),
                    )
                    .fill(palette.accent)
                    .stroke(Stroke::NONE)
                    .corner_radius(tokens::RADIUS_CONTROL)
                    .min_size(vec2(0.0, 27.0)),
                )
                .clicked()
            {
                self.start_duplicate_search();
            }
            if let Some(tree) = &self.tree {
                ui.label(
                    RichText::new(format::shorten_middle(
                        &tree.root_path.display().to_string(),
                        56,
                    ))
                    .font(FontId::monospace(9.5))
                    .color(palette.muted),
                )
                .on_hover_text(tree.root_path.display().to_string());
            }
        });
        ui.add_space(3.0);
        ui.horizontal_wrapped(|ui| {
            if let Some(audit) = self.duplicates.audit_path.clone() {
                ui.label(
                    RichText::new(icons::LIST)
                        .font(FontId::proportional(11.0))
                        .color(palette.muted),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(format::shorten_middle(&audit.display().to_string(), 58))
                            .color(palette.muted)
                            .size(9.5),
                    )
                    .truncate(),
                )
                .on_hover_text(audit.display().to_string());
                if components::secondary_button(ui, palette, self.text(Key::OpenAuditLog)).clicked()
                {
                    self.open_file_path(&audit);
                }
            } else {
                ui.label(
                    RichText::new(self.text(Key::AuditUnavailable))
                        .color(palette.danger)
                        .size(9.5),
                );
            }
        });
    }

    fn duplicates_banners(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        self.duplicates
            .apply_job
            .show_progress(ui, palette, self.language());
        if let Some(error) = self.duplicates.error.clone() {
            ui.add_space(6.0);
            Frame::new()
                .fill(Color32::from_rgba_unmultiplied(
                    palette.danger.r(),
                    palette.danger.g(),
                    palette.danger.b(),
                    if palette.is_dark() { 28 } else { 18 },
                ))
                .stroke(Stroke::new(1.0, palette.danger))
                .corner_radius(tokens::RADIUS_CARD)
                .inner_margin(Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(icons::WARNING)
                                .font(FontId::proportional(14.0))
                                .color(palette.danger),
                        );
                        ui.label(RichText::new(&error).color(palette.text).size(11.0));
                        if components::icon_button(
                            ui,
                            palette,
                            icons::CLOSE,
                            self.text(Key::Dismiss),
                        )
                        .clicked()
                        {
                            self.duplicates.error = None;
                        }
                    });
                });
        }

        if let Some(progress) = self.duplicates.progress() {
            ui.add_space(6.0);
            let current = progress
                .current_path
                .as_ref()
                .map(|path| format::shorten_middle(&path.display().to_string(), 58))
                .unwrap_or_default();
            Frame::new()
                .fill(palette.surface)
                .stroke(Stroke::new(1.0, palette.border))
                .corner_radius(tokens::RADIUS_CARD)
                .inner_margin(Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::Spinner::new().size(13.0).color(palette.accent));
                        ui.label(
                            RichText::new(match progress.phase {
                                DuplicatePhase::Collecting => self.text(Key::ScanningDuplicates),
                                DuplicatePhase::Hashing => self.text(Key::ScanningDuplicates),
                                DuplicatePhase::Done => self.text(Key::Ready),
                            })
                            .color(palette.accent)
                            .size(10.5),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} · {} {} · {}",
                                crate::app::files_label(self.language(), progress.files_scanned),
                                format::size(self.language(), progress.bytes_hashed),
                                self.text(Key::HashedBytes),
                                i18n::groups_text(self.language(), progress.groups_found)
                            ))
                            .color(palette.muted)
                            .size(9.5),
                        );
                        if let Some(elapsed) = self.duplicates.elapsed() {
                            ui.label(
                                RichText::new(format::duration(
                                    self.language(),
                                    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                                ))
                                .color(palette.muted)
                                .size(9.5),
                            );
                        }
                        if components::secondary_button(ui, palette, self.text(Key::Cancel))
                            .clicked()
                        {
                            self.cancel_duplicate_search();
                        }
                    });
                    ui.add_sized(
                        [ui.available_width(), 16.0],
                        egui::Label::new(RichText::new(&current).color(palette.muted).size(9.5))
                            .truncate(),
                    )
                    .on_hover_text(
                        progress
                            .current_path
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_default(),
                    );
                });
        }

        if let Some(apply) = self.duplicates.last_apply.clone() {
            let failed = apply.failed_count();
            let color = if failed == 0 {
                palette.success
            } else {
                palette.warning
            };
            ui.add_space(6.0);
            Frame::new()
                .fill(Color32::from_rgba_unmultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    if palette.is_dark() { 25 } else { 16 },
                ))
                .stroke(Stroke::new(1.0, color))
                .corner_radius(tokens::RADIUS_CARD)
                .inner_margin(Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(icons::CHECK)
                                .font(FontId::proportional(14.0))
                                .color(color),
                        );
                        ui.label(
                            RichText::new(if failed == 0 {
                                self.text(Key::ApplyComplete)
                            } else {
                                self.text(Key::ApplyPartial)
                            })
                            .font(theme::medium(11.5))
                            .color(color),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                i18n::items_text(self.language(), apply.trashed_count as u64),
                                format::size(self.language(), apply.bytes_trashed)
                            ))
                            .color(palette.text)
                            .size(10.5),
                        );
                        if let Some(audit) = self.duplicates.last_apply_audit.clone()
                            && components::secondary_button(
                                ui,
                                palette,
                                self.text(Key::OpenAuditLog),
                            )
                            .clicked()
                        {
                            self.open_file_path(&audit);
                        }
                        if components::icon_button(
                            ui,
                            palette,
                            icons::CLOSE,
                            self.text(Key::Dismiss),
                        )
                        .clicked()
                        {
                            self.duplicates.last_apply = None;
                            self.duplicates.last_apply_audit = None;
                        }
                    });
                });
        }
    }

    fn duplicates_need_scan(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        Frame::new()
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(tokens::RADIUS_CARD)
            .inner_margin(18)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(self.text(Key::DuplicatesNeedScan))
                        .font(theme::semibold(15.0))
                        .color(palette.text),
                );
                ui.label(
                    RichText::new(self.text(Key::DuplicatesNeedScanBody))
                        .color(palette.muted)
                        .size(11.0),
                );
                ui.add_space(10.0);
                if components::primary_button(ui, palette, self.text(Key::Analyze)).clicked() {
                    self.workspace = Workspace::Analyze;
                }
            });
    }

    fn duplicates_empty(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        Frame::new()
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(tokens::RADIUS_CARD)
            .inner_margin(18)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(self.text(Key::DuplicatesDescription))
                        .color(palette.text)
                        .size(11.5),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(self.text(Key::DuplicatesReviewNote))
                        .color(palette.muted)
                        .size(10.5),
                );
                ui.add_space(10.0);
                if !self.duplicates.is_scanning()
                    && components::primary_button(ui, palette, self.text(Key::FindDuplicates))
                        .clicked()
                {
                    self.start_duplicate_search();
                }
            });
    }

    fn duplicates_summary(&mut self, ui: &mut egui::Ui, report: &DuplicateReport) {
        let palette = self.palette;
        ui.add_space(6.0);
        let columns = if ui.available_width() >= 660.0 { 3 } else { 1 };
        let card_width = (ui.available_width() - (columns - 1) as f32 * 8.0) / columns as f32;
        egui::Grid::new("duplicate_summary")
            .num_columns(columns)
            .spacing(vec2(8.0, 8.0))
            .show(ui, |ui| {
                components::stat_card(
                    ui,
                    palette,
                    self.text(Key::DuplicateGroups),
                    &i18n::groups_text(self.language(), report.group_count() as u64),
                    &i18n::copies_text(
                        self.language(),
                        self.duplicates
                            .summary
                            .as_ref()
                            .map_or(0, |summary| summary.copies) as u64,
                    ),
                    palette.text,
                    card_width,
                );
                if columns == 1 {
                    ui.end_row();
                }
                components::stat_card(
                    ui,
                    palette,
                    self.text(Key::Reclaimable),
                    &format::size(
                        self.language(),
                        self.duplicates
                            .summary
                            .as_ref()
                            .map_or(0, |summary| summary.reclaimable),
                    ),
                    self.text(Key::NothingPermanentlyDeleted),
                    palette.success,
                    card_width,
                );
                if columns == 1 {
                    ui.end_row();
                }
                components::stat_card(
                    ui,
                    palette,
                    self.text(Key::HashedBytes),
                    &format::size(self.language(), report.bytes_hashed),
                    &i18n::hardlinks_text(self.language(), report.hardlink_duplicates),
                    palette.muted,
                    card_width,
                );
                if columns == 1 {
                    ui.end_row();
                }
            });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if components::secondary_button(ui, palette, self.text(Key::SelectAllExceptKeepers))
                .clicked()
                && let Err(error) = self.duplicates.select_all_except_keepers(report)
            {
                self.notify(error, ToastKind::Error);
            }
            if !self.duplicates.selected.is_empty()
                && components::secondary_button(ui, palette, self.text(Key::ClearSelection))
                    .clicked()
            {
                self.duplicates.clear_selection();
            }
            ui.label(
                RichText::new(self.text(Key::DuplicatesReviewNote))
                    .color(palette.muted)
                    .size(9.5),
            );
        });
    }

    fn duplicates_basket_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let selected_count = self.duplicates.selected.len();
        let selected_size = self.duplicates.selected_size();
        egui::Panel::bottom("duplicates_review_basket")
            .exact_size(58.0)
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(Margin::symmetric(10, 7)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(icons::TRASH)
                            .font(FontId::proportional(18.0))
                            .color(if selected_count == 0 {
                                palette.muted
                            } else {
                                palette.warning
                            }),
                    );
                    ui.vertical(|ui| {
                        if self.duplicates.is_applying() {
                            ui.label(
                                RichText::new(self.text(Key::Applying))
                                    .font(theme::medium(11.5))
                                    .color(palette.warning),
                            );
                        } else if selected_count == 0 {
                            ui.label(
                                RichText::new(self.text(Key::ReviewBasket))
                                    .font(theme::medium(11.5))
                                    .color(palette.text),
                            );
                            ui.label(
                                RichText::new(self.text(Key::DuplicatesBasketHint))
                                    .color(palette.muted)
                                    .size(10.0),
                            );
                        } else {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {} · {}",
                                    self.text(Key::ReviewBasket),
                                    i18n::items_text(self.language(), selected_count as u64),
                                    format::size(self.language(), selected_size)
                                ))
                                .font(theme::medium(11.5))
                                .color(palette.text),
                            );
                            ui.label(
                                RichText::new(self.text(Key::DuplicateTrashBody))
                                    .color(palette.muted)
                                    .size(10.0),
                            );
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let enabled = self.duplicates.can_apply()
                            && self.duplicates.audit_path.is_some()
                            && !self.duplicates.is_applying();
                        if ui
                            .add_enabled(
                                enabled,
                                Button::new(
                                    RichText::new(self.text(Key::MoveToTrash))
                                        .color(Color32::WHITE)
                                        .font(theme::medium(11.5)),
                                )
                                .fill(if enabled {
                                    palette.danger
                                } else {
                                    palette.surface_hover
                                })
                                .stroke(Stroke::new(1.0, palette.danger))
                                .corner_radius(tokens::RADIUS_CONTROL)
                                .min_size(vec2(0.0, 30.0)),
                            )
                            .clicked()
                        {
                            self.duplicates.confirm_open = true;
                            self.duplicates.confirm_focus_pending = true;
                        }
                        if !self.duplicates.selected.is_empty()
                            && !self.duplicates.is_applying()
                            && components::secondary_button(
                                ui,
                                palette,
                                self.text(Key::ClearSelection),
                            )
                            .clicked()
                        {
                            self.duplicates.clear_selection();
                        }
                    });
                });
            });
    }

    fn duplicates_body(&mut self, ui: &mut egui::Ui, report: &DuplicateReport) {
        let palette = self.palette;
        if report.groups.is_empty() {
            ScrollArea::vertical()
                .id_salt("duplicates_empty_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(12.0);
                    Frame::new()
                        .fill(palette.surface)
                        .stroke(Stroke::new(1.0, palette.border))
                        .corner_radius(tokens::RADIUS_CARD)
                        .inner_margin(18)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(self.text(Key::NoDuplicates))
                                    .font(theme::semibold(15.0))
                                    .color(palette.text),
                            );
                            ui.label(
                                RichText::new(self.text(Key::NoDuplicatesBody))
                                    .color(palette.muted)
                                    .size(11.0),
                            );
                        });
                    if !report.issues.is_empty() {
                        ui.add_space(8.0);
                        components::section_label(ui, palette, self.text(Key::CleanupIssues));
                        for issue in report.issues.iter().take(10) {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}",
                                    issue.path.display(),
                                    issue.message
                                ))
                                .color(palette.muted)
                                .size(10.0),
                            );
                        }
                    }
                });
            return;
        }

        let selected = &self.duplicates.selected;
        let expanded = &self.duplicates.expanded_groups;
        let mut actions = Vec::new();
        // Fixed row geometry keeps scrolling stable and avoids laying out off-screen paths.
        // Only expanded groups contribute file rows; even huge groups render one viewport.
        const HEADER: f32 = 58.0;
        const ROW: f32 = 26.0;
        const FOOTER: f32 = 38.0;
        const GAP: f32 = 8.0;
        ScrollArea::vertical()
            .id_salt("duplicates_scroll")
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let origin = ui.cursor().min;
                let width = ui.available_width();
                let mut offset = 0.0;
                for (index, group) in report.groups.iter().enumerate() {
                    let is_expanded = expanded.contains(&group.fingerprint);
                    let height = HEADER
                        + if is_expanded {
                            group.files.len() as f32 * ROW + FOOTER
                        } else {
                            0.0
                        };
                    let rect =
                        egui::Rect::from_min_size(origin + vec2(0.0, offset), vec2(width, height));
                    if offset + height >= viewport.top() && offset <= viewport.bottom() {
                        ui.painter()
                            .rect_filled(rect, tokens::RADIUS_CARD, palette.surface);
                        ui.painter().rect_stroke(
                            rect,
                            tokens::RADIUS_CARD,
                            Stroke::new(1.0, palette.border),
                            egui::StrokeKind::Inside,
                        );
                        let keeper = self.duplicates.keeper_for(group);
                        let header_rect = egui::Rect::from_min_size(
                            rect.min + vec2(10.0, 4.0),
                            vec2(width - 20.0, HEADER - 8.0),
                        );
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .id_salt((&group.fingerprint, "header"))
                                .max_rect(header_rect)
                                .layout(Layout::top_down(Align::Min)),
                            |ui| {
                                let first_name = group
                                    .files
                                    .first()
                                    .map_or_else(String::new, |file| format::file_name(&file.path));
                                let response = ui.add_sized(
                                    [ui.available_width(), 25.0],
                                    Button::new(
                                        RichText::new(format!(
                                            "{} {}",
                                            if is_expanded {
                                                icons::CARET_DOWN
                                            } else {
                                                icons::CARET_RIGHT
                                            },
                                            first_name
                                        ))
                                        .font(theme::semibold(12.5))
                                        .color(palette.text),
                                    )
                                    .frame(false)
                                    .truncate(),
                                );
                                if response.clicked() {
                                    actions.push(DuplicateUiAction::ToggleExpanded(
                                        group.fingerprint.clone(),
                                    ));
                                }
                                response.on_hover_text(&first_name);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!(
                                            "{} · {} {} · {} {}",
                                            i18n::copies_text(
                                                self.language(),
                                                group.copies() as u64
                                            ),
                                            format::size(self.language(), group.logical_size),
                                            self.text(Key::Each),
                                            self.text(Key::Reclaimable),
                                            format::size(
                                                self.language(),
                                                self.duplicates
                                                    .summary
                                                    .as_ref()
                                                    .and_then(|summary| summary
                                                        .group_savings
                                                        .get(index))
                                                    .copied()
                                                    .unwrap_or(0)
                                            )
                                        ))
                                        .size(10.0)
                                        .color(palette.muted),
                                    )
                                    .truncate(),
                                );
                            },
                        );
                        if is_expanded {
                            let first =
                                (((viewport.top() - offset - HEADER) / ROW).floor().max(0.0)
                                    as usize)
                                    .min(group.files.len());
                            let last = (((viewport.bottom() - offset - HEADER) / ROW)
                                .ceil()
                                .max(0.0) as usize)
                                .min(group.files.len());
                            for (file_index, file) in
                                group.files.iter().enumerate().take(last).skip(first)
                            {
                                let row_rect = egui::Rect::from_min_size(
                                    rect.min + vec2(10.0, HEADER + file_index as f32 * ROW),
                                    vec2(width - 20.0, ROW),
                                );
                                ui.scope_builder(
                                    egui::UiBuilder::new()
                                        .id_salt((&group.fingerprint, file_index))
                                        .max_rect(row_rect)
                                        .layout(Layout::top_down(Align::Min)),
                                    |ui| {
                                        let is_keeper = file.path == keeper;
                                        let mut checked = selected.contains_key(&file.path);
                                        ui.horizontal(|ui| {
                                            let radio = ui.radio(is_keeper, "");
                                            radio.widget_info(|| {
                                                egui::WidgetInfo::selected(
                                                    egui::Role::RadioButton,
                                                    true,
                                                    is_keeper,
                                                    format!(
                                                        "{} {}",
                                                        self.text(Key::Keep),
                                                        file.path.display()
                                                    ),
                                                )
                                            });
                                            if radio.clicked() && !is_keeper {
                                                actions.push(DuplicateUiAction::SetKeeper(
                                                    index,
                                                    file.path.clone(),
                                                ));
                                            }
                                            radio.on_hover_text(self.text(Key::Keep));
                                            let path_width =
                                                (ui.available_width() - 150.0).max(0.0);
                                            ui.add_sized(
                                                [path_width, 21.0],
                                                egui::Label::new(
                                                    RichText::new(format::shorten_middle(
                                                        &file.path.display().to_string(),
                                                        82,
                                                    ))
                                                    .font(FontId::monospace(9.5))
                                                    .color(if is_keeper {
                                                        palette.text
                                                    } else {
                                                        palette.muted
                                                    }),
                                                )
                                                .truncate()
                                                .sense(egui::Sense::click()),
                                            )
                                            .on_hover_text(file.path.display().to_string())
                                            .context_menu(|ui| {
                                                if ui.button(self.text(Key::Open)).clicked() {
                                                    actions.push(DuplicateUiAction::Open(
                                                        file.path.clone(),
                                                    ));
                                                    ui.close();
                                                }
                                                if ui.button(self.text(Key::Reveal)).clicked() {
                                                    actions.push(DuplicateUiAction::Reveal(
                                                        file.path.clone(),
                                                    ));
                                                    ui.close();
                                                }
                                                if ui.button(self.text(Key::CopyPath)).clicked() {
                                                    actions.push(DuplicateUiAction::CopyPath(
                                                        file.path.clone(),
                                                    ));
                                                    ui.close();
                                                }
                                            });
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(format::size(
                                                            self.language(),
                                                            file.size(),
                                                        ))
                                                        .font(FontId::monospace(9.5))
                                                        .color(palette.muted),
                                                    );
                                                    if is_keeper {
                                                        ui.label(
                                                            RichText::new(self.text(Key::Keeper))
                                                                .color(palette.accent)
                                                                .size(9.0),
                                                        );
                                                    } else {
                                                        let check = ui.add_sized(
                                                            [22.0, 20.0],
                                                            Checkbox::new(&mut checked, ""),
                                                        );
                                                        check.widget_info(|| {
                                                            egui::WidgetInfo::labeled(
                                                                egui::Role::CheckBox,
                                                                true,
                                                                format!(
                                                                    "{} {}",
                                                                    self.text(Key::MoveToTrash),
                                                                    file.path.display()
                                                                ),
                                                            )
                                                        });
                                                        if check.changed() {
                                                            actions.push(
                                                                DuplicateUiAction::Toggle(
                                                                    index,
                                                                    file.clone(),
                                                                    checked,
                                                                ),
                                                            );
                                                        }
                                                    }
                                                },
                                            );
                                        });
                                    },
                                );
                            }
                            let footer_rect = egui::Rect::from_min_size(
                                rect.min + vec2(10.0, height - FOOTER),
                                vec2(width - 20.0, FOOTER),
                            );
                            if footer_rect.intersects(ui.clip_rect()) {
                                ui.scope_builder(
                                    egui::UiBuilder::new()
                                        .id_salt((&group.fingerprint, "footer"))
                                        .max_rect(footer_rect)
                                        .layout(Layout::top_down(Align::Min)),
                                    |ui| {
                                        if components::secondary_button(
                                            ui,
                                            palette,
                                            self.text(Key::SelectRest),
                                        )
                                        .clicked()
                                        {
                                            actions.push(DuplicateUiAction::SelectRest(index));
                                        }
                                    },
                                );
                            }
                        }
                    }
                    offset += height + GAP;
                }
                ui.advance_cursor_after_rect(egui::Rect::from_min_size(
                    origin,
                    vec2(width, offset),
                ));
                if !report.issues.is_empty() {
                    components::section_label(ui, palette, self.text(Key::CleanupIssues));
                    for issue in report.issues.iter().take(10) {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "{}: {}",
                                    issue.path.display(),
                                    issue.message
                                ))
                                .color(palette.muted)
                                .size(10.0),
                            )
                            .truncate(),
                        );
                    }
                }
            });

        let context = ui.ctx().clone();
        for action in actions {
            match action {
                DuplicateUiAction::SetKeeper(index, path) => {
                    if let Some(group) = report.groups.get(index) {
                        self.duplicates.set_keeper(group, path);
                    }
                }
                DuplicateUiAction::Toggle(index, file, checked) => {
                    if let Some(group) = report.groups.get(index) {
                        let keeper = self.duplicates.keeper_for(group).to_path_buf();
                        if let Err(error) =
                            self.duplicates.toggle_copy(&file, checked, &keeper, group)
                        {
                            self.notify(error, ToastKind::Error);
                        }
                    }
                }
                DuplicateUiAction::SelectRest(index) => {
                    if let Some(group) = report.groups.get(index)
                        && let Err(error) = self.duplicates.select_rest(group)
                    {
                        self.notify(error, ToastKind::Error);
                    }
                }
                DuplicateUiAction::ToggleExpanded(fingerprint) => {
                    if !self.duplicates.expanded_groups.remove(&fingerprint) {
                        self.duplicates.expanded_groups.insert(fingerprint);
                    }
                }
                DuplicateUiAction::Open(path) => self.open_file_path(&path),
                DuplicateUiAction::Reveal(path) => self.reveal_path(&path),
                DuplicateUiAction::CopyPath(path) => self.copy_path(&context, &path),
            }
        }
    }

    pub(crate) fn duplicates_confirm_dialog(&mut self, context: &Context) {
        if !self.duplicates.confirm_open {
            return;
        }
        let palette = self.palette;
        let mut selected: Vec<CleanupItem> = self.duplicates.selected.values().cloned().collect();
        selected.sort_unstable_by(|left, right| {
            right
                .size()
                .cmp(&left.size())
                .then_with(|| left.path.cmp(&right.path))
        });
        let total_size = selected
            .iter()
            .fold(0_u64, |total, item| total.saturating_add(item.size()));
        let mut close = false;
        let mut confirm = false;
        let audit_path = self.duplicates.audit_path.clone();

        let modal = egui::Modal::new(Id::unique("duplicates_confirm_modal"))
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(tokens::RADIUS_CARD)
                    .inner_margin(20),
            )
            .show(context, |ui| {
                ui.set_width(560.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(icons::TRASH)
                            .font(FontId::proportional(21.0))
                            .color(palette.danger),
                    );
                    ui.label(
                        RichText::new(self.text(Key::MoveToTrashTitle))
                            .font(theme::semibold(18.0))
                            .color(palette.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if components::icon_button(ui, palette, icons::CLOSE, self.text(Key::Close))
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(8.0);
                ui.label(
                    RichText::new(self.text(Key::DuplicateTrashBody))
                        .color(palette.muted)
                        .size(11.0),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(i18n::items_text(self.language(), selected.len() as u64))
                            .font(theme::medium(11.5))
                            .color(palette.text),
                    );
                    ui.label(RichText::new("·").color(palette.muted).size(10.0));
                    ui.label(
                        RichText::new(format::size(self.language(), total_size))
                            .font(FontId::monospace(11.0))
                            .color(palette.text),
                    );
                    if let Some(path) = &audit_path {
                        ui.label(RichText::new("·").color(palette.muted).size(10.0));
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                self.text(Key::AuditLog),
                                format::shorten_middle(&path.display().to_string(), 34)
                            ))
                            .color(palette.muted)
                            .size(9.5),
                        );
                    }
                });
                ui.add_space(8.0);
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(tokens::RADIUS_CARD)
                    .inner_margin(8)
                    .show(ui, |ui| {
                        ScrollArea::vertical()
                            .id_salt("duplicates_confirm_list")
                            .max_height(220.0)
                            .show(ui, |ui| {
                                for item in &selected {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(icons::DUPLICATES)
                                                .font(FontId::proportional(10.5))
                                                .color(palette.muted),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(item.path.display().to_string())
                                                    .font(FontId::monospace(9.5)),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(item.path.display().to_string());
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(format::size(
                                                        self.language(),
                                                        item.size(),
                                                    ))
                                                    .font(FontId::monospace(9.5))
                                                    .color(palette.muted),
                                                );
                                            },
                                        );
                                    });
                                }
                            });
                    });
                ui.add_space(8.0);
                ui.label(
                    RichText::new(self.text(Key::NothingPermanentlyDeleted))
                        .color(palette.muted)
                        .size(10.0),
                );
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            Button::new(
                                RichText::new(self.text(Key::MoveToTrash))
                                    .color(Color32::WHITE)
                                    .font(theme::medium(11.5)),
                            )
                            .fill(palette.danger)
                            .stroke(Stroke::new(1.0, palette.danger))
                            .corner_radius(tokens::RADIUS_CONTROL)
                            .min_size(vec2(0.0, 30.0)),
                        )
                        .clicked()
                    {
                        confirm = true;
                    }
                    let cancel = components::secondary_button(ui, palette, self.text(Key::Cancel));
                    if self.duplicates.confirm_focus_pending {
                        cancel.request_focus();
                        self.duplicates.confirm_focus_pending = false;
                    }
                    if cancel.clicked() {
                        close = true;
                    }
                });
            });

        if modal.should_close() {
            close = true;
        }
        if confirm {
            self.start_duplicate_apply();
            return;
        }
        if close {
            self.duplicates.confirm_open = false;
        }
    }
}

enum DuplicateUiAction {
    SetKeeper(usize, PathBuf),
    Toggle(usize, DuplicateFile, bool),
    SelectRest(usize),
    ToggleExpanded(String),
    Open(PathBuf),
    Reveal(PathBuf),
    CopyPath(PathBuf),
}
