//! WYSIWYG compose body — wry WebView with a `contenteditable` page.
//!
//! Same pattern as Rusty Inbox: formatting via `document.execCommand`, HTML
//! synced to the host through IPC. The native child view sits above egui, so
//! the caller parks/hides it when the composer is not showing an editor rect.

use std::sync::mpsc::{self, Receiver, Sender};

use egui::Rect;
use wry::raw_window_handle::HasWindowHandle;
use wry::{NewWindowResponse, WebView, WebViewBuilder};
#[cfg(target_os = "linux")]
use wry::dpi::{PhysicalPosition, PhysicalSize, Position, Size};
#[cfg(target_os = "linux")]
use wry::Rect as WryRect;

#[cfg(target_os = "linux")]
use crate::mail_webview::ensure_gtk;
use crate::mail_webview::to_wry_rect;
use crate::theme;

/// Active formats under the caret (from `queryCommandState`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComposeFmtState {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub ul: bool,
    pub ol: bool,
}

#[derive(Debug, Clone)]
pub struct ComposeContent {
    pub html: String,
    pub text: String,
}

enum IpcMsg {
    Content(ComposeContent),
    Fmt(ComposeFmtState),
    Ready,
}

/// Request from the compose UI to show the editor in a screen rect.
#[derive(Clone)]
pub struct ComposeWebViewRequest {
    pub bounds: Rect,
    pub pixels_per_point: f32,
    /// Bumped when the draft HTML must be pushed into the editor.
    pub editor_rev: u64,
    pub initial_html: String,
    pub dark: bool,
    pub placeholder: String,
}

pub struct ComposeWebView {
    webview: Option<WebView>,
    loaded_rev: Option<u64>,
    last_bounds: Option<Rect>,
    last_ppp: f32,
    dark: bool,
    visible: bool,
    ipc_tx: Sender<IpcMsg>,
    ipc_rx: Receiver<IpcMsg>,
    /// Latest content pushed from the editor (input events).
    pub content: ComposeContent,
    pub fmt: ComposeFmtState,
    ready: bool,
    /// Pending flush result (send path).
    flush_rx: Option<Receiver<ComposeContent>>,
    pub last_error: Option<String>,
}

impl Default for ComposeWebView {
    fn default() -> Self {
        let (ipc_tx, ipc_rx) = mpsc::channel();
        Self {
            webview: None,
            loaded_rev: None,
            last_bounds: None,
            last_ppp: 0.0,
            dark: false,
            visible: false,
            ipc_tx,
            ipc_rx,
            content: ComposeContent {
                html: String::new(),
                text: String::new(),
            },
            fmt: ComposeFmtState::default(),
            ready: false,
            flush_rx: None,
            last_error: None,
        }
    }
}

impl ComposeWebView {
    pub fn is_alive(&self) -> bool {
        self.webview.is_some()
    }

