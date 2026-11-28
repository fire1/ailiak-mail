use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use egui::{
    Color32, FontData, FontDefinitions, FontFamily, FontId, Id, Rounding, Stroke, Style, TextStyle,
    Theme, ThemePreference, Visuals,
};
use mailbox_core::config::ThemeMode;

/// Ailiak.com Studio accent options (`src/utils/mood.ts`).
#[derive(Clone, Copy)]
pub struct AccentOption {
    pub id: &'static str,
    pub label: &'static str,
    pub color: Color32,
}

pub const ACCENT_OPTIONS: &[AccentOption] = &[
    AccentOption {
        id: "teal",
        label: "Ailiak",
        color: Color32::from_rgb(0x16, 0x9f, 0x84),
    },
    AccentOption {
        id: "cyan",
        label: "Cyan",
        color: Color32::from_rgb(0x49, 0x91, 0xa3),
    },
    AccentOption {
        id: "gold",
        label: "Gold",
        color: Color32::from_rgb(0xc6, 0x98, 0x33),
    },
    AccentOption {
        id: "peach",
        label: "Peach",
        color: Color32::from_rgb(0xb7, 0x8e, 0x6c),
    },
    AccentOption {
        id: "lavender",
        label: "Lavender",
        color: Color32::from_rgb(0x9c, 0x5d, 0xc6),
    },
    AccentOption {
        id: "leaf",
        label: "Leaf",
        color: Color32::from_rgb(0x6a, 0xad, 0x3e),
    },
    AccentOption {
        id: "sky",
        label: "Sky",
        color: Color32::from_rgb(0x48, 0x70, 0xbf),
    },
    AccentOption {
        id: "pink",
        label: "Pink",
        color: Color32::from_rgb(0xc4, 0x5b, 0x7a),
    },
    AccentOption {
        id: "stone",
        label: "Stone",
        color: Color32::from_rgb(0x6b, 0x72, 0x6f),
    },
    AccentOption {
        id: "cobalt",
        label: "Cobalt",
        color: Color32::from_rgb(0x2e, 0x5c, 0xcb),
    },
];

pub const DEFAULT_ACCENT_ID: &str = "teal";

pub fn resolve_accent(id: &str) -> AccentOption {
    ACCENT_OPTIONS
        .iter()
        .copied()
        .find(|a| a.id == id)
        .unwrap_or(ACCENT_OPTIONS[0])
}

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
    /// Mid message-list card fill.
    pub list_card: Color32,
    /// Mail reader card fill.
    pub reader_card: Color32,
    /// Unread message row fill (theme-aware lift vs list_card).
    pub unread_row: Color32,
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| -> u8 { ((x as f32) * (1.0 - t) + (y as f32) * t).round() as u8 };
    Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

fn lighten(c: Color32, t: f32) -> Color32 {
    mix(c, Color32::WHITE, t)
}

fn light_palette(accent: Color32) -> Palette {
    let panel = Color32::from_rgb(0xf0, 0xf1, 0xf2);
    Palette {
        accent,
        // Mid grey for nav icons/labels (Folders, Staks, Desk…) — between black and pale.
        mute: Color32::from_rgb(0x6e, 0x74, 0x72),
        unread: accent,
        panel,
        select: mix(panel, accent, 0.16),
        ink: Color32::from_rgb(0x3a, 0x42, 0x40),
        paper: Color32::from_rgb(0xf7, 0xf7, 0xf8),
        faint: Color32::from_rgb(0xe8, 0xe9, 0xea),
        chip: Color32::from_rgb(0xe4, 0xe6, 0xe7),
        // Very light accent wash (Sass-like mix of white ← accent).
        list_card: mix(Color32::WHITE, accent, 0.045),
        reader_card: Color32::WHITE,
        // Soft lift vs list_card wash so unread still reads as “new”.
        unread_row: Color32::WHITE,
    }
}

