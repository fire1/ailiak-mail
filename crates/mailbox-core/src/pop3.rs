//! Minimal POP3 client (RFC 1939) for older mail servers.
//! Supports POP3S (implicit TLS, typically :995) and plain POP3 (:110).
//! Leaves mail on the server (no DELE).

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;
use tracing::info;

use crate::config::MailAccountConfig;
use crate::message_parse::parse_raw_message;
use crate::types::{Folder, Message};

const INBOX: &str = "INBOX";

enum PopStream {
    Plain(BufReader<TcpStream>),
    Tls(BufReader<TlsStream<TcpStream>>),
}

impl PopStream {
    async fn write_all(&mut self, data: &[u8]) -> Result<()> {
        match self {
            Self::Plain(s) => s.get_mut().write_all(data).await?,
            Self::Tls(s) => s.get_mut().write_all(data).await?,
        }
        Ok(())
    }

    async fn read_until_lf(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        buf.clear();
        let n = match self {
            Self::Plain(s) => s.read_until(b'\n', buf).await?,
            Self::Tls(s) => s.read_until(b'\n', buf).await?,
        };
        Ok(n)
    }
}

struct PopSession {
    stream: PopStream,
}

impl PopSession {
    async fn connect(account: &MailAccountConfig) -> Result<Self> {
        let host = account.mail_host();
        let port = account.mail_port();
        let use_tls = account.mail_tls();

        let tcp = TcpStream::connect((host, port))
            .await
            .with_context(|| format!("connect to {host}:{port} (check POP3 host / network)"))?;

        let stream = if use_tls {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let tls_config = ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();
            let connector = TlsConnector::from(Arc::new(tls_config));
            let server_name = ServerName::try_from(host.to_string())
                .map_err(|_| anyhow!("invalid POP3 host name: {host}"))?;
            let tls = connector
                .connect(server_name, tcp)
                .await
                .context("TLS handshake with POP3 server")?;
            PopStream::Tls(BufReader::new(tls))
        } else {
            tracing::warn!(
                host,
                port,
                "POP3 without TLS — credentials travel in clear text"
            );
            PopStream::Plain(BufReader::new(tcp))
        };

        let mut session = Self { stream };
        let greeting = session.read_response_line().await?;
        if !greeting.starts_with("+OK") {
            bail!("POP3 greeting failed: {greeting}");
        }
        Ok(session)
    }

    async fn write_cmd(&mut self, cmd: &str) -> Result<()> {
        let line = format!("{cmd}\r\n");
        self.stream.write_all(line.as_bytes()).await?;
        Ok(())
    }

    async fn read_response_line(&mut self) -> Result<String> {
        let mut buf = Vec::new();
        let n = self.stream.read_until_lf(&mut buf).await?;
        if n == 0 {
            bail!("POP3 connection closed");
        }
        let line = String::from_utf8_lossy(&buf);
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }

    async fn expect_ok(&mut self) -> Result<String> {
        let line = self.read_response_line().await?;
        if line.starts_with("+OK") {
            Ok(line)
        } else {
            bail!("POP3 error: {line}")
        }
    }

    /// Read a multiline response (byte-safe for RFC822 bodies).
    async fn read_multiline(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = Vec::new();
        loop {
            let n = self.stream.read_until_lf(&mut buf).await?;
            if n == 0 {
                bail!("POP3 connection closed while reading multiline");
            }
            // Strip trailing CR LF for terminator check / dot-unstuff.
            let mut line = buf.as_slice();
            if line.ends_with(b"\n") {
                line = &line[..line.len() - 1];
            }
            if line.ends_with(b"\r") {
                line = &line[..line.len() - 1];
            }
            if line == b"." {
                break;
            }
            // Dot-stuffing: leading ".." → "."
            let data = if line.starts_with(b".") && line.len() > 1 {
                &line[1..]
            } else {
                line
            };
            out.extend_from_slice(data);
            out.extend_from_slice(b"\r\n");
        }
        Ok(out)
    }

    async fn login(&mut self, user: &str, password: &str) -> Result<()> {
        self.write_cmd(&format!("USER {user}")).await?;
        self.expect_ok().await.context("POP3 USER")?;
        self.write_cmd(&format!("PASS {password}")).await?;
        self.expect_ok()
            .await
            .context("POP3 PASS (check username/password)")?;
        Ok(())
    }