    /// True once the page has loaded and `editor_rev` has been pushed in.
    pub fn is_synced(&self, editor_rev: u64) -> bool {
        self.ready && self.loaded_rev == Some(editor_rev)
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        if let Some(wv) = &self.webview {
            #[cfg(target_os = "linux")]
            {
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

    pub fn destroy(&mut self) {
        let wv = self.webview.take();
        self.loaded_rev = None;
        self.last_bounds = None;
        self.last_ppp = 0.0;
        self.visible = false;
        self.ready = false;
        self.flush_rx = None;
        drop(wv);
        crate::mail_webview::MailWebView::pump_platform();
    }

    /// Drain IPC: content, format state, ready. Returns true if content changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.ipc_rx.try_recv() {
            match msg {
                IpcMsg::Content(c) => {
                    self.content = c;
                    changed = true;
                }
                IpcMsg::Fmt(f) => self.fmt = f,
                IpcMsg::Ready => self.ready = true,
            }
        }
        changed
    }

    /// Ask the editor for a definitive content snapshot (async).
    /// Call [`take_flush`] on later frames until it returns Some.
    pub fn request_flush(&mut self) {
        let Some(wv) = &self.webview else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.flush_rx = Some(rx);
        let _ = wv.evaluate_script_with_callback(
            r#"(function(){
                var ed=document.getElementById('ed');
                if(!ed) return JSON.stringify({html:'',text:''});
                return JSON.stringify({html:ed.innerHTML,text:ed.innerText||''});
            })()"#,
            move |s| {
                if let Some(c) = parse_content_json(&s) {
                    let _ = tx.send(c);
                }
            },
        );
    }

    /// Take a completed flush, if any.
    pub fn take_flush(&mut self) -> Option<ComposeContent> {
        let rx = self.flush_rx.as_ref()?;
        match rx.try_recv() {
            Ok(c) => {
                self.content = c.clone();
                self.flush_rx = None;
                Some(c)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.flush_rx = None;
                None
            }
        }
    }

    pub fn exec_command(&self, cmd: &str) {
        let Some(wv) = &self.webview else {
            return;
        };
        let js = format!(
            "document.execCommand({},false,null);var ed=document.getElementById('ed');if(ed)ed.focus();report();",
            serde_json_str(cmd)
        );
        let _ = wv.evaluate_script(&js);
    }

    pub fn show(
        &mut self,
        window: &impl HasWindowHandle,
        req: &ComposeWebViewRequest,
    ) {
        crate::mail_webview::MailWebView::pump_platform();

        if req.bounds.width() < 40.0 || req.bounds.height() < 28.0 {
            return;
        }

        let ppp = req.pixels_per_point.max(0.1);
        let theme_changed = self.dark != req.dark;
        self.dark = req.dark;

        if self.webview.is_some() && self.needs_recreate_for_ppp(ppp) {
            self.destroy();
        }

        if self.webview.is_none() {
            match self.create(window, req.bounds, ppp, req.dark, &req.placeholder) {
                Ok(()) => self.last_error = None,
                Err(e) => {
                    self.last_error = Some(e);
                    return;
                }
            }
        } else if theme_changed {
            self.reload_shell(&req.placeholder);
            self.loaded_rev = None;
            self.ready = false;
        }

        if let Some(wv) = &self.webview {
            let was_hidden = !self.visible;
            let drifted = self.last_bounds.is_none_or(|prev| {
                (prev.min.x - req.bounds.min.x).abs() > 0.5
                    || (prev.min.y - req.bounds.min.y).abs() > 0.5
                    || (prev.width() - req.bounds.width()).abs() > 0.5
                    || (prev.height() - req.bounds.height()).abs() > 0.5
            });
            if was_hidden || drifted {
                let _ = wv.set_bounds(to_wry_rect(req.bounds, ppp));
                self.last_bounds = Some(req.bounds);
                self.last_ppp = ppp;
            }
            #[cfg(not(target_os = "linux"))]
            if was_hidden {
                let _ = wv.set_visible(true);
            }
            self.visible = true;
        }

        if self.ready && self.loaded_rev != Some(req.editor_rev) {
            self.set_content(&req.initial_html);
            self.loaded_rev = Some(req.editor_rev);
        }
    }

    fn set_content(&self, html: &str) {
        let Some(wv) = &self.webview else {
            return;
        };
        let js = format!("setContent({})", serde_json_str(html));
        let _ = wv.evaluate_script(&js);
    }

    fn needs_recreate_for_ppp(&self, ppp: f32) -> bool {
        self.last_ppp > 0.0 && (self.last_ppp - ppp).abs() > 0.01
    }

    fn reload_shell(&self, placeholder: &str) {
        let Some(wv) = &self.webview else {
            return;
        };
        let doc = editor_document(self.dark, placeholder);
        let _ = wv.load_html(&doc);
    }

    fn create(
        &mut self,
        window: &impl HasWindowHandle,
        bounds: Rect,
        pixels_per_point: f32,
        dark: bool,
        placeholder: &str,
    ) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        ensure_gtk();

        let tx = self.ipc_tx.clone();
        let bg = theme::mail_webview_backdrop_rgba(dark);
        let doc = editor_document(dark, placeholder);

        let builder = WebViewBuilder::new()
            .with_transparent(false)
            .with_background_color(bg)
            .with_html(doc)
            .with_bounds(to_wry_rect(bounds, pixels_per_point))
            .with_ipc_handler(move |req| {
                let body = req.body();
                if let Some(msg) = parse_ipc(body) {
                    let _ = tx.send(msg);
                }
            })
            .with_navigation_handler(|url| {
                url.starts_with("data:text/html")
                    || url == "about:blank"
                    || url.is_empty()
            })
            .with_new_window_req_handler(|_url, _features| NewWindowResponse::Deny)
            .with_download_started_handler(|_url, _path| false);

        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            builder.build_as_child(window)
        }));
        let webview = match built {
            Ok(Ok(wv)) => wv,
            Ok(Err(e)) => {
                let msg = e.to_string();
                return Err(if msg.contains("window handle kind is not supported") {
                    format!(
                        "compose webview: {msg} (needs X11/XWayland; set GDK_BACKEND=x11)"
                    )
                } else {
                    format!("compose webview: {msg}")
                });
            }
            Err(_) => {
                return Err(
                    "compose webview: crashed while creating (try GDK_BACKEND=x11)".into(),
                );
            }
        };
        let _ = webview.set_background_color(bg);
        self.webview = Some(webview);
        self.last_bounds = Some(bounds);
        self.last_ppp = pixels_per_point.max(0.1);
        self.visible = true;
        self.dark = dark;
        self.ready = false;
        self.loaded_rev = None;
        Ok(())
    }
}

