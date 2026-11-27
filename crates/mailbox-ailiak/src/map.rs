use mailbox_core::config::AiliakConfig;
use mailbox_core::types::Message;
use serde::Serialize;

use crate::compose::{parse_compose_commands, ForDate, ParsedCompose};

const DETAIL_MAX: usize = 48 * 1024;
/// Ailiak `text` field limit (characters).
pub const TEXT_MAX: usize = 255;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskPayload {
    pub text: String,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub for_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spent_seconds: Option<u64>,
    #[serde(default, skip_serializing)]
    pub mark_done: bool,
}

/// Prefill a single Options Todo compose field from a mail message.
///
/// Short task line plus `/details` with From/Date/Message-ID and body.
pub fn todo_defaults(msg: &Message, cfg: &AiliakConfig) -> String {
    let subject = trim_chars(&msg.envelope.subject, 80);
    let hashtag = if cfg.hashtag.is_empty() {
        "mail".to_string()
    } else {
        cfg.hashtag.trim_start_matches('#').to_string()
    };
    let line = trim_chars(&format!("Reply: {subject}  #{hashtag}"), TEXT_MAX);
    let detail = format!(
        "From: {}\nDate: {}\nMessage-ID: {}\n\n{}",
        msg.display_from(),
        msg.envelope.date.format("%Y-%m-%d %H:%M UTC"),
        msg.envelope.message_id,
        msg.body_text
    );
    let detail = if detail.len() > DETAIL_MAX {
        let mut t = detail;
        t.truncate(DETAIL_MAX);
        t.push_str("\n…[truncated]");
        t
    } else {
        detail
    };
    if detail.trim().is_empty() {
        line
    } else {
        format!("{line}\n/details\n{detail}")
    }
}

pub fn map_message_to_task(msg: &Message, cfg: &AiliakConfig) -> CreateTaskPayload {
    let compose = todo_defaults(msg, cfg);
    build_payload(
        &compose,
        "",
        None,
        Some(short_task_number(&msg.envelope.message_id)),
        cfg,
    )
}

/// Build payload from Options Todo compose (slash dialect) + optional UI day override.
///
/// `ui_for_date`: `None` = today (omit field); `Some("")` = unscheduled; `Some(YYYY-MM-DD)` = that day.
/// Slash commands in `compose` override the UI day when present.
/// `detail_fallback` is merged when compose has no `/details` (usually empty now).
pub fn build_payload(
    compose: &str,
    detail_fallback: &str,
    ui_for_date: Option<String>,
    task_number: Option<String>,
    cfg: &AiliakConfig,
) -> CreateTaskPayload {
    let parsed = parse_compose_commands(compose);
    merge_parsed(parsed, detail_fallback, ui_for_date, task_number, cfg)
}

/// Flatten email selection to plain task text: drop control chars / newlines,
/// strip quote marks, collapse whitespace.
pub fn clean_selection_text(selection: &str) -> String {
    let mut out = String::with_capacity(selection.len());
    let mut prev_space = true; // trim leading
    for c in selection.chars() {
        // ASCII + common curly/smart double quotes from mail clients.
        if matches!(c, '"' | '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}') {
            continue;
        }
        if c.is_control() || c == '\u{00a0}' || c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    if prev_space {
        out.pop(); // trailing space
    }
    out
}

/// Insert email selection into the Task compose field.
///
/// Selection is cleaned to a single line of plain text. Text over [`TEXT_MAX`]
/// characters is split onto a `/details` block.
pub fn insert_selection_into_compose(compose: &mut String, selection: &str) {
    let sel = clean_selection_text(selection);
    if sel.is_empty() {
        return;
    }

    if compose.trim().is_empty() {
        *compose = format_text_with_details(&sel);
        return;
    }

    let (before, after) = split_details(compose);
    let (new_before, new_after) = if after.is_some() {
        let mut d = after.unwrap_or_default();
        if !d.is_empty() {
            d.push(' ');
        }
        d.push_str(&sel);
        (before, Some(d))
    } else {
        let mut body = before;
        if !body.is_empty() {
            body.push(' ');
        }
        body.push_str(&sel);
        if body.chars().count() <= TEXT_MAX {
            (body, None)
        } else {
            let head: String = body.chars().take(TEXT_MAX).collect();
            let rest: String = body.chars().skip(TEXT_MAX).collect();
            (head, Some(rest))
        }
    };

    *compose = join_details(&new_before, new_after.as_deref());
}

fn format_text_with_details(text: &str) -> String {
    if text.chars().count() <= TEXT_MAX {
        text.to_string()
    } else {
        let head: String = text.chars().take(TEXT_MAX).collect();
        let rest: String = text.chars().skip(TEXT_MAX).collect();
        join_details(&head, Some(&rest))
    }
}

fn split_details(compose: &str) -> (String, Option<String>) {
    let bytes = compose.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' {
            let at_boundary = i == 0 || bytes[i - 1].is_ascii_whitespace();
            if at_boundary {
                let rest = &compose[i + 1..];
                let name_end = rest
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
                    .unwrap_or(rest.len());
                let name = &rest[..name_end];
                if name.eq_ignore_ascii_case("details") || name.eq_ignore_ascii_case("detail") {
                    let before = compose[..i].trim_end().to_string();
                    let after_start = i + 1 + name_end;
                    let after = compose[after_start..].trim_start().to_string();
                    return (before, Some(after));
                }
            }
        }
        i += 1;
    }
    (compose.to_string(), None)
}

