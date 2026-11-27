//! Native OS webview (wry) for HTML mail — WebKitGTK / WKWebView / WebView2.
//!
//! Document is CSP-locked + script-stripped (see
//! `mailbox_core::build_mail_html_document`). Navigations are allowlisted;
//! remote images require an explicit opt-in rebuild; CID inlines are `data:` URLs.
//! Nested `srcdoc` iframes are avoided — they paint blank under WebKitGTK.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use egui::Rect;
use mailbox_core::oauth::open_url;
use mailbox_core::{
    build_mail_html_document, classify_mail_navigation, MailNavAction, MessageId,
};
#[cfg(target_os = "linux")]
use wry::dpi::{PhysicalPosition, PhysicalSize, Position, Size};
#[cfg(not(target_os = "linux"))]
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::raw_window_handle::HasWindowHandle;
use wry::{NewWindowResponse, Rect as WryRect, WebView, WebViewBuilder};

use crate::theme;

enum ExtLink {
    Open(String),
}

/// Request from the egui reader pane to show HTML in the native webview.
#[derive(Clone)]
pub struct MailWebViewRequest {
    pub bounds: Rect,
    /// egui `pixels_per_point` — needed on Linux where wry's X11 DPI (~1.0 from
    /// screen mm) disagrees with winit's guessed scale (~1.17).
    pub pixels_per_point: f32,
    pub msg_id: MessageId,
    pub body_html: String,
    pub load_images: bool,
    pub dark: bool,
}

/// Pending HTML load — waits out rapid list navigation so WebKit does not thrash
/// (black flash) through every intermediate message.
struct PendingLoad {
    msg_id: MessageId,
    body_html: String,
    load_images: bool,
    dark: bool,
    since: Instant,
}

pub struct MailWebView {
    webview: Option<WebView>,
    /// (msg_id, load_images, dark)
    loaded: Option<(MessageId, bool, bool)>,
    last_bounds: Option<Rect>,
    /// Scale used at last create — recreate when the window moves to a
    /// different-DPI monitor (bounds in points can stay similar).
    last_ppp: f32,
    dark: bool,
    visible: bool,
    link_tx: Sender<ExtLink>,
    link_rx: Receiver<ExtLink>,
    pending: Option<PendingLoad>,
    /// Pending `getSelection()` result from [`request_selection`].
    selection_rx: Option<Receiver<String>>,
    /// Last create error (e.g. Wayland without child webview support).
    pub last_error: Option<String>,
}

impl Default for MailWebView {
    fn default() -> Self {
        let (link_tx, link_rx) = mpsc::channel();
        Self {
            webview: None,
            loaded: None,
            last_bounds: None,
            last_ppp: 0.0,
            dark: false,
            visible: false,
            link_tx,
            link_rx,
            pending: None,
            selection_rx: None,
            last_error: None,
        }
    }
}

/// Settle time before committing a new message body (skips black flashes on fast clicks).
const LOAD_DEBOUNCE: Duration = Duration::from_millis(70);

impl MailWebView {
    /// Pump GTK on Linux (required alongside winit/eframe).
    ///
    /// Cap iterations so a flood of events (e.g. window close + WebKit teardown)
    /// cannot stall the egui frame forever.
    pub fn pump_platform() {
        #[cfg(target_os = "linux")]
        {
            ensure_gtk();
            for _ in 0..64 {
                if !gtk::events_pending() {
                    break;
                }
                let _ = gtk::main_iteration_do(false);
            }
        }
    }

    pub fn is_alive(&self) -> bool {
        self.webview.is_some()
    }

    /// True while the native view is missing or still loading a different message.
    pub fn is_loading(&self, msg_id: &MessageId) -> bool {
        match &self.loaded {
            Some((id, _, _)) if self.webview.is_some() && id == msg_id => false,
            _ => true,
        }
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.pending = None;
        if let Some(wv) = &self.webview {
            #[cfg(target_os = "linux")]
            {
                // Do not call set_visible(false): gtk_widget_hide /
                // _gdk_frame_clock_freeze asserts GDK_IS_FRAME_CLOCK on wry’s
                // X11 child WebKit views. Park off-screen instead.
                let park = WryRect {
                    position: Position::Physical(PhysicalPosition::new(-10_000, -10_000)),
                    size: Size::Physical(PhysicalSize::new(1, 1)),
                };
                let _ = wv.set_bounds(park);
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = wv.set_visible(false);
            }
        }
    }

    /// Drop the native WebKit view while the host window still exists.
    /// Calling this on window-close avoids a common Linux hang where wry’s
    /// `Drop` runs after the parent X11 window is already gone.
    pub fn destroy(&mut self) {
        // Take + drop without an extra set_visible(false): that path freezes the
        // GDK frame clock and asserts when the foreign X11 window is mid-teardown.
        let wv = self.webview.take();
        self.loaded = None;
        self.last_bounds = None;
        self.last_ppp = 0.0;
        self.visible = false;
        self.pending = None;
        self.selection_rx = None;
        drop(wv);
        Self::pump_platform();
    }

