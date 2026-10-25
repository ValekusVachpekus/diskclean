//! Global status bar.

use eframe::egui::{self, Align, FontId, Layout, RichText};

use crate::app::{DiskCleanupApp, ScanState, files_label, folders_label, issues_label};
use crate::i18n::Key;
use crate::ui::{format, icons};

impl DiskCleanupApp {
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let compact = ui.available_width() < 900.0;
        ui.horizontal(|ui| {
            let (dot, label, color) = match &self.state {
                ScanState::Running { .. } => (
                    icons::CLOCK,
                    self.text(Key::Scanning).to_owned(),
                    palette.accent,
                ),
                ScanState::Failed(_) => (
                    icons::WARNING,
                    self.text(Key::ScanFailed).to_owned(),
                    palette.danger,
                ),
                ScanState::Idle => (
                    icons::CHECK,
                    self.text(Key::Ready).to_owned(),
                    palette.success,
                ),
            };
            ui.label(
                RichText::new(dot)
                    .font(FontId::proportional(12.0))
                    .color(color),
            );
            ui.label(RichText::new(label).color(color).size(10.5));

            if let Some(tree) = &self.tree {
                ui.label(RichText::new("·").color(palette.muted).size(10.0));
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        format::size(self.language(), tree.stats.allocated_bytes),
                        self.text(Key::OnDisk)
                    ))
                    .color(palette.muted)
                    .size(10.5),
                );
                ui.label(RichText::new("·").color(palette.muted).size(10.0));
                if compact {
                    ui.label(
                        RichText::new(files_label(self.language(), tree.stats.files))
                            .color(palette.muted)
                            .size(10.5),
                    );
                } else {
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            files_label(self.language(), tree.stats.files),
                            folders_label(self.language(), tree.stats.directories)
                        ))
                        .color(palette.muted)
                        .size(10.5),
                    );
                    ui.label(RichText::new("·").color(palette.muted).size(10.0));
                    let issue_color = if tree.issues.is_empty() {
                        palette.muted
                    } else {
                        palette.warning
                    };
                    ui.label(
                        RichText::new(issues_label(self.language(), tree.issues.len() as u64))
                            .color(issue_color)
                            .size(10.5),
                    );
                }
            }

            if !compact {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .color(palette.muted)
                            .size(10.0),
                    );
                    if let Some(tree) = &self.tree {
                        ui.label(RichText::new("·").color(palette.muted).size(10.0));
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                self.text(Key::ScannedIn),
                                format::duration(self.language(), tree.duration_ms)
                            ))
                            .color(palette.muted)
                            .size(10.0),
                        );
                    }
                });
            }
        });
    }
}
