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
    /// contenteditable received focus (click / tab inside the webview).
    EditorFocused,
    /// Editor asks the host to do something (e.g. `insertImage` file picker).
    HostCmd(String),
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
    /// App accent as `#rrggbb` — custom caret bar color (native caret is hidden).
    pub accent_css: String,
}

/// Bump when `editor_document` HTML/JS changes so an existing webview reloads.
const EDITOR_SHELL_REV: u32 = 17;

pub struct ComposeWebView {
    webview: Option<WebView>,
    loaded_rev: Option<u64>,
    /// Last loaded `EDITOR_SHELL_REV` (forces HTML shell refresh after code changes).
    shell_rev: u32,
    last_bounds: Option<Rect>,
    last_ppp: f32,
    dark: bool,
    accent_css: String,
    visible: bool,
    ipc_tx: Sender<IpcMsg>,
    ipc_rx: Receiver<IpcMsg>,
    /// Latest content pushed from the editor (input events).
    pub content: ComposeContent,
    pub fmt: ComposeFmtState,
    ready: bool,
    /// Pending flush result (send path).
    flush_rx: Option<Receiver<ComposeContent>>,
    /// Host actions requested by the editor (slash menu → insert image, …).
    host_cmds: Vec<String>,
    /// Set when the webview editor takes focus; cleared by [`take_editor_focused`].
    editor_focused: bool,
    /// Ctrl+Enter in the editor requested send; cleared by [`take_send_hotkey`].
    send_hotkey: bool,
    /// Last flush channel disconnected without a payload (callback failed).
    flush_failed_flag: bool,
    pub last_error: Option<String>,
}

