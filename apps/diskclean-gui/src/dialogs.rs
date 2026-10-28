//! Settings, help, and transient notifications.

use std::time::Instant;

use eframe::egui::{
    self, Align, Align2, Button, Color32, Context, FontId, Frame, Id, Layout, Order, RichText,
    Stroke, vec2,
};

use crate::app::{DiskCleanupApp, ToastKind};
use crate::i18n::{Key, Language};
use crate::ui::theme::ThemeMode;
use crate::ui::{components, icons, theme, tokens};

impl DiskCleanupApp {
    pub(crate) fn settings_dialog(&mut self, context: &Context) {
        if !self.settings_open {
            return;
        }

        let palette = self.palette;
        let mut close = false;
        let mut reset = false;
        let language = self.language();

        let modal = egui::Modal::new(Id::unique("settings_modal"))
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(tokens::RADIUS_CARD)
                    .inner_margin(20),
            )
            .show(context, |ui| {
                let size = components::modal_size(context, 520.0);
                ui.set_width(size.x);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(self.text(Key::Settings)).font(theme::semibold(20.0)),
                        );
                        ui.label(
                            RichText::new(self.text(Key::ApplicationSubtitle))
                                .color(palette.muted)
                                .size(11.0),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if components::icon_button(ui, palette, icons::CLOSE, self.text(Key::Close))
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(12.0);

                egui::ScrollArea::vertical()
                    .id_salt("settings_scroll")
                    .max_height((size.y - 130.0).max(80.0))
                    .show(ui, |ui| {
                        components::section_label(ui, palette, self.text(Key::General));
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(self.text(Key::Theme))
                                    .color(palette.muted)
                                    .size(11.5),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                egui::ComboBox::from_id_salt("theme_combo")
                                    .selected_text(self.settings.theme.label(language))
                                    .width(170.0)
                                    .show_ui(ui, |ui| {
                                        for theme_mode in
                                            [ThemeMode::System, ThemeMode::Dark, ThemeMode::Light]
                                        {
                                            ui.selectable_value(
                                                &mut self.settings.theme,
                                                theme_mode,
                                                theme_mode.label(language),
                                            );
                                        }
                                    });
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(self.text(Key::Language))
                                    .color(palette.muted)
                                    .size(11.5),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                egui::ComboBox::from_id_salt("language_combo")
                                    .selected_text(self.settings.language.label(language))
                                    .width(170.0)
                                    .show_ui(ui, |ui| {
                                        for language_option in
                                            [Language::System, Language::English, Language::Russian]
                                        {
                                            ui.selectable_value(
                                                &mut self.settings.language,
                                                language_option,
                                                language_option.label(language),
                                            );
                                        }
                                    });
                            });
                        });
                        let show_inspector = self.text(Key::ShowInspector);
                        ui.checkbox(&mut self.settings.inspector_open, show_inspector);

                        components::section_label(ui, palette, self.text(Key::ScanningOptions));
                        ui.horizontal_wrapped(|ui| {
                            let threads_label = ui.label(self.text(Key::WorkerThreads));
                            let requested = self.settings.worker_threads;
                            let selected = if requested == 0 {
                                format!(
                                    "{} ({})",
                                    self.text(Key::AutomaticThreads),
                                    diskclean_core::parallel::worker_threads(0)
                                )
                            } else {
                                diskclean_core::parallel::worker_threads(requested).to_string()
                            };
                            egui::ComboBox::from_id_salt("worker_threads")
                                .selected_text(selected)
                                .width(150.0)
                                .show_ui(ui, |ui| {
                                    let automatic = self.text(Key::AutomaticThreads);
                                    ui.selectable_value(
                                        &mut self.settings.worker_threads,
                                        0,
                                        automatic,
                                    );
                                    for count in 1..=diskclean_core::parallel::MAX_WORKER_THREADS {
                                        ui.selectable_value(
                                            &mut self.settings.worker_threads,
                                            count,
                                            count.to_string(),
                                        );
                                    }
                                })
                                .response
                                .labelled_by(threads_label.id);
                        });
                        ui.label(
                            RichText::new(self.text(Key::WorkerThreadsNote))
                                .color(palette.muted)
                                .size(10.5),
                        );
                        let skip_hidden = self.text(Key::SkipHidden);
                        ui.checkbox(&mut self.settings.skip_hidden, skip_hidden);
                        let cross_filesystems = self.text(Key::CrossFilesystems);
                        ui.checkbox(&mut self.settings.cross_filesystems, cross_filesystems);
                        ui.label(
                            RichText::new(self.text(Key::ScanOptionsNote))
                                .color(palette.muted)
                                .size(10.5),
                        );

