//! Company logos from [Logo.dev](https://www.logo.dev) CDN (`img.logo.dev`).
//!
//! **Caching:** memory → on-disk PNG under the app data `logos/` dir → CDN only
//! when neither has the mark. HTTP 404s are remembered as `.miss` files so we
//! do not re-hit the API for known-missing domains. Transient network errors
//! are session-only misses (retry next launch).
//!
//! Keys (never commit): `LOGO_DEV_PUBLISHABLE_KEY` / `LOGO_DEV_SECRET_KEY`
//! (also accepts `AILIAKMAIL_LOGO_DEV_*`). Publishable (`pk_`) is used for CDN
//! image URLs; secret (`sk_`) is reserved for server-side API calls.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use image::imageops::FilterType;

/// CDN request size (retina → 2×). We Lanczos-downscale to [`TEX_PX`] for sharp UI marks.
const FETCH_PX: u32 = 128;
/// Texture uploaded to egui (displayed ~20–28 CSS px).
const TEX_PX: u32 = 64;
/// Current on-disk PNG suffix (`{domain}.{CACHE_VER}.png`).
const CACHE_VER: &str = "v2";

enum LogoMsg {
    Ready { key: String, rgba: ColorImage },
    /// Permanent miss (404) — also written as a `.miss` marker on disk.
    Miss { key: String },
    /// Transient failure — memory only; next launch may retry the CDN.
    SoftMiss { key: String },
}

#[derive(Clone)]
enum Cached {
    Image(TextureHandle),
    Miss,
}

pub struct LogoCache {
    by_key: HashMap<String, Cached>,
    pending: HashSet<String>,
    tx: Sender<LogoMsg>,
    rx: Receiver<LogoMsg>,
}

impl Default for LogoCache {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            by_key: HashMap::new(),
            pending: HashSet::new(),
            tx,
            rx,
        }
    }
}

