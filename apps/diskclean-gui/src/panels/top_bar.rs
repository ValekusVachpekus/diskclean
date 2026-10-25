//! Global command bar and scan feedback.

use eframe::egui::{
    self, Align, Button, Color32, FontId, Frame, Layout, Margin, RichText, Stroke, TextEdit, vec2,
};

use crate::app::{DiskCleanupApp, ScanState, path_field_id};
use crate::i18n::Key;
use crate::ui::{components, format, icons, theme, tokens};

impl DiskCleanupApp {
    pub(crate) fn top_bar(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let mut browse = false;
        let mut scan = false;
        let mut cancel = false;
        let mut settings = false;
        let mut help = false;
        let mut inspector = false;
        let mut theme_toggle = false;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icons::APP)
                    .font(FontId::proportional(20.0))
                    .color(palette.accent),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(self.text(Key::AppName))
                    .font(theme::semibold(16.0))
                    .color(palette.text),
            );
            ui.add_space(10.0);

            let path_width = (ui.available_width() - 330.0).clamp(160.0, 620.0);
            let path_hint = self.text(Key::PathHint);
            let path_response = ui
                .add(
                    TextEdit::singleline(&mut self.path_input)
                        .id(path_field_id())
                        .desired_width(path_width)
                        .hint_text(path_hint)
                        .margin(Margin::symmetric(9, 6)),
                )
                .on_hover_text(self.text(Key::Path));
            if path_response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                scan = true;
            }

            if components::icon_button(ui, palette, icons::FOLDER_OPEN, self.text(Key::Browse))
                .clicked()
            {
                browse = true;
            }

            let running = self.is_running();
            if running {
                if ui
                    .add(
                        Button::new(RichText::new(self.text(Key::Cancel)).color(palette.danger))
                            .fill(palette.surface)
                            .stroke(Stroke::new(1.0, palette.danger))
                            .corner_radius(tokens::RADIUS_CONTROL)
                            .min_size(vec2(72.0, 30.0)),
                    )
                    .clicked()
                {
                    cancel = true;
                }
            } else if components::primary_button(ui, palette, self.text(Key::Scan)).clicked() {
                scan = true;
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if components::icon_button(ui, palette, icons::HELP, self.text(Key::Help)).clicked()
                {
                    help = true;
                }
                if components::icon_button(ui, palette, icons::SETTINGS, self.text(Key::Settings))
                    .clicked()
                {
                    settings = true;
                }
                let theme_icon = if palette.is_dark() {
                    icons::SUN
                } else {
                    icons::MOON
                };
                if components::icon_button(ui, palette, theme_icon, self.text(Key::Theme)).clicked()
                {
                    theme_toggle = true;
                }
                if components::icon_button(ui, palette, icons::INFO, self.text(Key::Selection))
                    .clicked()
                {
                    inspector = true;
                }
            });
        });

        if let ScanState::Running {
            control,
            started,
            path,
            ..
        } = &self.state
        {
            let snapshot = control.snapshot();
            ui.add_space(5.0);
            ui.add(
                egui::ProgressBar::new(0.0)
                    .animate(true)
                    .desired_width(ui.available_width())
                    .desired_height(4.0)
                    .fill(palette.accent),
            );
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                let state_text = if snapshot.cancelled {
                    self.text(Key::Cancelling)
                } else {
                    self.text(Key::Scanning)
                };
                ui.label(
                    RichText::new(state_text)
                        .font(theme::medium(11.5))
                        .color(palette.accent),
                );
                ui.label(
                    RichText::new(format::duration(
                        self.language(),
                        started.elapsed().as_millis() as u64,
                    ))
                    .color(palette.muted)
                    .size(11.0),
                );
                ui.label(
                    RichText::new(format!(
                        "{} · {} · {}",
                        format::size(self.language(), snapshot.allocated_bytes),
                        crate::app::files_label(self.language(), snapshot.files),
                        crate::app::folders_label(self.language(), snapshot.directories),
                    ))
                    .color(palette.muted)
                    .size(11.0),
                );
                ui.label(
                    RichText::new(format::shorten_middle(&path.display().to_string(), 64))
                        .color(palette.muted)
                        .size(11.0),
                );
                if let Some(current) = &snapshot.current_path {
                    ui.label(
                        RichText::new(format::shorten_middle(&current.display().to_string(), 72))
                            .color(palette.muted)
                            .size(11.0),
                    );
                }
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        if let ScanState::Failed(error) = &self.state {
            let error = error.clone();
            ui.add_space(5.0);
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
                                .font(FontId::proportional(15.0))
                                .color(palette.danger),
                        );
                        ui.label(
                            RichText::new(self.text(Key::ScanFailed))
                                .font(theme::medium(12.0))
                                .color(palette.danger),
                        );
                        ui.label(RichText::new(error).color(palette.text).size(12.0));
                        if ui
                            .add(
                                Button::new(RichText::new(self.text(Key::Retry)).size(11.5))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::new(1.0, palette.danger))
                                    .corner_radius(tokens::RADIUS_CONTROL),
                            )
                            .clicked()
                        {
                            scan = true;
                        }
                        if ui
                            .add(
                                Button::new(
                                    RichText::new(self.text(Key::ChooseAnother)).size(11.5),
                                )
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::new(1.0, palette.border))
                                .corner_radius(tokens::RADIUS_CONTROL),
                            )
                            .clicked()
                        {
                            browse = true;
                        }
                    });
                });
        }

        if browse
            && !self.is_running()
            && let Some(folder) = rfd::FileDialog::new().pick_folder()
        {
            self.path_input = folder.display().to_string();
            scan = true;
        }
        if scan {
            self.start_scan();
        }
        if cancel {
            self.cancel_scan();
        }
        if settings {
            self.settings_open = true;
        }
        if help {
            self.help_open = true;
        }
        if inspector {
            if self.wide {
                self.settings.inspector_open = !self.settings.inspector_open;
            } else {
                self.narrow_inspector = !self.narrow_inspector;
            }
        }
        if theme_toggle {
            self.settings.theme = if self.palette.is_dark() {
                crate::ui::theme::ThemeMode::Light
            } else {
                crate::ui::theme::ThemeMode::Dark
            };
        }
    }
}
