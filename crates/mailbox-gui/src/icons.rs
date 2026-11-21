//! Lucide-inspired stroke icons (24×24 viewBox, thin stroke), painted with egui.

use egui::{pos2, Color32, Pos2, Response, Sense, Shape, Stroke, Ui, Vec2};

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
}

/// Draw a Lucide-style icon into `rect` (square).
pub fn paint(ui: &Ui, rect: egui::Rect, kind: FolderIcon, color: Color32) {
    let stroke = Stroke::new(1.1, color);
    let s = rect.width().min(rect.height());
    let c = rect.center();
    // Map 24×24 Lucide coords → rect
    let p = |x: f32, y: f32| {
        pos2(
            c.x + (x - 12.0) * (s / 24.0),
            c.y + (y - 12.0) * (s / 24.0),
        )
    };
    let painter = ui.painter();

    match kind {
        FolderIcon::Inbox => {
            // polyline 22,12 → 16,12 → 14,15 → 10,15 → 8,12 → 2,12
            painter.add(Shape::line(
                vec![p(22.0, 12.0), p(16.0, 12.0), p(14.0, 15.0), p(10.0, 15.0), p(8.0, 12.0), p(2.0, 12.0)],
                stroke,
            ));
            // tray outline
            painter.add(Shape::closed_line(
                vec![
                    p(5.45, 5.11),
                    p(2.0, 12.0),
                    p(2.0, 18.0),
                    p(4.0, 20.0),
                    p(20.0, 20.0),
                    p(22.0, 18.0),
                    p(22.0, 12.0),
                    p(18.55, 5.11),
                    p(16.76, 4.0),
                    p(7.24, 4.0),
                ],
                stroke,
            ));
        }
        FolderIcon::Sent => {
            // paper-plane (send)
            painter.add(Shape::closed_line(
                vec![
                    p(22.0, 2.0),
                    p(14.5, 21.7),
                    p(11.0, 13.0),
                    p(2.15, 9.5),
                ],
                stroke,
            ));
            painter.line_segment([p(22.0, 2.0), p(11.0, 13.0)], stroke);
        }
        FolderIcon::Archive => {
            // lid rect
            painter.rect_stroke(
                egui::Rect::from_min_max(p(2.0, 3.0), p(22.0, 8.0)),
                1.0,
                stroke,
            );
            // body
            painter.add(Shape::line(
                vec![p(4.0, 8.0), p(4.0, 19.0), p(6.0, 21.0), p(18.0, 21.0), p(20.0, 19.0), p(20.0, 8.0)],
                stroke,
            ));
            painter.line_segment([p(10.0, 12.0), p(14.0, 12.0)], stroke);
        }
        FolderIcon::Trash => {
            painter.line_segment([p(3.0, 6.0), p(21.0, 6.0)], stroke);
            painter.add(Shape::line(
                vec![p(8.0, 6.0), p(8.0, 4.0), p(10.0, 2.0), p(14.0, 2.0), p(16.0, 4.0), p(16.0, 6.0)],
                stroke,
            ));
            painter.add(Shape::line(
                vec![p(19.0, 6.0), p(19.0, 20.0), p(17.0, 22.0), p(7.0, 22.0), p(5.0, 20.0), p(5.0, 6.0)],
                stroke,
            ));
            painter.line_segment([p(10.0, 11.0), p(10.0, 17.0)], stroke);
            painter.line_segment([p(14.0, 11.0), p(14.0, 17.0)], stroke);
        }
        FolderIcon::Folder => {
            painter.add(Shape::closed_line(
                vec![
                    p(3.0, 7.0),
                    p(3.0, 5.0),
                    p(5.0, 3.0),
                    p(10.0, 3.0),
                    p(12.0, 5.0),
                    p(21.0, 5.0),
                    p(21.0, 19.0),
                    p(3.0, 19.0),
                ],
                stroke,
            ));
        }
    }
}

pub fn paint_panel_toggle(ui: &Ui, rect: egui::Rect, collapsed: bool, color: Color32) {
    let stroke = Stroke::new(1.1, color);
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let p = |x: f32, y: f32| {
        pos2(
            c.x + (x - 12.0) * (s / 24.0),
            c.y + (y - 12.0) * (s / 24.0),
        )
    };
    let painter = ui.painter();
    painter.rect_stroke(
        egui::Rect::from_min_max(p(3.0, 3.0), p(21.0, 21.0)),
        2.0,
        stroke,
    );
    painter.line_segment([p(9.0, 3.0), p(9.0, 21.0)], stroke);
    if collapsed {
        // panel-left-open chevron →
        painter.add(Shape::line(vec![p(14.0, 9.0), p(17.0, 12.0), p(14.0, 15.0)], stroke));
    } else {
        // panel-left-close chevron ←
        painter.add(Shape::line(vec![p(16.0, 15.0), p(13.0, 12.0), p(16.0, 9.0)], stroke));
    }
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
    let height = 34.0;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());

    let fill = if selected {
        theme::select(ui.ctx())
    } else if response.hovered() {
        theme::chip(ui.ctx())
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);

    let icon_color = if selected { accent } else { mute };
    let icon_size = 18.0;
    let icon_rect = egui::Rect::from_center_size(
        if collapsed {
            rect.center()
        } else {
            Pos2::new(rect.left() + 10.0 + icon_size * 0.5, rect.center().y)
        },
        Vec2::splat(icon_size),
    );
    paint(ui, icon_rect, kind, icon_color);

    if !collapsed {
        let text_pos = Pos2::new(rect.left() + 34.0, rect.center().y);
        let font = egui::FontId::proportional(13.5);
        let color = if selected {
            accent
        } else {
            ui.visuals().widgets.inactive.fg_stroke.color
        };
        ui.painter().text(
            text_pos,
            egui::Align2::LEFT_CENTER,
            label,
            font,
            if selected {
                color
            } else {
                ui.visuals().text_color()
            },
        );
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
