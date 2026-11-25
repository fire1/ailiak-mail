//! Inline MIME parts and `cid:` rewriting (ported from rusty-inbox mime helpers).
//!
//! Email HTML often references attachments as `cid:…`. Wry/WebKit has no CID
//! protocol handler, so we rewrite those to `data:` URLs at parse time.

use base64::Engine as _;
use mail_parser::{MessageParser, MimeHeaders};

/// True when the message has a real `text/html` part (mail-parser otherwise
/// synthesizes HTML from plain text).
pub fn message_has_real_html(raw: &[u8]) -> bool {
    let Some(msg) = MessageParser::default().parse(raw) else {
        return false;
    };
    msg.parts
        .iter()
        .any(|p| p.is_content_type("text", "html"))
}

/// Html still referencing `cid:` parts (attribute value or CSS `url()`).
pub fn has_cid_refs(html: &str) -> bool {
    let l = html.to_ascii_lowercase();
    ["\"cid:", "'cid:", "(cid:", "=cid:"]
        .iter()
        .any(|p| l.contains(p))
}

/// Inline (Content-ID) parts: `(content id without <>, mime, bytes)`.
///
/// Parts count as inline whenever they carry a Content-ID, whatever their
/// disposition — senders often omit `Content-Disposition: inline` for `cid:` images.
pub fn inline_parts(raw: &[u8]) -> Vec<(String, String, Vec<u8>)> {
    let Some(msg) = MessageParser::default().parse(raw) else {
        return vec![];
    };
    msg.parts
        .iter()
        .filter_map(|part| {
            let cid = part
                .content_id()?
                .trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string();
            if cid.is_empty() || part.is_content_type("multipart", "") {
                return None;
            }
            let mime = part
                .content_type()
                .map(|c| {
                    format!(
                        "{}/{}",
                        c.c_type,
                        c.c_subtype.as_deref().unwrap_or("octet-stream")
                    )
                })
                .unwrap_or_else(|| "application/octet-stream".into());
            Some((cid, mime, part.contents().to_vec()))
        })
        .collect()
}

/// Replace `cid:<id>` references (src/background/url()) with the given URLs.
/// Matching is case-insensitive on the id, and tolerates surrounding `<>`.
pub fn rewrite_cids(html: &str, urls: &[(String, String)]) -> String {
    let mut out = String::with_capacity(html.len());
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while let Some(off) = lower[i..].find("cid:") {
        let at = i + off;
        out.push_str(&html[i..at]);
        let rest = &html[at + 4..];
        let end = rest
            .find(['"', '\'', ')', ' ', '>', '\t', '\r', '\n'])
            .unwrap_or(rest.len());
        let id = rest[..end]
            .trim_start_matches("&lt;")
            .trim_end_matches("&gt;")
            .trim_matches(['<', '>']);
        let id = percent_decode_lossy(id);
        match urls
            .iter()
            .find(|(c, _)| c.eq_ignore_ascii_case(&id))
        {
            Some((_, url)) => {
                out.push_str(url);
                i = at + 4 + end;
            }
            None => {
                out.push_str("cid:");
                i = at + 4;
            }
        }
    }
    out.push_str(&html[i..]);
    out
}

/// Rewrite `cid:` references in `html` to `data:{mime};base64,…` using parts from `raw`.
pub fn resolve_inline_images(html: &str, raw: &[u8]) -> String {
    if !has_cid_refs(html) {
        return html.to_string();
    }
    let mut urls = Vec::new();
    for (cid, mime, bytes) in inline_parts(raw) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        urls.push((cid, format!("data:{mime};base64,{b64}")));
    }
    if urls.is_empty() {
        return html.to_string();
    }
    rewrite_cids(html, &urls)
}

/// Minimal percent-decode for CID tokens (avoids an extra crate).
fn percent_decode_lossy(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INLINE: &[u8] = b"From: a@example.com\r\n\
To: b@example.com\r\n\
Subject: logo\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=\"r1\"\r\n\
\r\n\
--r1\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<img src=\"cid:img1.png@example.com\" alt=\"logo\"><div style=\"background:url(CID:Img2.PNG@example.com)\"></div><img src=\"cid:missing@x\">\r\n\
--r1\r\n\
Content-Type: image/png\r\n\
Content-ID: <img1.png@example.com>\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--r1\r\n\
Content-Type: image/png\r\n\
Content-ID: <img2.png@example.com>\r\n\
Content-Disposition: attachment; filename=\"b.png\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--r1--\r\n";

    #[test]
    fn detects_real_html_and_cid() {
        assert!(message_has_real_html(INLINE));
        assert!(has_cid_refs(
            r#"<img src="cid:logo"><div style="background:url(cid:x)">"#
        ));
        assert!(!has_cid_refs("<p>no images</p>"));
    }

    #[test]
    fn inline_images_are_extracted_and_rewritten() {
        let parts = inline_parts(INLINE);
        let ids: Vec<&str> = parts.iter().map(|p| p.0.as_str()).collect();
        assert_eq!(ids, ["img1.png@example.com", "img2.png@example.com"]);
        assert_eq!(parts[0].1, "image/png");
        assert_eq!(&parts[0].2[..4], b"\x89PNG");

        let html = MessageParser::default()
            .parse(INLINE)
            .unwrap()
            .body_html(0)
            .unwrap()
            .into_owned();
        let out = resolve_inline_images(&html, INLINE);
        assert!(out.contains("data:image/png;base64,"), "{out}");
        assert!(
            !out.to_ascii_lowercase().contains("cid:img1"),
            "resolved cid left behind: {out}"
        );
        assert!(
            out.contains("cid:missing@x"),
            "unknown ids untouched: {out}"
        );
    }

    #[test]
    fn plain_only_has_no_real_html() {
        let raw = b"From: a@example.com\r\n\
To: b@example.com\r\n\
Subject: hi\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello plain\r\n";
        assert!(!message_has_real_html(raw));
    }
}
