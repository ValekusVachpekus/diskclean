//! Safe cleanup workspace: recommendations, review basket, audit actions.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use diskclean_cleanup::{
    ApplyReport, CleanupContext, CleanupItem, CleanupReport, ItemResultKind, ProtectedPaths,
    default_audit_path, discover_default,
};
use diskclean_rules::{ActionKind, Category, Risk};
use eframe::egui::{
    self, Align, Button, Checkbox, Color32, Context, FontId, Frame, Id, Layout, Margin, RichText,
    ScrollArea, Stroke, vec2,
};

use crate::app::{DiskCleanupApp, ToastKind};
use crate::i18n::{self, Key, Language};
use crate::jobs::CleanupApplyJob as ApplyJob;
use crate::ui::{components, format, icons, theme, tokens};

#[derive(Debug, Default)]
pub(crate) enum CleanupJob {
    #[default]
    Idle,
    Running {
        receiver: Receiver<Result<CleanupReport, String>>,
        started: Instant,
    },
    Failed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RiskFilter {
    #[default]
    All,
    Safe,
    High,
    Commands,
}

impl RiskFilter {
    fn label(self, language: Language) -> &'static str {
        match self {
            Self::All => language.text(Key::All),
            Self::Safe => language.text(Key::Safe),
            Self::High => language.text(Key::RiskHigh),
            Self::Commands => language.text(Key::SystemCommands),
        }
    }
}

#[derive(Default)]
pub(crate) struct CleanupWorkspace {
    pub(crate) report: Option<CleanupReport>,
    pub(crate) job: CleanupJob,
    pub(crate) apply_job: ApplyJob,
    pub(crate) selected: BTreeMap<PathBuf, CleanupItem>,
    pub(crate) risk_filter: RiskFilter,
    pub(crate) expanded_rules: BTreeSet<String>,
    pub(crate) confirm_open: bool,
    pub(crate) confirm_focus_pending: bool,
    pub(crate) acknowledge_high_risk: bool,
    pub(crate) last_apply: Option<ApplyReport>,
    pub(crate) last_apply_audit: Option<PathBuf>,
    pub(crate) context: Option<CleanupContext>,
    pub(crate) protected: Option<ProtectedPaths>,
    pub(crate) audit_path: Option<PathBuf>,
    pub(crate) error: Option<String>,
}

impl CleanupWorkspace {
    pub(crate) fn is_discovering(&self) -> bool {
        matches!(self.job, CleanupJob::Running { .. })
    }

    pub(crate) fn is_applying(&self) -> bool {
        matches!(self.apply_job, ApplyJob::Running { .. })
    }

    pub(crate) fn discovery_elapsed(&self) -> Option<Duration> {
        match &self.job {
            CleanupJob::Running { started, .. } => Some(started.elapsed()),
            _ => None,
        }
    }

    pub(crate) fn selected_size(&self) -> u64 {
        self.selected
            .values()
            .fold(0_u64, |total, item| total.saturating_add(item.size()))
    }

    pub(crate) fn selected_high_risk(&self) -> bool {
        self.selected.values().any(|item| item.risk == Risk::High)
    }

    pub(crate) fn can_apply(&self) -> bool {
        !self.selected.is_empty()
            && !self.is_discovering()
            && !self.is_applying()
            && (!self.selected_high_risk() || self.acknowledge_high_risk)
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selected.clear();
        self.acknowledge_high_risk = false;
    }

    pub(crate) fn select_safe(&mut self, report: &CleanupReport) {
        for recommendation in &report.recommendations {
            if recommendation.rule.action != ActionKind::Trash {
                continue;
            }
            for item in &recommendation.items {
                if item.risk.safe_to_select() {
                    self.selected.insert(item.path.clone(), item.clone());
                }
            }
        }
    }
}

#[derive(Debug)]
enum CleanupUiAction {
    Toggle(CleanupItem, bool),
    ToggleExpanded(String),
    Open(PathBuf),
    Reveal(PathBuf),
    CopyPath(PathBuf),
    CopyCommand(String),
}

