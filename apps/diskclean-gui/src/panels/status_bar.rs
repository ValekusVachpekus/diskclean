//! Global status bar.

use eframe::egui::{self, FontId, RichText};

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
                let mut details = format!(
                    "{} {} · {}",
                    format::size(self.language(), tree.stats.allocated_bytes),
                    self.text(Key::OnDisk),
                    files_label(self.language(), tree.stats.files),
                );
                if !compact {
                    details.push_str(&format!(
                        " · {} · {} · {} {}",
                        folders_label(self.language(), tree.stats.directories),
                        issues_label(self.language(), tree.issues.len() as u64),
                        self.text(Key::ScannedIn),
                        format::duration(self.language(), tree.duration_ms),
                    ));
                }
                ui.add(
                    egui::Label::new(RichText::new(&details).color(palette.muted).size(10.5))
                        .truncate(),
                )
                .on_hover_text(format!("{details}\n{}", tree.root_path.display()));
            } else {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .color(palette.muted)
                            .size(10.0),
                    )
                    .truncate(),
                );
            }
        });
    }
}
