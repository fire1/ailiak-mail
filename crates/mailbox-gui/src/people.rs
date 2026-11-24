//! People — human peers for chat-style conversations (vs brand Staks).

use std::collections::HashMap;

use mailbox_core::types::Envelope;

use crate::staks;

#[derive(Debug, Clone)]
pub struct Person {
    /// Lowercased email address.
    pub addr: String,
    pub name: String,
    pub unread: u32,
    pub total: u32,
    pub last_snippet: String,
}

/// True when this From address looks like a person (not brand / noreply / robot).
pub fn is_person_addr(from_addr: &str, from_name: &str) -> bool {
    let addr = from_addr.trim().to_lowercase();
    if addr.is_empty() || !addr.contains('@') {
        return false;
    }
    if staks::brand_id_for_addr(&addr).is_some() {
        // Shared consumer domains (gmail/outlook) still appear as brands in Staks —
        // carve those out so individuals remain people.
        if is_personal_provider(&addr) {
            return !looks_robot_local(&addr);
        }
        return false;
    }
    if looks_robot_local(&addr) {
        return false;
    }
    let name = from_name.trim().to_lowercase();
    if name.contains("noreply") || name.contains("no-reply") || name.contains("mailer-daemon") {
        return false;
    }
    true
}

fn is_personal_provider(addr: &str) -> bool {
    let domain = addr.split('@').nth(1).unwrap_or("");
    matches!(
        domain,
        "gmail.com"
            | "googlemail.com"
            | "outlook.com"
            | "hotmail.com"
            | "live.com"
            | "msn.com"
            | "yahoo.com"
            | "icloud.com"
            | "me.com"
            | "mac.com"
            | "proton.me"
            | "protonmail.com"
            | "abv.bg"
            | "mail.bg"
    )
}

fn looks_robot_local(addr: &str) -> bool {
    let local = addr.split('@').next().unwrap_or("");
    let l = local.to_lowercase();
    l.contains("noreply")
        || l.contains("no-reply")
        || l.contains("no_reply")
        || l == "mailer-daemon"
        || l == "postmaster"
        || l.starts_with("notifications")
        || l.starts_with("newsletter")
        || l.starts_with("news@")
        || l.ends_with("-noreply")
        || l.ends_with(".noreply")
}

/// Build People list from folder envelopes (Inbox-oriented; also fine for mixed lists).
pub fn build_people(envelopes: &[Envelope]) -> Vec<Person> {
    let mut map: HashMap<String, Person> = HashMap::new();
    for env in envelopes {
        if !is_person_addr(&env.from_addr, &env.from_name) {
            continue;
        }
        let addr = env.from_addr.trim().to_lowercase();
        let entry = map.entry(addr.clone()).or_insert_with(|| Person {
            addr: addr.clone(),
            name: if env.from_name.trim().is_empty() {
                addr.clone()
            } else {
                env.from_name.trim().to_string()
            },
            unread: 0,
            total: 0,
            last_snippet: String::new(),
        });
        entry.total += 1;
        if !env.seen {
            entry.unread += 1;
        }
        if entry.name == entry.addr && !env.from_name.trim().is_empty() {
            entry.name = env.from_name.trim().to_string();
        }
        if entry.last_snippet.is_empty() {
            entry.last_snippet = env.snippet.clone();
        }
    }

    let mut people: Vec<Person> = map.into_values().collect();
    people.sort_by(|a, b| {
        b.unread
            .cmp(&a.unread)
            .then(b.total.cmp(&a.total))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    people.truncate(48);
    people
}

pub fn display_name(person: &Person) -> &str {
    if person.name.is_empty() {
        &person.addr
    } else {
        &person.name
    }
}
