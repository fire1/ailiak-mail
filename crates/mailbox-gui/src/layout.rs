//! Shared panel chrome — opaque fills so no clear-color gaps show through.

use egui::{Color32, Frame, Margin, Stroke, Ui};

pub fn panel_frame(margin: Margin, fill: Color32) -> Frame {
    Frame::none()
        .fill(fill)
        .stroke(Stroke::NONE)
        .inner_margin(margin)
}

/// Paint the full SidePanel rect (width is owned by the shell via `exact_width`).
pub fn begin_panel(ui: &mut Ui, fill: Color32) {
    ui.painter().rect_filled(ui.max_rect(), 0.0, fill);
}

pub fn end_panel(ui: &mut Ui) {
    ui.expand_to_include_y(ui.max_rect().bottom());
}
