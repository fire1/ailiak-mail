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
    /// App feedback — To is locked to the author address.
    Feedback,
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
    /// Confirmed To recipients (shown as removable tags).
    pub to: Vec<String>,
    /// In-progress To typing (not yet a tag).
    pub to_input: String,
    pub cc: Vec<String>,
    pub cc_input: String,
    pub bcc: Vec<String>,
    pub bcc_input: String,
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
            to: Vec::new(),
            to_input: String::new(),
            cc: Vec::new(),
            cc_input: String::new(),
            bcc: Vec::new(),
            bcc_input: String::new(),
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

    pub fn to_locked(&self) -> bool {
        self.mode == ComposeMode::Feedback
    }

    pub fn new_feedback(to: &str) -> Self {
        let mut draft = Self::new_blank();
        draft.mode = ComposeMode::Feedback;
        let addr = to.trim();
        if !addr.is_empty() {
            draft.to.push(addr.to_string());
        }
        draft.subject = "AiliakMail feedback".into();
        draft
    }

    /// Flush typing buffers into tags, then return To list for SMTP.
    pub fn recipients_to(&mut self) -> Vec<String> {
        commit_recipient_input(&mut self.to, &mut self.to_input);
        self.to.clone()
    }

    pub fn recipients_cc(&mut self) -> Vec<String> {
        commit_recipient_input(&mut self.cc, &mut self.cc_input);
        self.cc.clone()
    }

    pub fn recipients_bcc(&mut self) -> Vec<String> {
        commit_recipient_input(&mut self.bcc, &mut self.bcc_input);
        self.bcc.clone()
    }

    pub fn body_for_send(&self) -> String {
        let placeholder = mailbox_core::missing_plain_placeholder();
        let mut out = self.body.trim_end().to_string();
        // Never send our reader stub as the message body.
        if out.trim() == placeholder {
            out.clear();
        }
        // Webview may leave plain empty while HTML has the real content.
        if out.trim().is_empty() && !html_body_is_empty(&self.body_html) {
            out = mailbox_core::html_to_plain(&self.body_html);
        }
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
        let placeholder = mailbox_core::missing_plain_placeholder();
        let mut html = self.body_html.trim().to_string();
        if html_body_is_empty(&html) {
            let plain = self.body.trim();
            if !plain.is_empty() && plain != placeholder {
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
            addrs
        } else {
            vec![format_addr(
                &msg.envelope.from_name,
                &msg.envelope.from_addr,
            )]
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
            to_input: String::new(),
            cc: Vec::new(),
            cc_input: String::new(),
            bcc: Vec::new(),
            bcc_input: String::new(),
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

    pub fn pick_attachments(&mut self) {
        let Some(paths) = crate::file_pick::pick_files("Attach files", &[]) else {
            return;
        };
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

fn strip_recipient_newlines(input: &mut String) {
    if input.contains('\n') || input.contains('\r') {
        *input = input
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .collect();
    }
}

fn commit_recipient_input(tags: &mut Vec<String>, input: &mut String) {
    strip_recipient_newlines(input);
    let raw = input.trim().trim_matches([',', ';']).trim();
    if raw.is_empty() {
        input.clear();
        return;
    }
    if raw.contains('@') && !tags.iter().any(|t| same_email(t, raw)) {
        tags.push(raw.to_string());
    }
    input.clear();
}

fn add_recipient_tag(tags: &mut Vec<String>, input: &mut String, tag: String) {
    if tag.trim().is_empty() {
        return;
    }
    if !tags.iter().any(|t| same_email(t, &tag)) {
        tags.push(tag);
    }
    input.clear();
}

fn suggestion_label(name: &str, addr: &str) -> String {
    let name = name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case(addr) {
        addr.to_string()
    } else {
        format!("{name}  ·  {addr}")
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
    html_looks_empty(html)
}

/// True when HTML has no visible text (e.g. empty editor `<br>`).
pub fn html_looks_empty(html: &str) -> bool {
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
    // Above — default tips drop into the native body webview.
    hover_text_above(resp, tip)
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
        ui.add_space(6.0);
        if fmt_btn(ui, ph::IMAGE, "Insert image into body", false).clicked() {
            result.pick_inline_image = true;
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

fn icon_btn(ui: &mut Ui, glyph: &str) -> egui::Response {
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
    resp
}

/// Attach files — plus glyph + “Attach” label.
fn attach_button(ui: &mut Ui) -> egui::Response {
    let mute = ui.visuals().weak_text_color();
    let ink = ui.visuals().text_color();
    let pad = egui::vec2(8.0, 4.0);
    let icon = 14.0;
    let galley = ui.painter().layout_no_wrap(
        "Attach".to_owned(),
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
        ink,
    );
    let size = egui::vec2(
        pad.x * 2.0 + icon + ui.spacing().icon_spacing + galley.size().x,
        (pad.y * 2.0 + icon.max(galley.size().y)).max(28.0),
    );
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        theme::select(ui.ctx())
    } else if resp.hovered() {
        theme::chip(ui.ctx())
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 6.0, fill);
    }
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + pad.x, rect.center().y - icon * 0.5),
        Vec2::splat(icon),
    );
    paint_glyph(ui, icon_rect, ph::PLUS, mute);
    ui.painter().galley(
        egui::pos2(
            icon_rect.right() + ui.spacing().icon_spacing,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        Color32::PLACEHOLDER,
    );
    hover_text_below(resp, "Attach files")
}

/// Discard / panel close (X) — single accent-tinted plate (no gray+stroke double layer).
pub(crate) fn close_icon_btn(ui: &mut Ui, tip: &str) -> egui::Response {
    let accent = theme::accent(ui.ctx());
    let size = Vec2::splat(28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    // Reader card is always light paper — mix white ← accent for one clear plate.
    let t = if resp.is_pointer_button_down_on() {
        0.32
    } else if resp.hovered() {
        0.22
    } else {
        0.14
    };
    let fill = Color32::from_rgb(
        ((255.0 * (1.0 - t)) + (f32::from(accent.r()) * t)) as u8,
        ((255.0 * (1.0 - t)) + (f32::from(accent.g()) * t)) as u8,
        ((255.0 * (1.0 - t)) + (f32::from(accent.b()) * t)) as u8,
    );
    ui.painter().rect_filled(rect, 6.0, fill);
    paint_glyph(ui, rect.shrink(6.0), ph::X, accent);
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
    hover_text_below(
        resp,
        if sending {
            "Sending…"
        } else if cfg!(target_os = "macos") {
            "Send (⌘Enter)"
        } else {
            "Send (Ctrl+Enter)"
        },
    )
}

/// Shown in the reader card after SMTP succeeds (compose form is cleared).
#[derive(Debug, Clone)]
pub struct ComposeSentNotice {
    pub to_line: String,
    pub subject: String,
}

impl ComposeSentNotice {
    pub fn from_draft(draft: &ComposeDraft) -> Self {
        let mut to = draft.to.clone();
        let typing = draft.to_input.trim();
        if !typing.is_empty() && !to.iter().any(|t| same_email(t, typing)) {
            to.push(typing.to_string());
        }
        let to_line = match to.len() {
            0 => "your recipients".into(),
            1 => short_recipient(&to[0]),
            2 => format!("{} and {}", short_recipient(&to[0]), short_recipient(&to[1])),
            n => format!("{} and {} others", short_recipient(&to[0]), n - 1),
        };
        let subject = {
            let s = draft.subject.trim();
            if s.is_empty() {
                "(no subject)".into()
            } else {
                s.chars().take(72).collect()
            }
        };
        Self { to_line, subject }
    }
}

fn short_recipient(raw: &str) -> String {
    let s = raw.trim();
    if let Some(start) = s.find('<') {
        let name = s[..start].trim().trim_matches('"');
        if !name.is_empty() {
            return name.to_string();
        }
        if let Some(end) = s.find('>') {
            return s[start + 1..end].trim().to_string();
        }
    }
    s.to_string()
}

/// Full-card confirmation after a successful send.
pub fn show_compose_sent(ui: &mut Ui, notice: &ComposeSentNotice) -> ComposeSentAction {
    let mut action = ComposeSentAction::None;
    let mute = theme::reader_mute();
    let ink = theme::reader_ink();
    let accent = theme::accent(ui.ctx());

    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.18);
        // Soft check plate
        let mark = ui.available_width().min(56.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(mark), Sense::hover());
        ui.painter().circle_filled(rect.center(), mark * 0.42, accent.linear_multiply(0.18));
        paint_glyph(ui, rect.shrink(mark * 0.22), ph::CHECK, accent);

        ui.add_space(18.0);
        ui.label(
            RichText::new("Message sent")
                .size(26.0)
                .strong()
                .color(ink),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("On its way to {}.", notice.to_line))
                .size(15.0)
                .color(mute),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("“{}”", notice.subject))
                .size(14.0)
                .italics()
                .color(mute),
        );
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            // Center the button row.
            let pad = ((ui.available_width() - 280.0) * 0.5).max(0.0);
            ui.add_space(pad);
            if ui
                .add(
                    egui::Button::new(RichText::new("Write another").size(14.0))
                        .rounding(8.0)
                        .min_size(Vec2::new(132.0, 34.0)),
                )
                .clicked()
            {
                action = ComposeSentAction::WriteAnother;
            }
            ui.add_space(10.0);
            let fill = accent;
            if ui
                .add(
                    egui::Button::new(RichText::new("Done").size(14.0).color(Color32::WHITE))
                        .fill(fill)
                        .rounding(8.0)
                        .min_size(Vec2::new(96.0, 34.0)),
                )
                .clicked()
            {
                action = ComposeSentAction::Done;
            }
        });
    });
    action
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComposeSentAction {
    #[default]
    None,
    WriteAnother,
    Done,
}

