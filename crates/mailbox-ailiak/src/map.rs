use mailbox_core::config::AiliakConfig;
use mailbox_core::types::Message;
use serde::Serialize;

use crate::compose::{parse_compose_commands, ForDate, ParsedCompose};

const DETAIL_MAX: usize = 48 * 1024;

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

/// Prefill Todo strip fields from a mail message.
pub fn todo_defaults(msg: &Message, cfg: &AiliakConfig) -> (String, String) {
    let subject = trim_chars(&msg.envelope.subject, 80);
    let hashtag = if cfg.hashtag.is_empty() {
        "mail".to_string()
    } else {
        cfg.hashtag.trim_start_matches('#').to_string()
    };
    let line = trim_chars(&format!("Reply: {subject}  #{hashtag}"), 255);
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
    (line, detail)
}

pub fn map_message_to_task(msg: &Message, cfg: &AiliakConfig) -> CreateTaskPayload {
    let (line, detail) = todo_defaults(msg, cfg);
    build_payload(
        &line,
        &detail,
        None,
        Some(short_task_number(&msg.envelope.message_id)),
        cfg,
    )
}

/// Build payload from Todo strip: compose line + detail + optional UI day override.
///
/// `ui_for_date`: `None` = today (omit field); `Some("")` = unscheduled; `Some(YYYY-MM-DD)` = that day.
/// Slash commands in `line` override the UI day when present.
pub fn build_payload(
    line: &str,
    detail: &str,
    ui_for_date: Option<String>,
    task_number: Option<String>,
    cfg: &AiliakConfig,
) -> CreateTaskPayload {
    let parsed = parse_compose_commands(line);
    merge_parsed(parsed, detail, ui_for_date, task_number, cfg)
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
        trim_chars(&format!("{title}  #{hashtag}"), 255)
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
                snippet: "hi".into(),
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
}
