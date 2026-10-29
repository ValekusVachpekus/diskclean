//! Background application inventory and a read-only applications workspace.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use diskclean_core::applications::{
    Application, ApplicationSource, ApplicationsReport, InstallationStatus, discover_applications,
};
use diskclean_core::{ScanControl, ScanError, ScanOptions};
use eframe::egui::{self, Align2, FontId, RichText, Stroke, vec2};

use crate::app::DiskCleanupApp;
use crate::i18n::Key;
use crate::ui::{components, format, icons, theme, tokens};

type InventoryResult = Result<ApplicationsReport, ScanError>;
struct InventoryJob {
    control: ScanControl,
    receiver: Receiver<InventoryResult>,
}
impl Drop for InventoryJob {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

#[derive(Default)]
pub(crate) struct ApplicationsWorkspace {
    pub(crate) root: String,
    pub(crate) source: ApplicationSource,
    pub(crate) report: Option<ApplicationsReport>,
    pub(crate) selected: Option<usize>,
    job: Option<InventoryJob>,
    message: Option<Key>,
    pub(crate) search: String,
    pub(crate) show_components: bool,
    view_key: Option<(usize, usize, String)>,
    groups: [Vec<usize>; 2],
}

impl DiskCleanupApp {
    pub(crate) fn start_applications(&mut self) {
        if self.applications.job.is_some() {
            return;
        }
        let roots = (!self.applications.root.trim().is_empty())
            .then(|| vec![PathBuf::from(&self.applications.root)]);
        let source = self.applications.source;
        let options = ScanOptions {
            threads: self.settings.worker_threads,
            cross_filesystems: self.settings.cross_filesystems,
            ..Default::default()
        };
        let control = ScanControl::new();
        let worker = control.clone();
        let (sender, receiver) = mpsc::channel();
        match std::thread::Builder::new()
            .name("applications".into())
            .spawn(move || {
                let _ = sender.send(discover_applications(
                    roots.as_deref(),
                    source,
                    &options,
                    &worker,
                ));
            }) {
            Ok(_) => {
                self.applications.job = Some(InventoryJob { control, receiver });
                self.applications.message = None;
            }
            Err(_) => self.applications.message = Some(Key::InventoryFailed),
        }
    }

    pub(crate) fn cancel_applications(&self) {
        if let Some(job) = &self.applications.job {
            job.control.cancel();
        }
    }

    pub(crate) fn poll_applications(&mut self, context: &egui::Context) {
        let Some(job) = &self.applications.job else {
            return;
        };
        match job.receiver.try_recv() {
            Ok(Ok(report)) => {
                self.applications.view_key = None;
                self.applications.report = Some(report);
                self.applications.selected = None;
                self.applications.job = None;
            }
            Ok(Err(error)) => {
                self.applications.message = Some(if matches!(error, ScanError::Cancelled) {
                    Key::InventoryCancelled
                } else {
                    Key::InventoryFailed
                });
                self.applications.job = None;
            }
            Err(TryRecvError::Disconnected) => {
                self.applications.message = Some(Key::InventoryFailed);
                self.applications.job = None;
            }
            Err(TryRecvError::Empty) => context.request_repaint_after(Duration::from_millis(100)),
        }
    }

