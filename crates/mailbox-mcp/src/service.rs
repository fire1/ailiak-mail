//! Read-only (plus mark-seen) access to the local Mailbox SQLite cache.

use anyhow::{anyhow, Context, Result};
use mailbox_core::config::AppConfig;
use mailbox_core::paths::AppPaths;
use mailbox_core::store::Store;
use mailbox_core::sync::{sync_account, SyncOptions};
use mailbox_core::types::{Envelope, Folder, Message};
use mailbox_core::{secrets, MailAccountConfig, MailCredential};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct MailboxService {
    paths: AppPaths,
}

#[derive(Debug, Serialize)]
pub struct AccountSummary {
    pub id: String,
    pub name: String,
    pub email: String,
    pub protocol: String,
}

impl MailboxService {
    pub fn from_paths(paths: AppPaths) -> Result<Self> {
        paths.ensure()?;
        Ok(Self { paths })
    }

    pub fn open_default() -> Result<Self> {
        Self::from_paths(AppPaths::resolve()?)
    }

    fn load_config(&self) -> Result<AppConfig> {
        AppConfig::load_or_init(&self.paths)
    }

    fn open_store(&self) -> Result<Store> {
        Store::open(&self.paths.db_file())
    }

    pub fn list_accounts(&self) -> Result<Vec<AccountSummary>> {
        let cfg = self.load_config()?;
        Ok(cfg
            .accounts
            .into_iter()
            .map(|a| AccountSummary {
                id: a.id,
                name: a.name,
                email: a.email,
                protocol: a.protocol.as_str().to_string(),
            })
            .collect())
    }

    fn resolve_account_id(&self, account_id: Option<&str>) -> Result<String> {
        let cfg = self.load_config()?;
        if let Some(id) = account_id.filter(|s| !s.is_empty()) {
            if cfg.account_by_id(id).is_some() {
                return Ok(id.to_string());
            }
            // Also allow store-only ids (demo / orphaned).
            return Ok(id.to_string());
        }
        if let Some(a) = cfg.primary_account() {
            return Ok(a.id.clone());
        }
        let store = self.open_store()?;
        store
            .account_ids_with_mail()?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no accounts configured and no mail in the local store"))
    }

    pub fn list_folders(&self, account_id: Option<&str>) -> Result<Vec<Folder>> {
        let id = self.resolve_account_id(account_id)?;
        self.open_store()?.list_folders(&id)
    }

