//! Safe structured mail reader (HTML → blocks) with opt-in remote images.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use egui::{ColorImage, RichText, Sense, TextureHandle, TextureOptions, Ui};
use mailbox_core::oauth::open_url;
use mailbox_core::{safe_url, BodyBlock, InlineSeg, MailBody};

use crate::theme;

pub enum ImgMsg {
    Ready { url: String, rgba: ColorImage },
    Miss { url: String },
}

pub struct MailImageCache {
    by_url: HashMap<String, TextureHandle>,
    pending: HashSet<String>,
    failed: HashSet<String>,
    tx: Sender<ImgMsg>,
    rx: Receiver<ImgMsg>,
}

impl Default for MailImageCache {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            by_url: HashMap::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            tx,
            rx,
        }
    }
}

impl MailImageCache {
    pub fn clear(&mut self) {
        self.by_url.clear();
        self.pending.clear();
        self.failed.clear();
        // Drain any in-flight results so they don't reappear for the next message.
        while self.rx.try_recv().is_ok() {}
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ImgMsg::Ready { url, rgba } => {
                    self.pending.remove(&url);
                    let tex = ctx.load_texture(
                        format!("mail-img-{}", url_hash(&url)),
                        rgba,
                        TextureOptions::LINEAR,
                    );
                    self.by_url.insert(url, tex);
                }
                ImgMsg::Miss { url } => {
                    self.pending.remove(&url);
                    self.failed.insert(url);
                }
            }
            ctx.request_repaint();
        }
    }

    fn request(&mut self, url: &str) {
        if self.by_url.contains_key(url)
            || self.pending.contains(url)
            || self.failed.contains(url)
        {
            return;
        }
        // Only https remote images (http allowed but discouraged — still fetch).
        let Some(safe) = safe_url(url) else {
            self.failed.insert(url.to_string());
            return;
        };
        if !(safe.starts_with("https://") || safe.starts_with("http://")) {
            self.failed.insert(url.to_string());
            return;
        }
        self.pending.insert(url.to_string());
        let tx = self.tx.clone();
        let url_owned = url.to_string();
        thread::spawn(move || {
            let result = (|| -> Option<ColorImage> {
                let client = reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(12))
                    .user_agent("AiliakMail/0.1")
                    .build()
                    .ok()?;
                let resp = client
                    .get(&safe)
                    .header("Referer", "")
                    .send()
                    .ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                // Cap download size (~2 MiB) to limit tracking bombs / memory.
                let bytes = resp.bytes().ok()?;
                if bytes.len() > 2 * 1024 * 1024 {
                    return None;
                }
                let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
                if img.width() > 4096 || img.height() > 4096 {
                    return None;
                }
                let size = [img.width() as usize, img.height() as usize];
                Some(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
            })();
            match result {
                Some(rgba) => {
                    let _ = tx.send(ImgMsg::Ready {
                        url: url_owned,
                        rgba,
                    });
                }
                None => {
                    let _ = tx.send(ImgMsg::Miss { url: url_owned });
                }
            }
        });
    }
}

fn url_hash(url: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    h.finish()
}

/// Render a pre-built structured body. `load_images` gates remote image fetches.
pub fn show_body(
    ui: &mut Ui,
    body: &MailBody,
    load_images: bool,
    images: &mut MailImageCache,
) {
    if body.has_image_threats() {
        let n = body.image_threats.len();
        egui::Frame::none()
            .fill(theme::reader_chip())
            .rounding(8.0)
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{n} tracking-like image(s) blocked for privacy (always, even if this sender is allowed)."
                    ))
                    .small()
                    .color(theme::reader_mute()),
                );
            });
        ui.add_space(10.0);
    } else if body.has_remote_images && !load_images {
        egui::Frame::none()
            .fill(theme::reader_chip())
            .rounding(8.0)
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "This message includes remote images. They are blocked until you allow this sender (privacy / tracking).",
                    )
                    .small()
                    .color(theme::reader_mute()),
                );
            });
        ui.add_space(10.0);
    }

    ui.style_mut().url_in_tooltip = true;

    for block in &body.blocks {
        match block {
            BodyBlock::Paragraph(spans) => {
                show_spans(ui, spans, 13.0, false);
                ui.add_space(8.0);
            }
            BodyBlock::Heading { level, spans } => {
                let size = match level {
                    1 => 18.0,
                    2 => 16.0,
                    _ => 14.0,
                };
                show_spans(ui, spans, size, true);
                ui.add_space(8.0);
            }
            BodyBlock::Bullet(spans) => {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("•").color(theme::reader_mute()));
                    ui.vertical(|ui| {
                        show_spans(ui, spans, 13.0, false);
                    });
                });
                ui.add_space(4.0);
            }
            BodyBlock::Quote(spans) => {
                egui::Frame::none()
                    .stroke(egui::Stroke::new(2.0_f32, theme::reader_mute()))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        show_spans(ui, spans, 13.0, false);
                    });
                ui.add_space(8.0);
            }
            BodyBlock::Rule => {
                ui.separator();
                ui.add_space(8.0);
            }
            BodyBlock::ImageBlocked { alt } => {
                ui.label(
                    RichText::new(format!("[Image blocked — {alt}]"))
                        .italics()
                        .small()
                        .color(theme::reader_mute()),
                );
                ui.add_space(6.0);
            }
            BodyBlock::Image { alt, url } => {
                images.request(url);
                if let Some(tex) = images.by_url.get(url) {
                    let max_w = (ui.available_width() - 8.0).max(64.0);
                    let size = tex.size_vec2();
                    let scale = (max_w / size.x).min(1.0);
                    let disp = size * scale;
                    ui.add(egui::Image::new((tex.id(), disp)).sense(Sense::hover()))
                        .on_hover_text(alt);
                } else if images.failed.contains(url) {
                    ui.label(
                        RichText::new(format!("[Image failed — {alt}]"))
                            .small()
                            .color(theme::reader_mute()),
                    );
                } else {
                    ui.label(
                        RichText::new(format!("Loading image… ({alt})"))
                            .small()
                            .color(theme::reader_mute()),
                    );
                }
                ui.add_space(8.0);
            }
        }
    }
}

fn show_spans(ui: &mut Ui, spans: &[InlineSeg], size: f32, strong: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for seg in spans {
            match seg {
                InlineSeg::Text(t) => {
                    let mut rt = RichText::new(t).size(size).color(theme::reader_ink());
                    if strong {
                        rt = rt.strong();
                    }
                    ui.label(rt);
                }
                InlineSeg::Link { text, url } => {
                    let resp = ui.add(
                        egui::Label::new(
                            RichText::new(text)
                                .size(size)
                                .color(theme::accent(ui.ctx()))
                                .underline(),
                        )
                        .sense(Sense::click()),
                    );
                    if resp.clicked() {
                        if let Some(safe) = safe_url(url) {
                            let _ = open_url(&safe);
                        }
                    }
                    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(url);
                }
            }
        }
    });
}