    pub fn poll_links(&mut self) {
        while let Ok(msg) = self.link_rx.try_recv() {
            match msg {
                ExtLink::Open(url) => {
                    let _ = open_url(&url);
                }
            }
        }
    }

    /// Ask the mail body webview for the current text selection (async).
    /// Poll with [`take_selection`] on later frames.
    pub fn request_selection(&mut self) {
        let Some(wv) = &self.webview else {
            self.selection_rx = None;
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.selection_rx = Some(rx);
        let ok = wv.evaluate_script_with_callback(
            r#"(function(){
                try { return window.getSelection().toString(); }
                catch (e) { return ''; }
            })()"#,
            move |s| {
                let _ = tx.send(s);
            },
        );
        if ok.is_err() {
            self.selection_rx = None;
        }
    }

    /// Take a completed selection string, if any.
    pub fn take_selection(&mut self) -> Option<String> {
        let rx = self.selection_rx.as_ref()?;
        match rx.try_recv() {
            Ok(s) => {
                self.selection_rx = None;
                Some(s)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.selection_rx = None;
                Some(String::new())
            }
        }
    }

    pub fn has_pending_selection(&self) -> bool {
        self.selection_rx.is_some()
    }

    /// Show HTML for `msg_id` in `bounds` (egui screen rect), creating the webview if needed.
    pub fn show_html(
        &mut self,
        window: &impl HasWindowHandle,
        msg_id: &MessageId,
        body_html: &str,
        load_images: bool,
        dark: bool,
        bounds: Rect,
        pixels_per_point: f32,
    ) {
        Self::pump_platform();

        // egui can report a tiny available rect for a frame while panels settle.
        // Creating the X11 child at that size is sticky on Linux — wait for a real area.
        if bounds.width() < 80.0 || bounds.height() < 80.0 {
            return;
        }

        let ppp = pixels_per_point.max(0.1);
        let theme_changed = self.dark != dark;
        self.dark = dark;

        let already = self
            .loaded
            .as_ref()
            .is_some_and(|(id, imgs, was_dark)| {
                id == msg_id && *imgs == load_images && *was_dark == dark
            });

        if already {
            self.pending = None;
        } else {
            let same_pending = self.pending.as_ref().is_some_and(|p| {
                p.msg_id == *msg_id && p.load_images == load_images && p.dark == dark
            });
            if !same_pending {
                self.pending = Some(PendingLoad {
                    msg_id: msg_id.clone(),
                    body_html: body_html.to_string(),
                    load_images,
                    dark,
                    since: Instant::now(),
                });
                // Paint theme chrome immediately so WebKit does not sit on black
                // while the debounce waits / HTML parses.
                self.apply_shell_backdrop(dark);
            }
        }

        // Recreate only on DPI change — destroy/recreate thrashing trips
        // Gdk-CRITICAL `_gdk_frame_clock_freeze` on Linux X11 child views.
        // Position/size drift uses set_bounds with Physical pixels (same as create).
        if self.webview.is_some() && self.needs_recreate_for_ppp(ppp) {
            self.destroy();
        }

        if self.webview.is_none() {
            match self.create(window, bounds, ppp, dark) {
                Ok(()) => self.last_error = None,
                Err(e) => {
                    self.last_error = Some(e);
                    return;
                }
            }
        } else if theme_changed {
            self.apply_shell_backdrop(dark);
        }

        if let Some(wv) = &self.webview {
            let was_hidden = !self.visible;
            let drifted = self.last_bounds.is_none_or(|prev| {
                (prev.min.x - bounds.min.x).abs() > 0.5
                    || (prev.min.y - bounds.min.y).abs() > 0.5
                    || (prev.width() - bounds.width()).abs() > 0.5
                    || (prev.height() - bounds.height()).abs() > 0.5
            });
            // Always restore bounds after a park-hide, and on ordinary moves.
            if was_hidden || drifted {
                let _ = wv.set_bounds(to_wry_rect(bounds, ppp));
                self.last_bounds = Some(bounds);
                self.last_ppp = ppp;
            }
            #[cfg(not(target_os = "linux"))]
            if was_hidden {
                let _ = wv.set_visible(true);
            }
            self.visible = true;
        }

        let commit = self
            .pending
            .as_ref()
            .is_some_and(|p| p.since.elapsed() >= LOAD_DEBOUNCE);
        if commit {
            if let Some(pending) = self.pending.take() {
                self.commit_html(
                    &pending.msg_id,
                    &pending.body_html,
                    pending.load_images,
                    pending.dark,
                );
            }
        } else if self.pending.is_some() {
            // Keep animating until debounce elapses.
            // Caller’s egui spinner path already request_repaints while is_loading.
        }
    }

    fn apply_shell_backdrop(&self, dark: bool) {
        let Some(wv) = &self.webview else {
            return;
        };
        let bg = theme::mail_webview_backdrop_rgba(dark);
        // Color only while debouncing — avoid load_html thrash (WebKit/GDK
        // frame-clock churn) on every intermediate list selection.
        let _ = wv.set_background_color(bg);
    }

    fn commit_html(&mut self, msg_id: &MessageId, body_html: &str, load_images: bool, dark: bool) {
        let Some(wv) = &self.webview else {
            return;
        };
        let bg = theme::mail_webview_backdrop_rgba(dark);
        let _ = wv.set_background_color(bg);
        let doc = build_mail_html_document(body_html, load_images, dark);
        if let Err(e) = wv.load_html(&doc) {
            self.last_error = Some(format!("load_html: {e}"));
            return;
        }
        self.loaded = Some((msg_id.clone(), load_images, dark));
    }

    fn needs_recreate_for_ppp(&self, ppp: f32) -> bool {
        self.last_ppp > 0.0 && (self.last_ppp - ppp).abs() > 0.01
    }

    fn create(
        &mut self,
        window: &impl HasWindowHandle,
        bounds: Rect,
        pixels_per_point: f32,
        dark: bool,
    ) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        ensure_gtk();

        let tx = self.link_tx.clone();
        let bg_rgba = theme::mail_webview_backdrop_rgba(dark);
        let css = theme::mail_webview_backdrop_css(dark);
        let placeholder = format!(
            "<!DOCTYPE html><html><body style=\"background:{css};margin:0\"></body></html>"
        );
        let builder = WebViewBuilder::new()
            .with_transparent(false)
            .with_background_color(bg_rgba)
            .with_html(placeholder)
            .with_bounds(to_wry_rect(bounds, pixels_per_point))
            .with_navigation_handler(move |url| match classify_mail_navigation(&url) {
                MailNavAction::Stay => true,
                MailNavAction::OpenExternal(safe) => {
                    let _ = tx.send(ExtLink::Open(safe));
                    false
                }
                MailNavAction::Block => {
                    // wry `with_html` / `load_html` may surface as data: for the document.
                    url.starts_with("data:text/html")
                }
            })
            .with_new_window_req_handler({
                let tx = self.link_tx.clone();
                move |url, _features| {
                    match classify_mail_navigation(&url) {
                        MailNavAction::OpenExternal(safe) => {
                            let _ = tx.send(ExtLink::Open(safe));
                        }
                        _ => {}
                    }
                    NewWindowResponse::Deny
                }
            })
            .with_download_started_handler(|_url, _path| false);

        // wry 0.53 unwraps GDK X11 display; a Wayland GDK + X11 winit mismatch panics.
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            builder.build_as_child(window)
        }));
        let webview = match built {
            Ok(Ok(wv)) => wv,
            Ok(Err(e)) => {
                let msg = e.to_string();
                return Err(if msg.contains("window handle kind is not supported") {
                    format!(
                        "webview: {msg} (HTML mail needs X11/XWayland; \
set GDK_BACKEND=x11 or restart AiliakMail)"
                    )
                } else {
                    format!("webview: {msg}")
                });
            }
            Err(_) => {
                return Err(
                    "webview: crashed while creating (GDK must use X11 — try GDK_BACKEND=x11)"
                        .into(),
                );
            }
        };
        let _ = webview.set_background_color(bg_rgba);
        self.webview = Some(webview);
        self.last_bounds = Some(bounds);
        self.last_ppp = pixels_per_point.max(0.1);
        self.visible = true;
        self.dark = dark;
        Ok(())
    }
}

