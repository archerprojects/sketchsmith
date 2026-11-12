//! SketchSmith — UI theme | v1.0.0 | Thursday 12 November 2026
//!
//! The light theme: light panels with dark, high-contrast text.
//! The app forces the light theme rather than following the desktop's
//! dark/light preference, so text stays readable on every system. Tooltips use
//! the same dark text, and disabled items are only gently faded.

use egui::{Color32, Context, Theme, ThemePreference, Visuals};

/// Main text colour.
const TEXT: Color32 = Color32::from_gray(20);
/// Secondary ("weak") text colour — still clearly readable.
const WEAK_TEXT: Color32 = Color32::from_gray(80);

/// Applies the SketchSmith light theme to `ctx`.
pub fn apply(ctx: &Context) {
    ctx.set_theme(ThemePreference::Light);
    let mut v = Visuals::light();
    v.weak_text_color = Some(WEAK_TEXT);
    v.disabled_alpha = 0.65;
    v.panel_fill = Color32::from_gray(244);
    v.window_fill = Color32::from_gray(250);
    v.extreme_bg_color = Color32::WHITE;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.fg_stroke.color = TEXT;
    }
    ctx.set_visuals_of(Theme::Light, v);
}