fn serde_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn parse_ipc(raw: &str) -> Option<IpcMsg> {
    let raw = raw.trim();
    if raw == "ready" {
        return Some(IpcMsg::Ready);
    }
    if let Some(rest) = raw.strip_prefix("fmt:") {
        let bits: Vec<char> = rest.chars().collect();
        if bits.len() >= 6 {
            return Some(IpcMsg::Fmt(ComposeFmtState {
                bold: bits[0] == '1',
                italic: bits[1] == '1',
                underline: bits[2] == '1',
                strike: bits[3] == '1',
                ul: bits[4] == '1',
                ol: bits[5] == '1',
            }));
        }
        return None;
    }
    if let Some(json) = raw.strip_prefix("content:") {
        return parse_content_json(json).map(IpcMsg::Content);
    }
    None
}

fn parse_content_json(s: &str) -> Option<ComposeContent> {
    // Minimal parser: {"html":"...","text":"..."} with JSON string escapes.
    let s = s.trim();
    let html = extract_json_string(s, "html")?;
    let text = extract_json_string(s, "text").unwrap_or_default();
    Some(ComposeContent { html, text })
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let i = json.find(&needle)?;
    let after = &json[i + needle.len()..];
    let colon = after.find(':')?;
    let mut rest = after[colon + 1..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    rest = &rest[1..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        hex.push(chars.next()?);
                    }
                    if let Ok(v) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(v) {
                            out.push(ch);
                        }
                    }
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

fn editor_document(dark: bool, placeholder: &str) -> String {
    let bg = theme::mail_webview_backdrop_css(dark);
    // Compose editor always uses dark ink on a light-ish field; match reader paper
    // for readability in both app themes.
    let (fg, mute, caret) = if dark {
        ("#e8eaed", "#9aa0a6", "#8ab4f8")
    } else {
        ("#1f2328", "#6b7280", "#169f84")
    };
    let ph = html_escape(placeholder);
    format!(
        r##"<!DOCTYPE html><html><head><meta charset="utf-8"><style>
html,body{{margin:0;height:100%;background:{bg};color:{fg};
font:14px/1.45 system-ui,-apple-system,"Segoe UI",sans-serif;}}
#ed{{min-height:100%;outline:none;padding:8px 10px;box-sizing:border-box;word-wrap:break-word;}}
#ed:empty:before{{content:attr(data-ph);color:{mute};pointer-events:none;}}
#ed blockquote{{margin:0.6em 0;padding-left:0.8em;border-left:3px solid {mute};color:{mute};}}
a{{color:{caret};}}
</style></head><body>
<div id="ed" contenteditable="true" data-ph="{ph}"></div>
<script>
var ed=document.getElementById('ed');
var n=0;
function post(m){{try{{window.ipc.postMessage(m)}}catch(e){{}}}}
function setContent(h){{ed.innerHTML=h||'';emit();}}
function report(){{
  var k=['bold','italic','underline','strikeThrough','insertUnorderedList','insertOrderedList'];
  post('fmt:'+k.map(function(c){{return document.queryCommandState(c)?1:0}}).join(''));
}}
function snap(){{
  return JSON.stringify({{html:ed.innerHTML,text:ed.innerText||''}});
}}
function emit(){{post('content:'+snap());report();}}
ed.addEventListener('input',emit);
ed.addEventListener('keyup',report);
ed.addEventListener('mouseup',report);
ed.addEventListener('focus',report);
document.addEventListener('selectionchange',function(){{if(document.activeElement===ed)report();}});
ed.addEventListener('keydown',function(e){{
  if(e.ctrlKey||e.metaKey){{
    var k=e.key.toLowerCase();
    if(k==='b'){{e.preventDefault();document.execCommand('bold');report();}}
    else if(k==='i'){{e.preventDefault();document.execCommand('italic');report();}}
    else if(k==='u'){{e.preventDefault();document.execCommand('underline');report();}}
  }}
}});
ed.addEventListener('paste',function(e){{
  var h=e.clipboardData&&e.clipboardData.getData('text/html');
  if(!h){{
    var t=e.clipboardData&&e.clipboardData.getData('text/plain');
    if(t){{e.preventDefault();document.execCommand('insertText',false,t);}}
  }}
}});
post('ready');
emit();
</script></body></html>"##,
        bg = bg,
        fg = fg,
        mute = mute,
        caret = caret,
        ph = ph,
    )
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Turn plain draft text into simple HTML paragraphs for the editor.
pub fn plain_to_editor_html(plain: &str) -> String {
    let t = plain.trim_end();
    if t.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, para) in t.split("\n\n").enumerate() {
        if i > 0 {
            out.push_str("");
        }
        out.push_str("<p>");
        let mut first = true;
        for line in para.lines() {
            if !first {
                out.push_str("<br>");
            }
            first = false;
            out.push_str(&html_escape(line));
        }
        out.push_str("</p>");
    }
    out
}

/// Escape plain quote lines into an HTML blockquote for send.
pub fn quote_plain_to_html(quote: &str) -> String {
    let t = quote.trim();
    if t.is_empty() {
        return String::new();
    }
    let mut out = String::from("<br><blockquote>");
    let mut first = true;
    for line in t.lines() {
        if !first {
            out.push_str("<br>");
        }
        first = false;
        // Strip classic "> " prefix for cleaner HTML quotes.
        let line = line.strip_prefix("> ").unwrap_or(line);
        out.push_str(&html_escape(line));
    }
    out.push_str("</blockquote>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_to_html_paragraphs() {
        let h = plain_to_editor_html("hi\nthere\n\nbye");
        assert!(h.contains("<p>hi<br>there</p>"));
        assert!(h.contains("<p>bye</p>"));
    }

    #[test]
    fn parse_content_roundtrip() {
        let c = parse_content_json(r#"{"html":"<b>x</b>","text":"x"}"#).unwrap();
        assert_eq!(c.html, "<b>x</b>");
        assert_eq!(c.text, "x");
    }
}
