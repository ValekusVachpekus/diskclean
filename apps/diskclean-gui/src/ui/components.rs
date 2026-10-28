//! Small reusable UI primitives for the shell.

use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Layout, Response, RichText, Role, Sense, Stroke,
    Ui, WidgetInfo, vec2,
};

use crate::ui::theme::{Palette, medium};
use crate::ui::tokens::{RADIUS_CONTROL, SPACE_2, SPACE_3};

pub(crate) fn section_label(ui: &mut Ui, palette: Palette, text: &str) {
    ui.add_space(SPACE_2);
    ui.label(
        RichText::new(text.to_uppercase())
            .font(medium(10.5))
            .color(palette.muted),
    );
    ui.add_space(2.0);
}

pub(crate) fn primary_button(ui: &mut Ui, palette: Palette, text: &str) -> Response {
    ui.add(
        Button::new(RichText::new(text).color(palette.on_accent))
            .fill(palette.accent)
            .stroke(Stroke::NONE)
            .corner_radius(RADIUS_CONTROL)
            .min_size(vec2(0.0, 30.0)),
    )
}

pub(crate) fn secondary_button(ui: &mut Ui, palette: Palette, text: &str) -> Response {
    ui.add(
        Button::new(RichText::new(text))
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(RADIUS_CONTROL)
            .min_size(vec2(0.0, 30.0)),
    )
}

/// All dialogs share the same viewport budget, including at high UI scaling.
pub(crate) fn modal_size(context: &egui::Context, preferred_width: f32) -> egui::Vec2 {
    let viewport = context.content_rect().size();
    vec2(
        preferred_width.min((viewport.x - 72.0).max(240.0)),
        (viewport.y - 96.0).max(120.0),
    )
}

pub(crate) fn workspace_title(
    ui: &mut Ui,
    palette: Palette,
    icon: &str,
    title: &str,
    subtitle: &str,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(23.0).color(palette.accent));
        ui.vertical(|ui| {
            ui.add(
                egui::Label::new(RichText::new(title).font(crate::ui::theme::semibold(21.0)))
                    .wrap(),
            );
            ui.add(
                egui::Label::new(RichText::new(subtitle).size(11.5).color(palette.muted)).wrap(),
            );
        });
    });
}

pub(crate) fn icon_button(ui: &mut Ui, _palette: Palette, icon: &str, tooltip: &str) -> Response {
    let response = ui.add(
        Button::new(RichText::new(icon).font(FontId::proportional(16.0)))
            .frame(false)
            .fill(Color32::TRANSPARENT)
            .min_size(vec2(28.0, 28.0)),
    );
    let accessible_name = if tooltip.is_empty() { icon } else { tooltip };
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, accessible_name));
    if tooltip.is_empty() {
        response.on_hover_text("")
    } else {
        response.on_hover_text(tooltip)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn nav_item(
    ui: &mut Ui,
    palette: Palette,
    width: f32,
    icon: &str,
    label: &str,
    badge: Option<&str>,
    selected: bool,
    enabled: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
    let fill = if selected {
        palette.accent_soft()
    } else if response.hovered() && enabled {
        palette.surface_hover
    } else {
        Color32::TRANSPARENT
    };
    let text_color = if enabled { palette.text } else { palette.muted };

    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, RADIUS_CONTROL, fill);
    }
    if selected {
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + 7.0),
            egui::pos2(rect.left() + 3.0, rect.bottom() - 7.0),
        );
        ui.painter().rect_filled(bar, 2.0, palette.accent);
    }

    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        icon,
        FontId::proportional(16.0),
        if enabled {
            palette.accent
        } else {
            palette.muted
        },
    );
    let badge_width = badge.map_or(0.0, |badge| {
        ui.painter()
            .layout_no_wrap(badge.to_owned(), medium(9.5), palette.muted)
            .size()
            .x
            + 18.0
    });
    let mut job =
        egui::text::LayoutJob::simple_singleline(label.to_owned(), medium(13.0), text_color);
    job.wrap.max_width = (rect.width() - 46.0 - badge_width).max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let label_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 36.0, rect.top()),
        egui::pos2(rect.right() - 10.0 - badge_width, rect.bottom()),
    );
    ui.painter()
        .with_clip_rect(label_rect.intersect(ui.clip_rect()))
        .galley(
            egui::pos2(label_rect.left(), rect.center().y - galley.size().y / 2.0),
            galley,
            text_color,
        );
    if let Some(badge) = badge {
        ui.painter().text(
            egui::pos2(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            badge,
            medium(9.5),
            palette.muted,
        );
    }

    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub(crate) fn usage_bar(
    ui: &mut Ui,
    palette: Palette,
    fraction: f32,
    width: f32,
    height: f32,
    color: Color32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let radius = height / 2.0;
    ui.painter()
        .rect_filled(rect, radius, palette.surface_hover);
    let filled = egui::Rect::from_min_size(
        rect.min,
        vec2(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    if filled.width() > 0.5 {
        ui.painter().rect_filled(filled, radius, color);
    }
    response
}

pub(crate) fn stat_row(ui: &mut Ui, palette: Palette, label: &str, value: &str) {
    ui.horizontal(|ui| {
        let width = ui.available_width();
        ui.allocate_ui_with_layout(
            vec2(width * 0.46, 22.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(RichText::new(label).color(palette.muted).size(12.0))
                        .truncate(),
                )
                .on_hover_text(label);
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(value).font(medium(12.5))).truncate())
                .on_hover_text(value);
        });
    });
}

