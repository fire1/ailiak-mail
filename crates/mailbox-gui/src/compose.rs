//! Reader-card compose + Talanoa-style inline reply dock + attachments.

use std::path::PathBuf;

use egui::{Align2, Color32, Frame, Key, Rect, RichText, Sense, Ui, Vec2};
use egui_phosphor::regular as ph;

use mailbox_core::types::Message;
use mailbox_core::{reply_references, reply_subject};

use crate::compose_webview::{plain_to_editor_html, quote_plain_to_html, ComposeFmtState};
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeMode {
    New,
    Reply,
    ReplyAll,
    #[allow(dead_code)]
    Forward,
}

#[derive(Debug, Clone)]
pub struct ComposeAttachment {
    pub path: PathBuf,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ComposeDraft {
    pub mode: ComposeMode,
    /// `true` = reply dock under the original; `false` = full reader-card composer.
    pub inline: bool,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    /// Plain-text mirror of the WYSIWYG body (from the editor webview).
    pub body: String,
    /// HTML body from the contenteditable editor.
    pub body_html: String,
    /// Bumped when `body_html` must be pushed into the editor webview.
    pub editor_rev: u64,
    /// Live format state under the caret (for toolbar highlights).
    pub fmt: ComposeFmtState,
    /// Appended after `body` on send (classic mail quote).
    pub quote_on_send: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub show_cc: bool,
    pub show_bcc: bool,
    pub attachments: Vec<ComposeAttachment>,
    /// Display name for “Reply to …”.
    pub reply_to_name: String,
    /// Account From shown in the dock.
    pub from_email: String,
    /// Inline dock expanded (Talanoa focus grow).
    pub dock_focused: bool,
    /// Message this reply was opened against (dismiss dock if selection changes).
    pub reply_source_id: Option<mailbox_core::types::MessageId>,
}

impl ComposeDraft {
    pub fn new_blank() -> Self {
        Self {
            mode: ComposeMode::New,
            inline: false,
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            subject: String::new(),
            body: String::new(),
            body_html: String::new(),
            editor_rev: 1,
            fmt: ComposeFmtState::default(),
            quote_on_send: String::new(),
            in_reply_to: None,
            references: None,
            show_cc: false,
            show_bcc: false,
            attachments: Vec::new(),
            reply_to_name: String::new(),
            from_email: String::new(),
            dock_focused: false,
            reply_source_id: None,
        }
    }

    pub fn is_reply(&self) -> bool {
        matches!(self.mode, ComposeMode::Reply | ComposeMode::ReplyAll)
    }