                        components::section_label(ui, palette, self.text(Key::Version));
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Disk Cleanup v{}",
                                    env!("CARGO_PKG_VERSION")
                                ))
                                .font(theme::medium(12.0)),
                            );
                            ui.label(
                                RichText::new("GPL-3.0-only")
                                    .color(palette.muted)
                                    .size(10.5),
                            );
                        });
                        ui.label(
                            RichText::new(self.text(Key::SafetyNote))
                                .color(palette.muted)
                                .size(10.5),
                        );
                    });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            Button::new(RichText::new(self.text(Key::ResetSettings)).size(11.5))
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::new(1.0, palette.border))
                                .corner_radius(tokens::RADIUS_CONTROL),
                        )
                        .clicked()
                    {
                        reset = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if components::primary_button(ui, palette, self.text(Key::Close)).clicked()
                        {
                            close = true;
                        }
                    });
                });
            });

        if modal.should_close() {
            close = true;
        }
        if reset {
            self.settings.reset_preferences();
            self.notify(self.text(Key::SettingsSaved).to_owned(), ToastKind::Info);
        }
        if close {
            self.settings_open = false;
        }
    }

    pub(crate) fn help_dialog(&mut self, context: &Context) {
        if !self.help_open {
            return;
        }

        let palette = self.palette;
        let mut close = false;
        let modal = egui::Modal::new(Id::unique("help_modal"))
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(tokens::RADIUS_CARD)
                    .inner_margin(20),
            )
            .show(context, |ui| {
                let size = components::modal_size(context, 460.0);
                ui.set_width(size.x);
                egui::ScrollArea::vertical()
                    .id_salt("help_content")
                    .max_height(size.y)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(self.text(Key::Shortcuts))
                                    .font(theme::semibold(20.0)),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if components::icon_button(
                                    ui,
                                    palette,
                                    icons::CLOSE,
                                    self.text(Key::Close),
                                )
                                .clicked()
                                {
                                    close = true;
                                }
                            });
                        });
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(self.text(Key::SafetyNote))
                                .color(palette.muted)
                                .size(11.0),
                        );
                        ui.add_space(10.0);
                        components::shortcut_row(
                            ui,
                            palette,
                            "Ctrl+O",
                            self.text(Key::ShortcutOpen),
                        );
                        components::shortcut_row(
                            ui,
                            palette,
                            "Ctrl+R / F5",
                            self.text(Key::ShortcutRescan),
                        );
                        components::shortcut_row(
                            ui,
                            palette,
                            "Ctrl+F",
                            self.text(Key::ShortcutSearch),
                        );
                        components::shortcut_row(
                            ui,
                            palette,
                            "Enter",
                            self.text(Key::ShortcutOpenItem),
                        );
                        components::shortcut_row(
                            ui,
                            palette,
                            "Backspace",
                            self.text(Key::ShortcutBack),
                        );
                        components::shortcut_row(
                            ui,
                            palette,
                            "Ctrl+1 / 2 / 3",
                            self.text(Key::ShortcutWorkspaces),
                        );
                        components::shortcut_row(
                            ui,
                            palette,
                            "Ctrl+,",
                            self.text(Key::ShortcutSettings),
                        );
                        components::shortcut_row(ui, palette, "Esc", self.text(Key::ShortcutClose));
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(self.text(Key::Tagline))
                                .color(palette.muted)
                                .size(10.5),
                        );
                        ui.add_space(10.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if components::primary_button(ui, palette, self.text(Key::Close))
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    });
            });

        if modal.should_close() {
            close = true;
        }
        if close {
            self.help_open = false;
        }
    }

    pub(crate) fn show_toast(&mut self, context: &Context) {
        let Some(toast) = self.toast.clone() else {
            return;
        };
        let now = Instant::now();
        if now >= toast.until {
            self.toast = None;
            return;
        }
        context.request_repaint_after(toast.until.saturating_duration_since(now));

        let palette = self.palette;
        let (icon, color) = match toast.kind {
            ToastKind::Info => (icons::CHECK, palette.success),
            ToastKind::Error => (icons::WARNING, palette.danger),
        };
        egui::Area::new(Id::unique("toast_area"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-18.0, -42.0))
            .order(Order::Foreground)
            .show(context, |ui| {
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, color))
                    .corner_radius(tokens::RADIUS_CARD)
                    .inner_margin(10)
                    .show(ui, |ui| {
                        ui.set_max_width((context.content_rect().width() - 80.0).min(440.0));
                        ui.horizontal_top(|ui| {
                            ui.label(
                                RichText::new(icon)
                                    .font(FontId::proportional(14.0))
                                    .color(color),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&toast.text).color(palette.text).size(11.5),
                                )
                                .wrap(),
                            );
                        });
                    });
            });
    }
}
