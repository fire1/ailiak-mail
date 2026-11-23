use anyhow::Result;
use chrono::{Duration, Utc};

use crate::store::Store;
use crate::types::{Envelope, Folder, Message};

const DEMO_ACCOUNT: &str = "demo";

/// Populate the store with calm sample mail so the GUI works before IMAP is configured.
pub fn seed_demo_if_empty(store: &Store) -> Result<bool> {
    if store.message_count()? > 0 {
        return Ok(false);
    }

    let folders = [
        ("Inbox", "INBOX", 2u32),
        ("Sent", "Sent", 0),
        ("Archive", "Archive", 0),
        ("Trash", "Trash", 0),
    ];
    for (name, path, unread) in folders {
        store.upsert_folder(&Folder {
            account_id: DEMO_ACCOUNT.into(),
            name: name.into(),
            path: path.into(),
            unread,
            hidden: false,
        })?;
    }

    let now = Utc::now();
    let messages = [
        Message {
            envelope: Envelope {
                id: "demo-1".into(),
                account_id: DEMO_ACCOUNT.into(),
                folder_path: "INBOX".into(),
                message_id: "<welcome@mailbox.local>".into(),
                subject: "Welcome to Mailbox".into(),
                from_name: "Mailbox".into(),
                from_addr: "hello@mailbox.local".into(),
                date: now - Duration::hours(2),
                seen: false,
                snippet: "A calm Rust email client for Linux…".into(),
            },
            body_text: "Welcome to Mailbox\n\n\
Mailbox keeps mail local. Sync IMAP when you are ready, then send selected \
messages to Ailiak as Desk tasks — without turning Ailiak into a second inbox.\n\n\
Tips:\n\
  • Browse folders on the left\n\
  • Open a message to read plain text\n\
  • Settings → configure IMAP, POP3, or Ailiak\n\
  • Send to Ailiak creates a Desk task from the open message\n\n\
— Mailbox\n"
                .into(),
            body_html: String::new(),
            to: vec!["you@example.com".into()],
            cc: vec![],
        },
        Message {
            envelope: Envelope {
                id: "demo-2".into(),
                account_id: DEMO_ACCOUNT.into(),
                folder_path: "INBOX".into(),
                message_id: "<desk@ailiak.example>".into(),
                subject: "Try Send to Ailiak".into(),
                from_name: "Ada".into(),
                from_addr: "ada@example.com".into(),
                date: now - Duration::hours(5),
                seen: false,
                snippet: "When you wire an agent token…".into(),
            },
            body_text: "Hi,\n\n\
When you wire an Ailiak agent token in Settings, open this message and choose \
Send to Ailiak. Mailbox will create a Desk task with the subject, a short #mail \
line, and enough detail to act — without uploading attachments.\n\n\
Ada\n"
                .into(),
            body_html: String::new(),
            to: vec!["you@example.com".into()],
            cc: vec![],
        },
        Message {
            envelope: Envelope {
                id: "demo-3".into(),
                account_id: DEMO_ACCOUNT.into(),
                folder_path: "INBOX".into(),
                message_id: "<sync@mailbox.local>".into(),
                subject: "Offline cache is ready".into(),
                from_name: "Mailbox".into(),
                from_addr: "hello@mailbox.local".into(),
                date: now - Duration::days(1),
                seen: true,
                snippet: "Messages live in a local SQLite store…".into(),
            },
            body_text: "Offline cache\n\n\
After the first sync, envelopes and bodies stay in SQLite under your XDG data \
directory. You can reopen Mailbox with the network off and still read what you \
already fetched.\n"
                .into(),
            body_html: String::new(),
            to: vec!["you@example.com".into()],
            cc: vec![],
        },
        Message {
            envelope: Envelope {
                id: "demo-sent-1".into(),
                account_id: DEMO_ACCOUNT.into(),
                folder_path: "Sent".into(),
                message_id: "<out@mailbox.local>".into(),
                subject: "Re: weekend plans".into(),
                from_name: "You".into(),
                from_addr: "you@example.com".into(),
                date: now - Duration::days(2),
                seen: true,
                snippet: "Sounds good — see you Saturday.".into(),
            },
            body_text: "Sounds good — see you Saturday.\n".into(),
            body_html: String::new(),
            to: vec!["friend@example.com".into()],
            cc: vec![],
        },
    ];

    for msg in messages {
        store.upsert_message(&msg)?;
    }

    Ok(true)
}

pub fn demo_account_id() -> &'static str {
    DEMO_ACCOUNT
}