/// Result of composing UI for one frame.
#[derive(Debug, Default)]
pub struct ComposeUiResult {
    pub close: bool,
    pub send: bool,
    pub open_full: bool,
    /// Screen rect for the WYSIWYG editor webview (None when there is no room).
    pub editor_bounds: Option<Rect>,
    /// Egui popups/menus that must stay visible above the native body webview.
    /// The host shrinks `editor_bounds` so these rects are not covered.
    pub egui_overlays: Vec<Rect>,
    /// `document.execCommand` name requested by the format toolbar.
    pub exec: Option<&'static str>,
    /// Insert raw HTML at the caret (e.g. inline `<img>`).
    pub insert_html: Option<String>,
    /// Open a file picker and insert the image into the body.
    pub pick_inline_image: bool,
    /// Clicked the body area — focus the WYSIWYG webview.
    pub focus_editor: bool,
    /// User focused the To / Subject egui fields (body should release sticky focus).
    pub header_focused: bool,
}

/// Tooltip placed above the widget so it does not fall under the native body webview.
fn hover_text_above(resp: egui::Response, tip: &str) -> egui::Response {
    if tip.is_empty() || !resp.enabled || !resp.hovered() {
        return resp;
    }
    let anchor = resp.rect.center_top() - Vec2::new(0.0, 4.0);
    egui::Area::new(resp.id.with("tip_above"))
        .order(egui::Order::Tooltip)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(anchor)
        .interactable(false)
        .show(&resp.ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(ui.spacing().tooltip_width);
                ui.label(tip);
            });
        });
    resp
}