    pub fn body_for_send(&self) -> String {
        let mut out = self.body.trim_end().to_string();
        if !self.quote_on_send.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&self.quote_on_send);
        }
        out
    }

    /// HTML for SMTP `multipart/alternative`, including the reply quote.
    pub fn body_html_for_send(&self) -> String {
        let mut html = self.body_html.trim().to_string();
        if html_body_is_empty(&html) {
            if !self.body.trim().is_empty() {
                html = plain_to_editor_html(&self.body);
            } else {
                html.clear();
            }
        }
        if !self.quote_on_send.is_empty() {
            html.push_str(&quote_plain_to_html(&self.quote_on_send));
        }
        html
    }

    /// Seed the editor from plain text (e.g. promoting the chat dock).
    pub fn set_plain_body(&mut self, plain: String) {
        self.body = plain;
        self.body_html = plain_to_editor_html(&self.body);
        self.editor_rev = self.editor_rev.saturating_add(1).max(1);
    }

    pub fn body_is_empty(&self) -> bool {
        self.body.trim().is_empty() && html_body_is_empty(&self.body_html)
    }

    pub fn from_reply(msg: &Message, all: bool, account_email: &str) -> Self {
        let self_l = account_email.trim().to_lowercase();
        let to = if all {
            let mut addrs = Vec::new();
            let from = format_addr(&msg.envelope.from_name, &msg.envelope.from_addr);
            if !addr_is_self(&msg.envelope.from_addr, &self_l) {
                addrs.push(from);
            }
            for a in msg.to.iter().chain(msg.cc.iter()) {
                if !addr_contains_self(a, &self_l) && !addrs.iter().any(|x| same_email(x, a)) {
                    addrs.push(a.clone());
                }
            }
            addrs.join(", ")
        } else {
            format_addr(&msg.envelope.from_name, &msg.envelope.from_addr)
        };
        let reply_to_name = if msg.envelope.from_name.trim().is_empty() {
            msg.envelope.from_addr.clone()
        } else {
            msg.envelope.from_name.trim().to_string()
        };
        Self {
            mode: if all {
                ComposeMode::ReplyAll
            } else {
                ComposeMode::Reply
            },
            inline: true,
            to,
            cc: String::new(),
            bcc: String::new(),
            subject: reply_subject(&msg.envelope.subject),
            body: String::new(),
            body_html: String::new(),
            editor_rev: 1,
            fmt: ComposeFmtState::default(),
            quote_on_send: quote_body(msg),
            in_reply_to: Some(msg.envelope.message_id.clone()),
            references: Some(reply_references(
                &msg.envelope.references,
                &msg.envelope.message_id,
            )),
            show_cc: all && !msg.cc.is_empty(),
            show_bcc: false,
            attachments: Vec::new(),
            reply_to_name,
            from_email: account_email.trim().to_string(),
            dock_focused: true,
            reply_source_id: Some(msg.envelope.id.clone()),
        }
    }

    pub fn parse_recipients(field: &str) -> Vec<String> {
        field
            .split([',', ';'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    pub fn pick_attachments(&mut self) {
        let files = rfd::FileDialog::new()
            .set_title("Attach files")
            .pick_files();
        if let Some(paths) = files {
            for path in paths {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "file".into());
                if self.attachments.iter().any(|a| a.path == path) {
                    continue;
                }
                self.attachments.push(ComposeAttachment { path, name });
            }
        }
    }
}

fn format_addr(name: &str, addr: &str) -> String {
    if name.trim().is_empty() {
        addr.trim().to_string()
    } else {
        format!("{} <{}>", name.trim(), addr.trim())
    }
}

fn addr_is_self(addr: &str, self_l: &str) -> bool {
    addr.trim().to_lowercase() == *self_l
}

fn addr_contains_self(raw: &str, self_l: &str) -> bool {
    extract_email(raw).eq_ignore_ascii_case(self_l)
}

fn same_email(a: &str, b: &str) -> bool {
    extract_email(a).eq_ignore_ascii_case(&extract_email(b))
}

fn extract_email(raw: &str) -> String {
    let s = raw.trim();
    if let Some(start) = s.find('<') {
        if let Some(end) = s.find('>') {
            return s[start + 1..end].trim().to_lowercase();
        }
    }
    s.to_lowercase()
}

fn quote_body(msg: &Message) -> String {
    let when = msg.envelope.date.format("%Y-%m-%d %H:%M UTC");
    let who = format_addr(&msg.envelope.from_name, &msg.envelope.from_addr);
    let mut out = String::from("\n\n");
    out.push_str(&format!("On {when}, {who} wrote:\n"));
    for line in msg.body_text.lines().take(80) {
        out.push_str("> ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn html_body_is_empty(html: &str) -> bool {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace('\u{a0}', " ")
        .replace("&nbsp;", " ")
        .chars()
        .all(|c| c.is_whitespace())
}

fn fmt_btn(ui: &mut Ui, glyph: &str, tip: &str, active: bool) -> egui::Response {
    let size = Vec2::splat(28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let mute = ui.visuals().weak_text_color();
    let accent = theme::accent(ui.ctx());
    if active {
        ui.painter()
            .rect_filled(rect, 6.0, accent.linear_multiply(0.22));
    } else if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, 6.0, ui.visuals().widgets.hovered.bg_fill);
    }
    paint_glyph(ui, rect.shrink(6.0), glyph, if active { accent } else { mute });
    resp.on_hover_text(tip)
}

fn paint_format_toolbar(ui: &mut Ui, draft: &ComposeDraft, result: &mut ComposeUiResult) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if fmt_btn(ui, ph::TEXT_B, "Bold (Ctrl+B)", draft.fmt.bold).clicked() {
            result.exec = Some("bold");
        }
        if fmt_btn(ui, ph::TEXT_ITALIC, "Italic (Ctrl+I)", draft.fmt.italic).clicked() {
            result.exec = Some("italic");
        }
        if fmt_btn(ui, ph::TEXT_UNDERLINE, "Underline (Ctrl+U)", draft.fmt.underline).clicked()
        {
            result.exec = Some("underline");
        }
        if fmt_btn(ui, ph::TEXT_STRIKETHROUGH, "Strikethrough", draft.fmt.strike).clicked() {
            result.exec = Some("strikeThrough");
        }
        ui.add_space(4.0);
        if fmt_btn(ui, ph::LIST_BULLETS, "Bullet list", draft.fmt.ul).clicked() {
            result.exec = Some("insertUnorderedList");
        }
        if fmt_btn(ui, ph::LIST_NUMBERS, "Numbered list", draft.fmt.ol).clicked() {
            result.exec = Some("insertOrderedList");
        }
        if fmt_btn(ui, ph::TEXT_T_SLASH, "Clear formatting", false).clicked() {
            result.exec = Some("removeFormat");
        }
    });
}