pub(crate) fn to_wry_rect(bounds: Rect, pixels_per_point: f32) -> WryRect {
    // Linux/X11: wry's create path uses scale_factor_from_x11 (screen mm → often
    // ~1.0 on XWayland) while winit/egui guess ~1.17. Passing Logical egui points
    // under-sizes/shifts the child left. Physical pixels (= points * ppp) match
    // egui's paint coordinates. Do not also call set_bounds on Linux — GTK's
    // scale_factor (1.0) would fight this.
    //
    // Elsewhere: Logical egui points; wry applies the window scale factor.
    #[cfg(target_os = "linux")]
    {
        let ppp = pixels_per_point.max(0.1) as f64;
        WryRect {
            position: Position::Physical(PhysicalPosition::new(
                (bounds.min.x as f64 * ppp).round() as i32,
                (bounds.min.y as f64 * ppp).round() as i32,
            )),
            size: Size::Physical(PhysicalSize::new(
                (bounds.width().max(1.0) as f64 * ppp).round() as u32,
                (bounds.height().max(1.0) as f64 * ppp).round() as u32,
            )),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pixels_per_point;
        WryRect {
            position: Position::Logical(LogicalPosition::new(
                bounds.min.x as f64,
                bounds.min.y as f64,
            )),
            size: Size::Logical(LogicalSize::new(
                bounds.width().max(1.0) as f64,
                bounds.height().max(1.0) as f64,
            )),
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn ensure_gtk() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Must match winit’s X11 window or wry’s gdk X11Display downcast panics.
        if std::env::var_os("DISPLAY").is_some()
            && std::env::var_os("GDK_BACKEND").is_none()
        {
            // SAFETY: before first gtk::init in this process.
            unsafe {
                std::env::set_var("GDK_BACKEND", "x11");
            }
        }
        if let Err(e) = gtk::init() {
            tracing::error!(error = %e, "gtk::init failed — HTML mail webview unavailable");
        }
    });
}
