//! Profile pictures: local override → Gravatar → colored initials.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;

use egui::{
    Align2, Color32, ColorImage, Context, FontId, Pos2, Response, Rounding, Sense, TextureHandle,
    TextureOptions, Ui, Vec2,
};
use md5::{Digest, Md5};
use sha2::Sha256;

use crate::theme;

/// Background result for a Gravatar (or failed) fetch.
pub enum AvatarMsg {
    Ready {
        email: String,
        rgba: ColorImage,
    },
    /// No Gravatar — keep showing initials (and stop re-fetching).
    Miss {
        email: String,
    },
}

#[derive(Clone)]
enum Cached {
    Image(TextureHandle),
    /// Confirmed no remote/local image.
    InitialsOnly,
}

pub struct AvatarCache {
    by_email: HashMap<String, Cached>,
    pending: HashSet<String>,
    /// Emails we already tried to fill via OAuth profile photo this session.
    tried_oauth_photo: HashSet<String>,
}

impl Default for AvatarCache {
    fn default() -> Self {
        Self {
            by_email: HashMap::new(),
            pending: HashSet::new(),
            tried_oauth_photo: HashSet::new(),
        }
    }
}

impl AvatarCache {
    pub fn normalize_email(email: &str) -> String {
        email.trim().to_lowercase()
    }

    pub fn local_path(avatars_dir: &Path, email: &str) -> PathBuf {
        let key = Self::normalize_email(email);
        let mut h = Sha256::new();
        h.update(key.as_bytes());
        let hex = format!("{:x}", h.finalize());
        avatars_dir.join(format!("{hex}.png"))
    }

    pub fn gravatar_hash(email: &str) -> String {
        let key = Self::normalize_email(email);
        let mut h = Md5::new();
        h.update(key.as_bytes());
        format!("{:x}", h.finalize())
    }

    pub fn initials(name: &str, email: &str) -> String {
        let source = if !name.trim().is_empty() {
            name.trim()
        } else {
            email.trim()
        };
        let mut letters = String::new();
        for part in source.split(|c: char| c.is_whitespace() || c == '.' || c == '@' || c == '-' || c == '_')
        {
            if let Some(c) = part.chars().find(|c| c.is_alphanumeric()) {
                letters.push(c.to_ascii_uppercase());
                if letters.chars().count() >= 2 {
                    break;
                }
            }
        }
        if letters.is_empty() {
            "?".into()
        } else {
            letters
        }
    }

    /// Pastel fill derived from email (mint-adjacent, readable with dark/light ink).
    pub fn color_for(email: &str) -> Color32 {
        let key = Self::normalize_email(email);
        let mut h = Md5::new();
        h.update(key.as_bytes());
        let dig = h.finalize();
        let r = 0x5a + (dig[0] % 0x50);
        let g = 0x8a + (dig[1] % 0x50);
        let b = 0x7a + (dig[2] % 0x50);
        Color32::from_rgb(r, g, b)
    }

    pub fn apply_msg(&mut self, ctx: &Context, msg: AvatarMsg) {
        match msg {
            AvatarMsg::Ready { email, rgba } => {
                let key = Self::normalize_email(&email);
                self.pending.remove(&key);
                let tex = ctx.load_texture(
                    format!("avatar-{key}"),
                    rgba,
                    TextureOptions::LINEAR,
                );
                self.by_email.insert(key, Cached::Image(tex));
            }
            AvatarMsg::Miss { email } => {
                let key = Self::normalize_email(&email);
                self.pending.remove(&key);
                // Don't replace a photo that arrived from Google / local override.
                if matches!(self.by_email.get(&key), Some(Cached::Image(_))) {
                    return;
                }
                self.by_email.insert(key, Cached::InitialsOnly);
            }
        }
    }

    /// Drop cached entry so the next paint reloads from disk / Gravatar.
    pub fn invalidate(&mut self, email: &str) {
        let key = Self::normalize_email(email);
        self.by_email.remove(&key);
        self.pending.remove(&key);
        self.tried_oauth_photo.remove(&key);
    }

    pub fn has_image(&self, email: &str) -> bool {
        matches!(
            self.by_email.get(&Self::normalize_email(email)),
            Some(Cached::Image(_))
        )
    }

    /// Returns true once per email per session for OAuth photo attempts.
    pub fn begin_oauth_photo_try(&mut self, email: &str) -> bool {
        let key = Self::normalize_email(email);
        if key.is_empty() || self.has_image(&key) || self.tried_oauth_photo.contains(&key) {
            return false;
        }
        self.tried_oauth_photo.insert(key);
        true
    }