fn paint_glyph(ui: &Ui, rect: egui::Rect, glyph: &str, color: Color32) {
    let size = rect.width().min(rect.height());
    ui.painter().text(
        ui.painter().round_pos_to_pixels(rect.center()),
        Align2::CENTER_CENTER,
        glyph,
        theme::icon_font(size),
        color,
    );
}

fn icon_btn(ui: &mut Ui, glyph: &str, tip: &str) -> egui::Response {
    let size = Vec2::splat(28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    // Follow Ui visuals so inline (forced light on white reader) and floating
    // (dark paper) both keep readable icons.
    let mute = ui.visuals().weak_text_color();
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, 6.0, ui.visuals().widgets.hovered.bg_fill);
    }
    paint_glyph(ui, rect.shrink(6.0), glyph, mute);
    resp.on_hover_text(tip)
}

/// Accent Send control — plane glyph via Phosphor font (not Button label).
fn send_button(ui: &mut Ui, enabled: bool, sending: bool) -> egui::Response {
    let accent = theme::accent(ui.ctx());
    let label = if sending { "Sending…" } else { "Send" };
    let icon = 14.0;
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
        Color32::WHITE,
    );
    let pad = egui::vec2(12.0, 6.0);
    let size = egui::vec2(
        pad.x * 2.0 + icon + 6.0 + galley.size().x,
        (pad.y * 2.0 + icon.max(galley.size().y)).max(28.0),
    );
    let sense = if enabled && !sending {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let fill = if !enabled || sending {
        accent.linear_multiply(0.45)
    } else if resp.is_pointer_button_down_on() {
        accent.linear_multiply(0.85)
    } else if resp.hovered() {
        accent.linear_multiply(0.92)
    } else {
        accent
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    let icon_c = rect.left_center() + egui::vec2(pad.x + icon * 0.5, 0.0);
    let icon_rect = egui::Rect::from_center_size(
        ui.painter().round_pos_to_pixels(icon_c),
        Vec2::splat(icon),
    );
    paint_glyph(ui, icon_rect, ph::PAPER_PLANE_TILT, Color32::WHITE);
    ui.painter().galley(
        egui::pos2(
            icon_rect.right() + 6.0,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        Color32::WHITE,
    );
    resp.on_hover_text(if sending { "Sending…" } else { "Send" })
}

/// Result of composing UI for one frame.
#[derive(Debug, Default)]
pub struct ComposeUiResult {
    pub close: bool,
    pub send: bool,
    pub open_full: bool,
    /// Screen rect for the WYSIWYG editor webview (None when there is no room).
    pub editor_bounds: Option<Rect>,
    /// `document.execCommand` name requested by the format toolbar.
    pub exec: Option<&'static str>,
}

fn paint_attachment_chips(ui: &mut Ui, draft: &mut ComposeDraft) {
    if draft.attachments.is_empty() {
        return;
    }
    let mute = theme::mute(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        let mut remove: Option<usize> = None;
        for (i, att) in draft.attachments.iter().enumerate() {
            egui::Frame::none()
                .fill(theme::chip(ui.ctx()))
                .rounding(12.0)
                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&att.name).small());
                        if icon_btn(ui, ph::X, "Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            let _ = mute;
        }
        if let Some(i) = remove {
            draft.attachments.remove(i);
        }
    });
    ui.add_space(4.0);
}