/// Tooltip placed below the widget (for chrome under the editor).
fn hover_text_below(resp: egui::Response, tip: &str) -> egui::Response {
    if tip.is_empty() || !resp.enabled || !resp.hovered() {
        return resp;
    }
    let anchor = resp.rect.center_bottom() + Vec2::new(0.0, 4.0);
    egui::Area::new(resp.id.with("tip_below"))
        .order(egui::Order::Tooltip)
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(anchor)
        .interactable(false)
        .show(&resp.ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(ui.spacing().tooltip_width);
                ui.label(tip);
            });
        });
    resp
}

/// Push the native webview away from egui overlays (recipient suggestions, etc.).
pub fn inset_editor_bounds_for_overlays(mut bounds: Rect, overlays: &[Rect]) -> Option<Rect> {
    for o in overlays {
        if !bounds.intersects(*o) {
            continue;
        }
        let inter = bounds.intersect(*o);
        if inter.height() <= 0.0 || inter.width() <= 0.0 {
            continue;
        }
        // Dropdowns sit on the top edge; shrink from the top. Bottom-edge
        // overlays (rare) shrink from the bottom.
        if inter.center().y <= bounds.center().y {
            bounds.min.y = inter.max.y + 2.0;
        } else {
            bounds.max.y = inter.min.y - 2.0;
        }
    }
    if bounds.width() < 40.0 || bounds.height() < 28.0 {
        None
    } else {
        Some(bounds)
    }
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
                        if hover_text_above(icon_btn(ui, ph::X), "Remove").clicked() {
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

/// Email shown inside a recipient pill (matches compose reference chips).
fn tag_label(tag: &str) -> String {
    let e = extract_email(tag);
    if e.is_empty() {
        tag.trim().to_string()
    } else {
        e
    }
}

fn paint_recipient_tag(ui: &mut Ui, tag: &str, mute: Color32, locked: bool) -> bool {
    let mut remove = false;
    let label = tag_label(tag);
    egui::Frame::none()
        .fill(Color32::from_rgb(0xee, 0xf0, 0xef))
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    RichText::new(label)
                        .size(13.0)
                        .color(theme::reader_ink()),
                );
                if !locked {
                    let (x_rect, x_resp) =
                        ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
                    paint_glyph(ui, x_rect, ph::X, mute);
                    if x_resp.on_hover_text("Remove").clicked() {
                        remove = true;
                    }
                }
            });
        });
    remove
}

