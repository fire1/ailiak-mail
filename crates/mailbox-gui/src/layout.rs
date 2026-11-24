//! Shared panel chrome — opaque fills so no clear-color gaps show through.

use egui::{Color32, Frame, Margin, Rounding, Stroke, Ui};

pub fn panel_frame(margin: Margin, fill: Color32) -> Frame {
    Frame::none()
        .fill(fill)
        .stroke(Stroke::NONE)
        .inner_margin(margin)
}

/// Round only the right edge (nav paper bands flush to the window left).
pub fn rounding_right(radius: f32) -> Rounding {
    Rounding {
        nw: 0.0,
        ne: radius,
        sw: 0.0,
        se: radius,
    }
}

/// Paper band for nav Accounts / tools — right corners rounded into chrome.
pub fn nav_paper_band(fill: Color32, pad: Margin, outer: Margin) -> Frame {
    Frame::none()
        .fill(fill)
        .stroke(Stroke::NONE)
        .rounding(rounding_right(8.0))
        .outer_margin(outer)
        .inner_margin(pad)
}

/// Inset rounded content card (mid list / mail reader).
///
/// `gutter` is outer margin so chrome shows around the radius.
/// `pad` is inner content padding.
pub fn content_card(fill: Color32, gutter: Margin, pad: Margin) -> Frame {
    content_card_stroke(fill, gutter, pad, Stroke::NONE)
}

/// Same as [`content_card`] with an optional edge stroke (helps mid-card radius read).
pub fn content_card_stroke(
    fill: Color32,
    gutter: Margin,
    pad: Margin,
    stroke: Stroke,
) -> Frame {
    Frame::none()
        .fill(fill)
        .stroke(stroke)
        .rounding(Rounding::same(8.0))
        .outer_margin(gutter)
        .inner_margin(pad)
}

/// Paint the full SidePanel rect (width is owned by the shell via `exact_width`).
pub fn begin_panel(ui: &mut Ui, fill: Color32) {
    ui.painter().rect_filled(ui.max_rect(), 0.0, fill);
}

pub fn end_panel(ui: &mut Ui) {
    ui.expand_to_include_y(ui.max_rect().bottom());
}
