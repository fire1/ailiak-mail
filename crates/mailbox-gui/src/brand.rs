//! Ailiak top-bar logo: accent-tinted mark (SVG) + crisp PNG wordmark.

use egui::{Color32, ColorImage, Context, TextureHandle, TextureOptions};

use crate::svg_tex;

const MARK_SVG: &str = include_str!("../assets/img/ailiak.svg");
const WORDMARK_LIGHT_PNG: &[u8] = include_bytes!("../assets/img/ailiak-wordmark-light.png");
const WORDMARK_DARK_PNG: &[u8] = include_bytes!("../assets/img/ailiak-wordmark-dark.png");

/// On-screen logo height in the top bar (mark side).
pub const DISPLAY_HEIGHT: f32 = 28.0;

/// Live textures for the top-bar logo; recolor mark when accent / theme changes.
pub struct AiliakLogo {
    mark: TextureHandle,
    wordmark: TextureHandle,
    wordmark_aspect: f32,
    accent: Color32,
    dark: bool,
}

impl AiliakLogo {
    pub fn load(ctx: &Context, accent: Color32, dark: bool) -> Self {
        let mark = ctx.load_texture(
            "ailiak-mark-svg",
            rasterize_mark(accent),
            TextureOptions::LINEAR,
        );
        let (wm_img, aspect) = load_wordmark_png(dark);
        let wordmark = ctx.load_texture("ailiak-wordmark-png", wm_img, TextureOptions::LINEAR);
        Self {
            mark,
            wordmark,
            wordmark_aspect: aspect,
            accent,
            dark,
        }
    }

    pub fn mark(&self) -> &TextureHandle {
        &self.mark
    }

    pub fn wordmark(&self) -> &TextureHandle {
        &self.wordmark
    }

    /// Width / height of the wordmark texture.
    pub fn wordmark_aspect(&self) -> f32 {
        self.wordmark_aspect
    }

    /// Like ailiak.com BrandLogo: mark box tracks `--accent`; wordmark follows theme.
    pub fn sync(&mut self, accent: Color32, dark: bool) {
        if self.accent != accent {
            self.mark
                .set(rasterize_mark(accent), TextureOptions::LINEAR);
            self.accent = accent;
        }
        if self.dark != dark {
            let (wm_img, aspect) = load_wordmark_png(dark);
            self.wordmark.set(wm_img, TextureOptions::LINEAR);
            self.wordmark_aspect = aspect;
            self.dark = dark;
        }
    }
}

fn color_hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn rasterize_mark(accent: Color32) -> ColorImage {
    let hex = color_hex(accent);
    // Prefer fill= attributes — usvg CSS class support is uneven for this asset.
    let svg = MARK_SVG
        .replace(
            "fill=\"#169f84\"\n     fill-opacity=\"0.85\"",
            &format!("fill=\"{hex}\"\n     fill-opacity=\"0.85\""),
        )
        .replace(
            "fill=\"#169f84\" fill-opacity=\"0.85\"",
            &format!("fill=\"{hex}\" fill-opacity=\"0.85\""),
        );
    // High-DPI mark so LINEAR downscale stays sharp at ~28px.
    svg_tex::rasterize_svg(&svg, 128)
}

fn load_wordmark_png(dark: bool) -> (ColorImage, f32) {
    let bytes = if dark {
        WORDMARK_DARK_PNG
    } else {
        WORDMARK_LIGHT_PNG
    };
    let img = image::load_from_memory(bytes)
        .expect("ailiak wordmark png")
        .to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let aspect = img.width() as f32 / img.height().max(1) as f32;
    (
        ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
        aspect,
    )
}
