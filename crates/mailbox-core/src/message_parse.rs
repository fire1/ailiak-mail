//! Shared RFC822 → Message parsing for IMAP and POP3.

use chrono::{DateTime, Utc};
use mail_parser::MessageParser;

use crate::types::{Envelope, Message};

pub fn parse_raw_message(
    account_id: &str,
    folder_path: &str,
    uid: u32,
    seen: bool,
    flagged: bool,
    raw: &[u8],
) -> Option<Message> {
    let parsed = MessageParser::default().parse(raw)?;

    let message_id = parsed
        .message_id()
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("uid-{uid}@{folder_path}"));

    let in_reply_to = header_ids(parsed.in_reply_to());
    let references = header_ids(parsed.references());
    let thread_id = Envelope::compute_thread_id(&message_id, &in_reply_to, &references);

    let subject = parsed.subject().unwrap_or("(no subject)").to_string();

    let (from_name, from_addr) = match parsed.from().and_then(|a| a.first()) {
        Some(addr) => (
            addr.name().unwrap_or("").to_string(),
            addr.address().unwrap_or("").to_string(),
        ),
        None => (String::new(), String::new()),
    };

    let date = parsed
        .date()
        .map(|d| DateTime::from_timestamp(d.to_timestamp(), 0).unwrap_or_else(Utc::now))
        .unwrap_or_else(Utc::now);

    let to = addresses(parsed.to());
    let cc = addresses(parsed.cc());

    let (mut body_text, mut body_html) = extract_bodies(&parsed);

    // mail-parser synthesizes HTML from plain; only keep HTML when a real part exists.
    if !crate::mime_inline::message_has_real_html(raw) {
        body_html.clear();
    } else if crate::mime_inline::has_cid_refs(&body_html) {
        // Resolve before truncate so data: URLs are not cut mid-stream.
        body_html = crate::mime_inline::resolve_inline_images(&body_html, raw);
    }

    let snippet = snippet_from(&body_text);

    if body_text.len() > 512 * 1024 {
        body_text.truncate(512 * 1024);
        body_text.push_str("\n…[truncated]");
    }
    // Cap after CID→data: rewrite. Large inlines can exceed 512 KiB; keep a higher
    // ceiling so signatures/logos survive.
    const HTML_CAP: usize = 2 * 1024 * 1024;
    if body_html.len() > HTML_CAP {
        body_html.truncate(HTML_CAP);
    }

    Some(Message {
        envelope: Envelope {
            id: format!("{account_id}:{folder_path}:{uid}"),
            account_id: account_id.to_string(),
            folder_path: folder_path.to_string(),
            message_id,
            subject,
            from_name,
            from_addr,
            date,
            seen,
            flagged,
            snippet,
            labels: Vec::new(),
            in_reply_to,
            references,
            thread_id,
        },
        body_text,
        body_html,
        to,
        cc,
    })
}

fn header_ids(value: &mail_parser::HeaderValue<'_>) -> String {
    if let Some(list) = value.as_text_list() {
        return list.join(" ");
    }
    value.as_text().unwrap_or("").to_string()
}

