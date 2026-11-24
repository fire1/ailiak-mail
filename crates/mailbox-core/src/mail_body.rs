//! Safe mail body model for the reader: sanitized HTML → structured blocks.
//!
//! Security defaults (email best practice):
//! - No scripts / event handlers / iframes (ammonia allowlist)
//! - Links only `http` / `https` / `mailto`
//! - Remote images omitted unless `load_remote_images` (tracking pixels / privacy)
//! - Tracking-like images are always blocked (heuristics), even when loading is allowed

use ammonia::Builder;

use crate::message_parse::{html_to_text, looks_like_html, readable_body};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineSeg {
    Text(String),
    /// Allowed schemes only — validated at build time.
    Link { text: String, url: String },
}

/// Why a remote image looks like a privacy / tracking threat (static HTML only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageThreatKind {
    /// Width/height ≤ 2px (classic open-tracking pixel).
    TinySize,
    /// Hidden via CSS (`display:none`, `visibility:hidden`, near-zero opacity).
    Hidden,
    /// URL path/query matches common open-tracking tokens.
    KnownPattern,
    /// Host is on a short denylist of known trackers.
    SuspiciousHost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteImageRisk {
    pub url: String,
    pub alt: String,
    pub threats: Vec<ImageThreatKind>,
}

impl RemoteImageRisk {
    pub fn has_threats(&self) -> bool {
        !self.threats.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyBlock {
    Paragraph(Vec<InlineSeg>),
    Heading { level: u8, spans: Vec<InlineSeg> },
    /// Shown only when remote images are enabled and not flagged as a threat.
    Image { alt: String, url: String },
    ImageBlocked { alt: String },
    Bullet(Vec<InlineSeg>),
    Quote(Vec<InlineSeg>),
    Rule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailBody {
    pub blocks: Vec<BodyBlock>,
    pub has_remote_images: bool,
    /// Remote images that matched tracking / privacy heuristics (always blocked).
    pub image_threats: Vec<RemoteImageRisk>,
}

impl MailBody {
    pub fn has_image_threats(&self) -> bool {
        !self.image_threats.is_empty()
    }
}

/// Build a reader body from stored plain + optional HTML.
pub fn build_mail_body(body_text: &str, body_html: &str, load_remote_images: bool) -> MailBody {
    let html = body_html.trim();
    if !html.is_empty() {
        return from_html(html, load_remote_images);
    }
    // Cached HTML may still live in body_text from older syncs.
    if looks_like_html(body_text) {
        return from_html(body_text, load_remote_images);
    }
    from_plain(body_text)
}

fn from_plain(text: &str) -> MailBody {
    let normalized = normalize_plain(text);
    let readable = readable_body(&normalized);
    let mut blocks = Vec::new();

    // Line-oriented plain (LinkedIn digests, marketing) keeps structure. Soft-wrapped
    // prose (long lines) still collapses runs of short wrapped lines into paragraphs.
    let lines: Vec<&str> = readable.lines().map(str::trim_end).collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }
        if is_rule_line(line) {
            blocks.push(BodyBlock::Rule);
            i += 1;
            continue;
        }

        // Soft-wrap join: consecutive medium lines that look like flowed prose.
        let mut chunk = line.to_string();
        if looks_like_soft_wrap_line(line) {
            while i + 1 < lines.len() {
                let next = lines[i + 1].trim();
                if next.is_empty() || is_rule_line(next) || !looks_like_soft_wrap_line(next) {
                    break;
                }
                if next.starts_with("http://") || next.starts_with("https://") {
                    break;
                }
                chunk.push(' ');
                chunk.push_str(next);
                i += 1;
            }
        }

        let spans = parse_plain_spans(&chunk);
        if !spans_empty(&spans) {
            let trimmed = chunk.trim_start();
            if trimmed.starts_with("- ") || trimmed.starts_with("• ") {
                let spans = strip_leading_bullet(spans);
                if !spans_empty(&spans) {
                    blocks.push(BodyBlock::Bullet(spans));
                }
            } else {
                blocks.push(BodyBlock::Paragraph(spans));
            }
        }
        i += 1;
    }

    MailBody {
        blocks,
        has_remote_images: false,
        image_threats: Vec::new(),
    }
}

fn is_rule_line(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 8
        && t
            .chars()
            .all(|c| c == '-' || c == '—' || c == '_' || c == '=' || c.is_whitespace())
}

fn looks_like_soft_wrap_line(line: &str) -> bool {
    let len = line.chars().count();
    // Flowed plain text is typically wrapped ~60–80 cols, not short titles / CTAs.
    (60..100).contains(&len)
        && !line.contains("http://")
        && !line.contains("https://")
        && !line.ends_with(':')
}

fn normalize_plain(s: &str) -> String {
    s.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
        .replace("<BR>", "\n")
}

fn strip_leading_bullet(mut spans: Vec<InlineSeg>) -> Vec<InlineSeg> {
    if let Some(InlineSeg::Text(t)) = spans.first_mut() {
        let trimmed = t.trim_start();
        *t = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("• "))
            .unwrap_or(trimmed)
            .to_string();
    }
    spans
}

