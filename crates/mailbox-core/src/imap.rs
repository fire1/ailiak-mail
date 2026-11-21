//! IMAP client used by sync. UI must not call this directly — go through `sync`.

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use async_imap::Session;
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use mail_parser::MessageParser;
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::config::MailAccountConfig;
use crate::types::{Envelope, Folder, Message};

type ImapSession = Session<tokio_rustls::client::TlsStream<TcpStream>>;

pub async fn connect_and_login(account: &MailAccountConfig, password: &str) -> Result<ImapSession> {
    if !account.imap_tls {
        bail!("IMAP TLS is required (refusing insecure connection)");
    }
    if password.trim().is_empty() {
        bail!("mail password is empty — add an app password in Settings");
    }

    let tcp = TcpStream::connect((account.imap_host.as_str(), account.imap_port))
        .await
        .with_context(|| {
            format!(
                "connect to {}:{} (check network / IMAP host)",
                account.imap_host, account.imap_port
            )
        })?;

    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls_config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(tls_config));
    let server_name = ServerName::try_from(account.imap_host.clone())
        .map_err(|_| anyhow!("invalid IMAP host name: {}", account.imap_host))?;
    let tls = connector
        .connect(server_name, tcp)
        .await
        .context("TLS handshake with IMAP server")?;

    let mut client = async_imap::Client::new(tls);
    // Gmail (and IMAP in general) send a greeting that must be consumed first.
    let _greeting = client.read_response().await.context("read IMAP greeting")?;

    let session = client
        .login(&account.username, password)
        .await
        .map_err(|(e, _)| anyhow!("IMAP login failed: {e}"))?;

    Ok(session)
}

pub async fn test_login(account: &MailAccountConfig, password: &str) -> Result<()> {
    let mut session = connect_and_login(account, password).await?;
    let _ = session.capabilities().await.context("IMAP CAPABILITY")?;
    session.logout().await.ok();
    Ok(())
}

pub async fn list_folders(session: &mut ImapSession, account_id: &str) -> Result<Vec<Folder>> {
    let list = session
        .list(Some(""), Some("*"))
        .await
        .context("IMAP LIST")?
        .try_collect::<Vec<_>>()
        .await
        .context("collect LIST")?;

    let mut folders = Vec::new();
    for name in list {
        let path = name.name().to_string();
        if path.is_empty() {
            continue;
        }
        // Skip Noselect mailboxes (e.g. "[Gmail]" parent)
        if name.attributes().iter().any(|a| match a {
            async_imap::types::NameAttribute::NoSelect => true,
            async_imap::types::NameAttribute::Extension(ext) => {
                ext.eq_ignore_ascii_case("\\NonExistent")
            }
            _ => false,
        }) {
            continue;
        }
        let display = friendly_folder_name(&path);
        folders.push(Folder {
            account_id: account_id.to_string(),
            name: display,
            path,
            unread: 0,
        });
    }

    if !folders.iter().any(|f| f.path.eq_ignore_ascii_case("INBOX")) {
        folders.insert(
            0,
            Folder {
                account_id: account_id.to_string(),
                name: "Inbox".into(),
                path: "INBOX".into(),
                unread: 0,
            },
        );
    }

    crate::types::sort_folders(&mut folders);
    Ok(folders)
}

pub async fn sync_folder(
    session: &mut ImapSession,
    account_id: &str,
    folder_path: &str,
    max_messages: u32,
) -> Result<(u32, Vec<Message>)> {
    let mailbox = session
        .select(folder_path)
        .await
        .with_context(|| format!("SELECT {folder_path}"))?;

    let exists = mailbox.exists;
    let unseen = mailbox.unseen.unwrap_or(0);

    if exists == 0 {
        return Ok((unseen, Vec::new()));
    }

    let start = exists.saturating_sub(max_messages.saturating_sub(1).min(exists));
    let end = exists;
    let sequence = format!("{start}:{end}");

    let fetches = session
        .fetch(&sequence, "(UID FLAGS BODY.PEEK[])")
        .await
        .context("IMAP FETCH")?
        .try_collect::<Vec<_>>()
        .await
        .context("collect FETCH")?;

    let mut messages = Vec::with_capacity(fetches.len());
    for fetch in fetches {
        let uid = match fetch.uid {
            Some(u) => u,
            None => continue,
        };
        let seen = fetch
            .flags()
            .any(|f| matches!(f, async_imap::types::Flag::Seen));
        let raw = match fetch.body() {
            Some(b) if !b.is_empty() => b,
            _ => continue,
        };

        if let Some(msg) = parse_raw_message(account_id, folder_path, uid, seen, raw) {
            messages.push(msg);
        }
    }

    Ok((unseen, messages))
}

