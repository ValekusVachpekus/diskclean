//! Contextual inspector for the selected node.

use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Frame, Layout, RichText, Sense, Stroke, vec2,
};

use crate::app::{DiskCleanupApp, Workspace, kind_icon, kind_label};
use crate::i18n::Key;
use crate::ui::{components, format, icons, theme, tokens};

impl DiskCleanupApp {
    pub(crate) fn inspector_dialog(&mut self, context: &egui::Context) {
        if !self.wide && self.narrow_inspector && self.workspace == Workspace::Analyze {
            let palette = self.palette;
            let modal = egui::Modal::new(egui::Id::unique("narrow_inspector"))
                .frame(
                    Frame::new()
                        .fill(palette.panel)
                        .stroke(Stroke::new(1.0, palette.border))
                        .corner_radius(tokens::RADIUS_CARD)
                        .inner_margin(16),
                )
                .show(context, |ui| {
                    let size = components::modal_size(context, 360.0);
                    ui.set_width(size.x);
                    ui.set_height(size.y.min(600.0));
                    if components::icon_button(ui, palette, icons::CLOSE, self.text(Key::Close))
                        .clicked()
                    {
                        self.narrow_inspector = false;
                    }
                    self.inspector(ui);
                });
            if modal.should_close() {
                self.narrow_inspector = false;
            }
        }
    }