/// Turn marketing plain text into text + safe links.
/// Handles `Label <[[url]]>`, `Label <url>`, and bare `Label: https://…` (LinkedIn).
fn parse_plain_spans(s: &str) -> Vec<InlineSeg> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some((url, consumed)) = take_angle_url(&s[i..]) {
                // Angle orphans are usually logos / tracking stubs — omit if unlabeled.
                push_plain_link(&mut spans, &mut buf, &url, false);
                i += consumed;
                continue;
            }
        }
        if bytes[i] == b'h'
            && (s[i..].starts_with("https://") || s[i..].starts_with("http://"))
        {
            if let Some((url, consumed)) = take_bare_url(&s[i..]) {
                // Bare URLs are intentional; show a compact label when unlabeled.
                push_plain_link(&mut spans, &mut buf, &url, true);
                i += consumed;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap_or('?');
        buf.push(ch);
        i += ch.len_utf8();
    }
    let rest = buf.trim();
    if !rest.is_empty() {
        spans.push(InlineSeg::Text(rest.to_string()));
    }
    collapse_spans(spans)
}

fn push_plain_link(
    spans: &mut Vec<InlineSeg>,
    buf: &mut String,
    url: &str,
    allow_unlabeled: bool,
) {
    let (prefix, label) = split_link_label(buf);
    buf.clear();
    if !prefix.is_empty() {
        spans.push(InlineSeg::Text(format!("{prefix} ")));
    }
    if let Some(safe) = safe_url(url) {
        if !label.is_empty() {
            spans.push(InlineSeg::Link {
                text: shorten_link_label(&label),
                url: safe,
            });
        } else if allow_unlabeled {
            spans.push(InlineSeg::Link {
                text: compact_url_label(&safe),
                url: safe,
            });
        }
    } else if !label.is_empty() {
        spans.push(InlineSeg::Text(label));
    }
}

fn shorten_link_label(label: &str) -> String {
    let t = label.trim().trim_end_matches(':').trim();
    if t.chars().count() > 80 {
        format!("{}…", t.chars().take(77).collect::<String>())
    } else {
        t.to_string()
    }
}

fn compact_url_label(url: &str) -> String {
    let without_scheme = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let path = without_scheme.split('?').next().unwrap_or(without_scheme);
    if path.chars().count() <= 48 {
        path.to_string()
    } else {
        format!("{}…", path.chars().take(45).collect::<String>())
    }
}

/// Split accumulated text into (prefix, link_label) before an angle-bracket URL.
fn split_link_label(buf: &str) -> (String, String) {
    let trimmed = buf.trim_end();
    if trimmed.is_empty() {
        return (String::new(), String::new());
    }
    let mut best: Option<usize> = None;
    for (pat, add) in [
        (" | ", 3usize),
        ("| ", 2),
        (". ", 2),
        (": ", 2),
        (" - ", 3),
        ("• ", 2),
        ("- ", 2),
    ] {
        if let Some(idx) = trimmed.rfind(pat) {
            let at = idx + add;
            if best.map(|b| at > b).unwrap_or(true) {
                best = Some(at);
            }
        }
    }
    if let Some(at) = best {
        let prefix = trimmed[..at].trim_end().to_string();
        let label = trimmed[at..].trim().to_string();
        if !label.is_empty() && label.chars().count() <= 120 {
            return (prefix, label);
        }
    }
    if trimmed.chars().count() <= 80 {
        return (String::new(), trimmed.to_string());
    }
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() > 8 {
        let at = words.len() - 8;
        (words[..at].join(" "), words[at..].join(" "))
    } else {
        (String::new(), trimmed.to_string())
    }
}

/// Parse `<[[ url ]]>` or `<http(s)://…>` starting at `s[0] == '<'`.
fn take_angle_url(s: &str) -> Option<(String, usize)> {
    if !s.starts_with('<') {
        return None;
    }
    if s.starts_with("<[[") {
        let end = s.find("]]>")?;
        let inner = s[3..end].trim();
        let url = inner.to_string();
        if url.starts_with("http://") || url.starts_with("https://") {
            return Some((url, end + 3));
        }
        return None;
    }
    if s.starts_with("<http://") || s.starts_with("<https://") {
        let end = s.find('>')?;
        let url = s[1..end].trim().to_string();
        return Some((url, end + 1));
    }
    None
}

/// Parse a bare `http(s)://…` URL; strips common trailing punctuation.
fn take_bare_url(s: &str) -> Option<(String, usize)> {
    if !(s.starts_with("https://") || s.starts_with("http://")) {
        return None;
    }
    let mut end = 0;
    for (idx, ch) in s.char_indices() {
        if ch.is_whitespace() {
            break;
        }
        end = idx + ch.len_utf8();
    }
    if end == 0 {
        return None;
    }
    let mut url = s[..end].to_string();
    while url
        .chars()
        .last()
        .is_some_and(|c| matches!(c, '.' | ',' | ';' | ')' | ']' | '"' | '\''))
    {
        url.pop();
    }
    let consumed = url.len();
    if consumed > 0 && (url.starts_with("https://") || url.starts_with("http://")) {
        Some((url, consumed))
    } else {
        None
    }
}