fn addresses(list: Option<&mail_parser::Address<'_>>) -> Vec<String> {
    let Some(list) = list else {
        return Vec::new();
    };
    list.iter()
        .map(|a| {
            let email = a.address().unwrap_or("");
            match a.name() {
                Some(name) if !name.is_empty() => format!("{name} <{email}>"),
                _ => email.to_string(),
            }
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn extract_bodies(parsed: &mail_parser::Message<'_>) -> (String, String) {
    let plain = parsed.body_text(0).map(|s| s.to_string());
    let html = parsed.body_html(0).map(|s| s.to_string());

    let body_html = html.clone().unwrap_or_default();

    let body_text = if let Some(ref text) = plain {
        if !looks_like_html(text) {
            text.clone()
        } else {
            html_to_text(text)
        }
    } else if let Some(ref html) = html {
        html_to_text(html)
    } else {
        String::from("(no plain-text body)")
    };

    // If plain looks like a marketing stub but HTML exists, prefer HTML-derived text.
    let body_text = if !body_html.is_empty()
        && (body_text.len() < 40 || body_text.matches("[[").count() >= 2)
    {
        html_to_text(&body_html)
    } else {
        body_text
    };

    (body_text, body_html)
}

/// For the reader: clean bodies that were cached before HTML stripping improved.
pub fn readable_body(body: &str) -> String {
    if looks_like_html(body) {
        html_to_text(body)
    } else {
        body.to_string()
    }
}

pub(crate) fn looks_like_html(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let tag_hits = [
        "<html", "<body", "<div", "<p>", "<p ", "<br", "<table", "<span", "<a ", "</a>",
        "<strong", "<style", "&nbsp;", "&rsquo;", "&mdash;",
    ]
    .iter()
    .filter(|t| lower.contains(**t))
    .count();
    tag_hits >= 2 || lower.contains("<html") || lower.contains("<!doctype")
}

/// Strip tags for SMTP `text/plain` (never injects UI placeholder copy).
pub fn html_to_plain(html: &str) -> String {
    html_to_text(html)
}

/// Strip tags / decode entities into readable plain text (for the calm reader).
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut skip_depth: u8 = 0; // inside <script>/<style>/<head>

    while i < bytes.len() {
        if bytes[i] == b'<' {
            let rest = &html[i..];
            let end = rest.find('>').map(|n| i + n).unwrap_or(bytes.len().saturating_sub(1));
            let tag_lower = html[i..=end.min(html.len().saturating_sub(1))].to_ascii_lowercase();

            if tag_opens(&tag_lower, "script")
                || tag_opens(&tag_lower, "style")
                || tag_opens(&tag_lower, "head")
                || tag_opens(&tag_lower, "noscript")
            {
                skip_depth = skip_depth.saturating_add(1);
            } else if tag_closes(&tag_lower, "script")
                || tag_closes(&tag_lower, "style")
                || tag_closes(&tag_lower, "head")
                || tag_closes(&tag_lower, "noscript")
            {
                skip_depth = skip_depth.saturating_sub(1);
            } else if skip_depth == 0 {
                if is_block_break(&tag_lower)
                    || tag_opens(&tag_lower, "br")
                    || tag_lower.starts_with("<br/")
                    || tag_lower.starts_with("<br ")
                {
                    push_newline(&mut out);
                }
            }
            i = end + 1;
            continue;
        }

        if skip_depth > 0 {
            i += 1;
            continue;
        }

        if bytes[i] == b'&' {
            if let Some((ch, consumed)) = decode_entity(&html[i..]) {
                out.push(ch);
                i += consumed;
                continue;
            }
        }

        out.push(html[i..].chars().next().unwrap_or('?'));
        i += html[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }

    normalize_whitespace(&out)
}

fn tag_opens(tag_lower: &str, name: &str) -> bool {
    tag_lower.starts_with(&format!("<{name}"))
        && !tag_lower.starts_with(&format!("</{name}"))
}

fn tag_closes(tag_lower: &str, name: &str) -> bool {
    tag_lower.starts_with(&format!("</{name}"))
}

fn is_block_break(tag_lower: &str) -> bool {
    const BLOCKS: &[&str] = &[
        "</p", "</div", "</tr", "</li", "</h1", "</h2", "</h3", "</h4", "</td", "</th",
        "</blockquote", "</section", "</article", "</header", "</footer", "<hr",
    ];
    BLOCKS.iter().any(|b| tag_lower.starts_with(b))
}

fn push_newline(out: &mut String) {
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

fn decode_entity(s: &str) -> Option<(char, usize)> {
    if !s.starts_with('&') {
        return None;
    }
    let end = s.find(';')?;
    if end == 0 || end > 32 {
        return None;
    }
    let name = &s[1..end];
    let ch = if let Some(num) = name.strip_prefix('#') {
        if let Some(hex) = num.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)?
        } else {
            num.parse::<u32>().ok().and_then(char::from_u32)?
        }
    } else {
        match name {
            "nbsp" => '\u{00A0}',
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "rsquo" => '\u{2019}',
            "saquo" => '\u{203A}',
            "lsquo" => '\u{2018}',
            "rdquo" => '\u{201D}',
            "ldquo" => '\u{201C}',
            "mdash" => '\u{2014}',
            "ndash" => '\u{2013}',
            "hellip" => '\u{2026}',
            "bull" => '\u{2022}',
            "copy" => '\u{00A9}',
            "reg" => '\u{00AE}',
            "trade" => '\u{2122}',
            "euro" => '\u{20AC}',
            _ => return None,
        }
    };
    // Present nbsp as a normal space for the plain reader.
    let ch = if ch == '\u{00A0}' { ' ' } else { ch };
    Some((ch, end + 1))
}

fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newline_run = 0;
    let mut space_pending = false;
    for ch in s.chars() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            space_pending = false;
            newline_run += 1;
            if newline_run <= 2 {
                out.push('\n');
            }
            continue;
        }
        newline_run = 0;
        if ch.is_whitespace() {
            space_pending = true;
            continue;
        }
        if space_pending {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            space_pending = false;
        }
        out.push(ch);
    }
    // Empty → empty. Never inject UI copy into stored/sent bodies.
    out.trim().to_string()
}

/// Reader-only label when HTML exists but plain extraction yielded nothing.
pub fn missing_plain_placeholder() -> &'static str {
    "(HTML message — plain text unavailable)"
}

fn snippet_from(body: &str) -> String {
    let one_line: String = body
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    one_line.chars().take(140).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_html_and_entities() {
        let t = html_to_text(
            "<p>You&rsquo;re invited to <strong>Microsoft Foundry</strong>&nbsp;— see <a href=\"https://example.com\">details</a>.</p>",
        );
        assert!(
            t.contains("You're invited") || t.contains("You\u{2019}re invited"),
            "{t}"
        );
        assert!(t.contains("Microsoft Foundry"), "{t}");
        assert!(!t.contains("<strong>"), "{t}");
        assert!(!t.contains("&rsquo;"), "{t}");
        assert!(!t.contains("&nbsp;"), "{t}");
    }

    #[test]
    fn skips_style_blocks() {
        let t = html_to_text("<style>.x{color:red}</style><p>Hello</p>");
        assert_eq!(t, "Hello");
    }

    #[test]
    fn detects_htmlish_plain() {
        assert!(looks_like_html("<p>You&rsquo;re invited</p><strong>Hi</strong>"));
        assert!(!looks_like_html("Just a normal email with <angle> brackets once."));
    }

    #[test]
    fn readable_body_cleans_cached_html() {
        let raw = "<p style=\"color:#1f497d\">Hello&nbsp;world</p>";
        let t = readable_body(raw);
        assert!(t.contains("Hello world"), "{t}");
        assert!(!t.contains("<p"), "{t}");
    }
}
