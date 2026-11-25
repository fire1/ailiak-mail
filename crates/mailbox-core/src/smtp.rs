//! SMTP submission (lettre) for compose / reply.

use anyhow::{anyhow, bail, Context, Result};
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MessageBuilder, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{SmtpTransport, Transport};

use crate::config::MailAccountConfig;
use crate::message_parse::html_to_plain;
use crate::oauth::MailCredential;

/// File to attach on send (already loaded into memory by the UI).
#[derive(Debug, Clone)]
pub struct OutgoingAttachment {
    pub filename: String,
    pub bytes: Vec<u8>,
    pub mime: String,
}

/// Outgoing mail draft built by the GUI.
#[derive(Debug, Clone)]
pub struct OutgoingMail {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    /// Optional HTML body. When set, the message is sent as
    /// `multipart/alternative` (plain + HTML).
    pub body_html: Option<String>,
    /// Message-ID of the message being replied to (bare or angle-bracketed).
    pub in_reply_to: Option<String>,
    /// Full References chain for the reply.
    pub references: Option<String>,
    pub attachments: Vec<OutgoingAttachment>,
}

fn parse_mailbox(raw: &str) -> Result<Mailbox> {
    let s = raw.trim();
    if s.is_empty() {
        bail!("empty address");
    }
    s.parse::<Mailbox>()
        .or_else(|_| format!("<{s}>").parse::<Mailbox>())
        .with_context(|| format!("invalid address: {raw}"))
}

fn normalize_msg_id(id: &str) -> String {
    let t = id.trim();
    if t.starts_with('<') && t.ends_with('>') {
        t.to_string()
    } else if t.is_empty() {
        t.to_string()
    } else {
        format!("<{t}>")
    }
}

