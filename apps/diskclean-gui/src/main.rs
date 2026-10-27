#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod dialogs;
mod i18n;
mod panels;
mod settings;
#[cfg(test)]
mod tests;
mod treemap;
mod ui;
mod views;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Disk Cleanup"),
        ..eframe::NativeOptions::default()
    };

    eframe::run_native(
        "Disk Cleanup",
        options,
        Box::new(|context| Ok(Box::new(app::DiskCleanupApp::new(context)))),
    )
}