/// CSS properties emails commonly need for layout (LinkedIn, marketing digests, …).
/// Kept as an allowlist so `position` / `behavior` / etc. stay blocked.
fn mail_style_properties() -> std::collections::HashSet<&'static str> {
    [
        // Box model / layout
        "width",
        "height",
        "max-width",
        "min-width",
        "max-height",
        "min-height",
        "display",
        "visibility",
        "overflow",
        "overflow-x",
        "overflow-y",
        "float",
        "clear",
        "box-sizing",
        "vertical-align",
        "opacity",
        // Spacing
        "padding",
        "padding-top",
        "padding-right",
        "padding-bottom",
        "padding-left",
        "margin",
        "margin-top",
        "margin-right",
        "margin-bottom",
        "margin-left",
        // Border / table
        "border",
        "border-top",
        "border-right",
        "border-bottom",
        "border-left",
        "border-width",
        "border-style",
        "border-color",
        "border-radius",
        "border-collapse",
        "border-spacing",
        "table-layout",
        "empty-cells",
        // Type
        "color",
        "font",
        "font-family",
        "font-size",
        "font-weight",
        "font-style",
        "line-height",
        "letter-spacing",
        "text-align",
        "text-decoration",
        "text-transform",
        "white-space",
        "word-break",
        "word-wrap",
        "overflow-wrap",
        "list-style",
        "list-style-type",
        "list-style-position",
        // Background (url() still constrained by ammonia URL schemes + CSP)
        "background",
        "background-color",
        "background-image",
        "background-repeat",
        "background-position",
        "background-size",
    ]
    .into_iter()
    .collect()
}

/// Sanitize untrusted mail HTML (scripts/handlers stripped; safe tags only).
///
/// Email HTML (LinkedIn, newsletters) depends on inline `style` plus `width` /
/// `height` on images and tables. Stripping those makes URLs leak out of
/// `display:none` wrappers and images render at intrinsic size.
pub fn sanitize_mail_html(html: &str) -> String {
    let truncated = if html.len() > 512 * 1024 {
        &html[..512 * 1024]
    } else {
        html
    };
    Builder::default()
        .add_tags([
            "img", "table", "thead", "tbody", "tfoot", "tr", "td", "th", "col", "colgroup", "font",
            "h1", "h2", "h3", "h4",
        ])
        .add_generic_attributes([
            "style", "bgcolor", "background", "align", "valign", "width", "height", "border",
            "cellpadding", "cellspacing",
        ])
        .add_tag_attributes("img", ["src", "alt", "width", "height", "align", "border"])
        .add_tag_attributes("a", ["href", "title", "name", "target"])
        .add_tag_attributes("font", ["color", "face", "size"])
        .add_tag_attributes("table", ["width", "height", "align", "bgcolor", "border", "cellpadding", "cellspacing"])
        .add_tag_attributes("td", ["width", "height", "align", "valign", "bgcolor", "colspan", "rowspan"])
        .add_tag_attributes("th", ["width", "height", "align", "valign", "bgcolor", "colspan", "rowspan", "scope"])
        .add_tag_attributes("tr", ["align", "valign", "bgcolor"])
        .filter_style_properties(mail_style_properties())
        .link_rel(Some("noopener noreferrer"))
        .clean(truncated)
        .to_string()
}

/// True when sanitized HTML still contains a remote `http(s)` image.
pub fn mail_html_has_remote_images(html: &str) -> bool {
    let clean = sanitize_mail_html(html);
    for_each_img_tag(&clean, |tag| {
        let src = attr(tag, "src").unwrap_or_default();
        matches!(
            safe_url(&src).as_deref(),
            Some(u) if u.starts_with("http://") || u.starts_with("https://")
        )
    })
}

/// Scan sanitized HTML for remote images and classify tracking / privacy risks.
/// Always safe to call — does not fetch anything.
pub fn analyze_remote_image_threats(html: &str) -> Vec<RemoteImageRisk> {
    let clean = sanitize_mail_html(html);
    let mut out = Vec::new();
    for_each_img_tag(&clean, |tag| {
        let src = attr(tag, "src").unwrap_or_default();
        let Some(url) = safe_url(&src) else {
            return false;
        };
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return false;
        }
        let alt = attr(tag, "alt").unwrap_or_else(|| "Image".into());
        let style = attr(tag, "style").unwrap_or_default();
        let threats = classify_remote_image_threats(&url, tag, &style);
        if !threats.is_empty() {
            out.push(RemoteImageRisk { url, alt, threats });
        }
        false
    });
    // Remote background= / background-image in style on any tag.
    for_each_tag_with_remote_background(&clean, |url, style_or_attr| {
        let threats = classify_remote_image_threats(&url, style_or_attr, style_or_attr);
        if !threats.is_empty()
            && !out.iter().any(|r| r.url == url)
        {
            out.push(RemoteImageRisk {
                url,
                alt: "Background".into(),
                threats,
            });
        }
    });
    out
}

fn for_each_img_tag(html: &str, mut f: impl FnMut(&str) -> bool) -> bool {
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest_lower = &lower[i..];
        let Some(rel) = rest_lower.find("<img") else {
            break;
        };
        let start = i + rel;
        let after = &html[start..];
        let end_rel = after.find('>').unwrap_or(after.len());
        let tag = &after[..=end_rel.min(after.len().saturating_sub(1))];
        if f(tag) {
            return true;
        }
        i = start + end_rel + 1;
    }
    false
}

fn for_each_tag_with_remote_background(html: &str, mut f: impl FnMut(String, &str)) {
    let lower = html.to_ascii_lowercase();
    let mut search = lower.as_str();
    let mut base = 0usize;
    while let Some(rel) = search.find("background") {
        let abs = base + rel;
        // Find enclosing tag start.
        let before = &html[..abs];
        let tag_start = before.rfind('<').unwrap_or(0);
        let after = &html[tag_start..];
        let tag_end = after.find('>').map(|n| tag_start + n).unwrap_or(html.len());
        let tag = &html[tag_start..tag_end.min(html.len())];
        if let Some(url) = background_remote_url(tag) {
            f(url, tag);
        }
        let next = (abs + 10).min(html.len());
        base = next;
        search = &lower[next..];
    }
}

