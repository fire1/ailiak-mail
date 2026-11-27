//! Desktop notifications for new mail and opened-reader cues.

use std::sync::mpsc::{self, SyncSender};
use std::sync::OnceLock;
use std::thread;

#[derive(Clone, Debug)]
enum NotifyEvent {
    NewMail {
        count: usize,
        from: String,
        subject: String,
    },
    /// Message opened in the reader — OS history doubles as a follow-up trail for Desk.
    OpenedMail {
        from: String,
        subject: String,
        /// Optional Desk / Options line (e.g. "On Desk · …" or "Press a for Options").
        hint: Option<String>,
    },
}

static TX: OnceLock<SyncSender<NotifyEvent>> = OnceLock::new();

fn sender() -> Option<&'static SyncSender<NotifyEvent>> {
    Some(TX.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<NotifyEvent>(8);
        thread::Builder::new()
            .name("ailiak-notify".into())
            .spawn(move || {
                while let Ok(ev) = rx.recv() {
                    if let Err(e) = show_desktop(ev) {
                        tracing::debug!(error = %e, "desktop notification failed");
                    }
                }
            })
            .ok();
        tx
    }))
}

/// Fire a system notification for one or more newly cached messages.
pub fn notify_new_mail(count: usize, from: &str, subject: &str) {
    if count == 0 {
        return;
    }
    let Some(tx) = sender() else {
        return;
    };
    let _ = tx.try_send(NotifyEvent::NewMail {
        count,
        from: from.to_string(),
        subject: subject.to_string(),
    });
}

/// Fire when a message is opened in the reader (Desk / Todo follow-up cue).
pub fn notify_opened_mail(from: &str, subject: &str, hint: Option<&str>) {
    let Some(tx) = sender() else {
        return;
    };
    let _ = tx.try_send(NotifyEvent::OpenedMail {
        from: from.to_string(),
        subject: subject.to_string(),
        hint: hint.map(str::to_string),
    });
}

fn show_desktop(ev: NotifyEvent) -> Result<(), String> {
    let (summary, body, icon) = match ev {
        NotifyEvent::NewMail {
            count,
            from,
            subject,
        } => {
            if count == 1 {
                let from = if from.is_empty() {
                    "New message".into()
                } else {
                    from
                };
                let subject = if subject.is_empty() {
                    "(no subject)".into()
                } else {
                    subject
                };
                (from, subject, "mail-message-new")
            } else {
                (
                    format!("{count} new messages"),
                    if from.is_empty() {
                        String::new()
                    } else {
                        format!("Latest from {from}: {subject}")
                    },
                    "mail-message-new",
                )
            }
        }
        NotifyEvent::OpenedMail {
            from,
            subject,
            hint,
        } => {
            let summary = if from.is_empty() {
                "Opened mail".into()
            } else {
                from
            };
            let subject = if subject.is_empty() {
                "(no subject)".into()
            } else {
                subject
            };
            let body = match hint {
                Some(h) if !h.is_empty() => format!("{subject}\n{h}"),
                _ => subject,
            };
            (summary, body, "mail-read")
        }
    };

    notify_rust::Notification::new()
        .appname("AiliakMail")
        .summary(&summary)
        .body(&body)
        .icon(icon)
        .timeout(notify_rust::Timeout::Milliseconds(8_000))
        .show()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
