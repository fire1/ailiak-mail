//! Phosphor glyph icons (font AA via egui text), tinted by paint color.

use egui::{Align2, Color32, FontId, Pos2, Response, Sense, Ui, Vec2};
use egui_phosphor::regular as ph;

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderIcon {
    Inbox,
    Starred,
    Snoozed,
    Important,
    Sent,
    Drafts,
    Archive,
    Spam,
    Trash,
    Folder,
}

impl FolderIcon {
    pub fn from_folder(path: &str, name: &str) -> Self {
        Self::from_role(mailbox_core::classify_mailbox(path, name))
    }

    pub fn from_role(role: mailbox_core::MailboxRole) -> Self {
        match role {
            mailbox_core::MailboxRole::Inbox => Self::Inbox,
            mailbox_core::MailboxRole::Starred => Self::Starred,
            mailbox_core::MailboxRole::Snoozed => Self::Snoozed,
            mailbox_core::MailboxRole::Important => Self::Important,
            mailbox_core::MailboxRole::Sent => Self::Sent,
            mailbox_core::MailboxRole::Drafts => Self::Drafts,
            mailbox_core::MailboxRole::Archive => Self::Archive,
            mailbox_core::MailboxRole::Spam => Self::Spam,
            mailbox_core::MailboxRole::Trash => Self::Trash,
            mailbox_core::MailboxRole::User => Self::Folder,
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Self::Inbox => ph::TRAY,
            Self::Starred => ph::STAR,
            Self::Snoozed => ph::CLOCK_AFTERNOON,
            Self::Important => ph::TAG,
            Self::Sent => ph::PAPER_PLANE_TILT,
            Self::Drafts => ph::NOTE_PENCIL,
            Self::Archive => ph::ARCHIVE,
            Self::Spam => ph::WARNING_CIRCLE,
            Self::Trash => ph::TRASH,
            Self::Folder => ph::FOLDER,
        }
    }
}

#[derive(Clone, Copy)]
pub enum ToolbarIcon {
    Sync,
    Compose,
    Reply,
    ReplyAll,
    Done,
    /// Mark as read (mid-list bulk / selection).
    MarkRead,
    Trash,
}

impl ToolbarIcon {
    fn glyph(self) -> &'static str {
        match self {
            Self::Sync => ph::LIGHTNING,
            Self::Compose => ph::NOTE_PENCIL,
            // Classic curved reply arrow; double bend = reply-all.
            Self::Reply => ph::ARROW_BEND_UP_LEFT,
            Self::ReplyAll => ph::ARROW_BEND_DOUBLE_UP_LEFT,
            Self::Done => ph::CHECK,
            // Same check mark as Done; mid-list uses it for bulk mark-read.
            Self::MarkRead => ph::CHECK,
            Self::Trash => ph::TRASH,
        }
    }
}

/// Icon-only toolbar control (Done / Trash / Mark read / …). Shows a spinner when `busy`.
pub fn toolbar_icon_button(
    ui: &mut Ui,
    kind: ToolbarIcon,
    tip: &str,
    enabled: bool,
    busy: bool,
) -> Response {
    let mute = theme::mute(ui.ctx());
    let size = Vec2::splat(28.0);
    let can_click = enabled && !busy;
    let sense = if can_click {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let fill = if !can_click {
        Color32::TRANSPARENT
    } else if response.is_pointer_button_down_on() {
        theme::select(ui.ctx())
    } else if response.hovered() {
        theme::chip(ui.ctx())
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 6.0, fill);
    }
    if busy {
        ui.put(rect, egui::Spinner::new().size(14.0));
        response.on_hover_text(format!("{tip} (working…)"))
    } else {
        let color = if enabled {
            mute
        } else {
            mute.linear_multiply(0.4)
        };
        paint_glyph(ui, rect.shrink(6.0), kind.glyph(), color);
        response.on_hover_text(tip)
    }
}