/// Ensure subject has a single `Re:` prefix for replies.
pub fn reply_subject(original: &str) -> String {
    let t = original.trim();
    if t.is_empty() {
        return "Re: ".into();
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("re:") || lower.starts_with("re :") {
        t.to_string()
    } else {
        format!("Re: {t}")
    }
}

/// Build References for a reply: prior References + prior Message-ID.
pub fn reply_references(prior_references: &str, prior_message_id: &str) -> String {
    let mut parts: Vec<String> = prior_references
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .map(normalize_msg_id)
        .collect();
    let mid = normalize_msg_id(prior_message_id);
    if !mid.is_empty() && !parts.iter().any(|p| p == &mid) {
        parts.push(mid);
    }
    parts.join(" ")
}

fn content_type_for(mime: &str) -> ContentType {
    ContentType::parse(mime).unwrap_or(ContentType::TEXT_PLAIN)
}

/// Send `mail` via the account’s SMTP settings.
///
/// Call from a worker thread (may block on network / TLS).
pub fn send(
    account: &MailAccountConfig,
    credential: &MailCredential,
    mail: &OutgoingMail,
) -> Result<()> {
    if mail.to.is_empty() {
        bail!("Add at least one To recipient");
    }
    if account.smtp_host.trim().is_empty() {
        bail!("SMTP host is not configured for this account");
    }

    let from: Mailbox = format!("{} <{}>", account.name, account.email)
        .parse()
        .or_else(|_| account.email.parse::<Mailbox>())
        .context("account From address")?;

    let mut builder = MessageBuilder::new()
        .from(from)
        .subject(mail.subject.clone());

    for addr in &mail.to {
        builder = builder.to(parse_mailbox(addr)?);
    }
    for addr in &mail.cc {
        if addr.trim().is_empty() {
            continue;
        }
        builder = builder.cc(parse_mailbox(addr)?);
    }
    for addr in &mail.bcc {
        if addr.trim().is_empty() {
            continue;
        }
        builder = builder.bcc(parse_mailbox(addr)?);
    }

    if let Some(ref irt) = mail.in_reply_to {
        let id = normalize_msg_id(irt);
        if !id.is_empty() {
            builder = builder.in_reply_to(id);
        }
    }
    if let Some(ref refs) = mail.references {
        let r = refs
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(normalize_msg_id)
            .collect::<Vec<_>>()
            .join(" ");
        if !r.is_empty() {
            builder = builder.references(r);
        }
    }

    // Always include a usable plain part derived from HTML when needed.
    // Never ship the reader placeholder string as message content.
    let placeholder = crate::message_parse::missing_plain_placeholder();
    let mut plain = mail.body_text.clone();
    if plain.trim() == placeholder {
        plain.clear();
    }
    if plain.trim().is_empty() {
        if let Some(html) = mail.body_html.as_ref() {
            plain = html_to_plain(html);
        }
    }

    let text = SinglePart::builder()
        .header(ContentType::TEXT_PLAIN)
        .body(plain);

    let html = mail
        .body_html
        .as_ref()
        .map(|h| h.trim())
        .filter(|h| !h.is_empty())
        .map(|h| {
            SinglePart::builder()
                .header(ContentType::TEXT_HTML)
                .body(h.to_string())
        });

    let msg = match (html, mail.attachments.is_empty()) {
        (None, true) => builder.singlepart(text).context("build MIME message")?,
        (None, false) => {
            let mut mixed = MultiPart::mixed().singlepart(text);
            for att in &mail.attachments {
                let ct = content_type_for(&att.mime);
                mixed = mixed.singlepart(
                    Attachment::new(att.filename.clone())
                        .body(att.bytes.clone(), ct),
                );
            }
            builder.multipart(mixed).context("build MIME message")?
        }
        (Some(html_part), true) => {
            let alt = MultiPart::alternative()
                .singlepart(text)
                .singlepart(html_part);
            builder.multipart(alt).context("build MIME message")?
        }
        (Some(html_part), false) => {
            let alt = MultiPart::alternative()
                .singlepart(text)
                .singlepart(html_part);
            let mut mixed = MultiPart::mixed().multipart(alt);
            for att in &mail.attachments {
                let ct = content_type_for(&att.mime);
                mixed = mixed.singlepart(
                    Attachment::new(att.filename.clone())
                        .body(att.bytes.clone(), ct),
                );
            }
            builder.multipart(mixed).context("build MIME message")?
        }
    };

    let creds = match credential {
        MailCredential::Password(pass) => Credentials::new(account.username.clone(), pass.clone()),
        MailCredential::OAuth2 { access_token } => {
            Credentials::new(account.username.clone(), access_token.clone())
        }
    };

    // Port 465 = implicit TLS (`relay`). Port 587 (and most submission) = STARTTLS.
    // Using `relay` on 587 causes: "received corrupt message of type InvalidContentType".
    let mut transport_builder = if !account.smtp_tls {
        SmtpTransport::builder_dangerous(&account.smtp_host)
    } else if account.smtp_port == 465 {
        SmtpTransport::relay(&account.smtp_host)
            .with_context(|| format!("SMTP TLS relay {}:465", account.smtp_host))?
    } else {
        SmtpTransport::starttls_relay(&account.smtp_host)
            .with_context(|| {
                format!(
                    "SMTP STARTTLS {}:{}",
                    account.smtp_host, account.smtp_port
                )
            })?
    };
    transport_builder = transport_builder
        .port(account.smtp_port)
        .credentials(creds);

    if matches!(credential, MailCredential::OAuth2 { .. }) {
        transport_builder = transport_builder.authentication(vec![Mechanism::Xoauth2]);
    }

    let mailer = transport_builder.build();
    mailer.send(&msg).map_err(|e| {
        let hint = if account.smtp_tls && account.smtp_port == 587 {
            ""
        } else if account.smtp_tls && account.smtp_port != 465 {
            " (try port 587 with STARTTLS, or 465 with SSL)"
        } else {
            ""
        };
        anyhow!("SMTP send failed: {e}{hint}")
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_subject_prefixes_once() {
        assert_eq!(reply_subject("Hello"), "Re: Hello");
        assert_eq!(reply_subject("Re: Hello"), "Re: Hello");
    }

    #[test]
    fn references_append_message_id() {
        let r = reply_references("<a@x>", "b@y");
        assert!(r.contains("<a@x>"));
        assert!(r.contains("<b@y>"));
    }
}