    async fn stat(&mut self) -> Result<(usize, u64)> {
        self.write_cmd("STAT").await?;
        let line = self.expect_ok().await?;
        let mut parts = line.split_whitespace().skip(1);
        let count: usize = parts
            .next()
            .ok_or_else(|| anyhow!("STAT missing count"))?
            .parse()
            .context("STAT count")?;
        let octets: u64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Ok((count, octets))
    }

    /// Prefer UIDL for stable IDs; `None` if the server does not implement it.
    async fn uidl_all(&mut self) -> Result<Option<Vec<(usize, String)>>> {
        self.write_cmd("UIDL").await?;
        let line = self.read_response_line().await?;
        if !line.starts_with("+OK") {
            return Ok(None);
        }
        let mut list = Vec::new();
        let mut buf = Vec::new();
        loop {
            let n = self.stream.read_until_lf(&mut buf).await?;
            if n == 0 {
                bail!("POP3 closed during UIDL");
            }
            let trimmed = String::from_utf8_lossy(&buf);
            let trimmed = trimmed.trim_end_matches(['\r', '\n']);
            if trimmed == "." {
                break;
            }
            let mut parts = trimmed.split_whitespace();
            let num: usize = parts
                .next()
                .ok_or_else(|| anyhow!("bad UIDL line"))?
                .parse()?;
            let uid = parts.next().unwrap_or("").to_string();
            list.push((num, uid));
        }
        Ok(Some(list))
    }

    async fn retr(&mut self, msg_number: usize) -> Result<Vec<u8>> {
        self.write_cmd(&format!("RETR {msg_number}")).await?;
        self.expect_ok()
            .await
            .with_context(|| format!("RETR {msg_number}"))?;
        self.read_multiline().await
    }

    async fn quit(mut self) -> Result<()> {
        self.write_cmd("QUIT").await?;
        let _ = self.expect_ok().await;
        Ok(())
    }
}

pub async fn test_login(account: &MailAccountConfig, password: &str) -> Result<()> {
    if password.trim().is_empty() {
        bail!("mail password is empty — add it in Settings");
    }
    let mut session = PopSession::connect(account).await?;
    session
        .login(&account.username, password)
        .await
        .context("POP3 login")?;
    let (count, _) = session.stat().await?;
    info!(count, "POP3 STAT ok");
    session.quit().await.ok();
    Ok(())
}

/// POP3 has a single mailbox — mapped to local folder `INBOX`.
pub async fn sync_inbox(
    account: &MailAccountConfig,
    password: &str,
    max_messages: u32,
) -> Result<(Vec<Folder>, u32, Vec<Message>)> {
    if password.trim().is_empty() {
        bail!("mail password is empty — add it in Settings");
    }

    let mut session = PopSession::connect(account).await?;
    session
        .login(&account.username, password)
        .await
        .context("POP3 login")?;

    let (count, _) = session.stat().await?;
    info!(count, "POP3 messages on server");

    let folders = vec![Folder {
        account_id: account.id.clone(),
        name: "Inbox".into(),
        path: INBOX.into(),
        unread: 0,
    }];

    if count == 0 {
        session.quit().await.ok();
        return Ok((folders, 0, Vec::new()));
    }

    let uidl = session.uidl_all().await?;
    let start = count.saturating_sub(max_messages as usize).saturating_add(1).max(1);
    let end = count;

    let mut messages = Vec::new();
    for msg_num in start..=end {
        let raw = match session.retr(msg_num).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(msg_num, error = %e, "POP3 RETR failed; skipping");
                continue;
            }
        };

        let uid = if let Some(ref list) = uidl {
            list.iter()
                .find(|(n, _)| *n == msg_num)
                .map(|(_, u)| stable_uid(u))
                .unwrap_or(msg_num as u32)
        } else {
            msg_num as u32
        };

        // POP3 has no \Seen flag — treat newly fetched as unread.
        if let Some(msg) = parse_raw_message(&account.id, INBOX, uid, false, &raw) {
            messages.push(msg);
        }
    }

    messages.sort_by(|a, b| b.envelope.date.cmp(&a.envelope.date));

    let unread = messages.iter().filter(|m| !m.envelope.seen).count() as u32;
    session.quit().await.ok();

    Ok((folders, unread, messages))
}

fn stable_uid(uidl: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in uidl.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(16777619);
    }
    if h == 0 {
        1
    } else {
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_hash_stable() {
        assert_eq!(stable_uid("abc"), stable_uid("abc"));
        assert_ne!(stable_uid("abc"), stable_uid("abd"));
    }
}