/// Remote-images opt-in: image glyph + “Load”. Caller hides this once images are on.
pub fn toolbar_load_images_button(ui: &mut Ui, enabled: bool, tip: &str) -> Response {
    let mute = theme::mute(ui.ctx());
    let ink = ui.visuals().text_color();
    let pad = egui::vec2(8.0, 4.0);
    let icon: f32 = 14.0;
    let label = "Load";
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::new(13.0, egui::FontFamily::Proportional),
        if enabled {
            ink
        } else {
            mute.linear_multiply(0.4)
        },
    );
    let size = egui::vec2(
        pad.x * 2.0 + icon + ui.spacing().icon_spacing + galley.size().x,
        (pad.y * 2.0 + icon.max(galley.size().y)).max(28.0),
    );
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if response.is_pointer_button_down_on() {
        theme::select(ui.ctx())
    } else if response.hovered() {
        theme::chip(ui.ctx())
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 6.0, fill);
    }
    let icon_color = if enabled {
        mute
    } else {
        mute.linear_multiply(0.4)
    };
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + pad.x, rect.center().y - icon * 0.5),
        Vec2::splat(icon),
    );
    paint_glyph(ui, icon_rect, ph::IMAGE, icon_color);
    let text_pos = egui::pos2(
        icon_rect.right() + ui.spacing().icon_spacing,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(text_pos, galley, Color32::PLACEHOLDER);
    response.on_hover_text(tip)
}

fn paint_glyph(ui: &Ui, rect: egui::Rect, glyph: &str, color: Color32) {
    let size = rect.width().min(rect.height());
    let pos = ui.painter().round_pos_to_pixels(rect.center());
    ui.painter().text(
        pos,
        Align2::CENTER_CENTER,
        glyph,
        theme::icon_font(size),
        color,
    );
}

/// Draw a folder icon into `rect` (square).
pub fn paint(ui: &Ui, rect: egui::Rect, kind: FolderIcon, color: Color32) {
    paint_glyph(ui, rect, kind.glyph(), color);
}

pub fn paint_search(ui: &Ui, rect: egui::Rect, color: Color32) {
    paint_glyph(ui, rect, ph::MAGNIFYING_GLASS, color);
}

/// Top-bar Sync: glyph + label (or spinner while busy).
pub fn toolbar_button(
    ui: &mut Ui,
    kind: ToolbarIcon,
    label: &str,
    tip: &str,
    busy: bool,
) -> Response {
    toolbar_button_inner(ui, kind, label, tip, busy, false, 0.0)
}

/// Glyph + label that always uses the hover chip fill so it stays noticeable
/// (Compose in the top bar). `min_height` matches the search field chrome.
pub fn toolbar_button_emphasized(
    ui: &mut Ui,
    kind: ToolbarIcon,
    label: &str,
    tip: &str,
    busy: bool,
    min_height: f32,
) -> Response {
    toolbar_button_inner(ui, kind, label, tip, busy, true, min_height)
}

fn toolbar_button_inner(
    ui: &mut Ui,
    kind: ToolbarIcon,
    label: &str,
    tip: &str,
    busy: bool,
    emphasized: bool,
    min_height: f32,
) -> Response {
    let mute = theme::mute(ui.ctx());
    let ink = ui.visuals().text_color();
    // Emphasized (Compose): wider pad for a larger hit box; same 13px label.
    let pad = if emphasized {
        egui::vec2(14.0, 0.0)
    } else {
        egui::vec2(8.0, 4.0)
    };
    let icon: f32 = 14.0;
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::new(13.0, egui::FontFamily::Proportional),
        ink,
    );
    let content_h = pad.y * 2.0 + icon.max(galley.size().y);
    let size = egui::vec2(
        pad.x * 2.0 + icon + ui.spacing().icon_spacing + galley.size().x,
        content_h.max(min_height).max(24.0_f32),
    );
    let sense = if busy { Sense::hover() } else { Sense::click() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let fill = if busy {
        theme::select(ui.ctx())
    } else if response.is_pointer_button_down_on() {
        theme::select(ui.ctx())
    } else if emphasized || response.hovered() {
        theme::chip(ui.ctx())
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 6.0, fill);
    }
    let icon_rect = egui::Rect::from_center_size(
        ui.painter().round_pos_to_pixels(Pos2::new(
            rect.left() + pad.x + icon * 0.5,
            rect.center().y,
        )),
        Vec2::splat(icon),
    );
    if busy {
        ui.put(icon_rect, egui::Spinner::new().size(icon));
    } else {
        paint_glyph(ui, icon_rect, kind.glyph(), mute);
    }
    ui.painter().galley(
        ui.painter().round_pos_to_pixels(Pos2::new(
            icon_rect.right() + ui.spacing().icon_spacing,
            rect.center().y - galley.size().y * 0.5,
        )),
        galley,
        ink,
    );
    response.on_hover_text(if busy {
        format!("{tip} (working…)")
    } else {
        tip.to_string()
    })
}