fn background_remote_url(tag: &str) -> Option<String> {
    if let Some(bg) = attr(tag, "background") {
        if let Some(u) = safe_url(&bg) {
            if u.starts_with("http://") || u.starts_with("https://") {
                return Some(u);
            }
        }
    }
    let style = attr(tag, "style").unwrap_or_default();
    let lower = style.to_ascii_lowercase();
    for key in ["background-image:", "background:"] {
        if let Some(idx) = lower.find(key) {
            let rest = &style[idx + key.len()..];
            if let Some(url) = extract_css_url(rest) {
                if let Some(u) = safe_url(&url) {
                    if u.starts_with("http://") || u.starts_with("https://") {
                        return Some(u);
                    }
                }
            }
        }
    }
    None
}

fn extract_css_url(s: &str) -> Option<String> {
    let lower = s.to_ascii_lowercase();
    let idx = lower.find("url(")?;
    let rest = s[idx + 4..].trim_start();
    let rest = rest.trim_start_matches(['\'', '"']);
    let end = rest
        .find(|c: char| c == ')' || c == '\'' || c == '"' || c == ';')
        .unwrap_or(rest.len());
    let url = rest[..end].trim();
    if url.is_empty() {
        None
    } else {
        Some(url.to_string())
    }
}

/// Classify a single remote image from its URL + raw tag/style snippets.
pub fn classify_remote_image_threats(url: &str, tag: &str, style: &str) -> Vec<ImageThreatKind> {
    let mut threats = Vec::new();
    if is_tiny_size(tag, style) {
        threats.push(ImageThreatKind::TinySize);
    }
    if is_hidden_style(tag, style) {
        threats.push(ImageThreatKind::Hidden);
    }
    if url_has_tracking_pattern(url) {
        threats.push(ImageThreatKind::KnownPattern);
    }
    if url_has_suspicious_host(url) {
        threats.push(ImageThreatKind::SuspiciousHost);
    }
    threats
}

fn is_tiny_size(tag: &str, style: &str) -> bool {
    let w = dim_attr(tag, "width").or_else(|| dim_from_style(style, "width"));
    let h = dim_attr(tag, "height").or_else(|| dim_from_style(style, "height"));
    match (w, h) {
        (Some(a), Some(b)) => a <= 2.0 && b <= 2.0,
        (Some(a), None) => a <= 2.0,
        (None, Some(b)) => b <= 2.0,
        (None, None) => false,
    }
}

fn dim_attr(tag: &str, name: &str) -> Option<f32> {
    attr(tag, name).and_then(|v| parse_css_px(&v))
}

fn dim_from_style(style: &str, prop: &str) -> Option<f32> {
    let lower = style.to_ascii_lowercase();
    let key = format!("{prop}:");
    let idx = lower.find(&key)?;
    let rest = style[idx + key.len()..].trim_start();
    let end = rest
        .find(|c: char| c == ';' || c.is_whitespace())
        .unwrap_or(rest.len());
    parse_css_px(&rest[..end])
}

fn parse_css_px(raw: &str) -> Option<f32> {
    let s = raw.trim().trim_end_matches("px").trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 0.0)
}

fn is_hidden_style(tag: &str, style: &str) -> bool {
    let mut combined = style.to_ascii_lowercase();
    if let Some(t) = attr(tag, "style") {
        if !combined.is_empty() {
            combined.push(';');
        }
        combined.push_str(&t.to_ascii_lowercase());
    }
    if combined.contains("display:none")
        || combined.contains("display: none")
        || combined.contains("visibility:hidden")
        || combined.contains("visibility: hidden")
    {
        return true;
    }
    if let Some(op) = dim_from_style(&combined, "opacity") {
        if op <= 0.01 {
            return true;
        }
    }
    if let Some(mh) = dim_from_style(&combined, "max-height") {
        if mh <= 0.0 {
            return true;
        }
    }
    false
}

fn url_has_tracking_pattern(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    const PATTERNS: &[&str] = &[
        "/track",
        "/tracking",
        "open.gif",
        "pixel.",
        "/pixel",
        "beacon",
        "/beacon",
        "utm_",
        "/wf/open",
        "list-manage.com/track",
        "/open?",
        "/open/",
        "opened.gif",
        "click.gif",
        "spacer.gif",
        "1x1",
        "1x1.gif",
        "blank.gif",
        "/t/v/",
        "mailgun",
        "sendgrid.net/wf",
        "doubleclick.net",
    ];
    PATTERNS.iter().any(|p| u.contains(p))
}

fn url_has_suspicious_host(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    let host = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))
        .unwrap_or(&u);
    let host = host.split('/').next().unwrap_or(host);
    let host = host.split('@').next_back().unwrap_or(host);
    const SUFFIXES: &[&str] = &[
        "list-manage.com",
        "mailchimp.com",
        "sendgrid.net",
        "mandrillapp.com",
        "returnpath.net",
        "mktoweb.com",
        "exacttarget.com",
        "exct.net",
        "sailthru.com",
        "cmail19.com",
        "cmail20.com",
        "rs6.net",
        "pstmrk.it",
    ];
    SUFFIXES
        .iter()
        .any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

