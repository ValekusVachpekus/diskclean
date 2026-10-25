//! Application palette, typography, and egui style.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke,
    Style, TextStyle, Theme, Visuals, vec2,
};
use serde::{Deserialize, Serialize};

use crate::i18n::{Key, Language};
use crate::ui::tokens::{FONT_BODY, FONT_DISPLAY, FONT_SMALL, RADIUS_CARD, RADIUS_CONTROL};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeMode {
    #[must_use]
    pub(crate) fn label(self, language: Language) -> &'static str {
        match self {
            Self::System => language.text(Key::System),
            Self::Dark => language.text(Key::Dark),
            Self::Light => language.text(Key::Light),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) dark: bool,
    pub(crate) bg: Color32,
    pub(crate) panel: Color32,
    pub(crate) surface: Color32,
    pub(crate) surface_hover: Color32,
    pub(crate) border: Color32,
    pub(crate) text: Color32,
    pub(crate) muted: Color32,
    pub(crate) accent: Color32,
    pub(crate) on_accent: Color32,
    pub(crate) success: Color32,
    pub(crate) warning: Color32,
    pub(crate) danger: Color32,
    pub(crate) info: Color32,
    pub(crate) tile_text: Color32,
}

impl Palette {
    pub(crate) const DARK: Self = Self {
        dark: true,
        bg: Color32::from_rgb(0x0E, 0x11, 0x16),
        panel: Color32::from_rgb(0x13, 0x18, 0x20),
        surface: Color32::from_rgb(0x1A, 0x20, 0x29),
        surface_hover: Color32::from_rgb(0x22, 0x2B, 0x36),
        border: Color32::from_rgb(0x2A, 0x34, 0x41),
        text: Color32::from_rgb(0xE8, 0xED, 0xF4),
        muted: Color32::from_rgb(0x8C, 0x99, 0xAA),
        accent: Color32::from_rgb(0x5E, 0xEA, 0xD4),
        on_accent: Color32::from_rgb(0x06, 0x2A, 0x26),
        success: Color32::from_rgb(0x5B, 0xD6, 0x8A),
        warning: Color32::from_rgb(0xF2, 0xB8, 0x4B),
        danger: Color32::from_rgb(0xF0, 0x6A, 0x6A),
        info: Color32::from_rgb(0x6E, 0xA8, 0xFE),
        tile_text: Color32::WHITE,
    };

    pub(crate) const LIGHT: Self = Self {
        dark: false,
        bg: Color32::from_rgb(0xF5, 0xF7, 0xFA),
        panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
        surface: Color32::from_rgb(0xEE, 0xF2, 0xF6),
        surface_hover: Color32::from_rgb(0xE4, 0xEA, 0xF1),
        border: Color32::from_rgb(0xD8, 0xE0, 0xE9),
        text: Color32::from_rgb(0x15, 0x20, 0x2B),
        muted: Color32::from_rgb(0x5E, 0x6C, 0x7D),
        accent: Color32::from_rgb(0x0F, 0x76, 0x6E),
        on_accent: Color32::WHITE,
        success: Color32::from_rgb(0x1F, 0x8A, 0x4C),
        warning: Color32::from_rgb(0xA8, 0x6A, 0x0A),
        danger: Color32::from_rgb(0xC0, 0x39, 0x2B),
        info: Color32::from_rgb(0x2B, 0x6C, 0xB0),
        tile_text: Color32::WHITE,
    };

    #[must_use]
    pub(crate) fn for_theme(mode: ThemeMode, system: Option<Theme>) -> Self {
        let dark = match mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => !matches!(system, Some(Theme::Light)),
        };
        if dark { Self::DARK } else { Self::LIGHT }
    }

    #[must_use]
    pub(crate) fn is_dark(self) -> bool {
        self.dark
    }

    #[must_use]
    pub(crate) fn accent_soft(self) -> Color32 {
        Color32::from_rgba_unmultiplied(
            self.accent.r(),
            self.accent.g(),
            self.accent.b(),
            if self.dark { 34 } else { 28 },
        )
    }

    #[must_use]
    pub(crate) fn tile_color(self, index: usize) -> Color32 {
        const DARK_TILES: [Color32; 6] = [
            Color32::from_rgb(0x2A, 0x8C, 0x82),
            Color32::from_rgb(0x3E, 0x6E, 0x8F),
            Color32::from_rgb(0x70, 0x62, 0x86),
            Color32::from_rgb(0x9A, 0x74, 0x58),
            Color32::from_rgb(0x5D, 0x84, 0x4A),
            Color32::from_rgb(0x7B, 0x64, 0x8F),
        ];
        const LIGHT_TILES: [Color32; 6] = [
            Color32::from_rgb(0x2A, 0x9D, 0x8F),
            Color32::from_rgb(0x45, 0x7B, 0x9D),
            Color32::from_rgb(0x7B, 0x6D, 0x8D),
            Color32::from_rgb(0xB0, 0x89, 0x68),
            Color32::from_rgb(0x6A, 0x99, 0x4E),
            Color32::from_rgb(0x8A, 0x6F, 0x9E),
        ];
        if self.dark {
            DARK_TILES[index % DARK_TILES.len()]
        } else {
            LIGHT_TILES[index % LIGHT_TILES.len()]
        }
    }
}