pub async fn fetch_uid_body(
    session: &mut ImapSession,
    folder_path: &str,
    uid: u32,
) -> Result<Option<Vec<u8>>> {
    session
        .select(folder_path)
        .await
        .with_context(|| format!("SELECT {folder_path}"))?;

    let fetches = session
        .uid_fetch(uid.to_string(), "BODY.PEEK[]")
        .await
        .context("UID FETCH")?
        .try_collect::<Vec<_>>()
        .await
        .context("collect UID FETCH")?;

    Ok(fetches
        .into_iter()
        .find_map(|f| f.body().map(|b| b.to_vec())))
}

fn parse_raw_message(
    account_id: &str,
    folder_path: &str,
    uid: u32,
    seen: bool,
    raw: &[u8],
) -> Option<Message> {
    let parsed = MessageParser::default().parse(raw)?;

    let message_id = parsed
        .message_id()
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("uid-{uid}@{folder_path}"));

    let subject = parsed.subject().unwrap_or("(no subject)").to_string();

    let (from_name, from_addr) = match parsed.from().and_then(|a| a.first()) {
        Some(addr) => (
            addr.name().unwrap_or("").to_string(),
            addr.address().unwrap_or("").to_string(),
        ),
        None => (String::new(), String::new()),
    };

    let date = parsed
        .date()
        .map(|d| DateTime::from_timestamp(d.to_timestamp(), 0).unwrap_or_else(Utc::now))
        .unwrap_or_else(Utc::now);

    let to = addresses(parsed.to());
    let cc = addresses(parsed.cc());

    let mut body_text = extract_text(&parsed);
    let snippet = snippet_from(&body_text);

    // Cap stored body so a huge mail doesn't blow the cache.
    if body_text.len() > 512 * 1024 {
        body_text.truncate(512 * 1024);
        body_text.push_str("\n…[truncated]");
    }

    Some(Message {
        envelope: Envelope {
            id: format!("{account_id}:{folder_path}:{uid}"),
            account_id: account_id.to_string(),
            folder_path: folder_path.to_string(),
            message_id,
            subject,
            from_name,
            from_addr,
            date,
            seen,
            snippet,
        },
        body_text,
        to,
        cc,
    })
}

fn addresses(list: Option<&mail_parser::Address<'_>>) -> Vec<String> {
    let Some(list) = list else {
        return Vec::new();
    };
    list.iter()
        .map(|a| {
            let email = a.address().unwrap_or("");
            match a.name() {
                Some(name) if !name.is_empty() => format!("{name} <{email}>"),
                _ => email.to_string(),
            }
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn extract_text(parsed: &mail_parser::Message<'_>) -> String {
    if let Some(text) = parsed.body_text(0) {
        return text.to_string();
    }
    if let Some(html) = parsed.body_html(0) {
        return html_to_text(&html);
    }
    String::from("(no plain-text body)")
}

fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // Collapse whitespace a bit
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        "(HTML message — plain text unavailable)".into()
    } else {
        collapsed
    }
}

fn snippet_from(body: &str) -> String {
    let one_line: String = body
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    one_line.chars().take(140).collect()
}

fn friendly_folder_name(path: &str) -> String {
    if path.eq_ignore_ascii_case("INBOX") {
        return "Inbox".into();
    }
    path.rsplit(['/', '.'])
        .next()
        .unwrap_or(path)
        .trim_start_matches('[')
        .trim_end_matches(']')
        .replace("Gmail]/", "")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_folder_names() {
        assert_eq!(friendly_folder_name("INBOX"), "Inbox");
        assert_eq!(friendly_folder_name("[Gmail]/Sent Mail"), "Sent Mail");
    }

    #[test]
    fn strips_html() {
        let t = html_to_text("<p>Hello <b>world</b></p>");
        assert!(t.contains("Hello"));
        assert!(t.contains("world"));
    }
}
