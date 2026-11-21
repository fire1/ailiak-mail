use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use egui::{
    Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, Style, TextStyle,
    Theme, ThemePreference, Visuals,
};
use mailbox_core::config::ThemeMode;

/// Soft mint accent (pastel take on Ailiak teal).
pub const ACCENT: Color32 = Color32::from_rgb(0x5a, 0xaf, 0x9a);

#[derive(Clone, Copy)]
pub struct Palette {
    pub accent: Color32,
    pub mute: Color32,
    pub unread: Color32,
    pub panel: Color32,
    pub select: Color32,
    pub ink: Color32,
    pub paper: Color32,
    pub faint: Color32,
    pub chip: Color32,
}

fn light_palette() -> Palette {
    // Pastel mint / sage — calm, airy, still Ailiak-adjacent.
    Palette {
        accent: ACCENT,
        mute: Color32::from_rgb(0x8a, 0x96, 0x91),
        unread: Color32::from_rgb(0x3d, 0x9a, 0x82),
        panel: Color32::from_rgb(0xea, 0xf3, 0xef),
        select: Color32::from_rgb(0xc8, 0xe8, 0xdc),
        ink: Color32::from_rgb(0x3a, 0x45, 0x41),
        paper: Color32::from_rgb(0xfb, 0xfc, 0xf9),
        faint: Color32::from_rgb(0xe0, 0xeb, 0xe6),
        chip: Color32::from_rgb(0xd4, 0xe8, 0xdf),
    }
}

fn dark_palette() -> Palette {
    // Soft slate with pastel mint — no harsh blacks.
    Palette {
        accent: Color32::from_rgb(0x7d, 0xce, 0xb8),
        mute: Color32::from_rgb(0x9a, 0xa8, 0xa2),
        unread: Color32::from_rgb(0x9a, 0xdf, 0xc8),
        panel: Color32::from_rgb(0x3a, 0x42, 0x44),
        select: Color32::from_rgb(0x3f, 0x5a, 0x52),
        ink: Color32::from_rgb(0xe8, 0xee, 0xeb),
        paper: Color32::from_rgb(0x32, 0x38, 0x3a),
        faint: Color32::from_rgb(0x2c, 0x32, 0x34),
        chip: Color32::from_rgb(0x45, 0x54, 0x4f),
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "RobotoMono".into(),
        FontData::from_static(include_bytes!("../assets/fonts/RobotoMono-Regular.ttf")),
    );
    fonts.font_data.insert(
        "RobotoMonoMedium".into(),
        FontData::from_static(include_bytes!("../assets/fonts/RobotoMono-Medium.ttf")),
    );
    fonts.font_data.insert(
        "RobotoMonoSemiBold".into(),
        FontData::from_static(include_bytes!("../assets/fonts/RobotoMono-SemiBold.ttf")),
    );

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .splice(
            0..0,
            [
                "RobotoMonoMedium".to_owned(),
                "RobotoMono".to_owned(),
                "RobotoMonoSemiBold".to_owned(),
            ],
        );

    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .splice(
            0..0,
            ["RobotoMono".to_owned(), "RobotoMonoMedium".to_owned()],
        );

    ctx.set_fonts(fonts);
}

fn base_style() -> Style {
    let mut style = Style::default();
    style.spacing.item_spacing = egui::vec2(10.0, 7.0);
    style.spacing.window_margin = egui::Margin::same(14.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(20.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(14.5, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(14.5, FontFamily::Monospace),
    );
    style
}

fn paint_visuals(mut visuals: Visuals, p: Palette) -> Visuals {
    let line = if visuals.dark_mode {
        Color32::from_rgba_unmultiplied(232, 238, 235, 28)
    } else {
        Color32::from_rgba_unmultiplied(58, 69, 65, 28)
    };
    visuals.window_fill = p.paper;
    visuals.panel_fill = p.panel;
    visuals.faint_bg_color = p.faint;
    visuals.extreme_bg_color = p.chip;
    visuals.widgets.noninteractive.bg_fill = p.panel;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.ink);
    visuals.widgets.inactive.bg_fill = p.chip;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.ink);
    visuals.widgets.hovered.bg_fill = p.select;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, p.accent);
    visuals.widgets.active.bg_fill = p.select;
    visuals.widgets.active.fg_stroke = Stroke::new(1.25, p.accent);
    visuals.selection.bg_fill = p.select;
    visuals.selection.stroke = Stroke::new(1.0, p.accent);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, line);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, line);
    visuals.window_stroke = Stroke::new(1.0, line);
    visuals.window_rounding = Rounding::same(10.0);
    visuals.menu_rounding = Rounding::same(8.0);
    visuals.widgets.noninteractive.rounding = Rounding::same(8.0);
    visuals.widgets.inactive.rounding = Rounding::same(8.0);
    visuals.widgets.hovered.rounding = Rounding::same(8.0);
    visuals.widgets.active.rounding = Rounding::same(8.0);
    visuals.hyperlink_color = p.accent;
    visuals
}

