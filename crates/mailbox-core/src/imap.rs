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
use crate::oauth::{MailCredential, XoAuth2};
use crate::types::{Folder, Message};

type ImapSession = Session<tokio_rustls::client::TlsStream<TcpStream>>;

pub async fn connect_and_login(
    account: &MailAccountConfig,
    credential: &MailCredential,
) -> Result<ImapSession> {
    if !account.imap_tls {
        bail!("IMAP TLS is required (refusing insecure connection)");
    }
    match credential {
        MailCredential::Password(p) if p.trim().is_empty() => {
            bail!("mail password is empty — Sign in with Google or add an app password");
        }
        MailCredential::OAuth2 { access_token } if access_token.trim().is_empty() => {
            bail!("OAuth token empty — Sign in with Google again");
        }
        _ => {}
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

    let user = if account.username.is_empty() {
        account.email.as_str()
    } else {
        account.username.as_str()
    };

    let session = match credential {
        MailCredential::Password(password) => client
            .login(user, password)
            .await
            .map_err(|(e, _)| anyhow!("IMAP login failed: {e}"))?,
        MailCredential::OAuth2 { access_token } => {
            let auth = XoAuth2 {
                user: user.to_string(),
                access_token: access_token.clone(),
            };
            client
                .authenticate("XOAUTH2", auth)
                .await
                .map_err(|(e, _)| anyhow!("IMAP XOAUTH2 failed: {e}"))?
        }
    };

    Ok(session)
}

pub async fn test_login(account: &MailAccountConfig, credential: &MailCredential) -> Result<()> {
    let mut session = connect_and_login(account, credential).await?;
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
            hidden: false,
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
                hidden: false,
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

/// Move one message by UID into `dest_folder` (UID MOVE, or COPY + delete fallback).
pub async fn move_uid(
    session: &mut ImapSession,
    from_folder: &str,
    uid: u32,
    dest_folder: &str,
) -> Result<()> {
    session
        .select(from_folder)
        .await
        .with_context(|| format!("SELECT {from_folder}"))?;

    let uid_s = uid.to_string();
    match session.uid_mv(&uid_s, dest_folder).await {
        Ok(()) => Ok(()),
        Err(move_err) => {
            tracing::debug!(error = %move_err, "UID MOVE failed; trying COPY+delete");
            session
                .uid_copy(&uid_s, dest_folder)
                .await
                .with_context(|| format!("UID COPY → {dest_folder}"))?;
            let _ = session
                .uid_store(&uid_s, "+FLAGS (\\Deleted)")
                .await
                .context("UID STORE \\Deleted")?
                .try_collect::<Vec<_>>()
                .await;
            let _ = session
                .expunge()
                .await
                .context("EXPUNGE")?
                .try_collect::<Vec<_>>()
                .await;
            Ok(())
        }
    }
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
    let leaf = path
        .rsplit(['/', '.'])
        .next()
        .unwrap_or(path)
        .trim_start_matches('[')
        .trim_end_matches(']')
        .replace("Gmail/", "");
    // IMAP mailbox names use Modified UTF-7 (RFC 3501) for non-ASCII.
    decode_modified_utf7(&leaf)
}

/// Decode IMAP Modified UTF-7 to Unicode. Leaves ASCII paths unchanged.
/// `&…-` shifts are Base64(UTF-16BE) with `,` instead of `/`; `&-` is a literal `&`.
fn decode_modified_utf7(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            out.push('&');
            i += 2;
            continue;
        }
        let start = i + 1;
        let Some(rel) = bytes[start..].iter().position(|&b| b == b'-') else {
            out.push_str(&input[i..]);
            break;
        };
        let end = start + rel;
        match decode_mutf7_shift(&input[start..end]) {
            Some(decoded) => out.push_str(&decoded),
            None => out.push_str(&input[i..=end]),
        }
        i = end + 1;
    }
    out
}

fn decode_mutf7_shift(encoded: &str) -> Option<String> {
    if encoded.is_empty() {
        return Some(String::new());
    }
    if !encoded
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b',')
    {
        return None;
    }
    let usable = encoded.len() * 6 / 8;
    let mut b64: String = encoded
        .chars()
        .map(|c| if c == ',' { '/' } else { c })
        .collect();
    let pad = (4 - (b64.len() % 4)) % 4;
    b64.extend(std::iter::repeat('=').take(pad));
    let raw = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).ok()?;
    let raw = &raw[..usable.min(raw.len())];
    let even = raw.len() - (raw.len() % 2);
    decode_utf16be(&raw[..even])
}

fn decode_utf16be(raw: &[u8]) -> Option<String> {
    let mut out = String::new();
    let mut i = 0;
    while i + 1 < raw.len() {
        let unit = u16::from_be_bytes([raw[i], raw[i + 1]]);
        i += 2;
        if (0xD800..=0xDBFF).contains(&unit) {
            if i + 1 >= raw.len() {
                out.push(char::REPLACEMENT_CHARACTER);
                break;
            }
            let low = u16::from_be_bytes([raw[i], raw[i + 1]]);
            i += 2;
            if (0xDC00..=0xDFFF).contains(&low) {
                let cp =
                    0x10000u32 + (((unit as u32 - 0xD800) << 10) | (low as u32 - 0xDC00));
                out.push(char::from_u32(cp).unwrap_or(char::REPLACEMENT_CHARACTER));
            } else {
                out.push(char::REPLACEMENT_CHARACTER);
            }
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            out.push(char::REPLACEMENT_CHARACTER);
        } else {
            out.push(char::from_u32(unit as u32).unwrap_or(char::REPLACEMENT_CHARACTER));
        }
    }
    Some(out)
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
    fn modified_utf7_cyrillic_folders() {
        assert_eq!(decode_modified_utf7("Работа"), "Работа");
        assert_eq!(decode_modified_utf7("&BCAEMAQxBD4EQgQw-"), "Работа");
        assert_eq!(decode_modified_utf7("&-"), "&");
        assert_eq!(
            friendly_folder_name("[Gmail]/&BCAEMAQxBD4EQgQw-"),
            "Работа"
        );
        // Wire form from Gmail LIST (UTF-16BE shift); path stays encoded for SELECT.
        let friends = decode_modified_utf7("&BB8EQAQ4BE8EQgQ1BDsE0A-");
        assert!(friends.starts_with("Приятел"), "{friends}");
        let saved = decode_modified_utf7("&BBcEMAQ,BDAENwQ1BD0E0A-");
        assert!(saved.starts_with("Запазен"), "{saved}");
    }
}