fn join_details(before: &str, after: Option<&str>) -> String {
    match after {
        Some(d) if !d.is_empty() => {
            if before.is_empty() {
                format!("/details\n{d}")
            } else {
                format!("{before}\n/details\n{d}")
            }
        }
        _ => before.to_string(),
    }
}

fn merge_parsed(
    parsed: ParsedCompose,
    detail_fallback: &str,
    ui_for_date: Option<String>,
    task_number: Option<String>,
    cfg: &AiliakConfig,
) -> CreateTaskPayload {
    let title = if parsed.title.is_empty() {
        // Use text before hashtags as title fallback
        let t = parsed
            .text
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches("Reply:")
            .trim();
        trim_chars(t, 80)
    } else {
        parsed.title.clone()
    };

    let text = if parsed.text.is_empty() {
        let hashtag = if cfg.hashtag.is_empty() {
            "mail".into()
        } else {
            cfg.hashtag.trim_start_matches('#').to_string()
        };
        trim_chars(&format!("{title}  #{hashtag}"), TEXT_MAX)
    } else {
        parsed.text.clone()
    };

    let detail = if parsed.detail.is_empty() {
        detail_fallback.to_string()
    } else if detail_fallback.is_empty() {
        parsed.detail.clone()
    } else {
        format!("{}\n\n---\n{}", parsed.detail, detail_fallback)
    };

    let for_date = if let Some(fd) = parsed.for_date {
        match fd {
            ForDate::Day(d) => Some(d),
            ForDate::Unscheduled => Some(String::new()),
        }
    } else if let Some(ui) = ui_for_date {
        Some(ui)
    } else {
        match cfg.default_for_date.as_str() {
            "null" | "none" | "unscheduled" => Some(String::new()),
            "today" | "" => None,
            other => Some(other.to_string()),
        }
    };

    CreateTaskPayload {
        text,
        title,
        detail,
        for_date,
        embed_url: None,
        task_number,
        estimate_seconds: if parsed.estimate_seconds > 0 {
            Some(parsed.estimate_seconds)
        } else {
            None
        },
        spent_seconds: if parsed.spent_seconds > 0 {
            Some(parsed.spent_seconds)
        } else {
            None
        },
        mark_done: parsed.done,
    }
}

fn trim_chars(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.pop();
        out.push('…');
    }
    out
}

pub fn short_task_number(message_id: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    message_id.hash(&mut h);
    let n = h.finish();
    format!("m{:x}", n).chars().take(24).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use mailbox_core::types::Envelope;

    #[test]
    fn maps_subject_and_hashtag() {
        let msg = Message {
            envelope: Envelope {
                id: "1".into(),
                account_id: "a".into(),
                folder_path: "INBOX".into(),
                message_id: "<x@y>".into(),
                subject: "Hello world".into(),
                from_name: "Ada".into(),
                from_addr: "ada@example.com".into(),
                date: Utc::now(),
                seen: true,
                flagged: false,
                snippet: "hi".into(),
                labels: Vec::new(),
                in_reply_to: String::new(),
                references: String::new(),
                thread_id: String::new(),
            },
            body_text: "body".into(),
            body_html: String::new(),
            to: vec![],
            cc: vec![],
        };
        let cfg = AiliakConfig::default();
        let p = map_message_to_task(&msg, &cfg);
        assert_eq!(p.title, "Hello world");
        assert!(p.text.contains("#mail"));
        assert!(p.detail.contains("Ada"));
        assert!(p.detail.contains("body"));
    }

    #[test]
    fn slash_overrides_day() {
        let cfg = AiliakConfig::default();
        let p = build_payload(
            "Ship /todo #mail",
            "detail",
            Some("2099-01-01".into()),
            None,
            &cfg,
        );
        assert!(p.for_date.as_ref().is_some_and(|d| !d.is_empty()));
        assert_ne!(p.for_date.as_deref(), Some("2099-01-01"));
    }

    #[test]
    fn selection_over_limit_adds_details() {
        let long: String = "x".repeat(300);
        let mut compose = String::new();
        insert_selection_into_compose(&mut compose, &long);
        assert!(compose.contains("/details"));
        let (before, after) = split_details(&compose);
        assert_eq!(before.chars().count(), TEXT_MAX);
        assert_eq!(after.as_ref().map(|s| s.chars().count()), Some(45));
    }

    #[test]
    fn selection_appends_into_existing_details() {
        let mut compose = "Reply: Hi  #mail\n/details\nmeta".into();
        insert_selection_into_compose(&mut compose, "picked");
        assert!(compose.contains("meta picked") || compose.contains("picked"));
        assert!(compose.contains("/details"));
    }

    #[test]
    fn selection_strips_newlines_and_controls() {
        let mut compose = String::new();
        insert_selection_into_compose(&mut compose, "Hello\n\nworld\t—\u{00a0}ok\r\n");
        assert_eq!(compose, "Hello world — ok");
        assert!(!compose.contains('\n'));
    }

    #[test]
    fn selection_strips_quotes() {
        let mut compose = String::new();
        insert_selection_into_compose(&mut compose, "\"Hello\" “world”");
        assert_eq!(compose, "Hello world");
        assert!(!compose.contains('"'));
    }
}
