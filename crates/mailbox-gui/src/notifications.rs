//! Desktop notifications for newly arrived mail.

use std::sync::mpsc::{self, SyncSender};
use std::sync::OnceLock;
use std::thread;

#[derive(Clone, Debug)]
struct NotifyEvent {
    count: usize,
    from: String,
    subject: String,
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
    let _ = tx.try_send(NotifyEvent {
        count,
        from: from.to_string(),
        subject: subject.to_string(),
    });
}

fn show_desktop(ev: NotifyEvent) -> Result<(), String> {
    let (summary, body) = if ev.count == 1 {
        let from = if ev.from.is_empty() {
            "New message".into()
        } else {
            ev.from
        };
        let subject = if ev.subject.is_empty() {
            "(no subject)".into()
        } else {
            ev.subject
        };
        (from, subject)
    } else {
        (
            format!("{} new messages", ev.count),
            if ev.from.is_empty() {
                String::new()
            } else {
                format!("Latest from {}: {}", ev.from, ev.subject)
            },
        )
    };

    notify_rust::Notification::new()
        .appname("AiliakMail")
        .summary(&summary)
        .body(&body)
        .icon("mail-message-new")
        .timeout(notify_rust::Timeout::Milliseconds(8_000))
        .show()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