fn url_is_threatened(url: &str, risks: &[RemoteImageRisk]) -> bool {
    risks.iter().any(|r| r.url == url)
}

/// Strip `src` from threatened remote images / backgrounds so CSP cannot fetch them.
fn strip_threatened_remote_assets(html: &str, risks: &[RemoteImageRisk]) -> String {
    if risks.is_empty() {
        return html.to_string();
    }
    let mut out = html.to_string();
    // Rewrite img tags with threatened src.
    let snapshot = out.clone();
    let lower = snapshot.to_ascii_lowercase();
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    let mut i = 0;
    while i < snapshot.len() {
        let Some(rel) = lower[i..].find("<img") else {
            break;
        };
        let start = i + rel;
        let after = &snapshot[start..];
        let end_rel = after.find('>').unwrap_or(after.len());
        let end = start + end_rel;
        let tag = &snapshot[start..=end.min(snapshot.len().saturating_sub(1))];
        if let Some(src) = attr(tag, "src") {
            if let Some(url) = safe_url(&src) {
                if url_is_threatened(&url, risks) {
                    let rewritten = rewrite_img_tag_block_src(tag);
                    replacements.push((start, end + 1, rewritten));
                }
            }
        }
        i = end + 1;
    }
    // Apply from the end so indices stay valid.
    for (start, end, rep) in replacements.into_iter().rev() {
        out.replace_range(start..end.min(out.len()), &rep);
    }

    // Neutralize threatened background URLs in attributes/styles.
    for risk in risks {
        if out.contains(&risk.url) {
            out = out.replace(&risk.url, "about:blank");
        }
    }
    out
}

fn rewrite_img_tag_block_src(tag: &str) -> String {
    // Drop src so the webview cannot fetch; keep alt for accessibility.
    let mut out = String::from("<img");
    if let Some(alt) = attr(tag, "alt") {
        out.push_str(" alt=\"");
        out.push_str(&alt.replace('"', "&quot;"));
        out.push('"');
    }
    out.push_str(" data-ailiak-blocked=\"tracking\"");
    if !tag.ends_with("/>") && !tag.ends_with(" />") {
        out.push('>');
    } else {
        out.push_str(" />");
    }
    out
}

/// Full HTML document for the embedded webview (CSP + sanitized body).
///
/// Always use a light reading canvas. HTML mail is authored for light
/// backgrounds; a dark `body { color }` inherits into white tables (LinkedIn,
/// etc.) and washes text out. `dark` is kept for API stability / callers that
/// still theme the native webview chrome around this document.
pub fn build_mail_html_document(html: &str, load_remote_images: bool, _dark: bool) -> String {
    let risks = analyze_remote_image_threats(html);
    let body = {
        let clean = sanitize_mail_html(html);
        strip_threatened_remote_assets(&clean, &risks)
    };
    let img_src = if load_remote_images {
        "img-src https: http: data: cid:;"
    } else {
        "img-src 'none';"
    };
    let (bg, fg, link) = ("#ffffff", "#1a1a1a", "#0b6e4f");
    // Scripts always blocked. Styles allowed inline (email HTML relies on them).
    let csp = format!(
        "default-src 'none'; style-src 'unsafe-inline'; {img_src} \
         font-src data:; media-src 'none'; object-src 'none'; script-src 'none'; \
         base-uri 'none'; form-action 'none'; frame-src 'none'; frame-ancestors 'none'; \
         connect-src 'none';"
    );
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="{csp}">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  html, body {{
    margin: 0;
    padding: 0;
    background: {bg};
    max-width: 100%;
  }}
  body {{
    font-family: system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
    font-size: 14px;
    line-height: 1.45;
    color: {fg};
    background: {bg};
    padding: 12px 16px 24px;
    word-wrap: break-word;
    overflow-wrap: anywhere;
    overflow-x: auto;
    box-sizing: border-box;
  }}
  /* Cap runaway assets (LinkedIn icons often ship without useful CSS after sanitize). */
  img, video {{
    max-width: 100% !important;
    height: auto !important;
  }}
  table {{
    max-width: 100% !important;
    border-collapse: collapse;
  }}
  td, th {{
    max-width: 100%;
  }}
  a {{ color: {link}; }}
</style>
</head>
<body>
{body}
</body>
</html>"#
    )
}

/// How the webview should treat a navigation / link activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailNavAction {
    /// Stay on the mail document (about:blank, custom scheme, empty).
    Stay,
    /// Open in the system browser / mail handler.
    OpenExternal(String),
    /// Reject (javascript:, data: navigations, etc.).
    Block,
}

/// Classify a URL the webview wants to navigate to / open.
pub fn classify_mail_navigation(url: &str) -> MailNavAction {
    let u = url.trim();
    if u.is_empty() || u == "about:blank" || u.starts_with("ailiak-mail:") {
        return MailNavAction::Stay;
    }
    match safe_url(u) {
        Some(safe) => MailNavAction::OpenExternal(safe),
        None => MailNavAction::Block,
    }
}