impl DiskCleanupApp {
    pub(crate) fn start_cleanup_discovery(&mut self) {
        if self.cleanup.is_discovering() || self.cleanup.is_applying() {
            return;
        }

        let context = CleanupContext::from_environment();
        let audit_path = default_audit_path();
        let audit_root = audit_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let protected = ProtectedPaths::for_context(&context, audit_root);

        let thread_context = context.clone();
        let thread_protected = protected.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = discover_default(&thread_context, &thread_protected)
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });

        self.cleanup.context = Some(context);
        self.cleanup.protected = Some(protected);
        self.cleanup.audit_path = audit_path;
        self.cleanup.selected.clear();
        self.cleanup.acknowledge_high_risk = false;
        self.cleanup.confirm_open = false;
        self.cleanup.error = None;
        self.cleanup.job = CleanupJob::Running {
            receiver,
            started: Instant::now(),
        };
    }

    pub(crate) fn poll_cleanup(&mut self, context: &egui::Context) {
        let received = match &self.cleanup.job {
            CleanupJob::Running { receiver, .. } => Some(receiver.try_recv()),
            _ => None,
        };

        match received {
            Some(Ok(Ok(report))) => {
                self.cleanup.report = Some(report);
                self.cleanup.job = CleanupJob::Idle;
                self.cleanup.selected.clear();
                self.cleanup.acknowledge_high_risk = false;
                self.cleanup.expanded_rules.clear();
                self.cleanup.error = None;
                self.cleanup.last_apply = None;
                self.cleanup.last_apply_audit = None;
            }
            Some(Ok(Err(error))) => {
                self.cleanup.error = Some(error.clone());
                self.notify(
                    format!("{}: {error}", self.text(Key::RecommendationsFailed)),
                    ToastKind::Error,
                );
                self.cleanup.job = CleanupJob::Failed;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                let error = self.text(Key::DiscoveryStopped).to_owned();
                self.cleanup.error = Some(error.clone());
                self.notify(
                    format!("{}: {error}", self.text(Key::RecommendationsFailed)),
                    ToastKind::Error,
                );
                self.cleanup.job = CleanupJob::Failed;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        let apply_received = match &self.cleanup.apply_job {
            ApplyJob::Running { receiver, .. } => Some(receiver.try_recv()),
            _ => None,
        };
        match apply_received {
            Some(Ok(Ok(report))) => {
                let trashed: std::collections::HashSet<PathBuf> = report
                    .results
                    .iter()
                    .filter(|result| result.kind == ItemResultKind::Trashed)
                    .map(|result| result.path.clone())
                    .collect();
                self.cleanup
                    .selected
                    .retain(|path, _| !trashed.contains(path));
                self.cleanup.acknowledge_high_risk = false;
                if let Some(cleanup_report) = self.cleanup.report.as_mut() {
                    for recommendation in &mut cleanup_report.recommendations {
                        recommendation
                            .items
                            .retain(|item| !trashed.contains(&item.path));
                    }
                }

                let failed = report.failed_count();
                let trashed_count = report.trashed_count;
                let bytes = report.bytes_trashed;
                let message = if failed == 0 {
                    format!(
                        "{}: {} · {}",
                        self.text(Key::ApplyComplete),
                        i18n::items_text(self.language(), trashed_count as u64),
                        format::size(self.language(), bytes)
                    )
                } else {
                    format!(
                        "{}: {} · {trashed_count} / {}",
                        self.text(Key::ApplyPartial),
                        self.text(Key::ApplyComplete),
                        trashed_count + failed
                    )
                };
                self.cleanup.last_apply_audit = self.cleanup.audit_path.clone();
                self.cleanup.last_apply = Some(report);
                self.cleanup.apply_job = ApplyJob::Idle;
                self.cleanup.error = None;
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
                self.cleanup.apply_job = ApplyJob::Idle;
                self.cleanup.confirm_open = false;
                self.cleanup.error = Some(error.clone());
                self.notify(
                    format!("{}: {error}", self.text(Key::ApplyFailed)),
                    ToastKind::Error,
                );
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.cleanup.apply_job = ApplyJob::Idle;
                self.cleanup.confirm_open = false;
                let error = self.text(Key::CleanupWorkerStopped).to_owned();
                self.cleanup.error = Some(error.clone());
                self.notify(
                    format!("{}: {error}", self.text(Key::ApplyFailed)),
                    ToastKind::Error,
                );
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        if self.cleanup.is_discovering() || self.cleanup.is_applying() {
            context.request_repaint_after(Duration::from_millis(100));
        }
    }

    pub(crate) fn start_cleanup_apply(&mut self) {
        if !self.cleanup.can_apply() || self.duplicates.is_applying() {
            return;
        }
        let Some(protected) = self.cleanup.protected.clone() else {
            self.cleanup.error = Some(self.text(Key::AuditUnavailable).to_owned());
            return;
        };
        let Some(audit_path) = self.cleanup.audit_path.clone() else {
            self.cleanup.error = Some(self.text(Key::AuditUnavailable).to_owned());
            return;
        };

        let items: Vec<CleanupItem> = self.cleanup.selected.values().cloned().collect();
        self.cleanup.apply_job = ApplyJob::start(items, protected, audit_path);
        self.cleanup.confirm_open = false;
        self.cleanup.acknowledge_high_risk = false;
        self.cleanup.error = None;
    }

    pub(crate) fn cleanup_workspace(&mut self, ui: &mut egui::Ui) {
        let report = self.cleanup.report.take();
        self.cleanup_header(ui, report.is_some());
        self.cleanup_banners(ui);

        if let Some(report) = report.as_ref() {
            self.cleanup_summary(ui, report);
            self.cleanup_filter_bar(ui, report);
            self.cleanup_basket_panel(ui);
            self.cleanup_body(ui, report);
        } else {
            self.cleanup_empty(ui);
        }

        self.cleanup.report = report;
    }

    fn cleanup_header(&mut self, ui: &mut egui::Ui, has_report: bool) {
        let palette = self.palette;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icons::BROOM)
                    .font(FontId::proportional(23.0))
                    .color(palette.accent),
            );
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(self.text(Key::Cleanup))
                        .font(theme::semibold(21.0))
                        .color(palette.text),
                );
                ui.label(
                    RichText::new(self.text(Key::CleanupSubtitle))
                        .color(palette.muted)
                        .size(11.0),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let busy = self.cleanup.is_discovering() || self.cleanup.is_applying();
                if !busy
                    && has_report
                    && components::secondary_button(ui, palette, self.text(Key::ScanAgain))
                        .clicked()
                {
                    self.start_cleanup_discovery();
                }
            });
        });

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if let Some(audit_path) = self.cleanup.audit_path.clone() {
                ui.label(
                    RichText::new(icons::LIST)
                        .font(FontId::proportional(12.0))
                        .color(palette.muted),
                );
                let audit_label = format::shorten_middle(&audit_path.display().to_string(), 62);
                ui.add(
                    egui::Label::new(RichText::new(audit_label).color(palette.muted).size(10.0))
                        .truncate(),
                )
                .on_hover_text(audit_path.display().to_string());
                if components::icon_button(ui, palette, icons::COPY, self.text(Key::CopyPath))
                    .clicked()
                {
                    self.copy_path(ui.ctx(), &audit_path);
                }
                if components::secondary_button(ui, palette, self.text(Key::OpenAuditLog)).clicked()
                {
                    self.open_file_path(&audit_path);
                }
            } else {
                ui.label(
                    RichText::new(self.text(Key::AuditUnavailable))
                        .color(palette.danger)
                        .size(10.5),
                );
            }
            let trash = self
                .cleanup
                .context
                .as_ref()
                .and_then(|context| context.trash.clone());
            if let Some(trash) = trash
                && trash.exists()
                && components::secondary_button(ui, palette, self.text(Key::OpenTrash)).clicked()
            {
                self.open_path(&trash);
            }
        });
    }

    fn cleanup_banners(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        if let Some(error) = self.cleanup.error.clone() {
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
                            self.cleanup.error = None;
                        }
                    });
                });
        }

        if let Some(apply) = self.cleanup.last_apply.clone() {
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
                            RichText::new(if failed == 0 {
                                icons::CHECK
                            } else {
                                icons::WARNING
                            })
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
                            .size(11.0),
                        );
                        if failed > 0 {
                            ui.label(
                                RichText::new(format!("{failed} {}", self.text(Key::FailedItems)))
                                    .color(palette.warning)
                                    .size(10.5),
                            );
                        }
                        if let Some(reason) = &apply.aborted_reason {
                            ui.label(
                                RichText::new(format::shorten_middle(reason, 90))
                                    .color(palette.muted)
                                    .size(10.0),
                            )
                            .on_hover_text(reason);
                        }
                        if let Some(audit) = self.cleanup.last_apply_audit.clone()
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
                            self.cleanup.last_apply = None;
                            self.cleanup.last_apply_audit = None;
                        }
                    });
                    if failed > 0 {
                        for result in apply
                            .results
                            .iter()
                            .filter(|result| result.kind != ItemResultKind::Trashed)
                            .take(4)
                        {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}",
                                    result.path.display(),
                                    result.error.as_deref().unwrap_or("")
                                ))
                                .color(palette.muted)
                                .size(10.0),
                            );
                        }
                    }
                });
        }

        if let Some(elapsed) = self.cleanup.discovery_elapsed() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0).color(palette.accent));
                ui.label(
                    RichText::new(self.text(Key::ScanningRecommendations))
                        .color(palette.accent)
                        .size(11.0),
                );
                ui.label(
                    RichText::new(format::duration(
                        self.language(),
                        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                    ))
                    .color(palette.muted)
                    .size(10.5),
                );
            });
        }
        if self.cleanup.is_applying() {
            ui.add_space(6.0);
            self.cleanup
                .apply_job
                .show_progress(ui, palette, self.language());
        }
    }

    fn cleanup_empty(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        ScrollArea::vertical()
            .id_salt("cleanup_empty_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(10.0);
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(tokens::RADIUS_CARD)
                    .inner_margin(18)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(self.text(Key::CleanupDescription))
                                .color(palette.text)
                                .size(12.0),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(self.text(Key::SafetyNote))
                                .color(palette.muted)
                                .size(10.5),
                        );
                        ui.add_space(12.0);
                        if !self.cleanup.is_discovering()
                            && components::primary_button(
                                ui,
                                palette,
                                self.text(Key::FindRecommendations),
                            )
                            .clicked()
                        {
                            self.start_cleanup_discovery();
                        }
                    });
                ui.add_space(10.0);
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(tokens::RADIUS_CARD)
                    .inner_margin(14)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(icons::INFO)
                                    .font(FontId::proportional(14.0))
                                    .color(palette.info),
                            );
                            ui.label(
                                RichText::new(self.text(Key::NothingPermanentlyDeleted))
                                    .color(palette.muted)
                                    .size(11.0),
                            );
                        });
                    });
            });
    }

    fn cleanup_summary(&mut self, ui: &mut egui::Ui, report: &CleanupReport) {
        let palette = self.palette;
        let mut total_items = 0_u64;
        let mut total_bytes = 0_u64;
        let mut safe_items = 0_u64;
        let mut safe_bytes = 0_u64;
        let mut high_items = 0_u64;
        for recommendation in &report.recommendations {
            if recommendation.rule.action != ActionKind::Trash {
                continue;
            }
            for item in &recommendation.items {
                total_items = total_items.saturating_add(1);
                total_bytes = total_bytes.saturating_add(item.size());
                if item.risk.safe_to_select() {
                    safe_items = safe_items.saturating_add(1);
                    safe_bytes = safe_bytes.saturating_add(item.size());
                } else if item.risk == Risk::High {
                    high_items = high_items.saturating_add(1);
                }
            }
        }

        ui.add_space(6.0);
        let checked = format::age(
            self.language(),
            i64::try_from(report.generated_at_unix_ms / 1_000).unwrap_or(i64::MAX),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{} {checked}", self.text(Key::CheckedAt)))
                    .color(palette.muted)
                    .size(10.0),
            );
            if !report.issues.is_empty() {
                ui.label(RichText::new("·").color(palette.muted).size(10.0));
                ui.label(
                    RichText::new(format!(
                        "{}: {}",
                        self.text(Key::CleanupIssues),
                        report.issues.len()
                    ))
                    .color(palette.warning)
                    .size(10.0),
                );
            }
        });

        ui.add_space(5.0);
        let columns = if ui.available_width() >= 660.0 { 3 } else { 1 };
        let card_width = (ui.available_width() - (columns - 1) as f32 * 8.0) / columns as f32;
        egui::Grid::new("cleanup_summary")
            .num_columns(columns)
            .spacing(vec2(8.0, 8.0))
            .show(ui, |ui| {
                components::stat_card(
                    ui,
                    palette,
                    self.text(Key::Candidates),
                    &i18n::items_text(self.language(), total_items),
                    &format::size(self.language(), total_bytes),
                    palette.text,
                    card_width,
                );
                if columns == 1 {
                    ui.end_row();
                }
                components::stat_card(
                    ui,
                    palette,
                    self.text(Key::SelectSafe),
                    &i18n::items_text(self.language(), safe_items),
                    &format!(
                        "{} {}",
                        format::size(self.language(), safe_bytes),
                        self.text(Key::PotentialSavings)
                    ),
                    palette.success,
                    card_width,
                );
                if columns == 1 {
                    ui.end_row();
                }
                components::stat_card(
                    ui,
                    palette,
                    self.text(Key::RiskHigh),
                    &i18n::items_text(self.language(), high_items),
                    self.text(Key::NeedsConfirmation),
                    if high_items > 0 {
                        palette.warning
                    } else {
                        palette.muted
                    },
                    card_width,
                );
                if columns == 1 {
                    ui.end_row();
                }
            });
    }

    fn cleanup_filter_bar(&mut self, ui: &mut egui::Ui, report: &CleanupReport) {
        let palette = self.palette;
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for filter in [
                RiskFilter::All,
                RiskFilter::Safe,
                RiskFilter::High,
                RiskFilter::Commands,
            ] {
                let selected = self.cleanup.risk_filter == filter;
                let response = ui.add(
                    Button::new(
                        RichText::new(filter.label(self.language()))
                            .size(11.0)
                            .color(if selected {
                                palette.accent
                            } else {
                                palette.muted
                            }),
                    )
                    .fill(if selected {
                        palette.accent_soft()
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(if selected {
                        Stroke::new(1.0, palette.accent)
                    } else {
                        Stroke::new(1.0, palette.border)
                    })
                    .corner_radius(12)
                    .min_size(vec2(0.0, 25.0)),
                );
                if response.clicked() {
                    self.cleanup.risk_filter = filter;
                }
            }

            if !matches!(self.cleanup.risk_filter, RiskFilter::Commands) {
                if components::secondary_button(ui, palette, self.text(Key::SelectSafe)).clicked() {
                    self.cleanup.select_safe(report);
                }
                if !self.cleanup.selected.is_empty()
                    && components::secondary_button(ui, palette, self.text(Key::ClearSelection))
                        .clicked()
                {
                    self.cleanup.clear_selection();
                }
            }
        });
        if self.cleanup.selected_high_risk() {
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    icons::WARNING,
                    self.text(Key::HighRiskSelected)
                ))
                .color(palette.warning)
                .size(10.5),
            );
        }
    }

    fn cleanup_basket_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let selected_count = self.cleanup.selected.len();
        let selected_size = self.cleanup.selected_size();
        egui::Panel::bottom("cleanup_review_basket")
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
                        if self.cleanup.is_applying() {
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
                                RichText::new(self.text(Key::ReviewBasketHint))
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
                                RichText::new(self.text(Key::NothingPermanentlyDeleted))
                                    .color(palette.muted)
                                    .size(10.0),
                            );
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let enabled = self.cleanup.can_apply()
                            && self.cleanup.audit_path.is_some()
                            && !self.cleanup.is_applying();
                        let response = ui.add_enabled(
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
                        );
                        if response.clicked() {
                            self.cleanup.confirm_open = true;
                            self.cleanup.confirm_focus_pending = true;
                            self.cleanup.acknowledge_high_risk = false;
                        }
                        if !self.cleanup.selected.is_empty()
                            && !self.cleanup.is_applying()
                            && components::secondary_button(
                                ui,
                                palette,
                                self.text(Key::ClearSelection),
                            )
                            .clicked()
                        {
                            self.cleanup.clear_selection();
                        }
                    });
                });
            });
    }

    fn cleanup_body(&mut self, ui: &mut egui::Ui, report: &CleanupReport) {
        let palette = self.palette;
        let language = self.language();
        let filter = self.cleanup.risk_filter;
        let selected = &self.cleanup.selected;
        let expanded = &self.cleanup.expanded_rules;
        let mut actions = Vec::new();

        let mut categories: BTreeMap<Category, Vec<&diskclean_cleanup::Recommendation>> =
            BTreeMap::new();
        let mut commands = Vec::new();
        for recommendation in &report.recommendations {
            if recommendation.rule.action == ActionKind::SystemCommand {
                commands.push(recommendation);
                continue;
            }
            if recommendation.items.is_empty() {
                continue;
            }
            let matches = match filter {
                RiskFilter::All => true,
                RiskFilter::Safe => recommendation.rule.risk.safe_to_select(),
                RiskFilter::High => recommendation.rule.risk == Risk::High,
                RiskFilter::Commands => false,
            };
            if matches {
                categories
                    .entry(recommendation.rule.category)
                    .or_default()
                    .push(recommendation);
            }
        }

        let has_candidate_rules = categories.values().any(|rules| !rules.is_empty());
        let show_commands = matches!(filter, RiskFilter::All | RiskFilter::Commands);
        let has_visible_commands = show_commands && !commands.is_empty();

        ScrollArea::vertical()
            .id_salt("cleanup_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !has_candidate_rules && !has_visible_commands {
                    if filter == RiskFilter::Commands {
                        components::empty_hint(ui, palette, self.text(Key::NoMatches));
                    } else {
                        components::empty_hint(ui, palette, self.text(Key::NoCleanupCandidates));
                        if filter == RiskFilter::All && report.item_count() == 0 {
                            ui.label(
                                RichText::new(self.text(Key::NoCleanupCandidatesBody))
                                    .color(palette.muted)
                                    .size(11.0),
                            );
                        } else {
                            ui.label(
                                RichText::new(self.text(Key::NoMatches))
                                    .color(palette.muted)
                                    .size(11.0),
                            );
                        }
                    }
                }

                for (category, recommendations) in categories {
                    components::section_label(ui, palette, category_text(language, category));
                    for recommendation in recommendations {
                        cleanup_rule_card(
                            ui,
                            palette,
                            language,
                            recommendation,
                            selected,
                            expanded,
                            &mut actions,
                        );
                        ui.add_space(6.0);
                    }
                }

                if show_commands && !commands.is_empty() {
                    ui.add_space(8.0);
                    components::section_label(ui, palette, self.text(Key::SystemCommands));
                    Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(
                            palette.warning.r(),
                            palette.warning.g(),
                            palette.warning.b(),
                            if palette.is_dark() { 20 } else { 12 },
                        ))
                        .stroke(Stroke::new(1.0, palette.warning))
                        .corner_radius(tokens::RADIUS_CONTROL)
                        .inner_margin(Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(self.text(Key::SystemCommandsNote))
                                    .color(palette.text)
                                    .size(10.5),
                            );
                        });
                    ui.add_space(5.0);
                    for recommendation in commands {
                        cleanup_command_card(ui, palette, language, recommendation, &mut actions);
                        ui.add_space(6.0);
                    }
                }

                if !report.issues.is_empty() {
                    ui.add_space(8.0);
                    components::section_label(ui, palette, self.text(Key::CleanupIssues));
                    for issue in report.issues.iter().take(10) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(icons::WARNING)
                                    .font(FontId::proportional(11.0))
                                    .color(palette.warning),
                            );
                            ui.label(
                                RichText::new(issue.path.display().to_string())
                                    .font(FontId::monospace(10.0))
                                    .color(palette.muted),
                            );
                            ui.label(
                                RichText::new(&issue.message)
                                    .color(palette.muted)
                                    .size(10.0),
                            );
                        });
                    }
                    if report.issues.len() > 10 {
                        ui.label(
                            RichText::new(format!("+{}", report.issues.len() - 10))
                                .color(palette.muted)
                                .size(10.0),
                        );
                    }
                }
                ui.add_space(8.0);
            });

        let context = ui.ctx().clone();
        for action in actions {
            match action {
                CleanupUiAction::Toggle(item, checked) => {
                    if checked {
                        self.cleanup.selected.insert(item.path.clone(), item);
                    } else {
                        self.cleanup.selected.remove(&item.path);
                        if !self.cleanup.selected_high_risk() {
                            self.cleanup.acknowledge_high_risk = false;
                        }
                    }
                }
                CleanupUiAction::ToggleExpanded(rule_id) => {
                    if !self.cleanup.expanded_rules.remove(&rule_id) {
                        self.cleanup.expanded_rules.insert(rule_id);
                    }
                }
                CleanupUiAction::Open(path) => self.open_file_path(&path),
                CleanupUiAction::Reveal(path) => self.reveal_path(&path),
                CleanupUiAction::CopyPath(path) => self.copy_path(&context, &path),
                CleanupUiAction::CopyCommand(command) => {
                    context.copy_text(command);
                    self.notify(self.text(Key::CopiedCommand).to_owned(), ToastKind::Info);
                }
            }
        }
    }

    pub(crate) fn cleanup_confirm_dialog(&mut self, context: &Context) {
        if !self.cleanup.confirm_open {
            return;
        }
        let palette = self.palette;
        let high_risk = self.cleanup.selected_high_risk();
        let mut selected: Vec<CleanupItem> = self.cleanup.selected.values().cloned().collect();
        selected.sort_unstable_by(|left, right| {
            right
                .size()
                .cmp(&left.size())
                .then_with(|| left.path.cmp(&right.path))
        });
        let total_size = selected
            .iter()
            .fold(0_u64, |total, item| total.saturating_add(item.size()));
        let mut acknowledge = self.cleanup.acknowledge_high_risk || !high_risk;
        let mut close = false;
        let mut confirm = false;
        let audit_path = self.cleanup.audit_path.clone();

        let modal = egui::Modal::new(Id::unique("cleanup_confirm_modal"))
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
                    RichText::new(self.text(Key::MoveToTrashBody))
                        .color(palette.muted)
                        .size(11.5),
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
                            RichText::new(self.text(Key::AuditLog))
                                .color(palette.muted)
                                .size(10.0),
                        );
                        ui.label(
                            RichText::new(format::shorten_middle(&path.display().to_string(), 38))
                                .font(FontId::monospace(9.5))
                                .color(palette.muted),
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
                            .id_salt("cleanup_confirm_list")
                            .max_height(220.0)
                            .show(ui, |ui| {
                                for item in &selected {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(if item.risk == Risk::High {
                                                icons::WARNING
                                            } else {
                                                icons::FILE
                                            })
                                            .font(FontId::proportional(11.0))
                                            .color(
                                                if item.risk == Risk::High {
                                                    palette.warning
                                                } else {
                                                    palette.muted
                                                },
                                            ),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(item.path.display().to_string())
                                                    .font(FontId::monospace(10.0)),
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
                                                    .font(FontId::monospace(10.0))
                                                    .color(palette.muted),
                                                );
                                            },
                                        );
                                    });
                                }
                            });
                    });
                if high_risk {
                    ui.add_space(8.0);
                    Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(
                            palette.warning.r(),
                            palette.warning.g(),
                            palette.warning.b(),
                            if palette.is_dark() { 28 } else { 16 },
                        ))
                        .stroke(Stroke::new(1.0, palette.warning))
                        .corner_radius(tokens::RADIUS_CONTROL)
                        .inner_margin(9)
                        .show(ui, |ui| {
                            ui.checkbox(
                                &mut acknowledge,
                                RichText::new(self.text(Key::HighRiskAck))
                                    .color(palette.text)
                                    .size(11.0),
                            );
                        });
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new(self.text(Key::NothingPermanentlyDeleted))
                        .color(palette.muted)
                        .size(10.5),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.text(Key::Cancel))
                            .color(palette.muted)
                            .size(10.5),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let enabled = !high_risk || acknowledge;
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
                            confirm = true;
                        }
                        let cancel =
                            components::secondary_button(ui, palette, self.text(Key::Cancel));
                        if self.cleanup.confirm_focus_pending {
                            cancel.request_focus();
                            self.cleanup.confirm_focus_pending = false;
                        }
                        if cancel.clicked() {
                            close = true;
                        }
                    });
                });
            });

        self.cleanup.acknowledge_high_risk = acknowledge;
        if modal.should_close() {
            close = true;
        }
        if confirm {
            self.start_cleanup_apply();
            return;
        }
        if close {
            self.cleanup.confirm_open = false;
            self.cleanup.acknowledge_high_risk = false;
        }
    }
}