    pub fn list_messages(
        &self,
        account_id: Option<&str>,
        folder: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Envelope>> {
        let id = self.resolve_account_id(account_id)?;
        let folder = folder.unwrap_or("INBOX");
        let mut envs = self.open_store()?.list_envelopes(&id, folder)?;
        if let Some(n) = limit {
            envs.truncate(n);
        }
        Ok(envs)
    }

    pub fn search_messages(
        &self,
        account_id: Option<&str>,
        query: &str,
    ) -> Result<Vec<Envelope>> {
        let id = self.resolve_account_id(account_id)?;
        if query.trim().is_empty() {
            return Err(anyhow!("query must not be empty"));
        }
        self.open_store()?.search_local(&id, query.trim())
    }

    pub fn get_message(&self, id: &str) -> Result<Message> {
        let id = id.to_string();
        self.open_store()?
            .get_message(&id)?
            .ok_or_else(|| anyhow!("message not found: {id}"))
    }

    pub fn mark_seen(&self, id: &str, seen: bool) -> Result<()> {
        let id = id.to_string();
        let store = self.open_store()?;
        if store.get_message(&id)?.is_none() {
            return Err(anyhow!("message not found: {id}"));
        }
        store.mark_seen(&id, seen)?;
        Ok(())
    }

    pub fn sync_folder(
        &self,
        account_id: Option<&str>,
        folder: Option<&str>,
        max_messages: Option<u32>,
    ) -> Result<Value> {
        let cfg = self.load_config()?;
        let id = self.resolve_account_id(account_id)?;
        let account: MailAccountConfig = cfg
            .account_by_id(&id)
            .cloned()
            .ok_or_else(|| anyhow!("account {id} not in config.toml — add it in AiliakMail Settings"))?;
        let credential = MailCredential::Password(
            secrets::get_mail_password(&account.id)?
                .ok_or_else(|| anyhow!("no password in OS keyring for account {}", account.id))?,
        );
        let store = Store::open_for_worker(&self.paths.db_file())?;
        let report = sync_account(
            &store,
            &account,
            &credential,
            SyncOptions {
                max_messages: max_messages.unwrap_or(40),
                folder_path: folder.unwrap_or("INBOX").to_string(),
                sync_all_folders: false,
            },
        )
        .with_context(|| format!("sync {}", account.id))?;
        Ok(json!({
            "account_id": account.id,
            "folder_path": report.folder_path,
            "folders": report.folders,
            "messages": report.messages,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use mailbox_core::types::{Envelope, Message};

    fn sample_msg(account: &str, folder: &str, id_suffix: &str) -> Message {
        Message {
            envelope: Envelope {
                id: format!("{account}:{folder}:{id_suffix}"),
                account_id: account.into(),
                folder_path: folder.into(),
                message_id: format!("<{id_suffix}@test>"),
                subject: format!("Subject {id_suffix}"),
                from_name: "Ada".into(),
                from_addr: "ada@example.com".into(),
                date: Utc::now(),
                seen: false,
                snippet: "hello".into(),
                in_reply_to: String::new(),
                references: String::new(),
                thread_id: String::new(),
            },
            body_text: "body".into(),
            body_html: String::new(),
            to: vec!["you@example.com".into()],
            cc: vec![],
        }
    }

    fn test_service() -> (tempfile::TempDir, MailboxService) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            config_dir: dir.path().join("config"),
            data_dir: dir.path().join("data"),
        };
        paths.ensure().unwrap();
        let mut cfg = AppConfig::default();
        cfg.upsert_account(MailAccountConfig {
            id: "demo".into(),
            name: "Demo".into(),
            email: "demo@example.com".into(),
            protocol: mailbox_core::MailProtocol::Imap,
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            imap_tls: true,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            smtp_tls: true,
            username: "demo@example.com".into(),
            auth: mailbox_core::MailAuthMethod::Password,
        });
        cfg.save(&paths.config_file()).unwrap();
        let store = Store::open(&paths.db_file()).unwrap();
        store
            .upsert_folder(&Folder {
                account_id: "demo".into(),
                name: "Inbox".into(),
                path: "INBOX".into(),
                unread: 1,
                hidden: false,
            })
            .unwrap();
        store.upsert_message(&sample_msg("demo", "INBOX", "1")).unwrap();
        store.upsert_message(&sample_msg("demo", "INBOX", "2")).unwrap();
        (dir, MailboxService::from_paths(paths).unwrap())
    }

    #[test]
    fn lists_accounts_folders_and_messages() {
        let (_dir, svc) = test_service();
        let accounts = svc.list_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "demo");

        let folders = svc.list_folders(None).unwrap();
        assert_eq!(folders[0].path, "INBOX");

        let msgs = svc.list_messages(None, Some("INBOX"), Some(1)).unwrap();
        assert_eq!(msgs.len(), 1);

        let found = svc.search_messages(None, "Subject").unwrap();
        assert_eq!(found.len(), 2);

        let msg = svc
            .get_message(&found[0].id)
            .unwrap();
        assert!(msg.body_text.contains("body"));

        svc.mark_seen(&msg.envelope.id, true).unwrap();
        let again = svc.get_message(&msg.envelope.id).unwrap();
        assert!(again.envelope.seen);
    }

    #[test]
    fn search_rejects_empty_query() {
        let (_dir, svc) = test_service();
        assert!(svc.search_messages(None, "  ").is_err());
    }
}