fn from_html(html: &str, load_remote_images: bool) -> MailBody {
    let image_threats = analyze_remote_image_threats(html);
    let clean = sanitize_mail_html(html);
    let mut walker = Walker {
        load_remote_images,
        has_remote_images: false,
        image_threats: image_threats.clone(),
        blocks: Vec::new(),
        inline: Vec::new(),
        text_buf: String::new(),
        in_anchor: None,
        list_item: false,
        heading: None,
        quote: false,
    };
    walker.feed(&clean);
    walker.flush_paragraph();
    MailBody {
        blocks: walker.blocks,
        has_remote_images: walker.has_remote_images,
        image_threats,
    }
}

struct Walker {
    load_remote_images: bool,
    has_remote_images: bool,
    image_threats: Vec<RemoteImageRisk>,
    blocks: Vec<BodyBlock>,
    inline: Vec<InlineSeg>,
    text_buf: String,
    in_anchor: Option<String>,
    list_item: bool,
    heading: Option<u8>,
    quote: bool,
}

impl Walker {
    fn feed(&mut self, html: &str) {
        let bytes = html.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'<' {
                self.flush_text();
                let end = html[i..]
                    .find('>')
                    .map(|n| i + n)
                    .unwrap_or(html.len().saturating_sub(1));
                let tag = &html[i..=end.min(html.len().saturating_sub(1))];
                self.handle_tag(tag);
                i = end + 1;
                continue;
            }
            if bytes[i] == b'&' {
                if let Some((ch, n)) = decode_entity(&html[i..]) {
                    self.text_buf.push(ch);
                    i += n;
                    continue;
                }
            }
            let ch = html[i..].chars().next().unwrap_or('?');
            self.text_buf.push(ch);
            i += ch.len_utf8();
        }
        self.flush_text();
    }

    fn handle_tag(&mut self, tag: &str) {
        let lower = tag.to_ascii_lowercase();
        if lower.starts_with("<br") {
            self.text_buf.push('\n');
            return;
        }
        if lower.starts_with("<hr") {
            self.flush_paragraph();
            self.blocks.push(BodyBlock::Rule);
            return;
        }
        if lower.starts_with("<img") {
            let src = attr(tag, "src").unwrap_or_default();
            let alt = attr(tag, "alt").unwrap_or_else(|| "Image".into());
            if let Some(url) = safe_url(&src) {
                if url.starts_with("http://") || url.starts_with("https://") {
                    self.has_remote_images = true;
                    self.flush_paragraph();
                    let threatened = url_is_threatened(&url, &self.image_threats);
                    if self.load_remote_images && !threatened {
                        self.blocks.push(BodyBlock::Image { alt, url });
                    } else {
                        self.blocks.push(BodyBlock::ImageBlocked { alt });
                    }
                }
            }
            return;
        }
        if lower.starts_with("<a ") || lower == "<a>" {
            if let Some(href) = attr(tag, "href").and_then(|u| safe_url(&u)) {
                self.flush_text();
                self.in_anchor = Some(href);
            }
            return;
        }
        if lower.starts_with("</a") {
            self.flush_text();
            self.in_anchor = None;
            return;
        }
        if let Some(level) = heading_open(&lower) {
            self.flush_paragraph();
            self.heading = Some(level);
            return;
        }
        if lower.starts_with("</h") {
            self.flush_paragraph();
            return;
        }
        if lower.starts_with("<li") {
            self.flush_paragraph();
            self.list_item = true;
            return;
        }
        if lower.starts_with("</li") {
            self.flush_paragraph();
            return;
        }
        if lower.starts_with("<blockquote") {
            self.flush_paragraph();
            self.quote = true;
            return;
        }
        if lower.starts_with("</blockquote") {
            self.flush_paragraph();
            self.quote = false;
            return;
        }
        if lower.starts_with("</p")
            || lower.starts_with("</div")
            || lower.starts_with("</tr")
            || lower.starts_with("</table")
        {
            self.flush_paragraph();
        }
    }

    fn flush_text(&mut self) {
        if self.text_buf.is_empty() {
            return;
        }
        let t = std::mem::take(&mut self.text_buf);
        if let Some(url) = &self.in_anchor {
            let label = t.trim();
            let text = if label.is_empty() {
                url.clone()
            } else {
                label.to_string()
            };
            self.inline.push(InlineSeg::Link {
                text,
                url: url.clone(),
            });
        } else {
            self.inline.push(InlineSeg::Text(t));
        }
    }

    fn flush_paragraph(&mut self) {
        self.flush_text();
        if self.inline.is_empty() {
            self.heading = None;
            self.list_item = false;
            return;
        }
        let spans = collapse_spans(std::mem::take(&mut self.inline));
        if spans_empty(&spans) {
            self.heading = None;
            self.list_item = false;
            return;
        }
        let block = if let Some(level) = self.heading.take() {
            BodyBlock::Heading { level, spans }
        } else if self.list_item {
            self.list_item = false;
            BodyBlock::Bullet(spans)
        } else if self.quote {
            BodyBlock::Quote(spans)
        } else {
            BodyBlock::Paragraph(spans)
        };
        self.blocks.push(block);
    }
}