/// Full composer painted inside the mail body / reader card.
pub fn show_reader_compose(
    ui: &mut Ui,
    draft: &mut ComposeDraft,
    sending: bool,
) -> ComposeUiResult {
    let mut result = ComposeUiResult::default();
    let mute = theme::reader_mute();
    let ink = theme::reader_ink();
    let field_fill = Color32::from_rgb(0xf3, 0xf4, 0xf6);

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(compose_title(draft))
                .strong()
                .size(18.0)
                .color(ink),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_btn(ui, ph::X, "Discard").clicked() {
                result.close = true;
            }
            if !draft.from_email.is_empty() {
                ui.label(
                    RichText::new(format!("from {}", draft.from_email))
                        .small()
                        .color(mute),
                );
            }
        });
    });
    ui.add_space(8.0);

    let label_w = 56.0;
    let field = |ui: &mut Ui, value: &mut String| {
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(ui.available_width())
                .frame(false),
        );
    };

    ui.horizontal(|ui| {
        ui.add_sized(
            [label_w, 20.0],
            egui::Label::new(RichText::new("To").color(mute)),
        );
        Frame::none()
            .fill(field_fill)
            .rounding(8.0)
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                field(ui, &mut draft.to);
            });
        if !draft.show_cc && ui.small_button("Cc").clicked() {
            draft.show_cc = true;
        }
        if !draft.show_bcc && ui.small_button("Bcc").clicked() {
            draft.show_bcc = true;
        }
    });
    if draft.show_cc {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_sized(
                [label_w, 20.0],
                egui::Label::new(RichText::new("Cc").color(mute)),
            );
            Frame::none()
                .fill(field_fill)
                .rounding(8.0)
                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                .show(ui, |ui| {
                    field(ui, &mut draft.cc);
                });
        });
    }
    if draft.show_bcc {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_sized(
                [label_w, 20.0],
                egui::Label::new(RichText::new("Bcc").color(mute)),
            );
            Frame::none()
                .fill(field_fill)
                .rounding(8.0)
                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                .show(ui, |ui| {
                    field(ui, &mut draft.bcc);
                });
        });
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_sized(
            [label_w, 20.0],
            egui::Label::new(RichText::new("Subject").color(mute)),
        );
        Frame::none()
            .fill(field_fill)
            .rounding(8.0)
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                field(ui, &mut draft.subject);
            });
    });
    ui.add_space(6.0);
    paint_attachment_chips(ui, draft);
    paint_format_toolbar(ui, draft, &mut result);
    ui.add_space(4.0);

    let action_h = 40.0;
    let body_h = (ui.available_height() - action_h - 4.0).max(120.0);
    let (editor_rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), body_h), Sense::hover());
    ui.painter().rect_filled(editor_rect, 10.0, field_fill);
    result.editor_bounds = Some(editor_rect);

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if icon_btn(ui, ph::PLUS, "Attach files").clicked() {
            draft.pick_attachments();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can = !sending && !draft.to.trim().is_empty();
            if send_button(ui, can, sending).clicked() {
                result.send = true;
            }
            if ui.button("Discard").clicked() {
                result.close = true;
            }
        });
    });
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        result.close = true;
    }
    result
}

