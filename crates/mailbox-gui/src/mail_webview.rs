//! Native OS webview (wry) for HTML mail — WebKitGTK / WKWebView / WebView2.
//!
//! Security: document is ammonia-sanitized + CSP; navigations are allowlisted;
//! remote images require an explicit opt-in rebuild.

use std::sync::mpsc::{self, Receiver, Sender};

use egui::Rect;
use mailbox_core::oauth::open_url;
use mailbox_core::{
    build_mail_html_document, classify_mail_navigation, MailNavAction, MessageId,
};
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::raw_window_handle::HasWindowHandle;
use wry::{NewWindowResponse, Rect as WryRect, WebView, WebViewBuilder};

enum ExtLink {
    Open(String),
}

/// Request from the egui reader pane to show HTML in the native webview.
#[derive(Clone)]
pub struct MailWebViewRequest {
    pub bounds: Rect,
    pub msg_id: MessageId,
    pub body_html: String,
    pub load_images: bool,
    pub dark: bool,
}

pub struct MailWebView {
    webview: Option<WebView>,
    /// (msg_id, load_images, dark)
    loaded: Option<(MessageId, bool, bool)>,
    last_bounds: Option<Rect>,
    dark: bool,
    visible: bool,
    link_tx: Sender<ExtLink>,
    link_rx: Receiver<ExtLink>,
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
            dark: false,
            visible: false,
            link_tx,
            link_rx,
            last_error: None,
        }
    }
}

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
        self.visible = false;
        if let Some(wv) = &self.webview {
            let _ = wv.set_visible(false);
        }
    }

    /// Drop the native WebKit view while the host window still exists.
    /// Calling this on window-close avoids a common Linux hang where wry’s
    /// `Drop` runs after the parent X11 window is already gone.
    pub fn destroy(&mut self) {
        self.webview = None;
        self.loaded = None;
        self.last_bounds = None;
        self.visible = false;
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

    /// Show HTML for `msg_id` in `bounds` (egui screen rect), creating the webview if needed.
    pub fn show_html(
        &mut self,
        window: &impl HasWindowHandle,
        msg_id: &MessageId,
        body_html: &str,
        load_images: bool,
        dark: bool,
        bounds: Rect,
    ) {
        Self::pump_platform();

        // egui can report a tiny available rect for a frame while panels settle.
        // Creating the X11 child at that size is sticky on Linux — wait for a real area.
        if bounds.width() < 80.0 || bounds.height() < 80.0 {
            return;
        }

        let needs_reload = self
            .loaded
            .as_ref()
            .map(|(id, imgs, was_dark)| {
                id != msg_id || *imgs != load_images || *was_dark != dark
            })
            .unwrap_or(true);

        // Theme flip: recreate so WebKit chrome matches (set_background_color alone
        // is unreliable after the first paint on some Linux builds).
        if self.webview.is_some() && self.dark != dark {
            self.destroy();
        }

        // WebKitGTK child `set_bounds` often fails to grow an already-created view.
        // Recreate when the reader area changes meaningfully so size sticks.
        if self.webview.is_some() && self.needs_recreate_for_bounds(bounds) {
            self.destroy();
        }

        self.dark = dark;

        if self.webview.is_none() {
            match self.create(window, bounds) {
                Ok(()) => self.last_error = None,
                Err(e) => {
                    self.last_error = Some(e);
                    return;
                }
            }
        }

        if let Some(wv) = &self.webview {
            // Always push bounds — Linux does not auto-resize child webviews.
            let _ = wv.set_bounds(to_wry_rect(bounds));
            self.last_bounds = Some(bounds);
            Self::pump_platform();
            if !self.visible {
                let _ = wv.set_visible(true);
                self.visible = true;
            }
            if needs_reload {
                let doc = build_mail_html_document(body_html, load_images, dark);
                if let Err(e) = wv.load_html(&doc) {
                    self.last_error = Some(format!("load_html: {e}"));
                    return;
                }
                self.loaded = Some((msg_id.clone(), load_images, dark));
            }
        }
    }

    fn needs_recreate_for_bounds(&self, bounds: Rect) -> bool {
        let Some(prev) = self.last_bounds else {
            return false;
        };
        // Recreate on large size jumps (panel resize / first good layout after a tiny frame).
        (prev.width() - bounds.width()).abs() > 8.0
            || (prev.height() - bounds.height()).abs() > 8.0
            || (prev.min.x - bounds.min.x).abs() > 8.0
            || (prev.min.y - bounds.min.y).abs() > 8.0
    }

    fn create(&mut self, window: &impl HasWindowHandle, bounds: Rect) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        ensure_gtk();

        let tx = self.link_tx.clone();
        let (bg_rgba, bg_css) = if self.dark {
            ((0, 0, 0, 255), "#000")
        } else {
            ((255, 255, 255, 255), "#fff")
        };
        let placeholder = format!(
            "<!DOCTYPE html><html><body style=\"background:{bg_css};margin:0\"></body></html>"
        );
        let builder = WebViewBuilder::new()
            // Match app theme so recreate/load doesn't flash the wrong chrome color.
            .with_transparent(false)
            .with_background_color(bg_rgba)
            .with_html(placeholder)
            .with_bounds(to_wry_rect(bounds))
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
        self.visible = true;
        Ok(())
    }
}

fn to_wry_rect(bounds: Rect) -> WryRect {
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

#[cfg(target_os = "linux")]
fn ensure_gtk() {
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