/// Text button that shows a trailing spinner while `busy`.
pub fn busy_text_button(ui: &mut Ui, label: &str, busy: bool) -> Response {
    if busy {
        ui.add_enabled_ui(false, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(label);
            });
        })
        .response
    } else {
        ui.button(label)
    }
}

pub fn paint_panel_toggle(ui: &Ui, rect: egui::Rect, collapsed: bool, color: Color32) {
    // Caret direction: collapsed rail → expand; open rail → collapse.
    let glyph = if collapsed {
        ph::CARET_RIGHT
    } else {
        ph::CARET_LEFT
    };
    paint_glyph(ui, rect, glyph, color);
}

/// Soft unread pill — same tint family as the selected-row background.
pub fn paint_count_badge_at(ui: &Ui, badge_rect: egui::Rect, count: u32) {
    if count == 0 {
        return;
    }
    let label = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    let fg = theme::accent(ui.ctx());
    let font = FontId::proportional(10.0);
    let galley = ui.fonts(|f| f.layout_no_wrap(label, font, fg));
    ui.painter()
        .rect_filled(badge_rect, badge_rect.height() * 0.5, theme::select(ui.ctx()));
    let text_pos = ui
        .painter()
        .round_pos_to_pixels(badge_rect.center() - galley.size() * 0.5);
    ui.painter().galley(text_pos, galley, fg);
}

pub fn count_badge_size(ui: &Ui, count: u32) -> Vec2 {
    let label = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    let font = FontId::proportional(10.0);
    let galley = ui.fonts(|f| f.layout_no_wrap(label, font, Color32::WHITE));
    let pad_x = 5.0;
    let pad_y = 1.0;
    Vec2::new(
        (galley.size().x + pad_x * 2.0).max(16.0),
        (galley.size().y + pad_y * 2.0).max(14.0),
    )
}

/// Unread count pill — right side when expanded, top-right corner when collapsed.
pub fn paint_count_badge(ui: &Ui, row: egui::Rect, count: u32, collapsed: bool) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let size = count_badge_size(ui, count);
    let badge_rect = if collapsed {
        egui::Rect::from_center_size(
            Pos2::new(row.right() - size.x * 0.35, row.top() + size.y * 0.35),
            size,
        )
    } else {
        egui::Rect::from_center_size(
            Pos2::new(row.right() - 6.0 - size.x * 0.5, row.center().y),
            size,
        )
    };
    paint_count_badge_at(ui, badge_rect, count);
    if collapsed {
        0.0
    } else {
        size.x + 10.0
    }
}

/// Paint a checkbox glyph into `rect` (no interaction).
pub fn paint_selection_checkbox(ui: &Ui, rect: egui::Rect, checked: bool) {
    let accent = theme::accent(ui.ctx());
    let mute = theme::mute(ui.ctx());
    let box_size = rect.width().min(rect.height()) * 0.85;
    let box_rect = egui::Rect::from_center_size(rect.center(), Vec2::splat(box_size));
    if checked {
        ui.painter().rect_filled(box_rect, 3.0, accent);
        let ink = Color32::WHITE;
        let s = box_size;
        let c = box_rect.center();
        let p1 = Pos2::new(c.x - s * 0.22, c.y + s * 0.02);
        let p2 = Pos2::new(c.x - s * 0.04, c.y + s * 0.18);
        let p3 = Pos2::new(c.x + s * 0.24, c.y - s * 0.16);
        ui.painter()
            .line_segment([p1, p2], egui::Stroke::new(1.8_f32, ink));
        ui.painter()
            .line_segment([p2, p3], egui::Stroke::new(1.8_f32, ink));
    } else {
        ui.painter()
            .rect_stroke(box_rect, 3.0, egui::Stroke::new(1.5_f32, mute));
    }
}

/// Icon-only or icon+label nav button spanning the full column width.
pub fn folder_nav_button(
    ui: &mut Ui,
    kind: FolderIcon,
    label: &str,
    badge: u32,
    selected: bool,
    collapsed: bool,
) -> Response {
    folder_nav_button_indent(ui, kind, label, badge, selected, collapsed, 0)
}