fn dark_palette(accent: Color32) -> Palette {
    // Slightly lighter accent on dark chrome for contrast (same ids as the site).
    let accent = lighten(accent, 0.18);
    let panel = Color32::from_rgb(0x3a, 0x42, 0x44);
    let paper = Color32::from_rgb(0x32, 0x38, 0x3a);
    let chip = Color32::from_rgb(0x45, 0x54, 0x4f);
    Palette {
        accent,
        mute: Color32::from_rgb(0x9a, 0xa8, 0xa2),
        unread: lighten(accent, 0.12),
        panel,
        select: mix(panel, accent, 0.28),
        ink: Color32::from_rgb(0xe8, 0xee, 0xeb),
        paper,
        faint: Color32::from_rgb(0x2c, 0x32, 0x34),
        chip,
        // Slightly darker than paper, with a touch of accent.
        list_card: mix(mix(paper, Color32::BLACK, 0.08), accent, 0.08),
        // HTML mail is authored for light paper — keep the reader card white
        // even when shell chrome is dark (see reader_ink / reader_mute).
        reader_card: Color32::WHITE,
        // Dark elevated row (not white) so unread stays on-theme vs list_card.
        unread_row: mix(mix(paper, Color32::WHITE, 0.10), accent, 0.14),
    }
}

fn accent_cache_id() -> Id {
    Id::new("ailiakmail_accent_color")
}

fn store_accent(ctx: &egui::Context, accent: Color32) {
    ctx.data_mut(|d| d.insert_temp(accent_cache_id(), accent));
}

fn cached_accent(ctx: &egui::Context) -> Color32 {
    ctx.data(|d| d.get_temp::<Color32>(accent_cache_id()))
        .unwrap_or_else(|| resolve_accent(DEFAULT_ACCENT_ID).color)
}

/// Roboto Mono — left rail + titles (existing AiliakMail chrome).
pub fn nav_family() -> FontFamily {
    FontFamily::Name("RobotoMono".into())
}

pub fn nav_font(size: f32) -> FontId {
    FontId::new(size, nav_family())
}

/// Lato Semibold — real weight for titles (egui `.strong()` only tints color).
pub fn semibold_family() -> FontFamily {
    FontFamily::Name("LatoSemibold".into())
}

/// Paint a single line of text, ellipsizing when it exceeds `max_width`.
/// `pos` is the left-center anchor (same as `Align2::LEFT_CENTER` for `painter.text`).
pub fn paint_truncated_line(
    ui: &egui::Ui,
    pos: egui::Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) {
    use egui::text::{LayoutJob, TextFormat, TextWrapping};

    let max_width = max_width.max(0.0);
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    job.wrap = TextWrapping {
        max_width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = ui.painter().layout_job(job);
    let anchor = egui::pos2(pos.x, pos.y - galley.size().y * 0.5);
    ui.painter().galley(anchor, galley, color);
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // Content (Talanoa uses Lato as --app-font-family).
    fonts.font_data.insert(
        "Lato".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Lato-Regular.ttf")),
    );
    fonts.font_data.insert(
        "LatoMedium".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Lato-Medium.ttf")),
    );
    fonts.font_data.insert(
        "LatoSemibold".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Lato-Semibold.ttf")),
    );

    // Nav / titles.
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

    // Prepend custom faces; keep egui defaults as fallbacks (they own ◻ / ? replacement glyphs).
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .splice(
            0..0,
            [
                "Lato".to_owned(),
                "LatoMedium".to_owned(),
                "LatoSemibold".to_owned(),
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
    {
        // Named family so RichText can pick real weight (egui `.strong()` is color-only).
        let mut semibold = vec!["LatoSemibold".to_owned(), "LatoMedium".to_owned(), "Lato".to_owned()];
        if let Some(prop) = fonts.families.get(&FontFamily::Proportional) {
            for name in prop {
                if !semibold.iter().any(|s| s == name) {
                    semibold.push(name.clone());
                }
            }
        }
        fonts
            .families
            .insert(FontFamily::Name("LatoSemibold".into()), semibold);
    }
    {
        let mut mono = vec![
            "RobotoMonoMedium".to_owned(),
            "RobotoMono".to_owned(),
            "RobotoMonoSemiBold".to_owned(),
        ];
        // Fall back to default proportional stack for missing glyphs.
        if let Some(prop) = fonts.families.get(&FontFamily::Proportional) {
            mono.extend(prop.iter().cloned());
        }
        fonts
            .families
            .insert(FontFamily::Name("RobotoMono".into()), mono);
    }

    // Phosphor icon glyphs — dedicated family; fall back so tofu never panics the atlas.
    fonts.font_data.insert(
        "phosphor".into(),
        egui_phosphor::Variant::Regular.font_data(),
    );
    {
        let mut phosphor = vec!["phosphor".to_owned()];
        if let Some(prop) = fonts.families.get(&FontFamily::Proportional) {
            phosphor.extend(prop.iter().cloned());
        }
        fonts
            .families
            .insert(FontFamily::Name("Phosphor".into()), phosphor);
    }

    ctx.set_fonts(fonts);
}

/// Icon font family (Phosphor Regular).
pub fn icon_family() -> FontFamily {
    FontFamily::Name("Phosphor".into())
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, icon_family())
}

fn base_style() -> Style {
    let mut style = Style::default();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(12.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    // Default: solid gutters for nav. Mail list panel overrides with [`mail_list_scroll_style`].
    style.spacing.scroll = egui::style::ScrollStyle {
        bar_width: 4.0,
        bar_inner_margin: 1.0,
        bar_outer_margin: 0.0,
        ..egui::style::ScrollStyle::solid()
    };
    // Titles keep Roboto Mono; body/content use Lato (integer px sizes stay sharp).
    style.text_styles.insert(TextStyle::Heading, nav_font(16.0));
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(12.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.0, FontFamily::Monospace),
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
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.ink);
    visuals.widgets.inactive.bg_fill = p.chip;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.ink);
    visuals.widgets.hovered.bg_fill = p.select;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.accent);
    visuals.widgets.active.bg_fill = p.select;
    visuals.widgets.active.fg_stroke = Stroke::new(1.25_f32, p.accent);
    visuals.selection.bg_fill = p.select;
    visuals.selection.stroke = Stroke::new(1.0_f32, p.accent);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, line);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, line);
    visuals.window_stroke = Stroke::new(1.0_f32, line);
    visuals.window_rounding = Rounding::same(10.0);
    visuals.menu_rounding = Rounding::same(8.0);
    visuals.widgets.noninteractive.rounding = Rounding::same(8.0);
    visuals.widgets.inactive.rounding = Rounding::same(8.0);
    visuals.widgets.hovered.rounding = Rounding::same(8.0);
    visuals.widgets.active.rounding = Rounding::same(8.0);
    visuals.hyperlink_color = p.accent;
    visuals
}

