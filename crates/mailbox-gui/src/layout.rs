//! Shared panel chrome — opaque fills so no clear-color gaps show through.

use egui::{Color32, Frame, Margin, Rounding, Shadow, Stroke, Ui, Vec2};

/// Corner radius for mail body / list cards and nav Accounts + tools paper bands.
/// Keep nav right-edge rounding and content cards on this same value.
pub const PANEL_CARD_RADIUS: f32 = 12.0;

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

/// Paper band for nav Accounts / tools — right corners match [`PANEL_CARD_RADIUS`].
pub fn nav_paper_band(fill: Color32, pad: Margin, outer: Margin) -> Frame {
    Frame::none()
        .fill(fill)
        .stroke(Stroke::NONE)
        .rounding(rounding_right(PANEL_CARD_RADIUS))
        .outer_margin(outer)
        .inner_margin(pad)
}

/// Inset rounded content card (mail list panel / mail body).
///
/// `gutter` is outer margin so chrome shows around the radius.
/// `pad` is inner content padding.
pub fn content_card(fill: Color32, gutter: Margin, pad: Margin) -> Frame {
    content_card_stroke(fill, gutter, pad, Stroke::NONE)
}

/// Soft drop shadow for inset content cards (list / reader body).
fn content_card_shadow() -> Shadow {
    Shadow {
        offset: Vec2::new(0.0, 2.0),
        blur: 6.0,
        spread: 0.0,
        color: Color32::from_black_alpha(32),
    }
}

/// Same as [`content_card`] with an optional edge stroke.
pub fn content_card_stroke(
    fill: Color32,
    gutter: Margin,
    pad: Margin,
    stroke: Stroke,
) -> Frame {
    Frame::none()
        .fill(fill)
        .stroke(stroke)
        .rounding(Rounding::same(PANEL_CARD_RADIUS))
        .shadow(content_card_shadow())
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
