//! Ailiak Desk slash dialect (subset of `composeCommands.ts`).
//!
//! Users type one line; we parse into structured create_task fields.

use chrono::{Datelike, Duration, Local, NaiveDate};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedCompose {
    pub title: String,
    pub detail: String,
    pub text: String,
    pub estimate_seconds: u64,
    pub spent_seconds: u64,
    pub done: bool,
    /// Set when `/from`, `/date`, or `/todo` appears.
    pub for_date: Option<ForDate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForDate {
    /// YYYY-MM-DD
    Day(String),
    /// Explicitly unscheduled (`forDate: null`)
    Unscheduled,
}

pub fn today_key() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}

pub fn shift_day(today: &str, days: i64) -> String {
    NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .unwrap_or_else(|_| Local::now().date_naive())
        .checked_add_signed(Duration::days(days))
        .unwrap_or_else(|| Local::now().date_naive())
        .format("%Y-%m-%d")
        .to_string()
}

/// Parse `/from` / `/date` values into a calendar day.
pub fn parse_for_date_arg(raw: &str, today: &str) -> Option<ForDate> {
    let s = raw.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    if s == "null" || s == "none" || s == "unscheduled" || s == "-" {
        return Some(ForDate::Unscheduled);
    }
    if s == "today" {
        return Some(ForDate::Day(today.to_string()));
    }
    if s == "tomorrow" {
        return Some(ForDate::Day(shift_day(today, 1)));
    }
    if s == "yesterday" {
        return Some(ForDate::Day(shift_day(today, -1)));
    }
    if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
        return Some(ForDate::Day(d.format("%Y-%m-%d").to_string()));
    }
    let parts: Vec<_> = s.split('-').collect();
    if parts.len() == 3 && parts[0].len() == 2 {
        if let (Ok(y), Ok(m), Ok(d)) = (
            parts[0].parse::<i32>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        ) {
            let century = today[..2].parse::<i32>().unwrap_or(20) * 100;
            if let Some(date) = NaiveDate::from_ymd_opt(century + y, m, d) {
                return Some(ForDate::Day(date.format("%Y-%m-%d").to_string()));
            }
        }
    }
    if parts.len() == 2 {
        if let (Ok(m), Ok(d)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
            let y = today[..4].parse::<i32>().unwrap_or(Local::now().year());
            if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
                return Some(ForDate::Day(date.format("%Y-%m-%d").to_string()));
            }
        }
    }
    if let Some(rest) = s.strip_prefix('+') {
        if let Ok(n) = rest.parse::<i64>() {
            return Some(ForDate::Day(shift_day(today, n)));
        }
    }
    if let Some(rest) = s.strip_prefix('-') {
        if let Ok(n) = rest.parse::<i64>() {
            return Some(ForDate::Day(shift_day(today, -n)));
        }
    }
    None
}

pub fn parse_duration_to_seconds(raw: &str) -> u64 {
    let s = raw.trim().to_lowercase();
    if s.is_empty() {
        return 0;
    }
    if let Some((h, rest)) = s.split_once(':') {
        let h: u64 = h.parse().unwrap_or(0);
        let parts: Vec<_> = rest.split(':').collect();
        if parts.len() == 1 {
            let m: u64 = parts[0].parse().unwrap_or(0);
            return h * 3600 + m * 60;
        }
        if parts.len() == 2 {
            let m: u64 = parts[0].parse().unwrap_or(0);
            let sec: u64 = parts[1].parse().unwrap_or(0);
            return h * 3600 + m * 60 + sec;
        }
    }
    if s.chars().all(|c| c.is_ascii_digit() || c == '.') {
        let mins: f64 = s.parse().unwrap_or(0.0);
        return (mins * 60.0).round() as u64;
    }
    let mut total = 0.0_f64;
    let mut matched = false;
    let re = regex_lite_duration(&s);
    for (n, unit) in re {
        matched = true;
        if unit.starts_with('h') {
            total += n * 3600.0;
        } else if unit.starts_with('m') {
            total += n * 60.0;
        } else {
            total += n;
        }
    }
    if matched {
        total.round().max(0.0) as u64
    } else {
        0
    }
}

fn regex_lite_duration(s: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        rest = rest.trim_start();
        let num_end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        if num_end == 0 {
            break;
        }
        let n: f64 = rest[..num_end].parse().unwrap_or(0.0);
        rest = &rest[num_end..];
        rest = rest.trim_start();
        let unit_end = rest
            .find(|c: char| c.is_whitespace() || c.is_ascii_digit())
            .unwrap_or(rest.len());
        if unit_end == 0 {
            break;
        }
        let unit = rest[..unit_end].to_lowercase();
        rest = &rest[unit_end..];
        if unit.starts_with('h')
            || unit.starts_with('m')
            || unit.starts_with('s')
            || unit == "hr"
            || unit == "hrs"
            || unit == "min"
            || unit == "mins"
            || unit == "sec"
            || unit == "secs"
        {
            out.push((n, unit));
        } else {
            break;
        }
    }
    out
}