/// Like [`folder_nav_button`], with horizontal indent for nested Labels.
pub fn folder_nav_button_indent(
    ui: &mut Ui,
    kind: FolderIcon,
    label: &str,
    badge: u32,
    selected: bool,
    collapsed: bool,
    indent_level: u8,
) -> Response {
    let accent = theme::accent(ui.ctx());
    let mute = theme::mute(ui.ctx());
    let height = 28.0;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());

    let fill = if selected {
        theme::select(ui.ctx())
    } else if response.hovered() {
        theme::chip(ui.ctx())
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, fill);

    let indent = if collapsed {
        0.0
    } else {
        (indent_level as f32) * 12.0
    };
    let icon_color = if selected { accent } else { mute };
    let icon_size = 15.0;
    let icon_center = if collapsed {
        rect.center()
    } else {
        Pos2::new(
            rect.left() + 10.0 + indent + icon_size * 0.5,
            rect.center().y,
        )
    };
    let icon_rect = egui::Rect::from_center_size(
        ui.painter().round_pos_to_pixels(icon_center),
        Vec2::splat(icon_size),
    );
    paint(ui, icon_rect, kind, icon_color);

    let badge_reserve = paint_count_badge(ui, rect, badge, collapsed);

    if !collapsed {
        let text_pos = ui.painter().round_pos_to_pixels(Pos2::new(
            rect.left() + 30.0 + indent,
            rect.center().y,
        ));
        let font = theme::nav_font(12.0);
        let color = if selected { accent } else { mute };
        let text_max_w = (rect.right() - badge_reserve - 4.0 - text_pos.x).max(0.0);
        theme::paint_truncated_line(ui, text_pos, label, font, color, text_max_w);
    }

    let tip = if badge > 0 {
        format!("{label} · {badge} unread")
    } else {
        label.to_string()
    };
    if collapsed || badge > 0 {
        response.on_hover_text(tip)
    } else {
        response
    }
}

pub fn collapse_button(ui: &mut Ui, collapsed: bool, color: Color32) -> Response {
    let size = Vec2::splat(28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, color.linear_multiply(0.2));
    }
    paint_panel_toggle(ui, rect.shrink(4.0), collapsed, color);
    response.on_hover_text(if collapsed {
        "Expand folders"
    } else {
        "Collapse to icons"
    })
}

/// Circular “+” control for adding an account.
pub fn plus_account_button(ui: &mut Ui, size: f32) -> Response {
    let mute = theme::mute(ui.ctx());
    let accent = theme::accent(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let color = if response.hovered() { accent } else { mute };
    paint_glyph(ui, rect.shrink(size * 0.12), ph::PLUS_CIRCLE, color);
    response.on_hover_text("Add account")
}

/// Same row chrome as [`folder_nav_button`]: full-width, icon (+ label when expanded).
fn nav_action_row(
    ui: &mut Ui,
    collapsed: bool,
    enabled: bool,
    selected: bool,
    label: &str,
    hover: &str,
    glyph: &'static str,
) -> Response {
    let accent = theme::accent(ui.ctx());
    let mute = theme::mute(ui.ctx());
    let height = 28.0;
    let width = ui.available_width();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), sense);

    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if selected {
        theme::select(ui.ctx())
    } else if response.hovered() {
        theme::chip(ui.ctx())
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, fill);

    let icon_color = if !enabled {
        mute.linear_multiply(0.45)
    } else if selected || response.hovered() {
        accent
    } else {
        mute
    };
    let icon_size = 15.0;
    let icon_center = if collapsed {
        rect.center()
    } else {
        Pos2::new(rect.left() + 10.0 + icon_size * 0.5, rect.center().y)
    };
    let icon_rect = egui::Rect::from_center_size(
        ui.painter().round_pos_to_pixels(icon_center),
        Vec2::splat(icon_size),
    );
    paint_glyph(ui, icon_rect, glyph, icon_color);

    if !collapsed {
        let text_pos = ui
            .painter()
            .round_pos_to_pixels(Pos2::new(rect.left() + 30.0, rect.center().y));
        let font = theme::nav_font(12.0);
        let text_color = if !enabled {
            mute.linear_multiply(0.55)
        } else if selected {
            accent
        } else {
            mute
        };
        ui.painter()
            .text(text_pos, Align2::LEFT_CENTER, label, font, text_color);
    }

    let tip = if enabled { hover } else { "Open a message first" };
    if collapsed || !enabled {
        response.on_hover_text(tip)
    } else {
        response.on_hover_text(hover)
    }
}

