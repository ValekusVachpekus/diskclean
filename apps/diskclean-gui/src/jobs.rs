//! Shared background apply job for cleanup and duplicate workspaces.
use crate::{
    i18n::{Key, Language},
    ui::{components, theme::Palette},
};
use diskclean_cleanup::{
    ApplyControl, ApplyReport, AuditLog, CleanupItem, ProtectedPaths, SystemTrash,
    apply_with_control,
};
use eframe::egui::{self, RichText};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
};

#[derive(Debug, Default)]
pub(crate) enum CleanupApplyJob {
    #[default]
    Idle,
    Running {
        receiver: Receiver<Result<ApplyReport, String>>,
        control: ApplyControl,
    },
}

impl CleanupApplyJob {
    pub(crate) fn start(
        items: Vec<CleanupItem>,
        protected: ProtectedPaths,
        audit_path: PathBuf,
    ) -> Self {
        let control = ApplyControl::default();
        let worker_control = control.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = (|| {
                let mut audit = AuditLog::open(audit_path).map_err(|error| error.to_string())?;
                Ok(apply_with_control(
                    &items,
                    &protected,
                    &mut SystemTrash,
                    &mut audit,
                    &worker_control,
                ))
            })();
            let _ = sender.send(result);
        });
        Self::Running { receiver, control }
    }

    pub(crate) fn cancel(&self) {
        if let Self::Running { control, .. } = self {
            control.cancel();
        }
    }

    pub(crate) fn show_progress(&self, ui: &mut egui::Ui, palette: Palette, language: Language) {
        if let Self::Running { control, .. } = self {
            let (completed, total) = control.progress();
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::Spinner::new().size(14.0).color(palette.warning));
                ui.label(
                    RichText::new(format!(
                        "{} · {completed}/{total}",
                        language.text(Key::Applying)
                    ))
                    .color(palette.warning),
                );
                ui.add_enabled_ui(!control.is_cancelled(), |ui| {
                    if components::secondary_button(ui, palette, language.text(Key::Cancel))
                        .clicked()
                    {
                        control.cancel();
                    }
                });
                ui.label(
                    RichText::new(language.text(Key::CancelAfterCurrent))
                        .small()
                        .color(palette.muted),
                );
            });
        }
    }
}

impl Drop for CleanupApplyJob {
    fn drop(&mut self) {
        self.cancel();
    }
}