fn recipient_matches(
    input: &str,
    tags: &[String],
    hints: &[(String, String)],
) -> Vec<(String, String)> {
    let q_chars = input.trim().chars().count();
    if q_chars < 3 {
        return Vec::new();
    }
    let q = input.trim().to_lowercase();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (name, addr) in hints {
        if addr.is_empty() || tags.iter().any(|t| same_email(t, addr)) {
            continue;
        }
        let key = addr.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        if addr.to_lowercase().contains(&q) || name.to_lowercase().contains(&q) {
            out.push((name.clone(), addr.clone()));
            if out.len() >= 8 {
                break;
            }
        }
    }
    out
}

/// Clean recipient row: one horizontal line — tags + input (+ trailing Cc/Bcc).
/// Returns `(field_focused, tab_wants_next, suggestion_overlay)`.
fn paint_clean_recipient_row(
    ui: &mut Ui,
    tags: &mut Vec<String>,
    input: &mut String,
    mute: Color32,
    hints: &[(String, String)],
    field_key: &'static str,
    hint: &str,
    locked: bool,
    autofocus: bool,
    trailing_w: f32,
    trailing: impl FnOnce(&mut Ui),
) -> (bool, bool, Option<Rect>) {
    // Single row only — never wrap the text field onto a second line.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.set_min_height(28.0);

        // Stable ids so Subject→To Tab can focus the field from outside this row.
        let edit_id = egui::Id::new(("mailbox_compose_rcpt", field_key));
        let popup_id = egui::Id::new(("mailbox_compose_rcpt_popup", field_key));
        let sel_id = egui::Id::new(("mailbox_compose_rcpt_sel", field_key));

        if !locked && (input.ends_with(',') || input.ends_with(';')) {
            commit_recipient_input(tags, input);
        }

        let matches = if locked {
            Vec::new()
        } else {
            recipient_matches(input, tags, hints)
        };

        let mut sel = ui
            .ctx()
            .data(|d| d.get_temp::<usize>(sel_id).unwrap_or(0));
        if !matches.is_empty() {
            sel = sel.min(matches.len() - 1);
        } else {
            sel = 0;
        }

        // Consume Enter/Tab *before* TextEdit so suggestions can be confirmed by keyboard.
        let focused = ui.memory(|m| m.has_focus(edit_id));
        let mut picked: Option<String> = None;
        let mut tab_next = false;
        if !locked && focused && !matches.is_empty() {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)) {
                sel = (sel + 1) % matches.len();
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)) {
                sel = if sel == 0 {
                    matches.len() - 1
                } else {
                    sel - 1
                };
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                picked = Some(matches[sel].1.clone());
            }
            // Tab confirms suggestion then advances (To → body), skipping the toolbar.
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab)) {
                picked = Some(matches[sel].1.clone());
                tab_next = true;
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                ui.memory_mut(|m| m.close_popup());
            }
        } else if !locked && focused && matches.is_empty() {
            if !input.trim().is_empty()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter))
            {
                commit_recipient_input(tags, input);
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab)) {
                if !input.trim().is_empty() {
                    commit_recipient_input(tags, input);
                }
                tab_next = true;
            }
        }

        ui.ctx().data_mut(|d| d.insert_temp(sel_id, sel));

        let mut remove: Option<usize> = None;
        for (i, tag) in tags.iter().enumerate() {
            if paint_recipient_tag(ui, tag, mute, locked) {
                remove = Some(i);
            }
        }
        if let Some(i) = remove {
            tags.remove(i);
        }

        let te = if locked {
            None
        } else {
            // Fill leftover width on this same row (small floor so it never wraps under tags).
            let edit_w = (ui.available_width() - trailing_w).max(48.0);
            let te = ui.add(
                egui::TextEdit::singleline(input)
                    .id(edit_id)
                    .desired_width(edit_w)
                    .frame(false)
                    .hint_text(RichText::new(hint).size(13.0).color(mute))
                    .text_color(theme::reader_ink()),
            );
            strip_recipient_newlines(input);
            if autofocus {
                te.request_focus();
            }
            Some(te)
        };

        trailing(ui);

        let Some(te) = te else {
            if let Some(choice) = picked {
                add_recipient_tag(tags, input, choice);
            }
            return (false, false, None);
        };

        let input_focused = te.has_focus() || focused;

        let popup_was_open = ui.memory(|m| m.is_popup_open(popup_id));
        // Keep popup alive across the click that steals TextEdit focus.
        let show_popup = !matches.is_empty() && (te.has_focus() || popup_was_open || focused);

        if te.has_focus() && !matches.is_empty() {
            ui.memory_mut(|m| m.open_popup(popup_id));
        }

        // Narrow custom Area — egui's popup_below_widget forces full TextEdit width.
        const SUGGEST_W: f32 = 300.0;
        let mut overlay: Option<Rect> = None;
        if show_popup && !matches.is_empty() {
            let anchor = te.rect.left_bottom();
            let area = egui::Area::new(popup_id)
                .order(egui::Order::Foreground)
                .fixed_pos(anchor)
                .pivot(Align2::LEFT_TOP)
                .default_width(SUGGEST_W)
                .show(ui.ctx(), |ui| {
                    // Use the Area's own style (tracks shell light/dark) instead of the
                    // reader card's light-forced style so dark mode keeps a dark popup.
                    egui::Frame::popup(ui.style())
                        .show(ui, |ui| {
                            ui.set_min_width(220.0);
                            ui.set_max_width(SUGGEST_W);
                            ui.spacing_mut().item_spacing.y = 2.0;
                            for (i, (name, addr)) in matches.iter().enumerate() {
                                let line = suggestion_label(name, addr);
                                let fill = if i == sel {
                                    theme::select(ui.ctx())
                                } else {
                                    Color32::TRANSPARENT
                                };
                                let resp = ui.add(
                                    egui::Button::new(RichText::new(&line).size(13.0))
                                        .fill(fill)
                                        .stroke(egui::Stroke::NONE)
                                        .rounding(4.0)
                                        .min_size(Vec2::new(ui.available_width(), 0.0)),
                                );
                                if resp.clicked() {
                                    picked = Some(addr.clone());
                                }
                            }
                        })
                        .inner
                });
            overlay = Some(area.response.rect);
            let close_outside =
                te.clicked_elsewhere() && area.response.clicked_elsewhere();
            if ui.input(|i| i.key_pressed(Key::Escape)) || close_outside {
                ui.memory_mut(|m| m.close_popup());
                overlay = None;
            }
        } else if popup_was_open && matches.is_empty() {
            ui.memory_mut(|m| m.close_popup());
        }

        if let Some(choice) = picked {
            add_recipient_tag(tags, input, choice);
            ui.memory_mut(|m| m.close_popup());
            if tab_next {
                ui.memory_mut(|m| m.surrender_focus(edit_id));
            } else {
                ui.ctx()
                    .memory_mut(|m| m.request_focus(edit_id));
            }
            return (true, tab_next, None);
        }
        (input_focused, tab_next, overlay)
    })
    .inner
}

