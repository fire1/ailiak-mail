//! Phosphor glyph icons (font AA via egui text), tinted by paint color.

use egui::{Align2, Color32, FontId, Pos2, Response, Sense, Ui, Vec2};
use egui_phosphor::regular as ph;

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderIcon {
    Inbox,
    Sent,
    Archive,
    Trash,
    Folder,
}

impl FolderIcon {
    pub fn from_folder(path: &str, name: &str) -> Self {
        let key = format!("{path} {name}").to_lowercase();
        if key.contains("inbox") {
            Self::Inbox
        } else if key.contains("sent") {
            Self::Sent
        } else if key.contains("archive") {
            Self::Archive
        } else if key.contains("trash") || key.contains("bin") || key.contains("deleted") {
            Self::Trash
        } else {
            Self::Folder
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Self::Inbox => ph::TRAY,
            Self::Sent => ph::PAPER_PLANE_TILT,
            Self::Archive => ph::ARCHIVE,
            Self::Trash => ph::TRASH,
            Self::Folder => ph::FOLDER,
        }
    }
}

#[derive(Clone, Copy)]
pub enum ToolbarIcon {
    Sync,
    Archive,
    Trash,
}

impl ToolbarIcon {
    fn glyph(self) -> &'static str {
        match self {
            Self::Sync => ph::ARROWS_CLOCKWISE,
            Self::Archive => ph::ARCHIVE,
            Self::Trash => ph::TRASH,
        }
    }
}

/// Icon-only toolbar control (Archive / Trash next to Sync).
pub fn toolbar_icon_button(ui: &mut Ui, kind: ToolbarIcon, tip: &str, enabled: bool) -> Response {
    let mute = theme::mute(ui.ctx());
    let size = Vec2::splat(28.0);
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
    let color = if enabled {
        mute
    } else {
        mute.linear_multiply(0.4)
    };
    paint_glyph(ui, rect.shrink(6.0), kind.glyph(), color);
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

/// Top-bar Sync / Calendar: glyph + label.
pub fn toolbar_button(ui: &mut Ui, kind: ToolbarIcon, label: &str, tip: &str) -> Response {
    let mute = theme::mute(ui.ctx());
    let ink = ui.visuals().text_color();
    let pad = egui::vec2(8.0, 4.0);
    let icon = 14.0;
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::new(13.0, egui::FontFamily::Proportional),
        ink,
    );
    let size = egui::vec2(
        pad.x * 2.0 + icon + ui.spacing().icon_spacing + galley.size().x,
        (pad.y * 2.0 + icon.max(galley.size().y)).max(24.0),
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        theme::select(ui.ctx())
    } else if response.hovered() {
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
    paint_glyph(ui, icon_rect, kind.glyph(), mute);
    ui.painter().galley(
        ui.painter().round_pos_to_pixels(Pos2::new(
            icon_rect.right() + ui.spacing().icon_spacing,
            rect.center().y - galley.size().y * 0.5,
        )),
        galley,
        ink,
    );
    response.on_hover_text(tip)
}

pub fn paint_panel_toggle(ui: &Ui, rect: egui::Rect, collapsed: bool, color: Color32) {
    let glyph = if collapsed {
        ph::SIDEBAR
    } else {
        ph::SIDEBAR_SIMPLE
    };
    paint_glyph(ui, rect, glyph, color);
}

/// Icon-only or icon+label nav button spanning the full column width.
pub fn folder_nav_button(
    ui: &mut Ui,
    kind: FolderIcon,
    label: &str,
    selected: bool,
    collapsed: bool,
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

    let icon_color = if selected { accent } else { mute };
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
    paint(ui, icon_rect, kind, icon_color);

    if !collapsed {
        let text_pos = ui
            .painter()
            .round_pos_to_pixels(Pos2::new(rect.left() + 30.0, rect.center().y));
        let font = theme::nav_font(12.0);
        let color = if selected {
            accent
        } else {
            ui.visuals().text_color()
        };
        ui.painter()
            .text(text_pos, Align2::LEFT_CENTER, label, font, color);
    }

    if collapsed {
        response.on_hover_text(label)
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
            ui.visuals().text_color()
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

pub fn help_nav_button(ui: &mut Ui, collapsed: bool) -> Response {
    nav_action_row(ui, collapsed, true, false, "Help", "Help (?)", ph::QUESTION)
}

/// Stak row in the nav rail (sender/domain stack).
pub fn stak_nav_button(
    ui: &mut Ui,
    label: &str,
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
    let icon_center = if collapsed {
        rect.center()
    } else {
        Pos2::new(rect.left() + 10.0 + icon_size * 0.5, rect.center().y)
    };
    let icon_rect = egui::Rect::from_center_size(
        ui.painter().round_pos_to_pixels(icon_center),
        Vec2::splat(icon_size),
    );
    if let Some(tex) = brand {
        egui::Image::new((tex.id(), icon_rect.size()))
            .rounding(4.0)
            .paint_at(ui, icon_rect);
    } else {
        let icon_color = if selected { accent } else { mute };
        paint_glyph(ui, icon_rect, ph::STACK, icon_color);
    }

    if !collapsed {
        let text_pos = ui
            .painter()
            .round_pos_to_pixels(Pos2::new(rect.left() + 30.0, rect.center().y));
        let font = theme::nav_font(12.0);
        let color = if selected {
            accent
        } else {
            ui.visuals().text_color()
        };
        ui.painter()
            .text(text_pos, Align2::LEFT_CENTER, label, font, color);
    }

    if collapsed {
        response.on_hover_text(label)
    } else {
        response
    }
}