fn collapse_spans(spans: Vec<InlineSeg>) -> Vec<InlineSeg> {
    let mut out = Vec::new();
    for s in spans {
        match s {
            InlineSeg::Text(t) => {
                if let Some(InlineSeg::Text(prev)) = out.last_mut() {
                    prev.push_str(&t);
                } else {
                    out.push(InlineSeg::Text(t));
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn spans_empty(spans: &[InlineSeg]) -> bool {
    spans.iter().all(|s| match s {
        InlineSeg::Text(t) => t.trim().is_empty(),
        InlineSeg::Link { text, .. } => text.trim().is_empty(),
    })
}

fn heading_open(lower: &str) -> Option<u8> {
    for (tag, level) in [("<h1", 1), ("<h2", 2), ("<h3", 3), ("<h4", 4)] {
        if lower.starts_with(tag) && !lower.starts_with("</") {
            return Some(level);
        }
    }
    None
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let key = format!("{name}=");
    let idx = lower.find(&key)?;
    let rest = &tag[idx + key.len()..];
    let rest = rest.trim_start();
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = rest[1..].find(quote)? + 1;
        Some(html_unescape(&rest[1..end]))
    } else {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        Some(html_unescape(&rest[..end]))
    }
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// Only http(s) and mailto — blocks javascript:, data:, etc.
pub fn safe_url(raw: &str) -> Option<String> {
    let u = raw.trim();
    if u.is_empty() {
        return None;
    }
    let lower = u.to_ascii_lowercase();
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
    {
        if u.contains('\n') || u.contains('\r') || u.contains(' ') {
            return None;
        }
        Some(u.to_string())
    } else {
        None
    }
}

fn decode_entity(s: &str) -> Option<(char, usize)> {
    if !s.starts_with('&') {
        return None;
    }
    let end = s.find(';').filter(|&e| e > 1 && e <= 32)?;
    let name = &s[1..end];
    let ch = if let Some(num) = name.strip_prefix('#') {
        if let Some(hex) = num.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)?
        } else {
            num.parse::<u32>().ok().and_then(char::from_u32)?
        }
    } else {
        match name {
            "nbsp" => ' ',
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "rsquo" => '\u{2019}',
            "lsquo" => '\u{2018}',
            "rdquo" => '\u{201D}',
            "ldquo" => '\u{201C}',
            "mdash" => '\u{2014}',
            "ndash" => '\u{2013}',
            "hellip" => '\u{2026}',
            _ => return None,
        }
    };
    Some((ch, end + 1))
}

/// Snippet helper when HTML is the primary part.
pub fn prefer_html_preview(html: &str) -> String {
    html_to_text(html).chars().take(140).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_keeps_link() {
        let html =
            r#"<p>Hi <a href="https://example.com">there</a><script>alert(1)</script></p>"#;
        let body = build_mail_body("", html, false);
        let has_link = body.blocks.iter().any(|b| match b {
            BodyBlock::Paragraph(spans) => {
                spans.iter().any(|s| matches!(s, InlineSeg::Link { .. }))
            }
            _ => false,
        });
        assert!(has_link);
    }

    #[test]
    fn blocks_images_by_default() {
        let html = r#"<p>Logo</p><img src="https://cdn.example/logo.png" alt="x" width="120" height="40">"#;
        let body = build_mail_body("", html, false);
        assert!(body.has_remote_images);
        assert!(body.image_threats.is_empty());
        assert!(body
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::ImageBlocked { .. })));
        let loaded = build_mail_body("", html, true);
        assert!(loaded
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::Image { .. })));
    }

    #[test]
    fn tiny_pixel_is_threat_even_when_loading() {
        let html = r#"<img src="https://cdn.example/spacer.png" width="1" height="1" alt="t">"#;
        let risks = analyze_remote_image_threats(html);
        assert!(
            risks.iter().any(|r| r.threats.contains(&ImageThreatKind::TinySize)),
            "{risks:?}"
        );
        let loaded = build_mail_body("", html, true);
        assert!(loaded.has_image_threats());
        assert!(loaded
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::ImageBlocked { .. })));
        assert!(!loaded
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::Image { .. })));
        let doc = build_mail_html_document(html, true, false);
        assert!(
            !doc.contains("https://cdn.example/spacer.png"),
            "threatened src must be stripped: {doc}"
        );
        assert!(doc.contains("data-ailiak-blocked=\"tracking\""), "{doc}");
    }

    #[test]
    fn hidden_and_known_pattern_threats() {
        let hidden = r#"<img src="https://cdn.example/a.png" style="display:none" alt="h">"#;
        let risks = analyze_remote_image_threats(hidden);
        assert!(
            risks.iter().any(|r| r.threats.contains(&ImageThreatKind::Hidden)),
            "{risks:?}"
        );

        let patterned =
            r#"<img src="https://cdn.example/track/open.gif?id=1" width="40" height="40" alt="p">"#;
        let risks = analyze_remote_image_threats(patterned);
        assert!(
            risks
                .iter()
                .any(|r| r.threats.contains(&ImageThreatKind::KnownPattern)),
            "{risks:?}"
        );
        let loaded = build_mail_body("", patterned, true);
        assert!(loaded
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::ImageBlocked { .. })));
    }

    #[test]
    fn suspicious_host_blocked_when_loading() {
        let html =
            r#"<img src="https://us1.list-manage.com/track/open.png" width="80" height="80" alt="mc">"#;
        let risks = analyze_remote_image_threats(html);
        assert!(
            risks
                .iter()
                .any(|r| r.threats.contains(&ImageThreatKind::SuspiciousHost)),
            "{risks:?}"
        );
        let loaded = build_mail_body("", html, true);
        assert!(loaded.has_image_threats());
        assert!(loaded
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::ImageBlocked { .. })));
    }

    #[test]
    fn normal_logo_not_flagged() {
        let html = r#"<img src="https://static.licdn.com/logo.png" width="84" height="21"
             style="width:84px;height:21px;display:block" alt="LinkedIn">"#;
        let risks = analyze_remote_image_threats(html);
        assert!(risks.is_empty(), "{risks:?}");
        let loaded = build_mail_body("", html, true);
        assert!(loaded
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::Image { .. })));
    }

    #[test]
    fn rejects_javascript_urls() {
        assert!(safe_url("javascript:alert(1)").is_none());
        assert!(safe_url("https://ok.example/a").is_some());
        assert!(safe_url("mailto:a@b.com").is_some());
    }

    #[test]
    fn cleans_microsoft_plain_brackets() {
        let s = "Having trouble? | View as a web page <[[https://emails.microsoft.com/v/long]]>\n\nHello";
        let body = build_mail_body(s, "", false);
        let text = format!("{:?}", body.blocks);
        assert!(!text.contains("[["), "{text}");
        assert!(text.contains("Hello"), "{text}");
        let has_link = body.blocks.iter().any(|b| match b {
            BodyBlock::Paragraph(spans) => spans.iter().any(|s| matches!(
                s,
                InlineSeg::Link { text, url }
                    if text.contains("View as a web page") && url.starts_with("https://")
            )),
            _ => false,
        });
        assert!(has_link, "{text}");
    }

    #[test]
    fn skips_orphan_tracking_urls() {
        let s = "<[[https://www.microsoft.com/]]>\n\nJoin Ignite\n\nFacebook <https://emails.microsoft.com/dc/AAAA=>";
        let body = build_mail_body(s, "", false);
        let text = format!("{:?}", body.blocks);
        assert!(text.contains("Join Ignite"), "{text}");
        assert!(!text.contains("www.microsoft.com"), "{text}");
        let has_fb = body.blocks.iter().any(|b| match b {
            BodyBlock::Paragraph(spans) => spans.iter().any(|s| matches!(
                s,
                InlineSeg::Link { text, .. } if text == "Facebook"
            )),
            _ => false,
        });
        assert!(has_fb, "{text}");
    }

    #[test]
    fn sanitize_and_document_block_scripts_and_images() {
        let html = r#"<p>Hi<script>alert(1)</script><img src="https://evil.example/x.gif" alt="x"></p>"#;
        let clean = sanitize_mail_html(html);
        assert!(!clean.to_ascii_lowercase().contains("<script"), "{clean}");
        assert!(mail_html_has_remote_images(html));

        let blocked = build_mail_html_document(html, false, false);
        assert!(blocked.contains("img-src 'none'"), "{blocked}");
        assert!(blocked.contains("script-src 'none'"), "{blocked}");
        assert!(!blocked.to_ascii_lowercase().contains("<script>alert"), "{blocked}");

        let allowed = build_mail_html_document(html, true, false);
        assert!(allowed.contains("img-src https:"), "{allowed}");
    }

    #[test]
    fn sanitize_keeps_linkedin_like_layout_hints() {
        // LinkedIn hides tracking/chrome URLs with display:none and sizes icons via style/attrs.
        let html = r#"
            <table width="600" style="width:600px;background-color:#f3f2ef">
              <tr>
                <td>
                  <span style="display:none;font-size:0;line-height:0;max-height:0;overflow:hidden">
                    https://www.linkedin.com/comm/feed/?trkEmail=eml
                  </span>
                  <a href="https://www.linkedin.com/">
                    <img src="https://static.licdn.com/logo.png" width="84" height="21"
                         style="width:84px;height:21px;display:block" alt="LinkedIn">
                  </a>
                  <img src="https://static.licdn.com/notif.png" style="width:48px;height:48px" alt="4">
                </td>
              </tr>
            </table>
            <p style="position:fixed;top:0">should lose position</p>
        "#;
        let clean = sanitize_mail_html(html);
        assert!(
            clean.contains("display:none") || clean.contains("display: none"),
            "display:none must survive so hidden URLs stay hidden: {clean}"
        );
        assert!(
            clean.contains("width=\"84\"") || clean.contains("width:84px"),
            "image sizing must survive: {clean}"
        );
        assert!(
            clean.contains("height=\"21\"") || clean.contains("height:21px"),
            "image height must survive: {clean}"
        );
        assert!(
            !clean.contains("position:fixed") && !clean.contains("position: fixed"),
            "dangerous position must be stripped: {clean}"
        );

        let doc = build_mail_html_document(html, true, false);
        assert!(
            doc.contains("max-width: 100% !important"),
            "document CSS must cap images: {doc}"
        );
    }

    #[test]
    fn classify_mail_navigation_allowlist() {
        assert_eq!(
            classify_mail_navigation("https://example.com/a"),
            MailNavAction::OpenExternal("https://example.com/a".into())
        );
        assert_eq!(
            classify_mail_navigation("mailto:a@b.com"),
            MailNavAction::OpenExternal("mailto:a@b.com".into())
        );
        assert_eq!(classify_mail_navigation("about:blank"), MailNavAction::Stay);
        assert_eq!(
            classify_mail_navigation("ailiak-mail://message"),
            MailNavAction::Stay
        );
        assert_eq!(
            classify_mail_navigation("javascript:alert(1)"),
            MailNavAction::Block
        );
        assert_eq!(
            classify_mail_navigation("data:text/html,hi"),
            MailNavAction::Block
        );
    }
}
