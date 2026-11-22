//! Shared RFC822 → Message parsing for IMAP and POP3.

use chrono::{DateTime, Utc};
use mail_parser::MessageParser;

use crate::types::{Envelope, Message};

pub fn parse_raw_message(
    account_id: &str,
    folder_path: &str,
    uid: u32,
    seen: bool,
    raw: &[u8],
) -> Option<Message> {
    let parsed = MessageParser::default().parse(raw)?;

    let message_id = parsed
        .message_id()
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("uid-{uid}@{folder_path}"));

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

    let mut body_text = extract_text(&parsed);
    let snippet = snippet_from(&body_text);

    if body_text.len() > 512 * 1024 {
        body_text.truncate(512 * 1024);
        body_text.push_str("\n…[truncated]");
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
            snippet,
        },
        body_text,
        to,
        cc,
    })
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

fn extract_text(parsed: &mail_parser::Message<'_>) -> String {
    if let Some(text) = parsed.body_text(0) {
        return text.to_string();
    }
    if let Some(html) = parsed.body_html(0) {
        return html_to_text(&html);
    }
    String::from("(no plain-text body)")
}

pub(crate) fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        "(HTML message — plain text unavailable)".into()
    } else {
        collapsed
    }
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
    fn strips_html() {
        let t = html_to_text("<p>Hello <b>world</b></p>");
        assert!(t.contains("Hello"));
        assert!(t.contains("world"));
    }
}