impl LogoCache {
    pub fn publishable_key() -> Option<String> {
        std::env::var("LOGO_DEV_PUBLISHABLE_KEY")
            .or_else(|_| std::env::var("AILIAKMAIL_LOGO_DEV_PUBLISHABLE_KEY"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Loaded for future Brand/Search API use; not required for CDN images.
    #[allow(dead_code)]
    pub fn secret_key() -> Option<String> {
        std::env::var("LOGO_DEV_SECRET_KEY")
            .or_else(|_| std::env::var("AILIAKMAIL_LOGO_DEV_SECRET_KEY"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn configured() -> bool {
        Self::publishable_key().is_some()
    }

    /// Map Stak / brand id → Logo.dev domain (or `None` if not fetchable).
    pub fn domain_for(brand_or_domain: &str) -> Option<String> {
        let id = brand_or_domain.trim().to_lowercase();
        if id.is_empty() {
            return None;
        }
        let domain = match id.as_str() {
            "google" => "google.com",
            "instagram" => "instagram.com",
            "linkedin" => "linkedin.com",
            "facebook" => "facebook.com",
            "meta" => "meta.com",
            "x" => "x.com",
            "github" => "github.com",
            "gitlab" => "gitlab.com",
            "discord" => "discord.com",
            "microsoft" => "microsoft.com",
            "teams" => "teams.microsoft.com",
            "apple" => "apple.com",
            "amazon" => "amazon.com",
            "aliexpress" => "aliexpress.com",
            "stripe" => "stripe.com",
            "notion" => "notion.so",
            "slack" => "slack.com",
            "dropbox" => "dropbox.com",
            "figma" => "figma.com",
            "svgator" => "svgator.com",
            "cursor" => "cursor.com",
            "warp" => "warp.dev",
            "openai" => "openai.com",
            "steam" => "steampowered.com",
            "reddit" => "reddit.com",
            "spotify" => "spotify.com",
            "paypal" => "paypal.com",
            "zoom" => "zoom.us",
            "whatsapp" => "whatsapp.com",
            "netflix" => "netflix.com",
            "telegram" => "telegram.org",
            "zed" => "zed.dev",
            "gnome" => "gnome.org",
            "abv" => "abv.bg",
            other if other.contains('.') && !other.contains(' ') => other,
            _ => return None,
        };
        Some(domain.to_string())
    }

    fn safe_stem(stem: &str) -> String {
        stem.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn cache_path(logos_dir: &Path, stem: &str) -> PathBuf {
        logos_dir.join(format!("{}.{CACHE_VER}.png", Self::safe_stem(stem)))
    }

    fn miss_path(logos_dir: &Path, stem: &str) -> PathBuf {
        logos_dir.join(format!("{}.{CACHE_VER}.miss", Self::safe_stem(stem)))
    }

    /// Current + legacy filenames we still accept as a local hit (no CDN).
    fn disk_candidates(logos_dir: &Path, stem: &str) -> Vec<PathBuf> {
        let safe = Self::safe_stem(stem);
        vec![
            logos_dir.join(format!("{safe}.{CACHE_VER}.png")),
            logos_dir.join(format!("{safe}.l.png")), // prior cache suffix
            logos_dir.join(format!("{safe}.png")),
        ]
    }

    fn cache_key(domain: &str) -> String {
        format!("{domain}|{CACHE_VER}")
    }

    pub fn poll(&mut self, ctx: &Context) {
        let mut got = false;
        while let Ok(msg) = self.rx.try_recv() {
            got = true;
            match msg {
                LogoMsg::Ready { key, rgba } => {
                    self.pending.remove(&key);
                    let tex = ctx.load_texture(
                        format!("logo-dev-{key}"),
                        rgba,
                        TextureOptions::LINEAR,
                    );
                    self.by_key.insert(key, Cached::Image(tex));
                }
                LogoMsg::Miss { key } | LogoMsg::SoftMiss { key } => {
                    self.pending.remove(&key);
                    self.by_key.insert(key, Cached::Miss);
                }
            }
        }
        if got {
            ctx.request_repaint();
        }
    }

    /// Return a Logo.dev texture when ready; kicks a background fetch **only**
    /// when memory and disk both lack the logo (and no in-flight fetch).
    pub fn texture(
        &mut self,
        ctx: &Context,
        logos_dir: &Path,
        brand_or_domain: &str,
        _dark: bool,
    ) -> Option<TextureHandle> {
        let domain = Self::domain_for(brand_or_domain)?;
        let key = Self::cache_key(&domain);

        if let Some(cached) = self.by_key.get(&key) {
            return match cached {
                Cached::Image(t) => Some(t.clone()),
                Cached::Miss => None,
            };
        }

        // Known CDN 404 — stay offline.
        if Self::miss_path(logos_dir, &domain).is_file() {
            self.by_key.insert(key, Cached::Miss);
            return None;
        }

        // Local PNG hit (current or legacy name) — never call the CDN.
        let current = Self::cache_path(logos_dir, &domain);
        for path in Self::disk_candidates(logos_dir, &domain) {
            if let Some(img) = load_processed_png(&path) {
                // Promote legacy filenames to the current cache path once.
                if path != current {
                    let _ = fs::create_dir_all(logos_dir);
                    let _ = fs::copy(&path, &current);
                }
                let _ = fs::remove_file(Self::miss_path(logos_dir, &domain));
                let tex = ctx.load_texture(
                    format!("logo-dev-{key}"),
                    img,
                    TextureOptions::LINEAR,
                );
                self.by_key.insert(key, Cached::Image(tex.clone()));
                return Some(tex);
            }
        }

        if self.pending.contains(&key) {
            return None;
        }
        let Some(token) = Self::publishable_key() else {
            return None;
        };

        self.pending.insert(key.clone());
        let tx = self.tx.clone();
        let logos_dir = logos_dir.to_path_buf();
        thread::Builder::new()
            .name("logo-dev-fetch".into())
            .spawn(move || match fetch_logo(&domain, &token, &logos_dir) {
                Ok(FetchOutcome::Image(img)) => {
                    let _ = tx.send(LogoMsg::Ready { key, rgba: img });
                }
                Ok(FetchOutcome::NotFound) => {
                    let _ = tx.send(LogoMsg::Miss { key });
                }
                Err(_) => {
                    let _ = tx.send(LogoMsg::SoftMiss { key });
                }
            })
            .ok();
        None
    }
}

fn load_processed_png(path: &Path) -> Option<ColorImage> {
    let bytes = fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    Some(rgba_to_color_image(&img))
}

enum FetchOutcome {
    Image(ColorImage),
    NotFound,
}

/// High-quality square fit into [`TEX_PX`] with transparent padding.
fn process_logo_rgba(src: image::RgbaImage) -> image::RgbaImage {
    let (w, h) = src.dimensions();
    if w == 0 || h == 0 {
        return image::RgbaImage::new(TEX_PX, TEX_PX);
    }
    // Fit inside TEX_PX×TEX_PX preserving aspect (letterbox).
    let scale = (TEX_PX as f32 / w as f32).min(TEX_PX as f32 / h as f32);
    let nw = ((w as f32) * scale).round().max(1.0) as u32;
    let nh = ((h as f32) * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(&src, nw, nh, FilterType::Lanczos3);
    let mut out = image::RgbaImage::from_pixel(TEX_PX, TEX_PX, image::Rgba([0, 0, 0, 0]));
    let ox = (TEX_PX.saturating_sub(nw)) / 2;
    let oy = (TEX_PX.saturating_sub(nh)) / 2;
    image::imageops::overlay(&mut out, &resized, ox as i64, oy as i64);
    out
}

fn rgba_to_color_image(img: &image::RgbaImage) -> ColorImage {
    let size = [img.width() as usize, img.height() as usize];
    ColorImage::from_rgba_unmultiplied(size, img.as_raw())
}

fn fetch_logo(domain: &str, token: &str, logos_dir: &Path) -> Result<FetchOutcome, String> {
    // Full-color PNG — avoid `theme=` (washes marks) and prefer a large source
    // then Lanczos to TEX_PX so 20–28px UI stays sharp.
    let url = format!(
        "https://img.logo.dev/{domain}?token={token}&format=png&size={FETCH_PX}&retina=true&fallback=404"
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .user_agent(concat!("AiliakMail/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().map_err(|e| e.to_string())?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        let _ = fs::create_dir_all(logos_dir);
        let _ = fs::write(LogoCache::miss_path(logos_dir, domain), b"");
        return Ok(FetchOutcome::NotFound);
    }
    if !resp.status().is_success() {
        return Err(format!("logo.dev HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        let _ = fs::create_dir_all(logos_dir);
        let _ = fs::write(LogoCache::miss_path(logos_dir, domain), b"");
        return Ok(FetchOutcome::NotFound);
    }
    let decoded = image::load_from_memory(&bytes)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let processed = process_logo_rgba(decoded);
    let _ = fs::create_dir_all(logos_dir);
    let path = LogoCache::cache_path(logos_dir, domain);
    processed
        .save(&path)
        .map_err(|e| format!("save logo cache {}: {e}", path.display()))?;
    let _ = fs::remove_file(LogoCache::miss_path(logos_dir, domain));
    Ok(FetchOutcome::Image(rgba_to_color_image(&processed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_brands_and_domains() {
        assert_eq!(
            LogoCache::domain_for("aliexpress").as_deref(),
            Some("aliexpress.com")
        );
        assert_eq!(
            LogoCache::domain_for("shopify.com").as_deref(),
            Some("shopify.com")
        );
        assert!(LogoCache::domain_for("not a brand").is_none());
    }

    #[test]
    fn process_preserves_square_size() {
        let src = image::RgbaImage::from_pixel(200, 100, image::Rgba([255, 0, 0, 255]));
        let out = process_logo_rgba(src);
        assert_eq!(out.dimensions(), (TEX_PX, TEX_PX));
    }

    #[test]
    fn disk_candidates_prefer_current_then_legacy() {
        let dir = Path::new("/tmp/ailiakmail-logos-test");
        let paths = LogoCache::disk_candidates(dir, "Example.COM");
        assert!(paths[0].ends_with("Example.COM.v2.png") || paths[0].to_string_lossy().contains("Example.COM.v2.png"));
        assert!(paths.iter().any(|p| p.to_string_lossy().ends_with(".l.png")));
        assert_eq!(LogoCache::safe_stem("a/b c.com"), "a_b_c.com");
    }
}
