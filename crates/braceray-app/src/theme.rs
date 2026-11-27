//! Dark and light visuals with one accent colour ("ray" cyan), and the theme switch.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme, ThemePreference, Visuals};

use crate::settings::ThemeChoice;

/// Accent on dark backgrounds (`#22D3EE`).
pub const ACCENT_DARK: Color32 = Color32::from_rgb(0x22, 0xD3, 0xEE);
/// Accent on light backgrounds (`#0891B2`).
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x08, 0x91, 0xB2);
/// Corner radius of widgets and windows.
pub const ROUNDING: u8 = 6;

/// Error colour (banner stripe, messages).
pub fn error_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xF8, 0x71, 0x71)
    } else {
        Color32::from_rgb(0xB9, 0x1C, 0x1C)
    }
}

/// Warning colour (file changed on disk).
pub fn warn_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xFB, 0xBF, 0x24)
    } else {
        Color32::from_rgb(0xB4, 0x53, 0x09)
    }
}

/// The accent of the current theme.
pub fn accent(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        ACCENT_DARK
    } else {
        ACCENT_LIGHT
    }
}

/// Installs both custom visuals and the spacing (once, at startup).
pub fn install(ctx: &egui::Context) {
    ctx.set_visuals_of(Theme::Dark, visuals(true));
    ctx.set_visuals_of(Theme::Light, visuals(false));
    for theme in [Theme::Dark, Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            style.spacing.button_padding = egui::vec2(10.0, 4.0);
            style.spacing.interact_size.y = 24.0;
            style.spacing.window_margin = egui::Margin::same(12);
        });
    }
}

/// Switches the theme (instant: egui repaints with the other visuals next frame).
pub fn apply(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_theme(preference(choice));
}

/// The egui preference of a choice.
pub fn preference(choice: ThemeChoice) -> ThemePreference {
    match choice {
        ThemeChoice::System => ThemePreference::System,
        ThemeChoice::Dark => ThemePreference::Dark,
        ThemeChoice::Light => ThemePreference::Light,
    }
}

fn visuals(dark: bool) -> Visuals {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let accent = if dark { ACCENT_DARK } else { ACCENT_LIGHT };
    let r = CornerRadius::same(ROUNDING);
    v.selection.bg_fill = accent.gamma_multiply(if dark { 0.35 } else { 0.25 });
    v.selection.stroke = Stroke::new(1.0, accent);
    v.hyperlink_color = accent;
    v.window_corner_radius = r;
    v.menu_corner_radius = r;
    if dark {
        v.panel_fill = Color32::from_rgb(0x16, 0x1A, 0x20);
        v.window_fill = Color32::from_rgb(0x1C, 0x21, 0x28);
        v.extreme_bg_color = Color32::from_rgb(0x0F, 0x12, 0x16);
        v.faint_bg_color = Color32::from_rgb(0x1E, 0x23, 0x2B);
    } else {
        v.panel_fill = Color32::from_rgb(0xF7, 0xF8, 0xFA);
        v.window_fill = Color32::WHITE;
        v.extreme_bg_color = Color32::WHITE;
        v.faint_bg_color = Color32::from_rgb(0xEE, 0xF1, 0xF4);
    }
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent.gamma_multiply(0.8));
    v.widgets.active.bg_stroke = Stroke::new(1.0, accent);
    v
}