#[must_use]
pub(crate) fn current_palette(ctx: &Context, mode: ThemeMode) -> Palette {
    Palette::for_theme(mode, ctx.system_theme())
}

pub(crate) fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "inter-regular".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "inter-medium".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/Inter-Medium.ttf"
        ))),
    );
    fonts.font_data.insert(
        "inter-semibold".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "jetbrains-mono".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "phosphor".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/icons/Phosphor.ttf"
        ))),
    );

    let mut proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    proportional.insert(0, "inter-regular".to_owned());
    proportional.push("phosphor".to_owned());
    fonts
        .families
        .insert(FontFamily::Proportional, proportional.clone());

    let mut medium = proportional.clone();
    medium.insert(0, "inter-medium".to_owned());
    fonts
        .families
        .insert(FontFamily::Name("medium".into()), medium);

    let mut semibold = proportional.clone();
    semibold.insert(0, "inter-semibold".to_owned());
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), semibold);

    let mut monospace = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    monospace.insert(0, "jetbrains-mono".to_owned());
    fonts.families.insert(FontFamily::Monospace, monospace);

    ctx.set_fonts(fonts);
}

pub(crate) fn apply(ctx: &Context, mode: ThemeMode) {
    for (theme, palette) in [(Theme::Dark, Palette::DARK), (Theme::Light, Palette::LIGHT)] {
        let mut style: Style = (*ctx.style_of(theme)).clone();

        style.visuals = visuals(palette);
        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.button_padding = vec2(10.0, 5.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.menu_margin = egui::Margin::same(10);
        style.animation_time = 0.12;
        style.interaction.selectable_labels = false;
        style.visuals.striped = false;
        style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(FONT_DISPLAY, FontFamily::Name("semibold".into())),
            ),
            (
                TextStyle::Body,
                FontId::new(FONT_BODY, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(FONT_BODY, FontFamily::Name("medium".into())),
            ),
            (
                TextStyle::Small,
                FontId::new(FONT_SMALL, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(FONT_SMALL, FontFamily::Monospace),
            ),
        ]
        .into();

        ctx.set_style_of(theme, style);
    }

    ctx.set_theme(match mode {
        ThemeMode::System => egui::ThemePreference::System,
        ThemeMode::Dark => egui::ThemePreference::Dark,
        ThemeMode::Light => egui::ThemePreference::Light,
    });
}

fn visuals(palette: Palette) -> Visuals {
    let mut visuals = if palette.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    visuals.panel_fill = palette.panel;
    visuals.window_fill = palette.surface;
    visuals.extreme_bg_color = palette.bg;
    visuals.faint_bg_color = palette.surface;
    visuals.code_bg_color = palette.surface;
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS_CARD);
    visuals.hyperlink_color = palette.accent;
    visuals.warn_fg_color = palette.warning;
    visuals.error_fg_color = palette.danger;
    visuals.override_text_color = Some(palette.text);
    visuals.weak_text_color = Some(palette.muted);
    visuals.weak_text_alpha = 1.0;
    visuals.selection.bg_fill = palette.accent_soft();
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if palette.dark { 120 } else { 40 }),
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if palette.dark { 100 } else { 32 }),
    };

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = palette.panel;
    widgets.noninteractive.weak_bg_fill = palette.panel;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);
    widgets.noninteractive.corner_radius = CornerRadius::same(RADIUS_CONTROL);

    widgets.inactive.bg_fill = palette.surface;
    widgets.inactive.weak_bg_fill = palette.surface;
    widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);
    widgets.inactive.corner_radius = CornerRadius::same(RADIUS_CONTROL);

    widgets.hovered.bg_fill = palette.surface_hover;
    widgets.hovered.weak_bg_fill = palette.surface_hover;
    widgets.hovered.bg_stroke = Stroke::new(1.0, palette.border);
    widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);
    widgets.hovered.corner_radius = CornerRadius::same(RADIUS_CONTROL);

    widgets.active.bg_fill = palette.accent_soft();
    widgets.active.weak_bg_fill = palette.accent_soft();
    widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    widgets.active.fg_stroke = Stroke::new(1.0, palette.text);
    widgets.active.corner_radius = CornerRadius::same(RADIUS_CONTROL);

    visuals
}

#[must_use]
pub(crate) fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

#[must_use]
pub(crate) fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}