#[allow(dead_code)] // Removed from nav footer; Desk still opens via logo / `d`.
pub fn desk_nav_button(ui: &mut Ui, collapsed: bool) -> Response {
    nav_action_row(
        ui,
        collapsed,
        true,
        false,
        "Desk",
        "Open Ailiak Desk in browser (d)",
        ph::BROWSER,
    )
}

pub fn todo_nav_button(ui: &mut Ui, enabled: bool, collapsed: bool) -> Response {
    nav_action_row(
        ui,
        collapsed,
        enabled,
        false,
        "Todo",
        "Todo — mail → Ailiak Desk (a)",
        ph::CHECK_SQUARE,
    )
}

pub fn calendar_nav_button(ui: &mut Ui, collapsed: bool) -> Response {
    nav_action_row(
        ui,
        collapsed,
        true,
        false,
        "Calendar",
        "Desk calendar (c)",
        ph::CALENDAR_BLANK,
    )
}

pub fn settings_nav_button(ui: &mut Ui, collapsed: bool) -> Response {
    nav_action_row(
        ui,
        collapsed,
        true,
        false,
        "Settings",
        "Settings (,)",
        ph::GEAR,
    )
}

#[allow(dead_code)]
pub fn feedback_nav_button(ui: &mut Ui, collapsed: bool) -> Response {
    nav_action_row(
        ui,
        collapsed,
        true,
        false,
        "Feedback",
        "Send feedback as an issue",
        ph::CHAT_CIRCLE,
    )
}

pub fn help_nav_button(ui: &mut Ui, collapsed: bool) -> Response {
    nav_action_row(ui, collapsed, true, false, "Help", "Help (?)", ph::QUESTION)
}

/// Stak row in the nav rail (sender/domain stack).
pub fn stak_nav_button(
    ui: &mut Ui,
    label: &str,
    badge: u32,
    selected: bool,
    collapsed: bool,
    brand: Option<&egui::TextureHandle>,
) -> Response {
    let accent = theme::accent(ui.ctx());
    let mute = theme::mute(ui.ctx());
    let height = 28.0;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());

    let fill = if selected {
        theme::select(ui.ctx())
    } else if response.hovered() {
        theme::chip(ui.ctx())
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, fill);

    let icon_size = crate::brand_logos::DISPLAY_SIZE;
    let icon_left = 10.0;
    let icon_center = if collapsed {
        rect.center()
    } else {
        Pos2::new(rect.left() + icon_left + icon_size * 0.5, rect.center().y)
    };
    let icon_rect = egui::Rect::from_center_size(
        ui.painter().round_pos_to_pixels(icon_center),
        Vec2::splat(icon_size),
    );
    if let Some(tex) = brand {
        // Soft plate behind marks so transparent Logo.dev PNGs stay readable.
        let plate = if ui.visuals().dark_mode {
            Color32::from_rgba_unmultiplied(255, 255, 255, 18)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 220)
        };
        ui.painter().rect_filled(icon_rect, 5.0, plate);
        egui::Image::new((tex.id(), icon_rect.size()))
            .maintain_aspect_ratio(true)
            .fit_to_exact_size(icon_rect.size())
            .rounding(5.0)
            .paint_at(ui, icon_rect);
    } else {
        let icon_color = if selected { accent } else { mute };
        paint_glyph(ui, icon_rect, ph::STACK, icon_color);
    }

    let badge_reserve = paint_count_badge(ui, rect, badge, collapsed);

    if !collapsed {
        // Gap between brand mark and label (was ~2px; needs room to breathe).
        let text_left = icon_left + icon_size + 10.0;
        let text_pos = ui
            .painter()
            .round_pos_to_pixels(Pos2::new(rect.left() + text_left, rect.center().y));
        let font = theme::nav_font(12.0);
        let color = if selected { accent } else { mute };
        let text_max_w = (rect.right() - badge_reserve.max(8.0) - text_pos.x).max(0.0);
        theme::paint_truncated_line(ui, text_pos, label, font, color, text_max_w);
    }

    let tip = if badge > 0 {
        format!("{label} · {badge} unread")
    } else {
        label.to_string()
    };
    response.on_hover_text(tip)
}
