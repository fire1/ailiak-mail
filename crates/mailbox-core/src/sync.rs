//! High-level mail sync. GUI calls this — never IMAP/POP3 types directly.

use anyhow::{bail, Context, Result};
use tracing::info;

use crate::config::{MailAccountConfig, MailProtocol};
use crate::imap;
use crate::oauth::MailCredential;
use crate::pop3;
use crate::store::Store;
use crate::types::{parse_imap_uid, Folder, MessageId, SpecialFolderKind};

#[derive(Debug, Clone)]
pub struct SyncReport {
    pub folders: usize,
    pub messages: usize,
    pub folder_path: String,
    /// Human-readable detail (e.g. POP3 server message count).
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Max messages to pull for each folder (newest first).
    pub max_messages: u32,
    /// Folder to fetch bodies for. Defaults to INBOX. Ignored for POP3.
    pub folder_path: String,
    /// When true, download mail for every listed IMAP folder (still capped per folder).
    pub sync_all_folders: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            max_messages: 40,
            folder_path: "INBOX".into(),
            sync_all_folders: false,
        }
    }
}

/// Verify login (IMAP or POP3 depending on account protocol).
/// On success for POP3, `detail` includes the server message count.
pub fn test_connection(account: &MailAccountConfig, credential: &MailCredential) -> Result<String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("start tokio runtime")?;
    match account.protocol {
        MailProtocol::Imap => {
            rt.block_on(imap::test_login(account, credential))?;
            Ok("IMAP login OK".into())
        }
        MailProtocol::Pop3 => match credential {
            MailCredential::Password(password) => {
                let count = rt.block_on(pop3::test_login_with_count(account, password))?;
                if count == 0 {
                    Ok("POP3 login OK — server inbox is empty (0 messages)".into())
                } else {
                    Ok(format!("POP3 login OK — {count} message(s) on server"))
                }
            }
            MailCredential::OAuth2 { .. } => {
                bail!("POP3 does not support OAuth — use an app password")
            }
        },
    }
}

/// List folders and sync one or all mailboxes into the local SQLite cache.
pub fn sync_account(
    store: &Store,
    account: &MailAccountConfig,
    credential: &MailCredential,
    opts: SyncOptions,
) -> Result<SyncReport> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("start tokio runtime")?;

    match account.protocol {
        MailProtocol::Imap => rt.block_on(sync_imap(store, account, credential, opts)),
        MailProtocol::Pop3 => match credential {
            MailCredential::Password(password) => {
                rt.block_on(sync_pop3(store, account, password, opts))
            }
            MailCredential::OAuth2 { .. } => {
                bail!("POP3 does not support OAuth — use IMAP (Gmail) or an app password")
            }
        },
    }
}

async fn sync_imap(
    store: &Store,
    account: &MailAccountConfig,
    credential: &MailCredential,
    opts: SyncOptions,
) -> Result<SyncReport> {
    let mut session = imap::connect_and_login(account, credential).await?;
    let folders = imap::list_folders(&mut session, &account.id).await?;
    info!(count = folders.len(), "listed IMAP folders");

    for folder in &folders {
        // Preserve local hidden flag if the folder already exists.
        let hidden = store
            .list_all_folders(&account.id)
            .ok()
            .and_then(|all| {
                all.into_iter()
                    .find(|f| f.path == folder.path)
                    .map(|f| f.hidden)
            })
            .unwrap_or(false);
        let mut f = folder.clone();
        f.hidden = hidden;
        store.upsert_folder(&f)?;
    }
    let keep: Vec<String> = folders.iter().map(|f| f.path.clone()).collect();
    store.retain_folders(&account.id, &keep)?;

    let targets: Vec<String> = if opts.sync_all_folders {
        folders.iter().map(|f| f.path.clone()).collect()
    } else if folders.iter().any(|f| f.path == opts.folder_path) {
        vec![opts.folder_path.clone()]
    } else {
        vec!["INBOX".into()]
    };

    let mut total_messages = 0usize;
    let mut primary = targets.first().cloned().unwrap_or_else(|| "INBOX".into());
    for path in &targets {
        match imap::sync_folder(&mut session, &account.id, path, opts.max_messages).await {
            Ok((unseen, messages)) => {
                if let Some(folder) = folders.iter().find(|f| &f.path == path) {
                    let mut updated = folder.clone();
                    updated.unread = unseen;
                    let _ = store.upsert_folder(&updated);
                }
                for msg in &messages {
                    store.upsert_message(msg)?;
                }
                total_messages += messages.len();
                if path == &opts.folder_path || targets.len() == 1 {
                    primary = path.clone();
                }
            }
            Err(e) => {
                tracing::warn!(folder = %path, error = %e, "folder sync failed; continuing");
            }
        }
    }

    session.logout().await.ok();

    let detail = if opts.sync_all_folders {
        format!(
            "IMAP: refreshed {} folders, fetched {total_messages} message(s)",
            folders.len()
        )
    } else {
        format!(
            "IMAP: {} folders on server · synced “{primary}”",
            folders.len()
        )
    };

    Ok(SyncReport {
        folders: folders.len(),
        messages: total_messages,
        folder_path: primary,
        detail,
    })
}

