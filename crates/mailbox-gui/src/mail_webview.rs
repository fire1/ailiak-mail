//! Native OS webview (wry) for HTML mail — WebKitGTK / WKWebView / WebView2.
//!
//! Security: document is ammonia-sanitized + CSP; navigations are allowlisted;
//! remote images require an explicit opt-in rebuild.

use std::sync::mpsc::{self, Receiver, Sender};

use egui::Rect;
use mailbox_core::oauth::open_url;
use mailbox_core::{
    build_mail_html_document, classify_mail_navigation, mail_html_has_remote_images, MailNavAction,
    MessageId,
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
}

pub struct MailWebView {
    webview: Option<WebView>,
    loaded: Option<(MessageId, bool)>,
    last_bounds: Option<Rect>,
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
            visible: false,
            link_tx,
            link_rx,
            last_error: None,
        }
    }
}

impl MailWebView {
    /// Pump GTK on Linux (required alongside winit/eframe).
    pub fn pump_platform() {
        #[cfg(target_os = "linux")]
        {
            ensure_gtk();
            while gtk::events_pending() {
                let _ = gtk::main_iteration_do(false);
            }
        }
    }

    pub fn hide(&mut self) {
        self.visible = false;
        if let Some(wv) = &self.webview {
            let _ = wv.set_visible(false);
        }
    }

    pub fn destroy(&mut self) {
        self.webview = None;
        self.loaded = None;
        self.last_bounds = None;
        self.visible = false;
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
        bounds: Rect,
    ) {
        Self::pump_platform();

        let needs_reload = self
            .loaded
            .as_ref()
            .map(|(id, imgs)| id != msg_id || *imgs != load_images)
            .unwrap_or(true);

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
            if self.bounds_changed(bounds) {
                let _ = wv.set_bounds(to_wry_rect(bounds));
                self.last_bounds = Some(bounds);
            }
            if !self.visible {
                let _ = wv.set_visible(true);
                self.visible = true;
            }
            if needs_reload {
                let doc = build_mail_html_document(body_html, load_images);
                if let Err(e) = wv.load_html(&doc) {
                    self.last_error = Some(format!("load_html: {e}"));
                    return;
                }
                self.loaded = Some((msg_id.clone(), load_images));
            }
        }
    }

    pub fn has_remote_images(body_html: &str) -> bool {
        mail_html_has_remote_images(body_html)
    }

    fn bounds_changed(&self, bounds: Rect) -> bool {
        match self.last_bounds {
            Some(prev) => {
                (prev.min.x - bounds.min.x).abs() > 0.5
                    || (prev.min.y - bounds.min.y).abs() > 0.5
                    || (prev.width() - bounds.width()).abs() > 0.5
                    || (prev.height() - bounds.height()).abs() > 0.5
            }
            None => true,
        }
    }

    fn create(&mut self, window: &impl HasWindowHandle, bounds: Rect) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        ensure_gtk();

        let tx = self.link_tx.clone();
        let builder = WebViewBuilder::new()
            .with_html("<!DOCTYPE html><html><body></body></html>")
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

        let webview = builder
            .build_as_child(window)
            .map_err(|e| format!("webview: {e}"))?;
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
        if let Err(e) = gtk::init() {
            tracing::error!(error = %e, "gtk::init failed — HTML mail webview unavailable");
        }
    });
}