    pub(crate) fn applications_workspace(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let language = self.language();
        let busy = self.applications.job.is_some();
        components::workspace_title(
            ui,
            palette,
            icons::APPS,
            language.text(Key::Applications),
            language.text(Key::ApplicationsDescription),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if busy {
                ui.spinner();
                ui.label(language.text(Key::Scanning));
                if components::secondary_button(ui, palette, language.text(Key::Cancel)).clicked() {
                    self.cancel_applications();
                }
            } else {
                let key = if self.applications.report.is_some() {
                    Key::RefreshApplications
                } else {
                    Key::DiscoverApplications
                };
                if components::primary_button(ui, palette, language.text(key)).clicked() {
                    self.start_applications();
                }
            }
            ui.label(
                RichText::new(if self.applications.root.trim().is_empty() {
                    language.text(Key::SteamAutomaticSource)
                } else {
                    language.text(Key::SteamCustomSource)
                })
                .color(palette.muted),
            );
        });
        egui::CollapsingHeader::new(language.text(Key::SteamLibrarySettings))
            .id_salt("steam_library_settings")
            .show(ui, |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (source, label) in [
                            (ApplicationSource::Steam, "Steam"),
                            (ApplicationSource::Epic, "Epic Games"),
                            (ApplicationSource::Legendary, "Legendary"),
                            (ApplicationSource::Heroic, "Heroic (Epic)"),
                            (ApplicationSource::Gog, "GOG"),
                            (ApplicationSource::Lutris, "Lutris"),
                            (ApplicationSource::HeroicGog, "Heroic (GOG)"),
                        ] {
                            ui.selectable_value(&mut self.applications.source, source, label);
                        }
                    });
                });
                let hint = match self.applications.source {
                    ApplicationSource::Lutris => Key::LutrisRootHint,
                    ApplicationSource::Gog => Key::GogRootHint,
                    ApplicationSource::HeroicGog => Key::HeroicGogRootHint,
                    ApplicationSource::Steam => Key::SteamRootHint,
                    ApplicationSource::Epic => Key::EpicRootHint,
                    ApplicationSource::Legendary | ApplicationSource::Heroic => {
                        Key::LegendaryRootHint
                    }
                };
                ui.label(language.text(hint));
                ui.add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut self.applications.root)
                        .hint_text(language.text(hint))
                        .desired_width(ui.available_width()),
                );
            });
        if let Some(job) = &self.applications.job {
            let progress = job.control.snapshot();
            if let Some(path) = progress.current_path {
                let path = path.to_string_lossy();
                ui.add(egui::Label::new(path.as_ref()).truncate())
                    .on_hover_text(path.as_ref());
            }
        }
        if let Some(key) = self.applications.message {
            ui.label(language.text(key));
        }
        ui.add_space(8.0);
        if self.applications.report.is_none() {
            components::empty_hint(ui, palette, language.text(Key::ApplicationsEmpty));
            return;
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.applications.search)
                .hint_text(language.text(Key::SearchHint))
                .desired_width(ui.available_width()),
        );
        self.applications.update_groups();
        let Some(report) = self.applications.report.as_ref() else {
            return;
        };
        if !report.issues.is_empty() {
            egui::CollapsingHeader::new(format!(
                "{} · {}",
                language.text(Key::Issues),
                report.issues.len()
            ))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("app_issues")
                    .max_height(100.0)
                    .show_rows(ui, 22.0, report.issues.len(), |ui, range| {
                        for index in range {
                            let issue = &report.issues[index];
                            ui.add(
                                egui::Label::new(format!(
                                    "{}: {}",
                                    language.text(Key::InventoryIssue),
                                    issue.path.to_string_lossy()
                                ))
                                .truncate(),
                            )
                            .on_hover_text(&issue.message);
                        }
                    });
            });
        }
        if report.applications.is_empty() {
            components::empty_hint(ui, palette, language.text(Key::NoApplications));
        } else if self.applications.groups.iter().all(Vec::is_empty) {
            components::empty_hint(ui, palette, language.text(Key::ApplicationsNoMatches));
        }
        let groups = &self.applications.groups;
        let searching = !self.applications.search.trim().is_empty();
        egui::ScrollArea::vertical()
            .id_salt("applications_grid")
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let origin = ui.cursor().min;
                let width = ui.available_width();
                let columns = ((width + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP))
                    .floor()
                    .max(1.0) as usize;
                let card_width = (width - CARD_GAP * (columns - 1) as f32) / columns as f32;
                let mut offset = 0.0;
                for (group, indices) in groups.iter().enumerate() {
                    if indices.is_empty() {
                        continue;
                    }
                    let heading_rect = egui::Rect::from_min_size(
                        origin + vec2(0.0, offset),
                        vec2(width, GROUP_HEIGHT),
                    );
                    let expanded = group == 0 || self.applications.show_components || searching;
                    let key = if group == 0 {
                        Key::ApplicationGames
                    } else {
                        Key::ApplicationComponents
                    };
                    if offset + GROUP_HEIGHT >= viewport.top() && offset <= viewport.bottom() {
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .id_salt(("app_group", group))
                                .max_rect(heading_rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            |ui| {
                                let title = format!(
                                    "{}  {} · {}",
                                    if expanded {
                                        icons::CARET_DOWN
                                    } else {
                                        icons::CARET_RIGHT
                                    },
                                    language.text(key),
                                    indices.len()
                                );
                                if group == 0 {
                                    ui.label(RichText::new(title).strong().color(palette.text));
                                } else if components::secondary_button(ui, palette, &title)
                                    .on_hover_text(language.text(Key::ApplicationComponentsHint))
                                    .clicked()
                                {
                                    self.applications.show_components =
                                        !self.applications.show_components;
                                }
                            },
                        );
                    }
                    offset += GROUP_HEIGHT;
                    if !expanded {
                        offset += CARD_GAP;
                        continue;
                    }
                    let rows = indices.len().div_ceil(columns);
                    let first = (((viewport.top() - offset) / CARD_STRIDE).floor().max(0.0)
                        as usize)
                        .min(rows);
                    let last = (((viewport.bottom() - offset) / CARD_STRIDE).ceil().max(0.0)
                        as usize)
                        .min(rows);
                    for row in first..last {
                        for column in 0..columns {
                            let Some(&index) = indices.get(row * columns + column) else {
                                break;
                            };
                            let rect = egui::Rect::from_min_size(
                                origin
                                    + vec2(
                                        column as f32 * (card_width + CARD_GAP),
                                        offset + row as f32 * CARD_STRIDE,
                                    ),
                                vec2(card_width, CARD_HEIGHT),
                            );
                            if application_card(
                                ui,
                                rect,
                                index,
                                &report.applications[index],
                                self.applications.selected == Some(index),
                                palette,
                                language,
                            )
                            .clicked()
                            {
                                self.applications.selected = Some(index);
                            }
                        }
                    }
                    offset += rows as f32 * CARD_STRIDE + CARD_GAP;
                }
                ui.advance_cursor_after_rect(egui::Rect::from_min_size(
                    origin,
                    vec2(width, offset),
                ));
            });
        self.application_details(ui.ctx());
    }
}

