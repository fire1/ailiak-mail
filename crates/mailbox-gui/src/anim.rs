//! UI motion helpers — gated by Settings → Appearance → Animations.

use egui::{emath::easing, Context, Id};

/// Nav rail collapse / expand.
pub const NAV_SECS: f32 = 0.22;
/// Settings page fade, Options / Calendar side panel.
pub const PANEL_SECS: f32 = 0.18;
/// Hover-reveal chrome (e.g. Accounts “+”).
pub const HOVER_SECS: f32 = 0.14;

/// Animate `0..=1` toward a bool target. Instant when animations are off.
pub fn bool_t(ctx: &Context, id: impl Into<Id>, on: bool, enabled: bool, secs: f32) -> f32 {
    if !enabled {
        return if on { 1.0 } else { 0.0 };
    }
    ctx.animate_bool_with_time_and_easing(id.into(), on, secs, easing::cubic_out)
}

/// Animate a scalar toward `target`. Instant when animations are off.
pub fn value(ctx: &Context, id: impl Into<Id>, target: f32, enabled: bool, secs: f32) -> f32 {
    if !enabled {
        return target;
    }
    ctx.animate_value_with_time(id.into(), target, secs)
}