/// Keep the numeric column visible even when the path fills the whole row.
pub(crate) fn path_size_row(ui: &mut Ui, palette: Palette, icon: &str, path: &str, size: &str) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.label(
            RichText::new(size)
                .font(FontId::monospace(10.0))
                .color(palette.muted),
        );
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new(icon).size(11.0).color(palette.muted));
            ui.add(egui::Label::new(RichText::new(path).font(FontId::monospace(10.0))).truncate())
                .on_hover_text(path);
        });
    });
}

pub(crate) fn chip(ui: &mut Ui, palette: Palette, text: &str, icon: Option<&str>) -> Response {
    let text = if let Some(icon) = icon {
        format!("{icon} {text}")
    } else {
        text.to_owned()
    };
    ui.add(
        Button::new(RichText::new(text).size(11.5))
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(10)
            .min_size(vec2(0.0, 22.0)),
    )
}

pub(crate) fn empty_hint(ui: &mut Ui, palette: Palette, text: &str) {
    ui.add_space(SPACE_3);
    ui.label(RichText::new(text).color(palette.muted));
    ui.add_space(SPACE_3);
}

pub(crate) fn shortcut_row(ui: &mut Ui, palette: Palette, keys: &str, label: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [90.0, 22.0],
            egui::Label::new(RichText::new(keys).font(FontId::monospace(11.0))),
        );
        ui.label(RichText::new(label).color(palette.muted));
    });
}

pub(crate) fn sort_header(
    ui: &mut Ui,
    palette: Palette,
    label: &str,
    active: bool,
    descending: bool,
) -> Response {
    let text = if active {
        format!(
            "{label} {}",
            if descending {
                crate::ui::icons::CARET_DOWN
            } else {
                crate::ui::icons::CARET_UP
            }
        )
    } else {
        label.to_owned()
    };
    ui.add(
        Button::new(RichText::new(text).size(10.5).color(if active {
            palette.text
        } else {
            palette.muted
        }))
        .frame(false)
        .fill(Color32::TRANSPARENT)
        .min_size(vec2(0.0, 22.0)),
    )
}

pub(crate) fn right_label(ui: &mut Ui, palette: Palette, text: &str, monospace: bool) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let rich = RichText::new(text).size(11.5).color(palette.text);
        if monospace {
            ui.label(rich.font(FontId::monospace(11.0)));
        } else {
            ui.label(rich);
        }
    });
}

pub(crate) fn stat_card(
    ui: &mut egui::Ui,
    palette: crate::ui::theme::Palette,
    title: &str,
    value: &str,
    detail: &str,
    color: Color32,
    width: f32,
) {
    egui::Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(crate::ui::tokens::RADIUS_CARD)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(width - 22.0);
            ui.set_min_height(96.0);
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.label(
                    RichText::new(title.to_uppercase())
                        .color(palette.muted)
                        .size(9.5),
                );
                ui.label(
                    RichText::new(value)
                        .font(crate::ui::theme::semibold(16.0))
                        .color(color),
                );
                ui.add(
                    egui::Label::new(RichText::new(detail).color(palette.muted).size(9.5)).wrap(),
                );
            });
        });
}