/// Talanoa-style reply dock — caller allocates a clipped rect; we fill it opaquely.
/// Must not expand the parent: everything is capped to `ui.max_rect()`.
pub fn show_inline_compose(
    ui: &mut Ui,
    draft: &mut ComposeDraft,
    sending: bool,
    _window_height: f32,
) -> ComposeUiResult {
    let mut result = ComposeUiResult::default();
    let mute = theme::reader_mute();
    let accent = theme::accent(ui.ctx());
    let paper = theme::reader_card(ui.ctx());
    // Dock always sits on white reader paper.
    let input_fill = Color32::from_rgb(0xf3, 0xf4, 0xf6);

    // Opaque fill; stay inside the caller-allocated dock rect.
    let dock = ui.max_rect();
    ui.painter().rect_filled(dock, 0.0, paper);
    ui.set_clip_rect(dock);

    // Fixed header band so the input card can never paint over chip / “Reply to”.
    let header_h: f32 = if draft.attachments.is_empty() {
        52.0
    } else {
        78.0
    };
    let width = dock.width();
    let (header_rect, _) =
        ui.allocate_exact_size(Vec2::new(width, header_h.min(dock.height() * 0.4)), Sense::hover());
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(header_rect), |ui| {
        ui.set_clip_rect(header_rect);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let subj = if draft.subject.is_empty() {
                "Reply".to_string()
            } else {
                let t = draft.subject.chars().take(42).collect::<String>();
                if draft.subject.chars().count() > 42 {
                    format!("{t}…")
                } else {
                    t
                }
            };
            egui::Frame::none()
                .fill(theme::select(ui.ctx()))
                .rounding(14.0)
                .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("↩  {subj}"))
                            .small()
                            .color(accent)
                            .strong(),
                    );
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_btn(ui, ph::X, "Discard reply").clicked() {
                    result.close = true;
                }
                if !draft.from_email.is_empty() {
                    ui.label(
                        RichText::new(format!("from {}", draft.from_email))
                            .small()
                            .color(mute),
                    );
                }
            });
        });
        ui.add_space(2.0);
        let reply_label = if draft.reply_to_name.is_empty() {
            "Reply".to_string()
        } else {
            format!("Reply to {}", draft.reply_to_name)
        };
        ui.label(RichText::new(&reply_label).small().color(mute));
        paint_attachment_chips(ui, draft);
    });

    // Soft input card — leftover dock height only; format + send stay inside.
    let action_h = 36.0;
    let card_h = ui.available_height().max(0.0);
    let (card_rect, _) = ui.allocate_exact_size(Vec2::new(width, card_h), Sense::hover());
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(card_rect), |ui| {
        ui.set_clip_rect(card_rect);
        Frame::none()
            .fill(input_fill)
            .rounding(16.0)
            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            .show(ui, |ui| {
                let gap = ui.spacing().item_spacing.y + 2.0;
                paint_format_toolbar(ui, draft, &mut result);
                let edit_h = (ui.available_height() - action_h - gap).max(28.0);
                let (editor_rect, editor_resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), edit_h), Sense::click());
                ui.painter().rect_filled(editor_rect, 8.0, input_fill);
                result.editor_bounds = Some(editor_rect);
                if editor_resp.clicked() || editor_resp.has_focus() {
                    draft.dock_focused = true;
                } else if draft.body_is_empty() && draft.attachments.is_empty() {
                    // Collapse when empty and not interacting — webview focus is opaque to egui.
                }

                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    if icon_btn(ui, ph::PLUS, "Attach files").clicked() {
                        draft.pick_attachments();
                    }
                    if icon_btn(ui, ph::ARROWS_OUT, "Expand in reader").clicked() {
                        draft.inline = false;
                        if !draft.quote_on_send.is_empty()
                            && !draft.body.contains('>')
                            && !draft.body_html.contains("blockquote")
                        {
                            let q = std::mem::take(&mut draft.quote_on_send);
                            draft.body.push_str(&q);
                            if !draft.body_html.is_empty() {
                                draft.body_html.push_str(&quote_plain_to_html(&q));
                            } else {
                                draft.body_html = plain_to_editor_html(&draft.body);
                            }
                            draft.editor_rev = draft.editor_rev.saturating_add(1).max(1);
                        }
                        result.open_full = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_send = !sending
                            && !draft.to.trim().is_empty()
                            && (!draft.body_is_empty() || !draft.attachments.is_empty());
                        if send_button(ui, can_send, sending).clicked() {
                            result.send = true;
                        }
                    });
                });
            });
    });

    if ui.input(|i| i.key_pressed(Key::Escape)) {
        result.close = true;
    }
    result
}

fn compose_title(draft: &ComposeDraft) -> String {
    match draft.mode {
        ComposeMode::New => "New message".into(),
        ComposeMode::Reply => {
            if draft.subject.is_empty() {
                "Reply".into()
            } else {
                draft.subject.clone()
            }
        }
        ComposeMode::ReplyAll => "Reply all".into(),
        ComposeMode::Forward => "Forward".into(),
    }
}