const CARD_MIN_WIDTH: f32 = 240.0;
const CARD_HEIGHT: f32 = 142.0;
const CARD_GAP: f32 = 12.0;
const CARD_STRIDE: f32 = CARD_HEIGHT + CARD_GAP;
const GROUP_HEIGHT: f32 = 44.0;

// Presentation hints only; never used for ownership evidence or cleanup policy.
fn steam_component(application: &Application) -> bool {
    let name = application.name.to_ascii_lowercase();
    application.launcher == "Steam"
        && (name == "proton"
            || name.starts_with("proton ")
            || name == "proton experimental"
            || name.starts_with("steam linux runtime")
            || name == "steamworks common redistributables")
}

impl ApplicationsWorkspace {
    fn update_groups(&mut self) {
        let Some(report) = &self.report else {
            return;
        };
        let key = (
            report.applications.as_ptr() as usize,
            report.applications.len(),
            self.search.clone(),
        );
        if self.view_key.as_ref() == Some(&key) {
            return;
        }
        self.groups = Default::default();
        let search = self.search.trim().to_lowercase();
        for (index, app) in report.applications.iter().enumerate() {
            if !search.is_empty()
                && !app.name.to_lowercase().contains(&search)
                && !app
                    .install_path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&search)
            {
                continue;
            }
            self.groups[usize::from(steam_component(app))].push(index);
        }
        for group in &mut self.groups {
            group.sort_by(|a, b| {
                let a = &report.applications[*a];
                let b = &report.applications[*b];
                b.allocated_bytes
                    .cmp(&a.allocated_bytes)
                    .then(a.name.cmp(&b.name))
                    .then(a.id.cmp(&b.id))
            });
        }
        self.view_key = Some(key);
    }
}

