//! Ailiak full logo from SVG (same asset as ailiak.com), with accent-tinted mark.

use egui::{Color32, Context, TextureHandle, TextureOptions};

use crate::svg_tex;

const FULL_LOGO_SVG: &str = include_str!("../assets/img/full-logo.svg");

/// Live texture for the top-bar wordmark; recolor when accent / theme changes.
pub struct AiliakLogo {
    tex: TextureHandle,
    accent: Color32,
    dark: bool,
}

impl AiliakLogo {
    pub fn load(ctx: &Context, accent: Color32, dark: bool) -> Self {
        let image = rasterize_full_logo(accent, dark);
        let tex = ctx.load_texture("ailiak-full-logo-svg", image, TextureOptions::LINEAR);
        Self { tex, accent, dark }
    }

    pub fn texture(&self) -> &TextureHandle {
        &self.tex
    }

    /// Like ailiak.com BrandLogo: `.logo-box-fill` tracks `--accent`.
    pub fn sync(&mut self, accent: Color32, dark: bool) {
        if self.accent == accent && self.dark == dark {
            return;
        }
        self.tex
            .set(rasterize_full_logo(accent, dark), TextureOptions::LINEAR);
        self.accent = accent;
        self.dark = dark;
    }
}

fn color_hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn prepare_svg(accent: Color32, dark: bool) -> String {
    let box_hex = color_hex(accent);
    let text_hex = if dark { "#d4d8d6" } else { "#6b726f" };
    FULL_LOGO_SVG
        .replace(
            ".logo-box-fill { fill: #169f84; fill-opacity: 0.85; }",
            &format!(".logo-box-fill {{ fill: {box_hex}; fill-opacity: 0.85; }}"),
        )
        .replace(
            ".logo-txt-fill { fill: #6b726f; fill-rule: nonzero; }",
            &format!(".logo-txt-fill {{ fill: {text_hex}; fill-rule: nonzero; }}"),
        )
}

fn rasterize_full_logo(accent: Color32, dark: bool) -> egui::ColorImage {
    // 2× the website BrandLogo full size (146×36).
    svg_tex::rasterize_svg(&prepare_svg(accent, dark), 292)
}