fn light_style() -> Style {
    let mut style = base_style();
    style.visuals = paint_visuals(Visuals::light(), light_palette());
    style
}

fn dark_style() -> Style {
    let mut style = base_style();
    style.visuals = paint_visuals(Visuals::dark(), dark_palette());
    style
}

pub fn detect_system_theme() -> Theme {
    if let Some(theme) = read_gsettings_color_scheme() {
        return theme;
    }
    if let Some(theme) = read_gtk_theme_name() {
        return theme;
    }
    Theme::Light
}

fn read_gsettings_color_scheme() -> Option<Theme> {
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
    if s.contains("prefer-dark") {
        Some(Theme::Dark)
    } else if s.contains("prefer-light") || s.contains("default") {
        Some(Theme::Light)
    } else {
        None
    }
}

fn read_gtk_theme_name() -> Option<Theme> {
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
    if s.contains("dark") {
        Some(Theme::Dark)
    } else {
        Some(Theme::Light)
    }
}

static LAST_SYSTEM_POLL: Mutex<Option<(Instant, Theme)>> = Mutex::new(None);

fn cached_system_theme() -> Theme {
    let mut guard = LAST_SYSTEM_POLL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, theme)) = *guard {
        if at.elapsed() < Duration::from_secs(2) {
            return theme;
        }
    }
    let theme = detect_system_theme();
    *guard = Some((Instant::now(), theme));
    theme
}

fn resolve_theme(mode: ThemeMode) -> Theme {
    match mode {
        ThemeMode::Light => Theme::Light,
        ThemeMode::Dark => Theme::Dark,
        ThemeMode::System => cached_system_theme(),
    }
}

pub fn resolve_theme_ctx(ctx: &egui::Context, mode: ThemeMode) -> Theme {
    match mode {
        ThemeMode::Light => Theme::Light,
        ThemeMode::Dark => Theme::Dark,
        ThemeMode::System => ctx.system_theme().unwrap_or_else(cached_system_theme),
    }
}

pub fn install(ctx: &egui::Context, mode: ThemeMode) {
    install_fonts(ctx);
    ctx.set_style_of(Theme::Light, light_style());
    ctx.set_style_of(Theme::Dark, dark_style());
    ctx.options_mut(|opt| {
        opt.fallback_theme = Theme::Light;
    });
    apply_preference(ctx, mode);
}

pub fn apply_preference(ctx: &egui::Context, mode: ThemeMode) {
    let theme = resolve_theme_ctx(ctx, mode);
    ctx.set_theme(match theme {
        Theme::Light => ThemePreference::Light,
        Theme::Dark => ThemePreference::Dark,
    });
}

pub fn sync_system(ctx: &egui::Context, mode: ThemeMode) {
    if mode != ThemeMode::System {
        return;
    }
    let want = resolve_theme_ctx(ctx, mode);
    if ctx.theme() != want {
        apply_preference(ctx, mode);
    }
}

pub fn resolved_label(mode: ThemeMode) -> String {
    match mode {
        ThemeMode::System => format!(
            "System ({})",
            match resolve_theme(mode) {
                Theme::Light => "light",
                Theme::Dark => "dark",
            }
        ),
        ThemeMode::Light => "Light".into(),
        ThemeMode::Dark => "Dark".into(),
    }
}

pub fn palette(ctx: &egui::Context) -> Palette {
    if ctx.style().visuals.dark_mode {
        dark_palette()
    } else {
        light_palette()
    }
}

pub fn accent(ctx: &egui::Context) -> Color32 {
    palette(ctx).accent
}

pub fn mute(ctx: &egui::Context) -> Color32 {
    palette(ctx).mute
}

pub fn unread(ctx: &egui::Context) -> Color32 {
    palette(ctx).unread
}

pub fn panel(ctx: &egui::Context) -> Color32 {
    palette(ctx).panel
}

pub fn paper(ctx: &egui::Context) -> Color32 {
    palette(ctx).paper
}

pub fn select(ctx: &egui::Context) -> Color32 {
    palette(ctx).select
}

pub fn chip(ctx: &egui::Context) -> Color32 {
    palette(ctx).chip
}
