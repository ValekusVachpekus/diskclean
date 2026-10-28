//! Left navigation: volumes, unified recents, tools, and settings.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align, Color32, FontId, Frame, Layout, RichText, Sense, Stroke, vec2};

use crate::app::{DiskCleanupApp, Workspace};
use crate::i18n::Key;
use crate::ui::{components, format, icons, theme, tokens};

impl DiskCleanupApp {
    pub(crate) fn navigation(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let width = ui.available_width();
        if width < 100.0 {
            self.navigation_rail(ui);
            return;
        }
        let content_height = (ui.available_height() - 82.0).max(40.0);

        ui.allocate_ui_with_layout(
            vec2(width, content_height),
            Layout::top_down(Align::LEFT),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("navigation_scroll")
                    .max_height(content_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(width);
                        self.navigation_content(ui);
                    });
            },
        );

        ui.separator();
        let settings_selected = false;
        let response = components::nav_item(
            ui,
            palette,
            width,
            icons::SETTINGS,
            self.text(Key::Settings),
            None,
            settings_selected,
            true,
        );
        if response.clicked() {
            self.settings_open = true;
        }

        ui.add_space(3.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                    .color(palette.muted)
                    .size(10.0),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new("GPL-3.0").color(palette.muted).size(10.0));
            });
        });
    }

    fn navigation_content(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let width = ui.available_width();

        self.navigation_tools(ui);
        ui.add_space(8.0);
        components::section_label(ui, palette, self.text(Key::Volumes));
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{}  {}", icons::VOLUME, self.volumes.len()))
                    .color(palette.muted)
                    .size(10.5),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if components::icon_button(ui, palette, icons::RETRY, self.text(Key::Refresh))
                    .clicked()
                {
                    self.volumes = diskclean_platform::list_volumes();
                }
            });
        });

        if self.volumes.is_empty() {
            ui.label(
                RichText::new(self.text(Key::NoVolumes))
                    .color(palette.muted)
                    .size(11.0),
            );
        }

        let mut chosen_volume = None;
        let mut start_after_choice = false;
        for volume in self.volumes.clone() {
            let selected = self.path_input.trim_end_matches(['/', '\\'])
                == volume.mount_point.trim_end_matches(['/', '\\']);
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
            let used = volume.total_space.saturating_sub(volume.available_space);
            let fraction = format::percent(used, volume.total_space) / 100.0;

            let response = Frame::new()
                .fill(fill)
                .stroke(stroke)
                .corner_radius(tokens::RADIUS_CARD)
                .inner_margin(9)
                .show(ui, |ui| {
                    ui.set_width((width - 20.0).max(80.0));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(icons::VOLUME)
                                .font(FontId::proportional(15.0))
                                .color(palette.accent),
                        );
                        ui.vertical(|ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            ui.label(
                                RichText::new(&volume.mount_point)
                                    .font(theme::medium(12.5))
                                    .color(palette.text),
                            );
                            let subtitle = if volume.name.trim().is_empty() {
                                volume.file_system.clone()
                            } else {
                                format!("{} · {}", volume.name, volume.file_system)
                            };
                            ui.label(
                                RichText::new(format::shorten_middle(&subtitle, 30))
                                    .color(palette.muted)
                                    .size(10.0),
                            );
                        });
                    });
                    ui.add_space(3.0);
                    components::usage_bar(
                        ui,
                        palette,
                        fraction,
                        ui.available_width(),
                        4.0,
                        palette.accent,
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!(
                            "{} {} · {}",
                            format::size(self.language(), volume.available_space),
                            self.text(Key::Free),
                            format::size(self.language(), volume.total_space),
                        ))
                        .color(palette.muted)
                        .size(10.0),
                    );
                })
                .response
                .interact(Sense::click())
                .on_hover_text(format!(
                    "{}\n{}",
                    volume.mount_point,
                    self.text(Key::DoubleClickScan)
                ));
            if response.clicked() {
                chosen_volume = Some(volume.mount_point.clone());
            }
            if response.double_clicked() {
                chosen_volume = Some(volume.mount_point.clone());
                start_after_choice = true;
            }
        }

        if let Some(path) = chosen_volume {
            self.path_input = path;
            if start_after_choice {
                self.start_scan();
            }
        }

        components::section_label(ui, palette, self.text(Key::Recent));
        let mut chosen_recent: Option<(PathBuf, Option<std::sync::Arc<diskclean_core::Tree>>)> =
            None;
        let recent = self.settings.recent_roots.clone();
        if recent.is_empty() {
            ui.label(
                RichText::new(self.text(Key::NoRecent))
                    .color(palette.muted)
                    .size(11.0),
            );
        }
        for recent_text in recent {
            let path = PathBuf::from(&recent_text);
            let cached = self.cache.iter().find(|entry| entry.path == path);
            let selected = self
                .tree
                .as_ref()
                .is_some_and(|tree| tree.root_path == path);
            let fill = if selected {
                palette.accent_soft()
            } else {
                Color32::TRANSPARENT
            };
            let response = Frame::new()
                .fill(fill)
                .corner_radius(tokens::RADIUS_CONTROL)
                .inner_margin(7)
                .show(ui, |ui| {
                    ui.set_width((width - 14.0).max(80.0));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if selected {
                                icons::FOLDER_OPEN
                            } else {
                                icons::FOLDER
                            })
                            .font(FontId::proportional(14.0))
                            .color(if selected {
                                palette.accent
                            } else {
                                palette.muted
                            }),
                        );
                        ui.vertical(|ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            ui.label(
                                RichText::new(format::shorten_middle(
                                    &format::file_name(&path),
                                    26,
                                ))
                                .font(theme::medium(12.0))
                                .color(palette.text),
                            );
                            let age = cached.map(|entry| {
                                let now = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .map_or(0, |duration| duration.as_secs())
                                    .saturating_sub(entry.scanned_at.elapsed().as_secs());
                                format::age(self.language(), now as i64)
                            });
                            ui.label(
                                RichText::new(match age {
                                    Some(age) => format!("{age} · {}", self.text(Key::Cached)),
                                    None => format::shorten_middle(&recent_text, 34),
                                })
                                .color(palette.muted)
                                .size(10.0),
                            );
                        });
                    });
                })
                .response
                .interact(Sense::click())
                .on_hover_text(recent_text.clone());
            if response.clicked() && !self.is_running() {
                chosen_recent =
                    Some((path, cached.map(|entry| std::sync::Arc::clone(&entry.tree))));
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

        ui.add_space(10.0);
    }
    fn navigation_tools(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let width = ui.available_width();
        components::section_label(ui, palette, self.text(Key::Tools));
        if components::nav_item(
            ui,
            palette,
            width,
            icons::CHART,
            self.text(Key::Analyze),
            None,
            self.workspace == Workspace::Analyze,
            true,
        )
        .clicked()
        {
            self.workspace = Workspace::Analyze;
            self.tab = crate::app::Tab::Map;
        }

        for (workspace, icon, label) in [
            (Workspace::Cleanup, icons::BROOM, self.text(Key::Cleanup)),
            (
                Workspace::Duplicates,
                icons::DUPLICATES,
                self.text(Key::Duplicates),
            ),
        ] {
            if components::nav_item(
                ui,
                palette,
                width,
                icon,
                label,
                None,
                self.workspace == workspace,
                true,
            )
            .clicked()
            {
                self.workspace = workspace;
            }
        }

        let applications = components::nav_item(
            ui,
            palette,
            width,
            icons::APPS,
            self.text(Key::Applications),
            Some(self.text(Key::Soon)),
            false,
            false,
        );
        applications.on_hover_text(self.text(Key::Soon));
    }

    fn navigation_rail(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        for (workspace, icon, key) in [
            (Workspace::Analyze, icons::CHART, Key::Analyze),
            (Workspace::Cleanup, icons::BROOM, Key::Cleanup),
            (Workspace::Duplicates, icons::DUPLICATES, Key::Duplicates),
        ] {
            let selected = self.workspace == workspace;
            let response = ui.add(
                egui::Button::new(RichText::new(icon).size(19.0).color(if selected {
                    palette.accent
                } else {
                    palette.muted
                }))
                .min_size(vec2(ui.available_width(), 38.0))
                .fill(if selected {
                    palette.accent_soft()
                } else {
                    Color32::TRANSPARENT
                })
                .stroke(Stroke::NONE)
                .corner_radius(tokens::RADIUS_CONTROL),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, self.text(key))
            });
            if response.on_hover_text(self.text(key)).clicked() {
                self.workspace = workspace;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
            if components::icon_button(ui, palette, icons::SETTINGS, self.text(Key::Settings))
                .clicked()
            {
                self.settings_open = true;
            }
        });
    }
}