    /// Save an image file as the local PNG override for `email`.
    pub fn set_local_override(avatars_dir: &Path, email: &str, source: &Path) -> anyhow::Result<()> {
        fs::create_dir_all(avatars_dir)?;
        let bytes = fs::read(source)?;
        let img = image::load_from_memory(&bytes)?.to_rgba8();
        let dest = Self::local_path(avatars_dir, email);
        img.save(&dest)?;
        Ok(())
    }

    pub fn clear_local_override(avatars_dir: &Path, email: &str) -> anyhow::Result<()> {
        let dest = Self::local_path(avatars_dir, email);
        if dest.exists() {
            fs::remove_file(&dest)?;
        }
        Ok(())
    }

    /// Download a remote profile photo (e.g. Google `picture` URL) into the local override cache.
    pub fn set_from_url(avatars_dir: &Path, email: &str, url: &str) -> anyhow::Result<()> {
        let url = url.trim();
        if url.is_empty() {
            anyhow::bail!("empty picture url");
        }
        fs::create_dir_all(avatars_dir)?;
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(12))
            .build()?;
        let resp = client
            .get(url)
            .send()?
            .error_for_status()
            .map_err(|e| anyhow::anyhow!("download profile photo: {e}"))?;
        let bytes = resp.bytes()?;
        let img = image::load_from_memory(&bytes)?.to_rgba8();
        let dest = Self::local_path(avatars_dir, email);
        img.save(&dest)?;
        Ok(())
    }

    /// Background: download `url`, save local override, then notify the UI.
    pub fn spawn_url_fetch(
        email: String,
        url: String,
        avatars_dir: PathBuf,
        tx: Sender<AvatarMsg>,
    ) {
        thread::spawn(move || {
            match Self::set_from_url(&avatars_dir, &email, &url)
                .ok()
                .and_then(|_| Self::load_rgba_file(&Self::local_path(&avatars_dir, &email)))
            {
                Some(rgba) => {
                    let _ = tx.send(AvatarMsg::Ready { email, rgba });
                }
                None => {
                    let _ = tx.send(AvatarMsg::Miss { email });
                }
            }
        });
    }

    fn load_rgba_file(path: &Path) -> Option<ColorImage> {
        let bytes = fs::read(path).ok()?;
        let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        Some(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
    }

    fn spawn_gravatar(email: String, tx: Sender<AvatarMsg>) {
        thread::spawn(move || {
            let hash = Self::gravatar_hash(&email);
            let url = format!("https://www.gravatar.com/avatar/{hash}?d=404&s=128");
            let result = (|| -> Option<ColorImage> {
                let client = reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(8))
                    .build()
                    .ok()?;
                let resp = client.get(&url).send().ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                let bytes = resp.bytes().ok()?;
                let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                Some(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
            })();
            match result {
                Some(rgba) => {
                    let _ = tx.send(AvatarMsg::Ready { email, rgba });
                }
                None => {
                    let _ = tx.send(AvatarMsg::Miss { email });
                }
            }
        });
    }

    /// Resolve display for an email; may kick off a background Gravatar fetch.
    pub fn resolve(
        &mut self,
        ctx: &Context,
        avatars_dir: &Path,
        email: &str,
        name: &str,
        avatar_tx: &Sender<AvatarMsg>,
    ) -> AvatarView {
        let key = Self::normalize_email(email);
        let letters = Self::initials(name, email);
        let fill = Self::color_for(email);

        if key.is_empty() {
            return AvatarView::Initials { letters, fill };
        }

        if let Some(cached) = self.by_email.get(&key) {
            return match cached {
                Cached::Image(tex) => AvatarView::Image(tex.clone()),
                Cached::InitialsOnly => AvatarView::Initials { letters, fill },
            };
        }

        // Local override (sync — small file).
        let local = Self::local_path(avatars_dir, &key);
        if local.exists() {
            if let Some(rgba) = Self::load_rgba_file(&local) {
                let tex = ctx.load_texture(
                    format!("avatar-{key}"),
                    rgba,
                    TextureOptions::LINEAR,
                );
                self.by_email.insert(key.clone(), Cached::Image(tex.clone()));
                return AvatarView::Image(tex);
            }
        }

        if !self.pending.contains(&key) {
            self.pending.insert(key.clone());
            Self::spawn_gravatar(key, avatar_tx.clone());
        }

        AvatarView::Initials { letters, fill }
    }
}