/// Move messages to Archive or Trash.
/// IMAP: UID MOVE on the server, then update the local cache.
/// POP3: local folder move only (server has no Archive/Trash folders).
/// Push `\Seen` to the IMAP server for one message (no-op for POP3 / missing UID).
pub fn mark_seen_on_server(
    account: &MailAccountConfig,
    credential: &MailCredential,
    message_id: &MessageId,
    folder_path: &str,
    seen: bool,
) -> Result<()> {
    if account.protocol != MailProtocol::Imap {
        return Ok(());
    }
    let Some(uid) = parse_imap_uid(message_id, &account.id, folder_path) else {
        return Ok(());
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("start tokio runtime")?;
    rt.block_on(async {
        let mut session = imap::connect_and_login(account, credential).await?;
        imap::set_seen(&mut session, folder_path, uid, seen).await?;
        session.logout().await.ok();
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

pub fn move_messages(
    store: &Store,
    account: &MailAccountConfig,
    credential: &MailCredential,
    message_ids: &[MessageId],
    kind: SpecialFolderKind,
) -> Result<usize> {
    if message_ids.is_empty() {
        return Ok(0);
    }

    let mut folders = store.list_all_folders(&account.id)?;
    // POP3 has no server folders beyond Inbox — keep local Archive/Trash mailboxes.
    if account.protocol == MailProtocol::Pop3 {
        ensure_special_folder(store, &account.id, &mut folders, kind)?;
    }
    let dest = crate::types::resolve_special_folder(&folders, kind).ok_or_else(|| {
        anyhow::anyhow!(
            "No {} folder found — Sync all folders (Shift+G) so Archive/Trash are listed",
            kind.label()
        )
    })?;

    // Snapshot envelopes before mutation.
    let mut jobs: Vec<(MessageId, String, Option<u32>)> = Vec::new();
    for id in message_ids {
        let Some(msg) = store.get_message(id)? else {
            continue;
        };
        let from = msg.envelope.folder_path.clone();
        if from == dest {
            continue;
        }
        let uid = parse_imap_uid(id, &account.id, &from);
        jobs.push((id.clone(), from, uid));
    }
    if jobs.is_empty() {
        return Ok(0);
    }

    if account.protocol == MailProtocol::Imap {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("start tokio runtime")?;
        rt.block_on(async {
            let mut session = imap::connect_and_login(account, credential).await?;
            for (id, from, uid) in &jobs {
                if let Some(uid) = uid {
                    if let Err(e) = imap::move_uid(&mut session, from, *uid, &dest).await {
                        tracing::warn!(error = %e, msg = %id, "IMAP move failed; moving locally");
                    }
                }
                store.move_message_local(id, &account.id, from, &dest)?;
            }
            session.logout().await.ok();
            Ok::<(), anyhow::Error>(())
        })?;
    } else {
        for (id, from, _) in &jobs {
            store.move_message_local(id, &account.id, from, &dest)?;
        }
    }

    Ok(jobs.len())
}

fn ensure_special_folder(
    store: &Store,
    account_id: &str,
    folders: &mut Vec<Folder>,
    kind: SpecialFolderKind,
) -> Result<()> {
    if crate::types::resolve_special_folder(folders, kind).is_some() {
        return Ok(());
    }
    // Local-only mailbox when the server never listed one (typical POP3).
    let path = kind.label().to_string();
    let folder = Folder {
        account_id: account_id.to_string(),
        name: path.clone(),
        path: path.clone(),
        unread: 0,
        hidden: false,
    };
    store.upsert_folder(&folder)?;
    folders.push(folder);
    Ok(())
}

async fn sync_pop3(
    store: &Store,
    account: &MailAccountConfig,
    password: &str,
    opts: SyncOptions,
) -> Result<SyncReport> {
    let (folders, unseen, messages, server_count) =
        pop3::sync_inbox(account, password, opts.max_messages).await?;
    info!(
        fetched = messages.len(),
        server_count,
        "POP3 messages fetched"
    );

    for folder in &folders {
        let mut f = folder.clone();
        f.unread = unseen;
        store.upsert_folder(&f)?;
    }
    store.retain_folders(
        &account.id,
        &folders.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
    )?;

    for msg in &messages {
        store.upsert_message(msg)?;
    }

    let pop3_folder_note =
        crate::config::pop3_sync_limitation_note(&account.imap_host, &account.email);
    let detail = if server_count == 0 {
        format!("{pop3_folder_note}; server inbox is empty")
    } else if messages.is_empty() {
        format!(
            "{pop3_folder_note}; server has {server_count} message(s) but none could be downloaded"
        )
    } else {
        format!(
            "{pop3_folder_note}; saved {} of {server_count} from Inbox",
            messages.len()
        )
    };

    Ok(SyncReport {
        folders: folders.len(),
        messages: messages.len(),
        folder_path: "INBOX".into(),
        detail,
    })
}