fn cleanup_rule_card(
    ui: &mut egui::Ui,
    palette: crate::ui::theme::Palette,
    language: Language,
    recommendation: &diskclean_cleanup::Recommendation,
    selected: &BTreeMap<PathBuf, CleanupItem>,
    expanded: &BTreeSet<String>,
    actions: &mut Vec<CleanupUiAction>,
) {
    let rule = &recommendation.rule;
    let is_expanded = expanded.contains(&rule.id);
    Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(tokens::RADIUS_CARD)
        .inner_margin(11)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                risk_badge(ui, palette, language, rule.risk);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(rule.title.localized(language.is_russian()))
                            .font(theme::semibold(13.0))
                            .color(palette.text),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            category_text(language, rule.category),
                            recommendation.count()
                        ))
                        .color(palette.muted)
                        .size(9.5),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format::size(language, recommendation.total_bytes()))
                            .font(FontId::monospace(11.0))
                            .color(palette.text),
                    );
                });
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(rule.description.localized(language.is_russian()))
                    .color(palette.muted)
                    .size(10.5),
            );
            ui.add_space(5.0);

            const VISIBLE_ITEMS: usize = 30;
            let limit = if is_expanded {
                recommendation.items.len()
            } else {
                VISIBLE_ITEMS
            };
            for item in recommendation.items.iter().take(limit) {
                let mut checked = selected.contains_key(&item.path);
                let path_text = format::shorten_middle(&item.path.display().to_string(), 86);
                ui.horizontal(|ui| {
                    let checkbox_width = (ui.available_width() - 104.0).max(120.0);
                    let response = ui.add_sized(
                        [checkbox_width, 22.0],
                        Checkbox::new(
                            &mut checked,
                            RichText::new(path_text)
                                .font(FontId::monospace(10.0))
                                .color(palette.text),
                        ),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::Role::CheckBox,
                            true,
                            item.path.display().to_string(),
                        )
                    });
                    response
                        .clone()
                        .on_hover_text(item.path.display().to_string())
                        .context_menu(|ui| {
                            if ui.button(language.text(Key::Open)).clicked() {
                                actions.push(CleanupUiAction::Open(item.path.clone()));
                                ui.close();
                            }
                            if ui.button(language.text(Key::Reveal)).clicked() {
                                actions.push(CleanupUiAction::Reveal(item.path.clone()));
                                ui.close();
                            }
                            if ui.button(language.text(Key::CopyPath)).clicked() {
                                actions.push(CleanupUiAction::CopyPath(item.path.clone()));
                                ui.close();
                            }
                        });
                    if response.changed() {
                        actions.push(CleanupUiAction::Toggle(item.clone(), checked));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format::size(language, item.size()))
                                .font(FontId::monospace(10.0))
                                .color(palette.muted),
                        );
                        if item.risk == Risk::High {
                            ui.label(
                                RichText::new(icons::WARNING)
                                    .font(FontId::proportional(10.5))
                                    .color(palette.warning),
                            );
                        }
                    });
                });
            }
            if recommendation.items.len() > VISIBLE_ITEMS {
                let label = if is_expanded {
                    format!(
                        "{} ({})",
                        language.text(Key::ShowLess),
                        recommendation.items.len()
                    )
                } else {
                    format!(
                        "{} +{}",
                        language.text(Key::ShowMore),
                        recommendation.items.len() - VISIBLE_ITEMS
                    )
                };
                if components::chip(ui, palette, &label, None).clicked() {
                    actions.push(CleanupUiAction::ToggleExpanded(rule.id.clone()));
                }
            }
        });
}