/// Full composer painted inside the mail body / reader card.
///
/// Clean header: placeholder Subject + To/Cc/Bcc (no gray field chrome), then body.
/// `recipient_hints` is `(name, email)` from prior mail for suggestions.
pub fn show_reader_compose(
    ui: &mut Ui,
    draft: &mut ComposeDraft,
    sending: bool,
    recipient_hints: &[(String, String)],
    autofocus_to: &mut bool,
    autofocus_subject: &mut bool,
) -> ComposeUiResult {
    let mut result = ComposeUiResult::default();
    let mute = theme::reader_mute();
    let ink = theme::reader_ink();

    let card = ui.max_rect();
    ui.set_clip_rect(card);
    ui.set_max_width(card.width());

    // Minimal chrome: close + from (no boxed fields).
    ui.horizontal(|ui| {
        ui.set_max_width(ui.available_width());
        if close_icon_btn(ui, "Discard").clicked() {
            result.close = true;
        }
        // Reply opened full-size from the dock — offer shrink back under the mail.
        if draft.is_reply() && draft.reply_source_id.is_some() {
            ui.add_space(4.0);
            if hover_text_above(icon_btn(ui, ph::ARROWS_IN), "Shrink reply").clicked() {
                draft.inline = true;
                draft.dock_focused = true;
            }
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(compose_title(draft))
                .size(13.0)
                .color(mute),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Feedback: hide account email (To is also hidden below).
            if !draft.to_locked() && !draft.from_email.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(&draft.from_email).small().color(mute),
                    )
                    .truncate(),
                );
            }
        });
    });
    ui.add_space(14.0);

    // Subject — large placeholder, no label / fill.
    // Stable id for Tab order: Subject → To → body.
    let subject_id = egui::Id::new("mailbox_compose_subject");
    let to_edit_id = egui::Id::new(("mailbox_compose_rcpt", "to"));
    let subject_resp = ui.add(
        egui::TextEdit::singleline(&mut draft.subject)
            .id(subject_id)
            .desired_width(ui.available_width())
            .frame(false)
            .hint_text(
                RichText::new("Subject")
                    .size(22.0)
                    .color(Color32::from_rgb(0xb0, 0xb4, 0xb2)),
            )
            .text_color(ink)
            .font(egui::FontId::proportional(22.0)),
    );
    if *autofocus_subject {
        subject_resp.request_focus();
        *autofocus_subject = false;
    }
    if subject_resp.gained_focus() || subject_resp.has_focus() {
        result.header_focused = true;
    }
    ui.add_space(6.0);

    // To — tags + input; Cc/Bcc toggles stay at the end of this row.
    // Feedback keeps To locked + hidden (recipient is fixed to FEEDBACK_TO_ADDR).
    let to_locked = draft.to_locked();
    let to_hint = if to_locked {
        ""
    } else if draft.to.is_empty() && draft.to_input.is_empty() {
        "To"
    } else {
        ""
    };
    let mut open_cc = false;
    let mut open_bcc = false;
    let (to_focused, to_tab, to_overlay) = if to_locked {
        (false, false, None)
    } else {
        // "To" is placeholder-only — hide once there are tags or typing.
        ui.horizontal(|ui| {
            let mut w = 0.0;
            if !draft.show_cc {
                w += 28.0;
            }
            if !draft.show_bcc {
                w += 36.0;
            }
            paint_clean_recipient_row(
                ui,
                &mut draft.to,
                &mut draft.to_input,
                mute,
                recipient_hints,
                "to",
                to_hint,
                false,
                *autofocus_to,
                w,
                |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if !draft.show_cc
                        && ui
                            .add(
                                egui::Label::new(RichText::new("Cc").size(13.0).color(mute))
                                    .sense(Sense::click()),
                            )
                            .on_hover_text("Add Cc")
                            .clicked()
                    {
                        open_cc = true;
                    }
                    if !draft.show_bcc
                        && ui
                            .add(
                                egui::Label::new(RichText::new("Bcc").size(13.0).color(mute))
                                    .sense(Sense::click()),
                            )
                            .on_hover_text("Add Bcc")
                            .clicked()
                    {
                        open_bcc = true;
                    }
                },
            )
        })
        .inner
    };
    if to_focused {
        result.header_focused = true;
    }
    if let Some(r) = to_overlay {
        result.egui_overlays.push(r);
    }
    *autofocus_to = false;
    if open_cc {
        draft.show_cc = true;
    }
    if open_bcc {
        draft.show_bcc = true;
    }

    // Cc/Bcc stay click-to-open only — Tab never lands here (Subject → To → body).
    if !to_locked && draft.show_cc {
        ui.add_space(4.0);
        let (cc_focused, cc_tab, cc_overlay) = ui
            .horizontal(|ui| {
                ui.label(RichText::new("Cc").size(13.0).color(mute));
                ui.add_space(4.0);
                paint_clean_recipient_row(
                    ui,
                    &mut draft.cc,
                    &mut draft.cc_input,
                    mute,
                    recipient_hints,
                    "cc",
                    "Add Cc",
                    false,
                    open_cc,
                    0.0,
                    |_| {},
                )
            })
            .inner;
        if cc_focused {
            result.header_focused = true;
        }
        if let Some(r) = cc_overlay {
            result.egui_overlays.push(r);
        }
        // If focus somehow sits on Cc, Tab still advances to body.
        if cc_tab {
            result.focus_editor = true;
            result.header_focused = false;
        }
    }
    if !to_locked && draft.show_bcc {
        ui.add_space(4.0);
        let (bcc_focused, bcc_tab, bcc_overlay) = ui
            .horizontal(|ui| {
                ui.label(RichText::new("Bcc").size(13.0).color(mute));
                ui.add_space(4.0);
                paint_clean_recipient_row(
                    ui,
                    &mut draft.bcc,
                    &mut draft.bcc_input,
                    mute,
                    recipient_hints,
                    "bcc",
                    "Add Bcc",
                    false,
                    open_bcc,
                    0.0,
                    |_| {},
                )
            })
            .inner;
        if bcc_focused {
            result.header_focused = true;
        }
        if let Some(r) = bcc_overlay {
            result.egui_overlays.push(r);
        }
        if bcc_tab {
            result.focus_editor = true;
            result.header_focused = false;
        }
    }

    // Tab order: (none) → Subject → To → body. Always skip Cc/Bcc and the toolbar.
    let subject_focused = subject_resp.has_focus();
    if to_tab {
        result.focus_editor = true;
        result.header_focused = false;
    } else if subject_focused
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab))
    {
        if to_locked {
            result.focus_editor = true;
            result.header_focused = false;
        } else {
            ui.memory_mut(|m| m.request_focus(to_edit_id));
            result.header_focused = true;
        }
    } else if !subject_focused
        && !to_focused
        && !result.focus_editor
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab))
    {
        // First Tab from nowhere / other chrome → Subject.
        ui.memory_mut(|m| m.request_focus(subject_id));
        result.header_focused = true;
    }

    ui.add_space(10.0);
    let sep_w = ui.available_width();
    let (sep_rect, _) = ui.allocate_exact_size(Vec2::new(sep_w, 1.0), Sense::hover());
    ui.painter().hline(
        sep_rect.x_range(),
        sep_rect.center().y,
        egui::Stroke::new(1.0_f32, Color32::from_rgb(0xe5, 0xe7, 0xe5)),
    );
    ui.add_space(8.0);

    paint_attachment_chips(ui, draft);
    paint_format_toolbar(ui, draft, &mut result);
    ui.add_space(4.0);

    let action_h = 40.0;
    let body_h = (ui.available_height() - action_h - 4.0).max(120.0);
    let body_w = ui.available_width().min(card.width()).max(0.0);
    // Clickable underlay — webview sits above, but egui still sees presses in gaps.
    let (editor_rect, editor_resp) =
        ui.allocate_exact_size(Vec2::new(body_w, body_h), Sense::click());
    // White underlay so it matches the clean card (webview paints on top).
    ui.painter()
        .rect_filled(editor_rect, 0.0, Color32::WHITE);
    if editor_resp.clicked()
        || editor_resp.gained_focus()
        || (editor_resp.hovered() && ui.input(|i| i.pointer.primary_pressed()))
    {
        result.focus_editor = true;
    }
    result.editor_bounds = Some(editor_rect);

    ui.add_space(4.0);
    // Lock width to the card so Send/Discard cannot expand past the visible edge.
    let action_w = ui.available_width().min(card.width()).max(0.0);
    ui.allocate_ui_with_layout(
        Vec2::new(action_w, 32.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_width(action_w);
            if attach_button(ui).clicked() {
                draft.pick_attachments();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can = !sending
                    && (!draft.to.is_empty() || !draft.to_input.trim().is_empty());
                if send_button(ui, can, sending).clicked() {
                    result.send = true;
                }
                if ui.button("Discard").clicked() {
                    result.close = true;
                }
            });
        },
    );
    // Ctrl+Enter (Linux/Windows/macOS) or Cmd+Enter (macOS) to send.
    let can_send =
        !sending && (!draft.to.is_empty() || !draft.to_input.trim().is_empty());
    if can_send
        && ui.input(|i| {
            (i.modifiers.ctrl || i.modifiers.mac_cmd) && i.key_pressed(Key::Enter)
        })
    {
        result.send = true;
    }
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
                // RTL: close is rightmost; Full size sits just before it.
                if close_icon_btn(ui, "Discard reply").clicked() {
                    result.close = true;
                }
                if hover_text_below(icon_btn(ui, ph::ARROWS_OUT), "Full size").clicked() {
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
                    if attach_button(ui).clicked() {
                        draft.pick_attachments();
                    }
                    // Only when the dock is expanded/focused — already-shrunk dock hides this.
                    if draft.dock_focused {
                        if hover_text_below(icon_btn(ui, ph::ARROWS_IN), "Shrink reply")
                            .clicked()
                        {
                            draft.dock_focused = false;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_send = !sending
                            && (!draft.to.is_empty() || !draft.to_input.trim().is_empty())
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
        ComposeMode::Feedback => "Feedback".into(),
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
                                if hover_text_above(icon_btn(ui, ph::X), "Remove").clicked() {
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
                    if attach_button(ui).clicked() {
                        if let Some(paths) = crate::file_pick::pick_files("Attach files", &[]) {
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
                    if hover_text_below(icon_btn(ui, ph::ARROWS_OUT), "Expand in reader").clicked()
                    {
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