    pub(crate) fn inspector(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let Some(tree) = self.tree.clone() else {
            components::section_label(ui, palette, self.text(Key::Selection));
            ui.label(
                RichText::new(self.text(Key::NoSelection))
                    .color(palette.muted)
                    .size(12.0),
            );
            return;
        };

        let selected = self.selected;
        let node = tree.node(selected);
        let path = tree.path(selected);
        let parent_size = node.parent.map_or(tree.stats.allocated_bytes, |parent| {
            tree.node(parent).allocated_size
        });
        let percent = format::percent(node.allocated_size, parent_size.max(1));

        let mut open = false;
        let mut reveal = false;
        let mut copy = false;
        let mut go_up = None;
        let mut chosen_child = None;

        egui::ScrollArea::vertical()
            .id_salt("inspector_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                components::section_label(ui, palette, self.text(Key::Selection));

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(kind_icon(node.kind))
                            .font(FontId::proportional(19.0))
                            .color(palette.accent),
                    );
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(node.display_name())
                                    .font(theme::semibold(15.0))
                                    .color(palette.text),
                            )
                            .truncate(),
                        )
                        .on_hover_text(node.display_name());
                        ui.add(
                            egui::Label::new(
                                RichText::new(format::shorten_middle(
                                    &path.display().to_string(),
                                    42,
                                ))
                                .color(palette.muted)
                                .size(10.5),
                            )
                            .truncate(),
                        )
                        .on_hover_text(path.display().to_string());
                    });
                });

                ui.add_space(10.0);
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(tokens::RADIUS_CARD)
                    .inner_margin(11)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(format::size(
                                        self.language(),
                                        node.allocated_size,
                                    ))
                                    .font(theme::semibold(21.0))
                                    .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{:.1}% {}",
                                        percent,
                                        self.text(Key::PercentOfParent)
                                    ))
                                    .color(palette.muted)
                                    .size(10.5),
                                );
                            });
                            if let Some(parent) = node.parent {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if components::icon_button(
                                        ui,
                                        palette,
                                        icons::ARROW_UP,
                                        self.text(Key::Up),
                                    )
                                    .clicked()
                                    {
                                        go_up = Some(parent);
                                    }
                                });
                            }
                        });
                    });

                ui.add_space(8.0);
                components::stat_row(
                    ui,
                    palette,
                    self.text(Key::OnDisk),
                    &format::size(self.language(), node.allocated_size),
                );
                components::stat_row(
                    ui,
                    palette,
                    self.text(Key::Logical),
                    &format::size(self.language(), node.logical_size),
                );
                components::stat_row(
                    ui,
                    palette,
                    self.text(Key::Actual),
                    &format::size(self.language(), node.actual_allocated_size),
                );
                components::stat_row(
                    ui,
                    palette,
                    self.text(Key::Files),
                    &crate::app::files_label(self.language(), node.file_count),
                );
                components::stat_row(
                    ui,
                    palette,
                    self.text(Key::Folders),
                    &crate::app::folders_label(self.language(), node.dir_count),
                );
                components::stat_row(
                    ui,
                    palette,
                    self.text(Key::Modified),
                    &format::age(self.language(), node.modified_unix),
                );
                components::stat_row(
                    ui,
                    palette,
                    self.text(Key::Kind),
                    kind_label(self.language(), node.kind),
                );
                if node.is_hardlink_duplicate {
                    ui.add_space(6.0);
                    Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(
                            palette.warning.r(),
                            palette.warning.g(),
                            palette.warning.b(),
                            if palette.is_dark() { 28 } else { 20 },
                        ))
                        .corner_radius(tokens::RADIUS_CONTROL)
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(icons::WARNING)
                                        .font(FontId::proportional(13.0))
                                        .color(palette.warning),
                                );
                                ui.label(
                                    RichText::new(self.text(Key::HardlinkNote))
                                        .color(palette.warning)
                                        .size(10.5),
                                );
                            });
                        });
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if components::secondary_button(ui, palette, self.text(Key::Open)).clicked() {
                        open = true;
                    }
                    if components::secondary_button(ui, palette, self.text(Key::Reveal)).clicked() {
                        reveal = true;
                    }
                });
                ui.horizontal(|ui| {
                    if components::secondary_button(ui, palette, self.text(Key::CopyPath)).clicked()
                    {
                        copy = true;
                    }
                });

                if node.kind.is_dir() {
                    ui.add_space(12.0);
                    components::section_label(ui, palette, self.text(Key::Children));
                    let children = self.indexed_children(&tree, selected);
                    if children.is_empty() {
                        ui.label(
                            RichText::new(self.text(Key::NoMatches))
                                .color(palette.muted)
                                .size(11.0),
                        );
                    }
                    for child in children.iter().copied().take(6) {
                        let child_node = tree.node(child);
                        let (rect, response) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                        let hovered = response.hovered();
                        if hovered {
                            ui.painter().rect_filled(
                                rect,
                                tokens::RADIUS_CONTROL,
                                palette.surface_hover,
                            );
                        }
                        let size = format::size(self.language(), child_node.allocated_size);
                        let size_width = ui
                            .painter()
                            .layout_no_wrap(size.clone(), FontId::monospace(10.5), palette.muted)
                            .size()
                            .x;
                        let name_rect = egui::Rect::from_min_max(
                            rect.min + vec2(5.0, 0.0),
                            egui::pos2(rect.right() - size_width - 16.0, rect.bottom()),
                        );
                        let mut job = egui::text::LayoutJob::simple_singleline(
                            child_node.display_name(),
                            FontId::proportional(11.5),
                            palette.text,
                        );
                        job.wrap.max_width = name_rect.width().max(0.0);
                        job.wrap.max_rows = 1;
                        job.wrap.break_anywhere = true;
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                        ui.painter()
                            .with_clip_rect(name_rect.intersect(ui.clip_rect()))
                            .galley(
                                egui::pos2(
                                    name_rect.left(),
                                    rect.center().y - galley.size().y / 2.0,
                                ),
                                galley,
                                palette.text,
                            );
                        ui.painter().text(
                            egui::pos2(rect.right() - 5.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            size,
                            FontId::monospace(10.5),
                            palette.muted,
                        );
                        if response.clicked() {
                            chosen_child = Some(child);
                        }
                        response.on_hover_text(tree.path(child).display().to_string());
                    }
                }

                if node.kind.is_dir() && node.dir_count > 0 {
                    ui.add_space(10.0);
                    let response = ui.add(
                        Button::new(RichText::new(self.text(Key::ShowAll)).size(11.5))
                            .fill(palette.surface)
                            .stroke(Stroke::new(1.0, palette.border))
                            .corner_radius(tokens::RADIUS_CONTROL)
                            .min_size(vec2(ui.available_width(), 28.0)),
                    );
                    if response.clicked() {
                        self.tab = crate::app::Tab::Folders;
                    }
                }

                ui.add_space(10.0);
            });

        if let Some(parent) = go_up {
            self.selected = parent;
        }
        if let Some(child) = chosen_child {
            self.selected = child;
        }
        if copy {
            self.copy_path(ui.ctx(), &path);
        }
        if reveal {
            self.reveal_path(&path);
        }
        if open {
            self.open_path(&path);
        }
    }
}