fn cleanup_command_card(
    ui: &mut egui::Ui,
    palette: crate::ui::theme::Palette,
    language: Language,
    recommendation: &diskclean_cleanup::Recommendation,
    actions: &mut Vec<CleanupUiAction>,
) {
    let rule = &recommendation.rule;
    Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(tokens::RADIUS_CARD)
        .inner_margin(11)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                risk_badge(ui, palette, language, rule.risk);
                ui.label(
                    RichText::new(rule.title.localized(language.is_russian()))
                        .font(theme::semibold(13.0))
                        .color(palette.text),
                );
            });
            ui.label(
                RichText::new(rule.description.localized(language.is_russian()))
                    .color(palette.muted)
                    .size(10.5),
            );
            ui.add_space(5.0);
            for command in &rule.commands {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(command.label.localized(language.is_russian()))
                            .font(theme::medium(10.5))
                            .color(palette.text),
                    );
                    if command.requires_admin {
                        ui.label(
                            RichText::new(language.text(Key::RequiresAdmin))
                                .color(palette.warning)
                                .size(9.5),
                        );
                    }
                });
                Frame::new()
                    .fill(palette.bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(tokens::RADIUS_CONTROL)
                    .inner_margin(Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&command.command)
                                        .font(FontId::monospace(10.5))
                                        .color(palette.text),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&command.command);
                            if components::icon_button(
                                ui,
                                palette,
                                icons::COPY,
                                language.text(Key::CopyCommand),
                            )
                            .clicked()
                            {
                                actions.push(CleanupUiAction::CopyCommand(command.command.clone()));
                            }
                        });
                    });
                if let Some(note) = &command.note {
                    ui.label(
                        RichText::new(note.localized(language.is_russian()))
                            .color(palette.muted)
                            .size(9.5),
                    );
                }
            }
        });
}