#[derive(Clone)]
pub enum AvatarView {
    Image(TextureHandle),
    Initials { letters: String, fill: Color32 },
}

/// Draw a circular avatar; returns the click response for the allocated rect.
pub fn avatar_button(
    ui: &mut Ui,
    view: &AvatarView,
    size: f32,
    selected: bool,
    tooltip: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let center = rect.center();
    let radius = size * 0.5;
    let painter = ui.painter();

    if selected {
        painter.circle_stroke(
            center,
            radius + 1.5,
            egui::Stroke::new(2.0_f32, theme::accent(ui.ctx())),
        );
    } else if response.hovered() {
        painter.circle_stroke(
            center,
            radius + 1.0,
            egui::Stroke::new(1.5_f32, theme::mute(ui.ctx())),
        );
    }

    match view {
        AvatarView::Image(tex) => {
            let image_rect = egui::Rect::from_center_size(center, Vec2::splat(size - 2.0));
            egui::Image::new((tex.id(), image_rect.size()))
                .rounding(Rounding::same((size - 2.0) * 0.5))
                .paint_at(ui, image_rect);
        }
        AvatarView::Initials { letters, fill } => {
            painter.circle_filled(center, radius - 1.0, *fill);
            let font_size = (size * 0.38).clamp(9.0, 16.0);
            let ink = if fill.r() as u16 + fill.g() as u16 + fill.b() as u16 > 400 {
                Color32::from_rgb(0x2a, 0x35, 0x31)
            } else {
                Color32::from_rgb(0xf4, 0xf8, 0xf6)
            };
            painter.text(
                center,
                Align2::CENTER_CENTER,
                letters,
                FontId::proportional(font_size),
                ink,
            );
        }
    }

    if !tooltip.is_empty() {
        response.on_hover_text(tooltip)
    } else {
        response
    }
}

/// Non-interactive rounded-square avatar for message list rows.
pub fn avatar_widget(ui: &mut Ui, view: &AvatarView, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let center = rect.center();
    let rounding = Rounding::same((size * 0.22).clamp(4.0, 8.0));
    let painter = ui.painter();

    match view {
        AvatarView::Image(tex) => {
            egui::Image::new((tex.id(), rect.size()))
                .rounding(rounding)
                .paint_at(ui, rect);
        }
        AvatarView::Initials { letters, fill } => {
            painter.rect_filled(rect, rounding, *fill);
            let font_size = (size * 0.4).clamp(8.0, 12.0);
            let ink = if fill.r() as u16 + fill.g() as u16 + fill.b() as u16 > 400 {
                Color32::from_rgb(0x2a, 0x35, 0x31)
            } else {
                Color32::from_rgb(0xf4, 0xf8, 0xf6)
            };
            painter.text(
                Pos2::new(center.x, center.y),
                Align2::CENTER_CENTER,
                letters,
                FontId::proportional(font_size),
                ink,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(
            AvatarCache::normalize_email("  Ada@Example.COM "),
            "ada@example.com"
        );
    }

    #[test]
    fn gravatar_hash_matches_known_vector() {
        // Gravatar docs: MD5 of "myemailaddress@example.com"
        assert_eq!(
            AvatarCache::gravatar_hash("MyEmailAddress@example.com"),
            "0bc83cb571cd1c50ba6f3e8a78ef1346"
        );
    }

    #[test]
    fn initials_from_name_and_email() {
        assert_eq!(AvatarCache::initials("Ada Lovelace", ""), "AL");
        assert_eq!(AvatarCache::initials("", "bob.smith@example.com"), "BS");
        assert_eq!(AvatarCache::initials("", ""), "?");
    }

    #[test]
    fn color_for_is_stable() {
        let a = AvatarCache::color_for("ada@example.com");
        let b = AvatarCache::color_for("  ADA@example.com ");
        assert_eq!(a, b);
    }

    #[test]
    fn local_override_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.png");
        // 1×1 opaque PNG
        let png = image::RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]));
        png.save(&src).unwrap();
        AvatarCache::set_local_override(dir.path(), "ada@example.com", &src).unwrap();
        let dest = AvatarCache::local_path(dir.path(), "ada@example.com");
        assert!(dest.exists());
        AvatarCache::clear_local_override(dir.path(), "ada@example.com").unwrap();
        assert!(!dest.exists());
    }
}
