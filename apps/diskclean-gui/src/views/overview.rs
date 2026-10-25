//! Empty/first-launch overview.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use diskclean_core::Tree;
use eframe::egui::{self, Align, FontId, Frame, Layout, RichText, Sense, Stroke};

use crate::app::DiskCleanupApp;
use crate::i18n::Key;
use crate::ui::{components, format, icons, theme, tokens};

impl DiskCleanupApp {
    pub(crate) fn overview(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let mut scan_home = false;
        let mut chosen_volume = None;
        let mut scan_volume = false;
        let mut chosen_recent: Option<(PathBuf, Option<Arc<Tree>>)> = None;

        egui::ScrollArea::vertical()
            .id_salt("overview_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(icons::APP)
                            .font(FontId::proportional(34.0))
                            .color(palette.accent),
                    );
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(self.text(Key::EmptyTitle))
                                .font(theme::semibold(24.0))
                                .color(palette.text),
                        );
                        ui.label(
                            RichText::new(self.text(Key::Tagline))
                                .color(palette.muted)
                                .size(12.5),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if home_path().is_some()
                            && components::primary_button(ui, palette, self.text(Key::ScanHome))
                                .clicked()
                        {
                            scan_home = true;
                        }
                    });
                });

                ui.add_space(10.0);
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(tokens::RADIUS_CARD)
                    .inner_margin(10)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(icons::INFO)
                                    .font(FontId::proportional(13.0))
                                    .color(palette.info),
                            );
                            ui.label(
                                RichText::new(self.text(Key::SafetyNote))
                                    .color(palette.muted)
                                    .size(11.0),
                            );
                        });
                    });

                components::section_label(ui, palette, self.text(Key::Volumes));
                if self.volumes.is_empty() {
                    ui.label(
                        RichText::new(self.text(Key::NoVolumes))
                            .color(palette.muted)
                            .size(12.0),
                    );
                } else {
                    let gap = 12.0;
                    let card_width = ((ui.available_width() - gap) / 2.0).max(220.0);
                    egui::Grid::new("overview_volumes")
                        .num_columns(2)
                        .spacing([gap, gap])
                        .show(ui, |ui| {
                            for volume in self.volumes.clone() {
                                let selected = self.path_input.trim_end_matches(['/', '\\'])
                                    == volume.mount_point.trim_end_matches(['/', '\\']);
                                let used =
                                    volume.total_space.saturating_sub(volume.available_space);
                                let fraction = format::percent(used, volume.total_space) / 100.0;
                                let fill = if selected {
                                    palette.accent_soft()
                                } else {
                                    palette.surface
                                };
                                let stroke = if selected {
                                    Stroke::new(1.0, palette.accent)
                                } else {
                                    Stroke::new(1.0, palette.border)
                                };
                                let response = Frame::new()
                                    .fill(fill)
                                    .stroke(stroke)
                                    .corner_radius(tokens::RADIUS_CARD)
                                    .inner_margin(12)
                                    .show(ui, |ui| {
                                        ui.set_width(card_width - 24.0);
                                        ui.set_min_height(84.0);
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(icons::VOLUME)
                                                    .font(FontId::proportional(21.0))
                                                    .color(palette.accent),
                                            );
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(&volume.mount_point)
                                                        .font(theme::medium(13.5)),
                                                );
                                                let subtitle = if volume.name.trim().is_empty() {
                                                    volume.file_system.clone()
                                                } else {
                                                    format!(
                                                        "{} · {}",
                                                        volume.name, volume.file_system
                                                    )
                                                };
                                                ui.label(
                                                    RichText::new(subtitle)
                                                        .color(palette.muted)
                                                        .size(10.5),
                                                );
                                            });
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "{} {}",
                                                            format::size(
                                                                self.language(),
                                                                volume.available_space
                                                            ),
                                                            self.text(Key::Free)
                                                        ))
                                                        .color(palette.muted)
                                                        .size(10.5),
                                                    );
                                                },
                                            );
                                        });
                                        ui.add_space(5.0);
                                        components::usage_bar(
                                            ui,
                                            palette,
                                            fraction,
                                            ui.available_width(),
                                            5.0,
                                            palette.accent,
                                        );
                                        ui.add_space(3.0);
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} {}",
                                                    format::size(self.language(), used),
                                                    self.text(Key::Used)
                                                ))
                                                .color(palette.muted)
                                                .size(10.0),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(format::size(
                                                            self.language(),
                                                            volume.total_space,
                                                        ))
                                                        .color(palette.muted)
                                                        .font(FontId::monospace(10.0)),
                                                    );
                                                },
                                            );
                                        });
                                    })
                                    .response
                                    .interact(Sense::click())
                                    .on_hover_text(self.text(Key::DoubleClickScan));
                                if response.clicked() {
                                    chosen_volume = Some(volume.mount_point.clone());
                                }
                                if response.double_clicked() {
                                    chosen_volume = Some(volume.mount_point.clone());
                                    scan_volume = true;
                                }
                            }
                        });
                }

                components::section_label(ui, palette, self.text(Key::Recent));
                let recent = self.settings.recent_roots.clone();
                if recent.is_empty() {
                    ui.label(
                        RichText::new(self.text(Key::NoRecent))
                            .color(palette.muted)
                            .size(12.0),
                    );
                }
                for recent_text in recent {
                    let path = PathBuf::from(&recent_text);
                    let cached = self.cache.iter().find(|entry| entry.path == path);
                    let response = Frame::new()
                        .fill(palette.surface)
                        .stroke(Stroke::new(1.0, palette.border))
                        .corner_radius(tokens::RADIUS_CARD)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(icons::FOLDER)
                                        .font(FontId::proportional(16.0))
                                        .color(palette.accent),
                                );
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(format::file_name(&path))
                                            .font(theme::medium(12.5)),
                                    );
                                    ui.label(
                                        RichText::new(format::shorten_middle(&recent_text, 72))
                                            .color(palette.muted)
                                            .size(10.0),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if let Some(entry) = cached {
                                        let now = SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .map_or(0, |duration| duration.as_secs())
                                            .saturating_sub(entry.scanned_at.elapsed().as_secs());
                                        ui.label(
                                            RichText::new(format!(
                                                "{} · {}",
                                                format::age(self.language(), now as i64),
                                                self.text(Key::Cached)
                                            ))
                                            .color(palette.muted)
                                            .size(10.0),
                                        );
                                    }
                                });
                            });
                        })
                        .response
                        .interact(Sense::click())
                        .on_hover_text(recent_text.clone());
                    if response.clicked() && !self.is_running() {
                        chosen_recent = Some((path, cached.map(|entry| Arc::clone(&entry.tree))));
                    }
                }

                ui.add_space(16.0);
                ui.label(
                    RichText::new(format!(
                        "{}  {}",
                        self.text(Key::Path),
                        format::shorten_middle(&self.path_input, 96)
                    ))
                    .color(palette.muted)
                    .size(10.5),
                );
                ui.add_space(12.0);
            });

        if scan_home && let Some(home) = home_path() {
            self.path_input = home.display().to_string();
            self.start_scan();
        }
        if let Some(path) = chosen_volume {
            self.path_input = path;
            if scan_volume {
                self.start_scan();
            }
        }
        if let Some((path, tree)) = chosen_recent {
            if let Some(tree) = tree {
                self.load_cached(path, tree);
            } else {
                self.path_input = path.display().to_string();
                self.start_scan();
            }
        }
    }
}

fn home_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty() && path.exists())
}
