//! High-level mail sync. GUI calls this — never IMAP types directly.

use anyhow::{Context, Result};
use tracing::info;

use crate::config::MailAccountConfig;
use crate::imap;
use crate::store::Store;

#[derive(Debug, Clone)]
pub struct SyncReport {
    pub folders: usize,
    pub messages: usize,
    pub folder_path: String,
}

#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Max messages to pull for the target folder (newest first).
    pub max_messages: u32,
    /// Folder to fetch bodies for. Defaults to INBOX.
    pub folder_path: String,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            max_messages: 40,
            folder_path: "INBOX".into(),
        }
    }
}

/// Verify IMAP login (Gmail: use an App Password, not your normal password).
pub fn test_connection(account: &MailAccountConfig, password: &str) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("start tokio runtime")?;
    rt.block_on(imap::test_login(account, password))
}

/// List folders and sync one mailbox into the local SQLite cache.
pub fn sync_account(
    store: &Store,
    account: &MailAccountConfig,
    password: &str,
    opts: SyncOptions,
) -> Result<SyncReport> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("start tokio runtime")?;

    rt.block_on(async {
        let mut session = imap::connect_and_login(account, password).await?;
        let folders = imap::list_folders(&mut session, &account.id).await?;
        info!(count = folders.len(), "listed IMAP folders");

        for folder in &folders {
            store.upsert_folder(folder)?;
        }

        let folder_path = if folders.iter().any(|f| f.path == opts.folder_path) {
            opts.folder_path.clone()
        } else {
            "INBOX".to_string()
        };

        let (unseen, messages) =
            imap::sync_folder(&mut session, &account.id, &folder_path, opts.max_messages).await?;

        if let Some(folder) = folders.iter().find(|f| f.path == folder_path) {
            let mut updated = folder.clone();
            updated.unread = unseen;
            store.upsert_folder(&updated)?;
        }

        for msg in &messages {
            store.upsert_message(msg)?;
        }

        session.logout().await.ok();

        Ok(SyncReport {
            folders: folders.len(),
            messages: messages.len(),
            folder_path,
        })
    })
}