impl Default for ComposeWebView {
    fn default() -> Self {
        let (ipc_tx, ipc_rx) = mpsc::channel();
        Self {
            webview: None,
            loaded_rev: None,
            shell_rev: 0,
            last_bounds: None,
            last_ppp: 0.0,
            dark: false,
            accent_css: String::new(),
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
            host_cmds: Vec::new(),
            editor_focused: false,
            send_hotkey: false,
            flush_failed_flag: false,
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
        self.shell_rev = 0;
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
                IpcMsg::EditorFocused => self.editor_focused = true,
                IpcMsg::HostCmd(cmd) => {
                    if cmd == "send" {
                        self.send_hotkey = true;
                    } else {
                        self.host_cmds.push(cmd);
                    }
                }
            }
        }
        changed
    }

    pub fn take_host_cmds(&mut self) -> Vec<String> {
        std::mem::take(&mut self.host_cmds)
    }

    /// True once since last take — webview body was focused (click/keyboard).
    pub fn take_editor_focused(&mut self) -> bool {
        let v = self.editor_focused;
        self.editor_focused = false;
        v
    }

    /// Ctrl+Enter in the compose body.
    pub fn take_send_hotkey(&mut self) -> bool {
        let v = self.send_hotkey;
        self.send_hotkey = false;
        v
    }

    /// Flush callback died without a payload — caller should send with synced draft.
    pub fn flush_failed(&mut self) -> bool {
        let v = self.flush_failed_flag;
        self.flush_failed_flag = false;
        v
    }

    /// Ask the editor for a definitive content snapshot (async).
    /// Call [`take_flush`] on later frames until it returns Some.
    pub fn request_flush(&mut self) {
        let Some(wv) = &self.webview else {
            self.flush_failed_flag = true;
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.flush_rx = Some(rx);
        self.flush_failed_flag = false;
        let ok = wv.evaluate_script_with_callback(
            r#"(function(){
                var ed=document.getElementById('ed');
                if(!ed) return JSON.stringify({html:'',text:''});
                var t=(ed.innerText||'').replace(/\u00a0/g,' ');
                if(!t.replace(/\s/g,'').length){{
                  var out='',walk=document.createTreeWalker(ed,NodeFilter.SHOW_TEXT,null),n;
                  while((n=walk.nextNode())) out+=n.nodeValue||'';
                  t=out.replace(/\u00a0/g,' ');
                }}
                return JSON.stringify({html:ed.innerHTML,text:t});
            })()"#,
            move |s| {
                let c = parse_content_json(&s).unwrap_or_else(|| ComposeContent {
                    html: String::new(),
                    text: String::new(),
                });
                let _ = tx.send(c);
            },
        );
        if ok.is_err() {
            self.flush_rx = None;
            self.flush_failed_flag = true;
        }
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
                self.flush_failed_flag = true;
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

    /// Bookmark the current caret so a modal file dialog cannot wipe the insert point.
    pub fn prepare_insert(&self) {
        let Some(wv) = &self.webview else {
            return;
        };
        let _ = wv.evaluate_script(
            "if(typeof saveInsertPoint==='function')saveInsertPoint();",
        );
    }

    /// Insert HTML at the caret (inline images, styled blocks, …).
    pub fn insert_html(&self, html: &str) {
        let Some(wv) = &self.webview else {
            return;
        };
        // Use the page helper — restores the bookmarked caret after the OS file dialog
        // steals focus (bare execCommand('insertHTML') silently no-ops with no selection).
        let js = format!(
            "if(typeof insertHtmlAtCaret==='function')insertHtmlAtCaret({});\
             else{{document.execCommand('insertHTML',false,{});var ed=document.getElementById('ed');if(ed)ed.focus();emit();}}",
            serde_json_str(html),
            serde_json_str(html),
        );
        let _ = wv.evaluate_script(&js);
    }

    /// Move keyboard focus into the contenteditable (clear egui field focus first).
    ///
    /// Call only when transferring focus from egui (underlay click). Do **not** call
    /// on every IPC `focus` — the webview already has focus and re-running this
    /// via `evaluate_script` fights GTK and freezes the UI.
    pub fn focus_editor(&self) {
        let Some(wv) = &self.webview else {
            return;
        };
        let _ = wv.evaluate_script(
            r#"var ed=document.getElementById('ed');
if(ed){
  ed.focus();
  var sel=window.getSelection();
  if(!sel.rangeCount||!ed.contains(sel.anchorNode)){
    if(typeof placeCaretEnd==='function') placeCaretEnd();
    else {
      var r=document.createRange();
      r.selectNodeContents(ed);
      r.collapse(false);
      sel.removeAllRanges();
      sel.addRange(r);
    }
  }
  if(typeof updateCaret==='function') updateCaret();
}"#,
        );
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
        let theme_changed = self.dark != req.dark || self.accent_css != req.accent_css;
        self.dark = req.dark;
        self.accent_css = req.accent_css.clone();

        if self.webview.is_some() && self.needs_recreate_for_ppp(ppp) {
            self.destroy();
        }

        if self.webview.is_none() {
            match self.create(
                window,
                req.bounds,
                ppp,
                req.dark,
                &req.placeholder,
                &req.accent_css,
            ) {
                Ok(()) => {
                    self.last_error = None;
                    self.shell_rev = EDITOR_SHELL_REV;
                }
                Err(e) => {
                    self.last_error = Some(e);
                    return;
                }
            }
        } else if theme_changed {
            self.reload_shell(&req.placeholder, &req.accent_css);
            self.shell_rev = EDITOR_SHELL_REV;
            self.loaded_rev = None;
            self.ready = false;
        }

        if self.webview.is_some() && self.shell_rev != EDITOR_SHELL_REV {
            self.reload_shell(&req.placeholder, &req.accent_css);
            self.shell_rev = EDITOR_SHELL_REV;
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

    fn reload_shell(&self, placeholder: &str, accent_css: &str) {
        let Some(wv) = &self.webview else {
            return;
        };
        let doc = editor_document(self.dark, placeholder, accent_css);
        let _ = wv.load_html(&doc);
    }

    fn create(
        &mut self,
        window: &impl HasWindowHandle,
        bounds: Rect,
        pixels_per_point: f32,
        dark: bool,
        placeholder: &str,
        accent_css: &str,
    ) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        ensure_gtk();

        let tx = self.ipc_tx.clone();
        let bg = theme::mail_webview_backdrop_rgba(dark);
        let doc = editor_document(dark, placeholder, accent_css);

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
    if raw == "focus" {
        return Some(IpcMsg::EditorFocused);
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
    if let Some(cmd) = raw.strip_prefix("cmd:") {
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            return Some(IpcMsg::HostCmd(cmd.to_string()));
        }
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

fn editor_document(dark: bool, placeholder: &str, accent_css: &str) -> String {
    let bg = theme::mail_webview_backdrop_css(dark);
    // Native WebKitGTK caret is unreliable here — hide it and paint a terminal-style bar.
    let (fg, mute) = if dark {
        ("#e8eaed", "#9aa0a6")
    } else {
        ("#1f2328", "#6b7280")
    };
    let accent = if accent_css.starts_with('#') && accent_css.len() >= 4 {
        accent_css
    } else {
        "#169f84"
    };
    let ph = html_escape(placeholder);
    format!(
        r##"<!DOCTYPE html><html><head><meta charset="utf-8"><style>
html,body{{margin:0;height:100%;background:{bg};color:{fg};
font:14px/1.45 system-ui,-apple-system,"Segoe UI",sans-serif;}}
#ed{{min-height:100%;outline:none;padding:8px 10px;box-sizing:border-box;word-wrap:break-word;
  cursor:text;color:{fg};caret-color:transparent;}}
#ed:empty:before{{content:attr(data-ph);color:{mute};pointer-events:none;}}
#ed blockquote{{margin:0.6em 0;padding-left:0.8em;border-left:3px solid {mute};color:{mute};}}
#ed h1{{font-size:1.6em;margin:0.4em 0;}}
#ed h2{{font-size:1.35em;margin:0.4em 0;}}
#ed h3{{font-size:1.15em;margin:0.4em 0;}}
#ed p{{margin:0 0 0.6em;}}
#ed img{{max-width:100%;height:auto;border-radius:4px;cursor:pointer;}}
#ed img.sel{{outline:2px solid {accent};outline-offset:2px;}}
a{{color:{accent};}}
/* Terminal-style bar caret — WebKitGTK native caret often never paints. */
#caret{{display:none;position:fixed;z-index:10;width:3px;border-radius:1px;
  background:{accent};pointer-events:none;will-change:opacity;
  animation:caretBlink 1.05s step-end infinite;}}
@keyframes caretBlink{{50%{{opacity:0;}}}}
/* Drag handle — bottom-right of the selected image. */
#imgHandle{{display:none;position:fixed;z-index:16;width:14px;height:14px;box-sizing:border-box;
  background:{accent};border:2px solid #fff;border-radius:3px;cursor:nwse-resize;
  box-shadow:0 1px 4px rgba(0,0,0,.25);}}