/// Parse a compose field that may include slash commands.
pub fn parse_compose_commands(raw: &str) -> ParsedCompose {
    let today = today_key();
    let source = raw;
    let mut parsed = ParsedCompose::default();
    let mut body_parts: Vec<&str> = Vec::new();
    let mut i = 0;
    let bytes = source.as_bytes();

    while i < source.len() {
        // Find next `/cmd`
        if let Some(rel) = source[i..].find('/') {
            let slash = i + rel;
            if slash > i {
                body_parts.push(&source[i..slash]);
            }
            // Identify command name
            let after = &source[slash + 1..];
            let name_end = after
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(after.len());
            let name = after[..name_end].to_lowercase();
            let content_start = slash + 1 + name_end;
            // skip whitespace after command
            let mut cs = content_start;
            while cs < source.len() && bytes[cs].is_ascii_whitespace() && bytes[cs] != b'\n' {
                cs += 1;
            }

            match name.as_str() {
                "title" => {
                    let end = source[cs..]
                        .find('\n')
                        .map(|n| cs + n)
                        .unwrap_or(source.len());
                    parsed.title = source[cs..end].trim().chars().take(80).collect();
                    i = if end < source.len() { end + 1 } else { end };
                }
                "help" => {
                    let end = source[cs..]
                        .find('\n')
                        .map(|n| cs + n)
                        .unwrap_or(source.len());
                    parsed.detail = source[cs..end].trim().chars().take(4000).collect();
                    i = if end < source.len() { end + 1 } else { end };
                }
                "details" | "detail" => {
                    parsed.detail = source[cs..].trim().chars().take(4000).collect();
                    i = source.len();
                }
                "estimate" => {
                    let end = duration_end(source, cs);
                    parsed.estimate_seconds = parse_duration_to_seconds(&source[cs..end]);
                    i = end;
                }
                "spent" | "spend" => {
                    let end = duration_end(source, cs);
                    parsed.spent_seconds = parse_duration_to_seconds(&source[cs..end]);
                    i = end;
                }
                "done" => {
                    parsed.done = true;
                    i = cs;
                }
                "todo" => {
                    parsed.for_date = Some(ForDate::Day(shift_day(&today, 1)));
                    i = cs;
                }
                "from" | "date" => {
                    let end = token_end(source, cs);
                    parsed.for_date = parse_for_date_arg(&source[cs..end], &today);
                    i = end;
                }
                "draft" | "tpl" => {
                    let end = source[cs..]
                        .find('\n')
                        .map(|n| cs + n)
                        .unwrap_or(source.len());
                    let name = source[cs..end].trim();
                    if !name.is_empty() && parsed.title.is_empty() {
                        parsed.title = name.chars().take(80).collect();
                    }
                    i = if end < source.len() { end + 1 } else { end };
                }
                _ => {
                    // Unknown slash — keep literal in body
                    body_parts.push(&source[slash..slash + 1 + name_end]);
                    i = slash + 1 + name_end;
                }
            }
        } else {
            body_parts.push(&source[i..]);
            break;
        }
    }

    let text: String = body_parts
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    parsed.text = text.chars().take(255).collect();
    parsed
}

fn token_end(source: &str, start: usize) -> usize {
    let mut i = start;
    let b = source.as_bytes();
    while i < source.len() && !b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn duration_end(source: &str, start: usize) -> usize {
    // Consume a greedy duration run similar to Ailiak endOfDuration.
    let mut i = start;
    let b = source.as_bytes();
    let skip_space = |i: &mut usize| {
        while *i < source.len() && (b[*i] == b' ' || b[*i] == b'\t') {
            *i += 1;
        }
    };
    skip_space(&mut i);
    // colon form
    let slice = &source[i..];
    if let Some(m) = slice.find(|c: char| c.is_whitespace()) {
        let token = &slice[..m];
        if token.contains(':') {
            return i + m;
        }
    } else if slice.contains(':') {
        return source.len();
    }
    // unit tokens
    let start_i = i;
    let mut consumed = false;
    loop {
        skip_space(&mut i);
        let rest = &source[i..];
        let num_end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        if num_end == 0 {
            break;
        }
        let after_num = &rest[num_end..];
        let unit = after_num.split_whitespace().next().unwrap_or("");
        let u = unit.to_lowercase();
        if u.starts_with('h') || u.starts_with('m') || u.starts_with('s') {
            i += num_end + unit.len();
            consumed = true;
        } else if !consumed && num_end > 0 {
            // bare minutes
            i += num_end;
            consumed = true;
            break;
        } else {
            break;
        }
    }
    if consumed {
        i
    } else {
        start_i
    }
}

/// Soft hint like Ailiak `composeHint`.
pub fn compose_hint(parsed: &ParsedCompose) -> String {
    let mut bits: Vec<String> = Vec::new();
    if !parsed.title.is_empty() {
        bits.push("title".into());
    }
    if !parsed.detail.is_empty() {
        bits.push("details".into());
    }
    match &parsed.for_date {
        Some(ForDate::Day(d)) => bits.push(format!("day {d}")),
        Some(ForDate::Unscheduled) => bits.push("unscheduled".into()),
        None => {}
    }
    if parsed.estimate_seconds > 0 {
        bits.push("estimate".into());
    }
    if parsed.spent_seconds > 0 {
        bits.push("spent".into());
    }
    if parsed.done {
        bits.push("done".into());
    }
    if !parsed.text.is_empty() {
        bits.push("note".into());
    }
    if bits.is_empty() {
        "Try /from tomorrow · /estimate 1h · #mail".into()
    } else {
        bits.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_todo_tomorrow() {
        let p = parse_compose_commands("Reply later /todo #mail");
        assert!(matches!(p.for_date, Some(ForDate::Day(_))));
        assert!(p.text.contains("Reply later"));
        assert!(p.text.contains("#mail"));
    }

    #[test]
    fn parses_estimate() {
        let p = parse_compose_commands("Ship it /estimate 1h 30m");
        assert_eq!(p.estimate_seconds, 5400);
    }

    #[test]
    fn parses_from_date() {
        let today = today_key();
        let p = parse_compose_commands(&format!("Hi /from {today}"));
        assert_eq!(p.for_date, Some(ForDate::Day(today)));
    }
}