fn light_style(accent: Color32) -> Style {
    let mut style = base_style();
    style.visuals = paint_visuals(Visuals::light(), light_palette(accent));
    style
}

fn dark_style(accent: Color32) -> Style {
    let mut style = base_style();
    style.visuals = paint_visuals(Visuals::dark(), dark_palette(accent));
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

pub fn install(ctx: &egui::Context, mode: ThemeMode, accent_id: &str) {
    install_fonts(ctx);
    ctx.options_mut(|opt| {
        opt.fallback_theme = Theme::Light;
    });
    apply_appearance(ctx, mode, accent_id);
    apply_screen_zoom(ctx);
}

/// Content zoom from display size (1.0 on laptop-sized screens, slightly higher on large PCs).
///
/// Uses the monitor short side when known, else the window. Never shrinks below 1.0 so the
/// current laptop look stays; caps the bump so large monitors only get a mild enlarge.
pub fn screen_content_zoom(short_side_pts: f32) -> f32 {
    // ~800pt short side ≈ 1080p laptop / typical maximized window feeling “right”.
    const REF: f32 = 800.0;
    // At 1.5× REF (~1200pt short, big desktop), apply +15% content zoom.
    const MAX_EXTRA: f32 = 0.15;
    let raw = (short_side_pts / REF).clamp(1.0, 1.5);
    let t = (raw - 1.0) / 0.5;
    1.0 + MAX_EXTRA * t
}

fn viewport_short_side_pts(ctx: &egui::Context) -> f32 {
    ctx.input(|i| {
        let vp = i.viewport();
        if let Some(size) = vp.monitor_size {
            size.x.min(size.y)
        } else if let Some(outer) = vp.outer_rect {
            outer.width().min(outer.height())
        } else if let Some(inner) = vp.inner_rect {
            inner.width().min(inner.height())
        } else {
            REF_FALLBACK_SHORT
        }
    })
}

const REF_FALLBACK_SHORT: f32 = 800.0;

/// Multiply native DPI by [`screen_content_zoom`] so text/controls grow a bit on large screens
/// without changing layout ratios (same as a mild OS-level zoom).
pub fn apply_screen_zoom(ctx: &egui::Context) {
    let native = ctx.native_pixels_per_point().unwrap_or(1.0);
    let zoom = screen_content_zoom(viewport_short_side_pts(ctx));
    let want = native * zoom;
    let cur = ctx.pixels_per_point();
    if (want - cur).abs() > 0.008 {
        ctx.set_pixels_per_point(want);
    }
}

/// Rebuild light/dark styles for the chosen accent and apply theme preference.
pub fn apply_appearance(ctx: &egui::Context, mode: ThemeMode, accent_id: &str) {
    let accent = resolve_accent(accent_id).color;
    store_accent(ctx, accent);
    ctx.set_style_of(Theme::Light, light_style(accent));
    ctx.set_style_of(Theme::Dark, dark_style(accent));
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
    let accent = cached_accent(ctx);
    if ctx.style().visuals.dark_mode {
        dark_palette(accent)
    } else {
        light_palette(accent)
    }
}

pub fn accent(ctx: &egui::Context) -> Color32 {
    if ctx.style().visuals.dark_mode {
        dark_palette(cached_accent(ctx)).accent
    } else {
        cached_accent(ctx)
    }
}

pub fn ink(ctx: &egui::Context) -> Color32 {
    palette(ctx).ink
}

pub fn mute(ctx: &egui::Context) -> Color32 {
    palette(ctx).mute
}

pub fn unread(ctx: &egui::Context) -> Color32 {
    palette(ctx).unread
}

pub fn unread_row(ctx: &egui::Context) -> Color32 {
    palette(ctx).unread_row
}

/// Ink for text on [`unread_row`] — follows light/dark shell.
pub fn unread_row_ink(ctx: &egui::Context) -> Color32 {
    ink(ctx)
}

/// Mute label color on [`unread_row`].
pub fn unread_row_mute(ctx: &egui::Context) -> Color32 {
    mute(ctx)
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

pub fn list_card(ctx: &egui::Context) -> Color32 {
    palette(ctx).list_card
}

/// Floating scrollbar for the mail list panel: hairline thumb, slightly wider on hover.
pub fn mail_list_scroll_style() -> egui::style::ScrollStyle {
    egui::style::ScrollStyle {
        floating: true,
        bar_width: 4.0,
        floating_width: 1.5,
        floating_allocated_width: 0.0,
        bar_inner_margin: 0.0,
        bar_outer_margin: 0.0,
        handle_min_length: 24.0,
        foreground_color: true,
        // Always show a faint handle so scroll position is visible.
        dormant_background_opacity: 0.0,
        dormant_handle_opacity: 0.45,
        active_background_opacity: 0.15,
        active_handle_opacity: 0.7,
        interact_background_opacity: 0.28,
        interact_handle_opacity: 1.0,
    }
}

pub fn reader_card(ctx: &egui::Context) -> Color32 {
    palette(ctx).reader_card
}

/// Ink for text on [`reader_card`] / unread white rows (always light-surface colors).
pub fn reader_ink() -> Color32 {
    Color32::from_rgb(0x3a, 0x42, 0x40)
}

/// Mute label color on [`reader_card`] (always light-surface).
pub fn reader_mute() -> Color32 {
    Color32::from_rgb(0x6e, 0x74, 0x72)
}

/// Chip / hint plate on [`reader_card`] (always light-surface).
pub fn reader_chip() -> Color32 {
    Color32::from_rgb(0xe4, 0xe6, 0xe7)
}

/// Native webview / loading underlay while HTML mail paints.
/// Always light paper so it matches the HTML document and white reader card
/// (avoids a dark flash between messages in dark shell theme).
pub fn mail_webview_backdrop(_dark: bool) -> Color32 {
    Color32::WHITE
}

pub fn mail_webview_backdrop_rgba(dark: bool) -> (u8, u8, u8, u8) {
    let c = mail_webview_backdrop(dark);
    (c.r(), c.g(), c.b(), 255)
}

pub fn mail_webview_backdrop_css(dark: bool) -> String {
    let c = mail_webview_backdrop(dark);
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_zoom_laptop_stays_one() {
        assert!((screen_content_zoom(720.0) - 1.0).abs() < f32::EPSILON);
        assert!((screen_content_zoom(800.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn screen_zoom_large_display_nudges_up() {
        let mid = screen_content_zoom(1000.0);
        let big = screen_content_zoom(1200.0);
        assert!(mid > 1.0 && mid < 1.15, "mid={mid}");
        assert!((big - 1.15).abs() < 0.001, "big={big}");
        assert!(big >= mid);
    }
}
