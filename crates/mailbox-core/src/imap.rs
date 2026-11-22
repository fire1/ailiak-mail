//! IMAP client used by sync. UI must not call this directly — go through `sync`.

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use async_imap::Session;
use futures::TryStreamExt;
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::config::MailAccountConfig;
use crate::message_parse::parse_raw_message;
use crate::types::{Folder, Message};

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
}
