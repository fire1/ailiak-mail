//! Staks — group messages by sender brand/domain (Mailspring-style notification stacks).

use std::collections::HashMap;

use mailbox_core::{classify_mailbox, MailboxRole};
use mailbox_core::types::Envelope;

#[derive(Debug, Clone)]
pub struct Stak {
    /// Stable id (usually eTLD+1 domain).
    pub id: String,
    /// Display label (e.g. "Google").
    pub label: String,
    pub unread: u32,
    pub total: u32,
}

/// Trash / Spam must not feed Staks (search spans folders; skip those copies).
fn skip_for_staks(env: &Envelope) -> bool {
    matches!(
        classify_mailbox(&env.folder_path, &env.folder_path),
        MailboxRole::Trash | MailboxRole::Spam
    )
}

/// Build Staks for the current folder’s envelopes.
/// Keeps stacks with 2+ messages, or any with unread — sorted by unread then total.
pub fn build_staks(envelopes: &[Envelope]) -> Vec<Stak> {
    let mut map: HashMap<String, (String, u32, u32)> = HashMap::new();
    for env in envelopes {
        if skip_for_staks(env) {
            continue;
        }
        let (id, label) = stak_identity(env);
        let entry = map.entry(id).or_insert((label, 0, 0));
        entry.2 += 1;
        if !env.seen {
            entry.1 += 1;
        }
    }

    let mut staks: Vec<Stak> = map
        .into_iter()
        .filter(|(_, (_, unread, total))| *total >= 2 || *unread > 0)
        .map(|(id, (label, unread, total))| Stak {
            id,
            label,
            unread,
            total,
        })
        .collect();

    staks.sort_by(|a, b| {
        b.unread
            .cmp(&a.unread)
            .then(b.total.cmp(&a.total))
            .then(a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    // Keep the rail calm — top stacks only.
    staks.truncate(24);
    staks
}

pub fn envelope_in_stak(env: &Envelope, stak_id: &str) -> bool {
    !skip_for_staks(env) && stak_identity(env).0 == stak_id
}

/// Brand id for logos (`google`, `instagram`, …) when the address maps to a known mark.
pub fn brand_id_for_addr(from_addr: &str) -> Option<&'static str> {
    let domain = from_addr
        .trim()
        .to_lowercase()
        .split('@')
        .nth(1)?
        .to_string();
    known_brand(&domain).map(|(id, _)| id)
}

fn stak_identity(env: &Envelope) -> (String, String) {
    let addr = env.from_addr.trim().to_lowercase();
    let domain = addr.split('@').nth(1).unwrap_or("").trim();
    if domain.is_empty() {
        let fallback = if !env.from_name.trim().is_empty() {
            env.from_name.trim().to_string()
        } else if !addr.is_empty() {
            addr
        } else {
            "Unknown".into()
        };
        return (fallback.to_lowercase(), fallback);
    }

    if let Some((id, label)) = known_brand(domain) {
        return (id.into(), label.into());
    }

    let base = registrable_domain(domain);
    let label = brand_label(&base);
    (base, label)
}

pub(crate) fn known_brand(domain: &str) -> Option<(&'static str, &'static str)> {
    const BRANDS: &[(&str, &str, &str)] = &[
        ("google.com", "google", "Google"),
        ("gmail.com", "google", "Google"),
        ("googlemail.com", "google", "Google"),
        ("youtube.com", "google", "Google"),
        ("instagram.com", "instagram", "Instagram"),
        ("facebook.com", "facebook", "Facebook"),
        ("facebookmail.com", "facebook", "Facebook"),
        ("fb.com", "facebook", "Facebook"),
        ("meta.com", "meta", "Meta"),
        ("twitter.com", "x", "X"),
        ("x.com", "x", "X"),
        ("x.ai", "x", "X"),
        ("linkedin.com", "linkedin", "LinkedIn"),
        ("github.com", "github", "GitHub"),
        ("gitlab.com", "gitlab", "GitLab"),
        ("discord.com", "discord", "Discord"),
        ("discordapp.com", "discord", "Discord"),
        // Teams before generic microsoft.com so notification hosts get the Teams mark.
        ("teams.mail.microsoft.com", "teams", "Teams"),
        ("email.teams.microsoft.com", "teams", "Teams"),
        ("teams.microsoft.com", "teams", "Teams"),
        ("teams.mail.microsoft", "teams", "Teams"),
        ("microsoft.com", "microsoft", "Microsoft"),
        ("outlook.com", "microsoft", "Microsoft"),
        ("hotmail.com", "microsoft", "Microsoft"),
        ("live.com", "microsoft", "Microsoft"),
        ("apple.com", "apple", "Apple"),
        ("amazon.com", "amazon", "Amazon"),
        ("aliexpress.com", "aliexpress", "AliExpress"),
        ("stripe.com", "stripe", "Stripe"),
        ("notion.so", "notion", "Notion"),
        ("slack.com", "slack", "Slack"),
        ("dropbox.com", "dropbox", "Dropbox"),
        ("figma.com", "figma", "Figma"),
        ("svgator.com", "svgator", "SVGator"),
        ("cursor.com", "cursor", "Cursor"),
        ("cursor.sh", "cursor", "Cursor"),
        ("warp.dev", "warp", "Warp"),
        ("openai.com", "openai", "OpenAI"),
        ("chatgpt.com", "openai", "OpenAI"),
        ("steampowered.com", "steam", "Steam"),
        ("steamcommunity.com", "steam", "Steam"),
        ("reddit.com", "reddit", "Reddit"),
        ("spotify.com", "spotify", "Spotify"),
        ("paypal.com", "paypal", "PayPal"),
        ("zoom.us", "zoom", "Zoom"),
        ("whatsapp.com", "whatsapp", "WhatsApp"),
        ("netflix.com", "netflix", "Netflix"),
        ("telegram.org", "telegram", "Telegram"),
        ("t.me", "telegram", "Telegram"),
        ("zed.dev", "zed", "Zed"),
        ("gnome.org", "gnome", "GNOME"),
        ("abv.bg", "abv", "ABV"),
    ];
    for (suffix, id, label) in BRANDS {
        if domain == *suffix {
            return Some((*id, *label));
        }
        // Subdomain match without allocating (".google.com").
        if domain.len() > suffix.len()
            && domain.as_bytes()[domain.len() - suffix.len() - 1] == b'.'
            && domain.ends_with(suffix)
        {
            return Some((*id, *label));
        }
    }
    None
}

/// Rough eTLD+1: `mail.google.com` → `google.com`, `co.uk` kept as two labels when short.
fn registrable_domain(domain: &str) -> String {
    let parts: Vec<&str> = domain.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 2 {
        let last = parts[parts.len() - 1];
        let second = parts[parts.len() - 2];
        // common multi-part public suffixes
        if matches!(last, "uk" | "au" | "br" | "jp") && second.len() <= 3 && parts.len() >= 3 {
            return format!(
                "{}.{}.{}",
                parts[parts.len() - 3],
                second,
                last
            );
        }
        format!("{second}.{last}")
    } else {
        domain.to_string()
    }
}

fn brand_label(domain: &str) -> String {
    let name = domain.split('.').next().unwrap_or(domain);
    if name.is_empty() {
        return domain.to_string();
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return name.to_string();
    };
    let mut out = first.to_uppercase().collect::<String>();
    out.extend(chars);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use mailbox_core::types::Envelope;

    fn env(addr: &str, name: &str, seen: bool) -> Envelope {
        Envelope {
            id: format!("{addr}-{seen}"),
            account_id: "a".into(),
            folder_path: "INBOX".into(),
            message_id: "<x>".into(),
            subject: "s".into(),
            from_name: name.into(),
            from_addr: addr.into(),
            date: Utc::now(),
            seen,
            flagged: false,
            snippet: String::new(),
            labels: Vec::new(),
            in_reply_to: String::new(),
            references: String::new(),
            thread_id: String::new(),
        }
    }

    #[test]
    fn groups_google_domains() {
        let list = vec![
            env("noreply@google.com", "Google", false),
            env("no-reply@accounts.google.com", "Google", true),
            env("alerts@instagram.com", "Instagram", false),
        ];
        let staks = build_staks(&list);
        assert!(staks.iter().any(|s| s.id == "google" && s.total == 2 && s.unread == 1));
        assert!(staks.iter().any(|s| s.id == "instagram" && s.unread == 1));
    }

    #[test]
    fn skips_trash_and_spam() {
        let mut trash = env("noreply@google.com", "Google", false);
        trash.folder_path = "[Gmail]/Trash".into();
        trash.id = "trash-1".into();
        let mut spam = env("noreply@google.com", "Google", false);
        spam.folder_path = "[Gmail]/Spam".into();
        spam.id = "spam-1".into();
        let inbox = env("noreply@google.com", "Google", false);
        let staks = build_staks(&[trash.clone(), spam, inbox.clone()]);
        assert_eq!(staks.len(), 1);
        assert_eq!(staks[0].total, 1);
        assert!(!envelope_in_stak(&trash, "google"));
        assert!(envelope_in_stak(&inbox, "google"));
    }

    #[test]
    fn maps_discord_gitlab_facebookmail() {
        assert_eq!(brand_id_for_addr("noreply@discord.com"), Some("discord"));
        assert_eq!(brand_id_for_addr("gitlab@mg.gitlab.com"), Some("gitlab"));
        assert_eq!(
            brand_id_for_addr("notification@facebookmail.com"),
            Some("facebook")
        );
        assert_eq!(brand_id_for_addr("hello@tm.openai.com"), Some("openai"));
    }

    #[test]
    fn maps_x_ai_and_teams() {
        assert_eq!(brand_id_for_addr("noreply@x.com"), Some("x"));
        assert_eq!(brand_id_for_addr("hello@mail.x.ai"), Some("x"));
        assert_eq!(
            brand_id_for_addr("noreply@email.teams.microsoft.com"),
            Some("teams")
        );
        assert_eq!(
            brand_id_for_addr("bot@teams.mail.microsoft"),
            Some("teams")
        );
    }
}