fn risk_badge(
    ui: &mut egui::Ui,
    palette: crate::ui::theme::Palette,
    language: Language,
    risk: Risk,
) {
    let color = risk_color(palette, risk);
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            if palette.is_dark() { 30 } else { 20 },
        ))
        .stroke(Stroke::new(1.0, color))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(risk_text(language, risk))
                    .color(color)
                    .size(9.5),
            );
        });
}

fn risk_color(palette: crate::ui::theme::Palette, risk: Risk) -> Color32 {
    match risk {
        Risk::Low => palette.success,
        Risk::Medium => palette.info,
        Risk::High => palette.warning,
        Risk::SystemOnly => palette.muted,
    }
}

fn risk_text(language: Language, risk: Risk) -> &'static str {
    match risk {
        Risk::Low => language.text(Key::RiskLow),
        Risk::Medium => language.text(Key::RiskMedium),
        Risk::High => language.text(Key::RiskHigh),
        Risk::SystemOnly => language.text(Key::RiskSystem),
    }
}

fn category_text(language: Language, category: Category) -> &'static str {
    match category {
        Category::Temporary => language.text(Key::CategoryTemporary),
        Category::AppCache => language.text(Key::CategoryAppCache),
        Category::PackageCache => language.text(Key::CategoryPackageCache),
        Category::Trash => language.text(Key::CategoryTrash),
        Category::OldDownloads => language.text(Key::CategoryOldDownloads),
        Category::LargeOldFiles => language.text(Key::CategoryLargeOldFiles),
        Category::DevArtifacts => language.text(Key::CategoryDevArtifacts),
        Category::Duplicates => language.text(Key::CategoryDuplicates),
        Category::System => language.text(Key::CategorySystem),
    }
}
