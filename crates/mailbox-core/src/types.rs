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
}

/// Display order: Inbox → Sent → Archive → Trash → everything else.
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
    } else if p.contains("[gmail]") {
        5
    } else {
        4
    };
    (rank, p)
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
            },
            Folder {
                account_id: "a".into(),
                name: "Archive".into(),
                path: "Archive".into(),
                unread: 0,
            },
            Folder {
                account_id: "a".into(),
                name: "Inbox".into(),
                path: "INBOX".into(),
                unread: 0,
            },
            Folder {
                account_id: "a".into(),
                name: "Sent".into(),
                path: "Sent".into(),
                unread: 0,
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