/// People chat dock — same visual language as inline reply.
///
/// Height is capped; the editor is sized to leftover space so the Send toolbar
/// is never clipped (desired_rows alone used to push it past the dock edge).
pub fn show_chat_reply_dock(
    ui: &mut Ui,
    body: &mut String,
    focused: &mut bool,
    attachments: &mut Vec<ComposeAttachment>,
    window_height: f32,
    sending: bool,
    reply_to_name: &str,
    from_email: &str,
) -> ChatDockResult {
    let mut out = ChatDockResult::default();
    let mute = theme::mute(ui.ctx());
    let input_fill = if ui.visuals().dark_mode {
        theme::panel(ui.ctx())
    } else {
        Color32::from_rgb(0xf3, 0xf4, 0xf6)
    };
    // Caller allocates an exact dock rect — fill it; never expand past it.
    let dock = ui.max_rect();
    ui.set_clip_rect(dock);
    let _ = window_height; // height chosen by caller

    let header_h: f32 = if attachments.is_empty() { 28.0 } else { 56.0 };
    let width = dock.width();
    let (header_rect, _) =
        ui.allocate_exact_size(Vec2::new(width, header_h.min(dock.height() * 0.35)), Sense::hover());
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(header_rect), |ui| {
        ui.set_clip_rect(header_rect);
        ui.horizontal(|ui| {
            let label = if reply_to_name.is_empty() {
                "Reply".to_string()
            } else {
                format!("Reply to {reply_to_name}")
            };
            ui.label(RichText::new(label).small().color(mute));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !from_email.is_empty() {
                    ui.label(
                        RichText::new(format!("from {from_email}"))
                            .small()
                            .color(mute),
                    );
                }
            });
        });
        if !attachments.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let mut remove = None;
                for (i, att) in attachments.iter().enumerate() {
                    egui::Frame::none()
                        .fill(theme::chip(ui.ctx()))
                        .rounding(12.0)
                        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&att.name).small());
                                if icon_btn(ui, ph::X, "Remove").clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                }
                if let Some(i) = remove {
                    attachments.remove(i);
                }
            });
        }
    });

    let toolbar_h = 36.0;
    let card_h = ui.available_height().max(0.0);
    let (card_rect, _) = ui.allocate_exact_size(Vec2::new(width, card_h), Sense::hover());
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(card_rect), |ui| {
        ui.set_clip_rect(card_rect);
        Frame::none()
            .fill(input_fill)
            .rounding(16.0)
            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                let gap = ui.spacing().item_spacing.y + 2.0;
                let edit_h = (ui.available_height() - toolbar_h - gap).max(28.0);
                let hint = if reply_to_name.is_empty() {
                    "Write a reply…".to_string()
                } else {
                    format!("Reply to {reply_to_name}…")
                };
                let edit = egui::TextEdit::multiline(body)
                    .desired_width(ui.available_width())
                    .desired_rows(if *focused { 3 } else { 2 })
                    .frame(false)
                    .hint_text(hint);
                let resp = ui.add_sized([ui.available_width(), edit_h], edit);
                if resp.gained_focus() || resp.has_focus() {
                    *focused = true;
                } else if resp.lost_focus() && body.trim().is_empty() && attachments.is_empty() {
                    *focused = false;
                }
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    if icon_btn(ui, ph::PLUS, "Attach files").clicked() {
                        if let Some(paths) = rfd::FileDialog::new()
                            .set_title("Attach files")
                            .pick_files()
                        {
                            for path in paths {
                                let name = path
                                    .file_name()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "file".into());
                                if !attachments.iter().any(|a| a.path == path) {
                                    attachments.push(ComposeAttachment { path, name });
                                }
                            }
                        }
                    }
                    if icon_btn(ui, ph::ARROWS_OUT, "Expand in reader").clicked() {
                        out.open_full = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can =
                            !sending && (!body.trim().is_empty() || !attachments.is_empty());
                        if send_button(ui, can, sending).clicked() {
                            out.send = true;
                        }
                    });
                });
            });
    });
    out
}

#[derive(Debug, Default)]
pub struct ChatDockResult {
    pub send: bool,
    pub open_full: bool,
}

/// Guess a MIME type from file extension.
pub fn mime_for_path(path: &std::path::Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
    .into()
}