fn status_key(status: InstallationStatus) -> Key {
    match status {
        InstallationStatus::Installed => Key::ApplicationInstalled,
        InstallationStatus::Missing => Key::ApplicationMissing,
        InstallationStatus::Unknown => Key::ApplicationUnknown,
    }
}

fn application_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    index: usize,
    app: &Application,
    selected: bool,
    palette: theme::Palette,
    language: crate::i18n::Language,
) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.scope_id().with(("application_card", index)),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &app.name));
    let active = selected || response.has_focus();
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(
        rect,
        tokens::RADIUS_CARD,
        if response.hovered() {
            palette.surface_hover
        } else {
            palette.surface
        },
    );
    painter.rect_stroke(
        rect,
        tokens::RADIUS_CARD,
        Stroke::new(
            1.0,
            if active {
                palette.accent
            } else {
                palette.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    let left = rect.left() + 14.0;
    let mut title = egui::text::LayoutJob::simple(
        app.name.clone(),
        theme::semibold(14.0),
        palette.text,
        rect.width() - 28.0,
    );
    title.wrap.max_rows = 2;
    title.wrap.break_anywhere = true;
    let title = ui.fonts_mut(|fonts| fonts.layout_job(title));
    painter.galley(egui::pos2(left, rect.top() + 14.0), title, palette.text);
    let size = app
        .allocated_bytes
        .map(|bytes| format::size(language, bytes))
        .unwrap_or_else(|| language.text(Key::Unknown).to_owned());
    painter.text(
        egui::pos2(left, rect.top() + 63.0),
        Align2::LEFT_TOP,
        size,
        theme::semibold(20.0),
        palette.accent,
    );
    let color = if app.status == InstallationStatus::Installed {
        palette.success
    } else {
        palette.warning
    };
    let status = language.text(status_key(app.status));
    painter.circle_filled(egui::pos2(left + 3.0, rect.top() + 103.0), 3.0, color);
    let mut status_job = egui::text::LayoutJob::simple_singleline(
        format!("{} · {status}", app.launcher),
        FontId::proportional(11.5),
        palette.muted,
    );
    status_job.wrap.max_width = rect.width() - 42.0;
    status_job.wrap.max_rows = 1;
    let status_galley = ui.fonts_mut(|fonts| fonts.layout_job(status_job));
    painter.galley(
        egui::pos2(left + 13.0, rect.top() + 96.0),
        status_galley,
        palette.muted,
    );
    let footer = if app.allocated_bytes.is_some() && !app.size_complete {
        Key::ApplicationPartialSize
    } else {
        Key::ApplicationOpenDetails
    };
    painter.text(
        egui::pos2(left, rect.top() + 120.0),
        Align2::LEFT_TOP,
        language.text(footer),
        FontId::proportional(10.5),
        palette.muted,
    );
    response.on_hover_text(format!(
        "{}\n{}\n{}\n{}",
        app.name,
        status,
        app.install_path.to_string_lossy(),
        if app.allocated_bytes.is_some() && !app.size_complete {
            language.text(Key::ApplicationPartialSizeHelp)
        } else {
            ""
        }
    ))
}

impl DiskCleanupApp {
    fn application_details(&mut self, context: &egui::Context) {
        let Some(app) = self
            .applications
            .selected
            .and_then(|index| self.applications.report.as_ref()?.applications.get(index))
        else {
            return;
        };
        let language = self.language();
        let palette = self.palette;
        let mut close = false;
        let mut analyze = None;
        let modal = egui::Modal::new(egui::Id::unique("application_details"))
            .frame(
                egui::Frame::new()
                    .fill(palette.panel)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(tokens::RADIUS_CARD)
                    .inner_margin(18),
            )
            .show(context, |ui| {
                let size = components::modal_size(context, 560.0);
                ui.set_width(size.x);
                egui::ScrollArea::vertical()
                    .id_salt("application_detail_scroll")
                    .max_height((size.y - 44.0).max(80.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&app.name).font(theme::semibold(20.0)))
                                .wrap(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                app.launcher,
                                language.text(status_key(app.status))
                            ))
                            .color(palette.muted),
                        );
                        let bytes = app
                            .allocated_bytes
                            .map(|bytes| format::size(language, bytes))
                            .unwrap_or_else(|| language.text(Key::Unknown).into());
                        components::stat_row(ui, palette, language.text(Key::Size), &bytes);
                        if !app.size_complete && app.allocated_bytes.is_some() {
                            ui.label(language.text(Key::ApplicationPartialSizeHelp));
                        }
                        ui.add_space(8.0);
                        let path = app.install_path.to_string_lossy();
                        ui.add(egui::Label::new(path.as_ref()).truncate())
                            .on_hover_text(path.as_ref());
                        ui.label(language.text(match app.launcher.as_str() {
                            "Lutris" => Key::LutrisOwnership,
                            "GOG" => Key::GogOwnership,
                            "Heroic (GOG)" => Key::HeroicGogOwnership,
                            "Steam" => Key::SteamOwnership,
                            "Legendary" | "Heroic (Epic)" => Key::LegendaryOwnership,
                            _ => Key::EpicOwnership,
                        }));
                        ui.label(language.text(Key::ApplicationDataScope));
                        if let Some(build) = &app.build_id {
                            ui.add(
                                egui::Label::new(format!(
                                    "{}: {build}",
                                    language.text(Key::ApplicationBuild)
                                ))
                                .truncate(),
                            )
                            .on_hover_text(build);
                        }
                        if let Some(played) = app.last_played_unix {
                            ui.label(format!(
                                "{}: {}",
                                language.text(Key::ApplicationLastPlayed),
                                format::age(language, played.min(i64::MAX as u64) as i64)
                            ));
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if app.allocated_bytes.is_some()
                        && components::primary_button(ui, palette, language.text(Key::Analyze))
                            .clicked()
                    {
                        analyze = Some(app.install_path.clone());
                    }
                    if components::secondary_button(ui, palette, language.text(Key::CopyPath))
                        .clicked()
                    {
                        ui.ctx()
                            .copy_text(app.install_path.to_string_lossy().into_owned());
                    }
                    if components::secondary_button(ui, palette, language.text(Key::Close))
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
        if close || modal.should_close() {
            self.applications.selected = None;
        }
        if let Some(path) = analyze {
            self.applications.selected = None;
            self.path_input = path.to_string_lossy().into_owned();
            self.start_scan();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn cancelled_and_disconnected_jobs_preserve_previous_inventory() {
        for disconnected in [false, true] {
            let mut app = DiskCleanupApp::new_for_test(Settings::default(), None);
            app.applications.report = Some(ApplicationsReport::default());
            app.applications.selected = Some(5);
            let (sender, receiver) = mpsc::channel();
            let control = ScanControl::new();
            app.applications.job = Some(InventoryJob {
                control: control.clone(),
                receiver,
            });
            app.cancel_applications();
            assert!(control.is_cancelled());
            if !disconnected {
                sender.send(Err(ScanError::Cancelled)).expect("result");
            }
            drop(sender);
            app.poll_applications(&egui::Context::default());
            assert!(app.applications.report.is_some());
            assert_eq!(app.applications.selected, Some(5));
            assert!(app.applications.job.is_none());
            assert_eq!(
                app.applications.message,
                Some(if disconnected {
                    Key::InventoryFailed
                } else {
                    Key::InventoryCancelled
                })
            );
        }
    }
}