#slash{{display:none;position:fixed;z-index:20;min-width:220px;max-height:280px;overflow:auto;
  background:#fff;color:#1f2328;border-radius:10px;box-shadow:0 8px 28px rgba(0,0,0,.18);
  border:1px solid #e5e7eb;padding:6px 0;font:13px/1.35 system-ui,sans-serif;}}
#slash .sec{{padding:6px 14px 4px;font-size:11px;letter-spacing:.04em;color:#9ca3af;text-transform:uppercase;}}
#slash .row{{display:flex;align-items:center;gap:10px;padding:8px 14px;cursor:pointer;}}
#slash .row:hover{{background:#f3e8ff;}}
#slash .row.sel{{background:#ede9fe;outline:2px solid #a78bfa;outline-offset:-2px;}}
#slash .ico{{width:22px;color:#6b7280;font-weight:600;font-size:12px;}}
</style></head><body>
<div id="ed" contenteditable="true" data-ph="{ph}"></div>
<div id="caret"></div>
<div id="imgHandle" title="Drag to resize"></div>
<div id="slash"></div>
<script>
var ed=document.getElementById('ed');
var caretEl=document.getElementById('caret');
var slash=document.getElementById('slash');
var slashOpen=false,slashIdx=0,slashQuery='',slashRange=null;
var lastInsertSel=null;
var STYLES=[
  {{id:'p',label:'Text',ico:'T'}},
  {{id:'h1',label:'Heading 1',ico:'H1'}},
  {{id:'h2',label:'Heading 2',ico:'H2'}},
  {{id:'h3',label:'Heading 3',ico:'H3'}},
  {{id:'bold',label:'Bold',ico:'B'}},
  {{id:'italic',label:'Italic',ico:'I'}},
  {{id:'ul',label:'Bullet List',ico:'•'}},
  {{id:'img',label:'Image',ico:'▣'}}
];
function post(m){{try{{window.ipc.postMessage(m)}}catch(e){{}}}}
function isEmpty(){{
  var t=(ed.innerText||'').replace(/\u00a0/g,' ').trim();
  return !t;
}}
function ensureBlockBr(block){{
  // Only pad an existing empty block — never fill a truly empty #ed (breaks :empty caret).
  if(!block||block.querySelector('br')) return;
  if((block.textContent||'').replace(/\u00a0/g,' ').trim()) return;
  block.appendChild(document.createElement('br'));
}}
function placeCaretEnd(){{
  var sel=window.getSelection();
  var r=document.createRange();
  r.selectNodeContents(ed);
  r.collapse(false);
  sel.removeAllRanges();
  sel.addRange(r);
  updateCaret();
}}
function hideCaret(){{
  if(caretEl) caretEl.style.display='none';
}}
function updateCaret(){{
  if(!caretEl) return;
  if(document.activeElement!==ed||slashOpen){{hideCaret();return;}}
  var sel=window.getSelection();
  if(!sel||!sel.rangeCount||!sel.isCollapsed){{hideCaret();return;}}
  var r=sel.getRangeAt(0);
  if(!ed.contains(r.commonAncestorContainer)){{hideCaret();return;}}
  var rect=r.getBoundingClientRect();
  if(!(rect.height>0)){{
    // Empty / collapsed-at-block: probe with a ZWSP marker (WebKit often returns 0x0).
    try{{
      var probe=document.createElement('span');
      probe.textContent='\u200b';
      probe.style.cssText='display:inline;padding:0;margin:0;border:0;line-height:inherit;font:inherit;';
      r.insertNode(probe);
      rect=probe.getBoundingClientRect();
      var p=probe.parentNode;
      p.removeChild(probe);
      p.normalize();
      sel.removeAllRanges();
      sel.addRange(r);
    }}catch(e){{
      var er=ed.getBoundingClientRect();
      rect={{left:er.left+10,top:er.top+8,height:20,width:0,bottom:er.top+28,right:er.left+10}};
    }}
  }}
  if(!(rect.height>0)){{hideCaret();return;}}
  caretEl.style.left=Math.round(rect.left)+'px';
  caretEl.style.top=Math.round(rect.top)+'px';
  caretEl.style.height=Math.max(14,Math.round(rect.height))+'px';
  caretEl.style.display='block';
  // Restart blink so it stays solid while typing.
  caretEl.style.animation='none';
  void caretEl.offsetWidth;
  caretEl.style.animation='';
}}
function setContent(h){{
  ed.innerHTML=h||'';
  emit();
  updateCaret();
}}
function report(){{
  var k=['bold','italic','underline','strikeThrough','insertUnorderedList','insertOrderedList'];
  post('fmt:'+k.map(function(c){{return document.queryCommandState(c)?1:0}}).join(''));
}}
function editorPlain(){{
  var t=(ed.innerText||'').replace(/\u00a0/g,' ');
  if(t.replace(/\s/g,'').length) return t;
  var out='';
  var walk=document.createTreeWalker(ed,NodeFilter.SHOW_TEXT,null);
  var n;
  while((n=walk.nextNode())) out+=n.nodeValue||'';
  return out.replace(/\u00a0/g,' ');
}}
function snap(){{
  return JSON.stringify({{html:ed.innerHTML,text:editorPlain()}});
}}
function emit(){{post('content:'+snap());report();}}
function filteredStyles(){{
  var q=slashQuery.toLowerCase();
  if(!q) return STYLES.slice();
  return STYLES.filter(function(s){{return s.label.toLowerCase().indexOf(q)>=0||s.id.indexOf(q)>=0;}});
}}
function renderSlash(){{
  var items=filteredStyles();
  if(!items.length){{hideSlash();return;}}
  if(slashIdx<0||slashIdx>=items.length) slashIdx=0;
  var html='<div class="sec">Style</div>';
  items.forEach(function(s,i){{
    html+='<div class="row'+(i===slashIdx?' sel':'')+'" data-i="'+i+'"><span class="ico">'+s.ico+'</span>'+s.label+'</div>';
  }});
  slash.innerHTML=html;
  slash.style.display='block';
  slashOpen=true;
  Array.prototype.forEach.call(slash.querySelectorAll('.row'),function(row){{
    row.onmousedown=function(e){{e.preventDefault();pickSlash(+row.getAttribute('data-i'));}};
  }});
}}
function hideSlash(){{
  slash.style.display='none';
  slashOpen=false;
  slashQuery='';
  slashIdx=0;
  slashRange=null;
}}
function slashTrigger(){{
  var sel=window.getSelection();
  if(!sel||!sel.rangeCount) return null;
  var r=sel.getRangeAt(0);
  if(!ed.contains(r.startContainer)) return null;
  var node=r.startContainer;
  if(node.nodeType!==3) return null;
  var text=node.textContent||'';
  var off=r.startOffset;
  var before=text.slice(0,off);
  var m=before.match(/(?:^|[\s\u00a0])\/([^\s]*)$/);
  if(!m) return null;
  var slashPos=before.lastIndexOf('/');
  if(slashPos<0) return null;
  return {{node:node,start:slashPos,end:off,query:m[1]}};
}}
function placeSlash(){{
  var sel=window.getSelection();
  if(!sel||!sel.rangeCount) return;
  var rect=sel.getRangeAt(0).getBoundingClientRect();
  if(!rect||(rect.width===0&&rect.height===0&&savedSel)){{
    try{{rect=savedSel.getBoundingClientRect();}}catch(e){{}}
  }}
  var top=(rect&&rect.bottom||8)+6;
  var left=rect&&rect.left||8;
  if(top+200>window.innerHeight) top=Math.max(8,(rect&&rect.top||8)-200);
  if(left+240>window.innerWidth) left=Math.max(8,window.innerWidth-240);
  slash.style.top=top+'px';
  slash.style.left=left+'px';
}}
function spaceBeforeSlash(info){{
  if(!info||!info.node||info.node.nodeType!==3) return false;
  var t=info.node.textContent||'';
  var s=info.start;
  if(s>0) return /[\s\u00a0]/.test(t.charAt(s-1));
  var prev=info.node.previousSibling;
  while(prev){{
    if(prev.nodeType===3){{
      var pt=prev.textContent||'';
      if(!pt.length){{prev=prev.previousSibling;continue;}}
      return /[\s\u00a0]/.test(pt.charAt(pt.length-1));
    }}
    if(prev.nodeType===1) return false;
    prev=prev.previousSibling;
  }}
  return false;
}}
function caretHasSpaceBefore(){{
  var sel=window.getSelection();
  if(!sel||!sel.rangeCount||!sel.isCollapsed) return true;
  var r=sel.getRangeAt(0);
  var n=r.startContainer,o=r.startOffset;
  if(n.nodeType===3){{
    if(o>0) return /[\s\u00a0]/.test(n.data.charAt(o-1));
    var prev=n.previousSibling;
    while(prev){{
      if(prev.nodeType===3){{
        var pt=prev.data||'';
        if(!pt.length){{prev=prev.previousSibling;continue;}}
        return /[\s\u00a0]/.test(pt.charAt(pt.length-1));
      }}
      if(prev.nodeType===1){{
        // Boundary after an element — only treat as "has space" if that el ends with space.
        var t=(prev.innerText||prev.textContent||'');
        return /[\s\u00a0]$/.test(t.replace(/\u200b/g,''));
      }}
      prev=prev.previousSibling;
    }}
    return false; // start of content — caller may still need a separator after a prior word in another node
  }}
  if(n.nodeType===1&&o>0){{
    var c=n.childNodes[o-1];
    if(!c) return false;
    if(c.nodeType===3){{
      var ct=c.data||'';
      return ct.length>0&&/[\s\u00a0]/.test(ct.charAt(ct.length-1));
    }}
    var it=(c.innerText||c.textContent||'');
    return /[\s\u00a0]$/.test(it.replace(/\u200b/g,''));
  }}
  return false;
}}
function protectSpaceBeforeCaret(){{
  // WebKit formatBlock collapses trailing ASCII spaces — upgrade to NBSP before applying.
  var sel=window.getSelection();
  if(!sel||!sel.rangeCount||!sel.isCollapsed) return;
  var r=sel.getRangeAt(0);
  var n=r.startContainer,o=r.startOffset;
  if(n.nodeType===3&&o>0&&n.data.charAt(o-1)===' '){{
    n.data=n.data.slice(0,o-1)+'\u00a0'+n.data.slice(o);
    r.setStart(n,o);
    r.collapse(true);
    sel.removeAllRanges();
    sel.addRange(r);
    return;
  }}
  if(n.nodeType===3&&o===0){{
    var prev=n.previousSibling;
    if(prev&&prev.nodeType===3&&prev.data&&prev.data.charAt(prev.data.length-1)===' '){{
      prev.data=prev.data.slice(0,-1)+'\u00a0';
    }}
  }}
}}
function ensureSpaceBeforeCaret(){{
  if(caretHasSpaceBefore()) return;
  // NBSP survives formatBlock; a normal space often does not.
  document.execCommand('insertText',false,'\u00a0');
}}
function deleteSlashToken(){{
  // Rewrite the text node (avoid Range.deleteContents eating neighbors).
  if(!slashRange) return false;
  var node=slashRange.node;
  var start=slashRange.start;
  var end=slashRange.end;
  var keepSpace=false;
  try{{
    if(!node||node.nodeType!==3||!node.parentNode){{
      slashRange=null;
      placeCaretEnd();
      return false;
    }}
    var t=node.textContent||'';
    if(t.charAt(start)!=='/'){{
      var i=t.lastIndexOf('/',end);
      if(i>=0) start=i;
    }}
    keepSpace=start>0&&/[\s\u00a0]/.test(t.charAt(start-1));
    if(!keepSpace&&start===0) keepSpace=spaceBeforeSlash({{node:node,start:0,end:end,query:''}});
    var newText=t.slice(0,start)+t.slice(end);
    // ASCII trailing spaces are collapsed by WebKit formatBlock — store NBSP instead.
    if(keepSpace&&start>0&&newText.charAt(start-1)===' '){{
      newText=newText.slice(0,start-1)+'\u00a0'+newText.slice(start);
    }}
    if(keepSpace&&start===0){{
      var prevSp=node.previousSibling;
      while(prevSp){{
        if(prevSp.nodeType===3){{
          var pdt=prevSp.data||'';
          if(!pdt.length){{prevSp=prevSp.previousSibling;continue;}}
          if(pdt.charAt(pdt.length-1)===' ') prevSp.data=pdt.slice(0,-1)+'\u00a0';
          break;
        }}
        if(prevSp.nodeType===1) break;
        prevSp=prevSp.previousSibling;
      }}
    }}
    var sel=window.getSelection();
    var r=document.createRange();
    if(!newText){{
      var parent=node.parentNode;
      var idx=Array.prototype.indexOf.call(parent.childNodes,node);
      parent.removeChild(node);
      if(parent.childNodes.length===0&&parent!==ed){{
        parent.appendChild(document.createElement('br'));
        r.setStart(parent,0);
      }} else {{
        r.setStart(parent,Math.min(idx,parent.childNodes.length));
      }}
      r.collapse(true);
    }} else {{
      node.textContent=newText;
      r.setStart(node,Math.min(start,newText.length));
      r.collapse(true);
    }}
    sel.removeAllRanges();
    sel.addRange(r);
  }}catch(e){{
    placeCaretEnd();
  }}
  slashRange=null;
  var sel=window.getSelection();
  if(!sel.rangeCount||!ed.contains(sel.anchorNode)) placeCaretEnd();
  return keepSpace;
}}
function applyPlain(){{
  var sel=window.getSelection();
  if(!sel||!sel.rangeCount) return;
  var r=sel.getRangeAt(0);
  if(r.collapsed){{
    document.execCommand('removeFormat');
    document.execCommand('unlink');
    ['bold','italic','underline','strikeThrough'].forEach(function(c){{
      if(document.queryCommandState(c)) document.execCommand(c);
    }});
    return;
  }}
  var text=r.toString();
  r.deleteContents();
  var node=document.createTextNode(text);
  r.insertNode(node);
  r.setStart(node,0);
  r.setEnd(node,node.data.length);
  sel.removeAllRanges();
  sel.addRange(r);
}}
function applyBlock(tag){{
  // Prefer formatBlock when there is real content; fall back to insertHTML (first-command / empty).
  var had=!!(ed.innerText||'').replace(/\u00a0/g,' ').trim();
  if(had){{
    document.execCommand('formatBlock',false,tag);
  }}
  var block=null;
  try{{
    var sel=window.getSelection();
    if(sel&&sel.rangeCount){{
      var n=sel.getRangeAt(0).commonAncestorContainer;
      if(n.nodeType===3) n=n.parentNode;
      while(n&&n!==ed){{
        if(n.nodeType===1&&n.tagName&&n.tagName.toLowerCase()===tag){{block=n;break;}}
        n=n.parentNode;
      }}
    }}
  }}catch(e){{}}
  if(!block) block=ed.querySelector(tag);
  if(!block){{
    document.execCommand('insertHTML',false,'<'+tag+'><br></'+tag+'>');
    block=ed.querySelector(tag);
  }}
  if(block) ensureBlockBr(block);
}}
function applyStyle(id){{
  ed.focus();
  var sel=window.getSelection();
  if(!sel.rangeCount||!ed.contains(sel.anchorNode)) placeCaretEnd();
  if(id==='ul') document.execCommand('insertUnorderedList');
  else if(id==='bold') document.execCommand('bold');
  else if(id==='italic') document.execCommand('italic');
  else if(id==='p'){{
    applyPlain();
    applyBlock('p');
  }}
  else if(id==='img'){{
    saveInsertPoint();
    post('cmd:insertImage');
    return;
  }}
  else applyBlock(id);
  emit();
}}
var savedSel=null;
function saveSel(){{
  var sel=window.getSelection();
  if(!sel||!sel.rangeCount){{savedSel=null;return;}}
  var r=sel.getRangeAt(0);
  if(!ed.contains(r.commonAncestorContainer)){{savedSel=null;return;}}
  if(r.collapsed){{savedSel=null;return;}}
  savedSel=r.cloneRange();
}}
function restoreSel(){{
  if(!savedSel) return false;
  try{{
    var sel=window.getSelection();
    sel.removeAllRanges();
    sel.addRange(savedSel);
    return true;
  }}catch(e){{return false;}}
}}
function pickSlash(i){{
  var items=filteredStyles();
  var s=items[i];
  if(!s) return;
  var keepSel=!!savedSel;
  var keptSpace=false;
  if(slashRange){{
    keptSpace=spaceBeforeSlash(slashRange);
    deleteSlashToken();
  }}
  hideSlash();
  ed.focus();
  if(keepSel) restoreSel();
  else {{
    var sel=window.getSelection();
    if(!sel.rangeCount||!ed.contains(sel.anchorNode)) placeCaretEnd();
  }}
  // Freeze the word-separating space as NBSP before execCommand can collapse it.
  if(keptSpace){{
    protectSpaceBeforeCaret();
    ensureSpaceBeforeCaret();
  }}
  applyStyle(s.id);
  if(keptSpace){{
    protectSpaceBeforeCaret();
    ensureSpaceBeforeCaret();
  }}
  savedSel=null;
  ed.focus();
  updateCaret();
}}
function syncSlashFromInput(){{
  if(savedSel) return;
  var info=slashTrigger();
  if(info){{
    var q=info.query||'';
    var justOpened=!slashOpen;
    var queryChanged=q!==slashQuery;
    slashRange=info;
    slashQuery=q;
    if(justOpened||queryChanged||!q) slashIdx=0;
    placeSlash();
    renderSlash();
  }} else if(slashOpen){{
    hideSlash();
  }}
}}
ed.addEventListener('input',function(){{
  emit();
  syncSlashFromInput();
  updateCaret();
}});
ed.addEventListener('keyup',function(){{report();updateCaret();}});
ed.addEventListener('mouseup',function(){{report();updateCaret();}});
ed.addEventListener('focus',function(){{
  post('focus');
  report();
  updateCaret();
}});
ed.addEventListener('blur',hideCaret);
ed.addEventListener('mousedown',function(e){{
  post('focus');
  if(e.target&&e.target.tagName==='IMG'&&ed.contains(e.target)){{
    e.preventDefault();
    selectComposeImg(e.target);
    return;
  }}
  if(!(e.target&&e.target.id==='imgHandle')) clearComposeImg();
}});
var imgHandle=document.getElementById('imgHandle');
var selectedImg=null,imgDrag=null;
function clearComposeImg(){{
  if(selectedImg) selectedImg.classList.remove('sel');
  selectedImg=null;
  if(imgHandle) imgHandle.style.display='none';
}}
function placeImgHandle(){{
  if(!imgHandle||!selectedImg) return;
  var r=selectedImg.getBoundingClientRect();
  imgHandle.style.left=Math.round(r.right-8)+'px';
  imgHandle.style.top=Math.round(r.bottom-8)+'px';
  imgHandle.style.display='block';
}}
function selectComposeImg(img){{
  clearComposeImg();
  selectedImg=img;
  img.classList.add('sel');
  placeImgHandle();
  hideCaret();
}}
if(imgHandle){{
  imgHandle.addEventListener('mousedown',function(e){{
    if(!selectedImg) return;
    e.preventDefault();
    e.stopPropagation();
    post('focus');
    var w=selectedImg.getBoundingClientRect().width;
    imgDrag={{startX:e.clientX,startW:w}};
  }});
}}
document.addEventListener('mousemove',function(e){{
  if(!imgDrag||!selectedImg) return;
  var dx=e.clientX-imgDrag.startX;
  var edW=ed.getBoundingClientRect().width-20;
  var nw=Math.round(Math.max(80,Math.min(edW,imgDrag.startW+dx)));
  selectedImg.style.width=nw+'px';
  selectedImg.style.height='auto';
  selectedImg.style.maxWidth='100%';
  selectedImg.setAttribute('width',String(nw));
  selectedImg.removeAttribute('height');
  placeImgHandle();
}});
document.addEventListener('mouseup',function(){{
  if(imgDrag){{
    imgDrag=null;
    emit();
    placeImgHandle();
  }}
}});
document.addEventListener('selectionchange',function(){{
  if(document.activeElement===ed){{saveInsertPoint();report();updateCaret();}}
  else hideCaret();
}});
window.addEventListener('resize',function(){{updateCaret();placeImgHandle();}});
window.addEventListener('scroll',function(){{updateCaret();placeImgHandle();}},true);
ed.addEventListener('keydown',function(e){{
  if(selectedImg&&(e.key==='Backspace'||e.key==='Delete')&&!slashOpen){{
    e.preventDefault();
    var img=selectedImg;
    clearComposeImg();
    if(img&&img.parentNode) img.parentNode.removeChild(img);
    emit();
    updateCaret();
    return;
  }}
  if(e.key==='Escape'&&selectedImg){{clearComposeImg();return;}}
  if(e.key==='/'&&!slashOpen&&!e.ctrlKey&&!e.metaKey&&!e.altKey){{
    var sel=window.getSelection();
    if(sel&&!sel.isCollapsed&&ed.contains(sel.anchorNode)){{
      e.preventDefault();
      saveSel();
      slashRange=null;
      slashQuery='';
      slashIdx=0;
      placeSlash();
      renderSlash();
      return;
    }}
  }}
  if(slashOpen){{
    var items=filteredStyles();
    if(e.key==='ArrowDown'){{e.preventDefault();e.stopPropagation();slashIdx=(slashIdx+1)%items.length;renderSlash();return;}}
    if(e.key==='ArrowUp'){{e.preventDefault();e.stopPropagation();slashIdx=(slashIdx-1+items.length)%items.length;renderSlash();return;}}
    if(e.key==='Enter'||e.key==='Tab'){{
      e.preventDefault();
      e.stopPropagation();
      pickSlash(slashIdx);
      return;
    }}
    if(e.key==='Escape'){{e.preventDefault();hideSlash();savedSel=null;return;}}
  }}
  if((e.ctrlKey||e.metaKey)&&e.key==='Enter'){{
    e.preventDefault();
    e.stopPropagation();
    post('cmd:send');
    return;
  }}
  if(e.ctrlKey||e.metaKey){{
    var k=e.key.toLowerCase();
    if(k==='b'){{e.preventDefault();document.execCommand('bold');report();}}
    else if(k==='i'){{e.preventDefault();document.execCommand('italic');report();}}
    else if(k==='u'){{e.preventDefault();document.execCommand('underline');report();}}
  }}
}},true);
ed.addEventListener('paste',function(e){{
  var t=e.clipboardData&&e.clipboardData.getData('text/plain');
  if(t!=null){{
    e.preventDefault();
    document.execCommand('insertText',false,t);
    emit();
  }}
}});
function saveInsertPoint(){{
  var sel=window.getSelection();
  if(!sel||!sel.rangeCount){{lastInsertSel=null;return;}}
  var r=sel.getRangeAt(0);
  if(!ed.contains(r.commonAncestorContainer)){{lastInsertSel=null;return;}}
  try{{lastInsertSel=r.cloneRange();}}catch(e){{lastInsertSel=null;}}
}}
function restoreInsertPoint(){{
  ed.focus();
  var sel=window.getSelection();
  if(lastInsertSel){{
    try{{
      sel.removeAllRanges();
      sel.addRange(lastInsertSel);
      return true;
    }}catch(e){{}}
  }}
  if(!sel.rangeCount||!ed.contains(sel.anchorNode)) placeCaretEnd();
  return false;
}}
function insertHtmlAtCaret(html){{
  // File dialog / toolbar click clears the live selection — restore bookmark first.
  restoreInsertPoint();
  var sel=window.getSelection();
  if(!sel.rangeCount||!ed.contains(sel.anchorNode)) placeCaretEnd();
  var ok=false;
  try{{ok=document.execCommand('insertHTML',false,html);}}catch(e){{ok=false;}}
  if(!ok){{
    try{{
      sel=window.getSelection();
      if(!sel.rangeCount) placeCaretEnd();
      sel=window.getSelection();
      var r=sel.getRangeAt(0);
      r.deleteContents();
      var tmp=document.createElement('div');
      tmp.innerHTML=html;
      var frag=document.createDocumentFragment();
      var node,last=null;
      while((node=tmp.firstChild)){{last=frag.appendChild(node);}}
      r.insertNode(frag);
      if(last){{
        r.setStartAfter(last);
        r.collapse(true);
        sel.removeAllRanges();
        sel.addRange(r);
      }}
    }}catch(e2){{
      ed.insertAdjacentHTML('beforeend',html);
    }}
  }}
  saveInsertPoint();
  emit();
  updateCaret();
}}
window.applyStyle=applyStyle;
window.placeCaretEnd=placeCaretEnd;
window.updateCaret=updateCaret;
window.saveInsertPoint=saveInsertPoint;
window.insertHtmlAtCaret=insertHtmlAtCaret;
post('ready');
emit();
updateCaret();
</script></body></html>"##,
        bg = bg,
        fg = fg,
        mute = mute,
        accent = accent,
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
