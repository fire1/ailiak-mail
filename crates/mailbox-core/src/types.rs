use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type AccountId = String;
pub type MessageId = String;

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
