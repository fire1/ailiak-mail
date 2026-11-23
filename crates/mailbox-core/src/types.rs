use chrono::{DateTime, Duration, Local, Utc};
use serde::{Deserialize, Serialize};

pub type AccountId = String;
pub type MessageId = String;

/// Message-list section header for a UTC timestamp (local calendar day).
/// Returns uppercase labels: `TODAY`, `YESTERDAY`, or e.g. `MON 12 NOV`.
pub fn date_group_label(dt: DateTime<Utc>) -> String {
    let local = dt.with_timezone(&Local);
    let today = Local::now().date_naive();
    let d = local.date_naive();
    if d == today {
        "TODAY".into()
    } else if d == today - Duration::days(1) {
        "YESTERDAY".into()
    } else {
        local.format("%a %d %b").to_string().to_uppercase()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Folder {
    pub account_id: AccountId,
    pub name: String,
    pub path: String,
    pub unread: u32,
    /// When true, folder is kept in the DB but hidden from the nav rail.
    #[serde(default)]
    pub hidden: bool,
}

/// Local-only virtual folder for snoozed mail (not an IMAP mailbox).
pub const SNOOZED_FOLDER_PATH: &str = "Snoozed";

pub fn is_snoozed_folder(path: &str) -> bool {
    path.eq_ignore_ascii_case(SNOOZED_FOLDER_PATH)
}

/// Display order: Inbox → Sent → Archive → Trash → … → Snoozed → Gmail specials.
pub fn folder_sort_key(path: &str) -> (u8, String) {
    let p = path.to_lowercase();
    let rank = if p == "inbox" || p.ends_with("/inbox") || p.ends_with(".inbox") {
        0
    } else if p.contains("sent") {
        1
    } else if p.contains("archive") {
        2
    } else if p.contains("trash") || p.contains("bin") || p.contains("deleted") {
        3
    } else if is_snoozed_folder(path) {
        6
    } else if p.contains("[gmail]") {
        5
    } else {
        4
    };
    (rank, p)
}

/// Target mailbox for quick Archive / Trash actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialFolderKind {
    Archive,
    Trash,
}

impl SpecialFolderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Archive => "Archive",
            Self::Trash => "Trash",
        }
    }

    fn matches(self, path: &str, name: &str) -> bool {
        let key = format!("{path} {name}").to_lowercase();
        match self {
            Self::Archive => {
                key.contains("archive")
                    // Gmail has no classic Archive mailbox; All Mail is the usual target.
                    || key.contains("[gmail]/all mail")
            }
            Self::Trash => {
                key.contains("trash") || key.contains("bin") || key.contains("deleted")
            }
        }
    }
}

/// Pick the best local/IMAP path for Archive or Trash from known folders.
pub fn resolve_special_folder(folders: &[Folder], kind: SpecialFolderKind) -> Option<String> {
    let mut best: Option<(i32, String)> = None;
    for f in folders {
        if !kind.matches(&f.path, &f.name) {
            continue;
        }
        let key = format!("{} {}", f.path, f.name).to_lowercase();
        // Prefer exact-ish names over Gmail “All Mail” for archive.
        let score = match kind {
            SpecialFolderKind::Archive if key.contains("archive") => 3,
            SpecialFolderKind::Archive => 1,
            SpecialFolderKind::Trash if key.contains("trash") => 3,
            SpecialFolderKind::Trash => 2,
        };
        if best.as_ref().map(|(s, _)| *s).unwrap_or(0) < score {
            best = Some((score, f.path.clone()));
        }
    }
    best.map(|(_, path)| path)
}

/// Parse IMAP UID from local id `{account_id}:{folder_path}:{uid}`.
pub fn parse_imap_uid(id: &str, account_id: &str, folder_path: &str) -> Option<u32> {
    let prefix = format!("{account_id}:{folder_path}:");
    id.strip_prefix(&prefix)?.parse().ok()
}

pub fn sort_folders(folders: &mut [Folder]) {
    folders.sort_by_key(|a| folder_sort_key(&a.path));
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn date_group_today_yesterday_and_other() {
        let now = Utc::now();
        assert_eq!(date_group_label(now), "TODAY");
        assert_eq!(date_group_label(now - Duration::days(1)), "YESTERDAY");
        let older = Utc.with_ymd_and_hms(2020, 1, 15, 12, 0, 0).unwrap();
        let label = date_group_label(older);
        assert!(!label.is_empty());
        assert_eq!(label, label.to_uppercase());
        assert_ne!(label, "TODAY");
        assert_ne!(label, "YESTERDAY");
    }

    #[test]
    fn folder_order_inbox_sent_archive_trash() {
        let mut folders = vec![
            Folder {
                account_id: "a".into(),
                name: "Trash".into(),
                path: "Trash".into(),
                unread: 0,
                hidden: false,
            },
            Folder {
                account_id: "a".into(),
                name: "Archive".into(),
                path: "Archive".into(),
                unread: 0,
                hidden: false,
            },
            Folder {
                account_id: "a".into(),
                name: "Inbox".into(),
                path: "INBOX".into(),
                unread: 0,
                hidden: false,
            },
            Folder {
                account_id: "a".into(),
                name: "Sent".into(),
                path: "Sent".into(),
                unread: 0,
                hidden: false,
            },
        ];
        sort_folders(&mut folders);
        let names: Vec<_> = folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Inbox", "Sent", "Archive", "Trash"]);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Envelope {
    pub id: MessageId,
    pub account_id: AccountId,
    pub folder_path: String,
    pub message_id: String,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub date: DateTime<Utc>,
    pub seen: bool,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub envelope: Envelope,
    pub body_text: String,
    /// Sanitized-enough HTML for the reader (empty when the message was plain-only).
    #[serde(default)]
    pub body_html: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
}

impl Message {
    pub fn display_from(&self) -> String {
        if self.envelope.from_name.is_empty() {
            self.envelope.from_addr.clone()
        } else {
            format!("{} <{}>", self.envelope.from_name, self.envelope.from_addr)
        }
    }
}
