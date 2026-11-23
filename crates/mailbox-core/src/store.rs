use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::types::{AccountId, Envelope, Folder, Message, MessageId};

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("open sqlite {}", path.display()))?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let store = Self {
            conn: Connection::open_in_memory()?,
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS folders (
                account_id TEXT NOT NULL,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                unread INTEGER NOT NULL DEFAULT 0,
                hidden INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, path)
            );
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL,
                folder_path TEXT NOT NULL,
                message_id TEXT NOT NULL,
                subject TEXT NOT NULL,
                from_name TEXT NOT NULL,
                from_addr TEXT NOT NULL,
                date_rfc3339 TEXT NOT NULL,
                seen INTEGER NOT NULL DEFAULT 0,
                snippet TEXT NOT NULL,
                body_text TEXT NOT NULL,
                body_html TEXT NOT NULL DEFAULT '',
                to_addrs TEXT NOT NULL DEFAULT '[]',
                cc_addrs TEXT NOT NULL DEFAULT '[]'
            );
            CREATE INDEX IF NOT EXISTS idx_messages_folder
                ON messages(account_id, folder_path, date_rfc3339 DESC);
            CREATE TABLE IF NOT EXISTS ailiak_links (
                message_id TEXT PRIMARY KEY,
                ailiak_task_id TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            "#,
        )?;
        // Older DBs created before `hidden` existed.
        let _ = self
            .conn
            .execute("ALTER TABLE folders ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0", []);
        // HTML body for the safe reader (plain text remains in body_text).
        let _ = self
            .conn
            .execute("ALTER TABLE messages ADD COLUMN body_html TEXT NOT NULL DEFAULT ''", []);
        Ok(())
    }

    pub fn upsert_folder(&self, folder: &Folder) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO folders (account_id, name, path, unread, hidden)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(account_id, path) DO UPDATE SET
                name = excluded.name,
                unread = excluded.unread
            "#,
            params![
                folder.account_id,
                folder.name,
                folder.path,
                folder.unread,
                folder.hidden as i64
            ],
        )?;
        Ok(())
    }

    pub fn set_folder_hidden(&self, account_id: &str, path: &str, hidden: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE folders SET hidden = ?1 WHERE account_id = ?2 AND path = ?3",
            params![hidden as i64, account_id, path],
        )?;
        Ok(())
    }

    /// Visible folders for the nav rail.
    pub fn list_folders(&self, account_id: &str) -> Result<Vec<Folder>> {
        self.list_folders_filtered(account_id, false)
    }

    /// All folders including hidden (for management UI).
    pub fn list_all_folders(&self, account_id: &str) -> Result<Vec<Folder>> {
        self.list_folders_filtered(account_id, true)
    }

    fn list_folders_filtered(&self, account_id: &str, include_hidden: bool) -> Result<Vec<Folder>> {
        let sql = if include_hidden {
            "SELECT account_id, name, path, unread, hidden FROM folders WHERE account_id = ?1"
        } else {
            "SELECT account_id, name, path, unread, hidden FROM folders WHERE account_id = ?1 AND hidden = 0"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![account_id], |row| {
            Ok(Folder {
                account_id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                unread: row.get::<_, i64>(3)? as u32,
                hidden: row.get::<_, i64>(4).unwrap_or(0) != 0,
            })
        })?;
        let mut folders = rows.collect::<Result<Vec<_>, _>>()?;
        crate::types::sort_folders(&mut folders);
        Ok(folders)
    }

    /// Drop local folder rows that are no longer on the server (keeps message cache).
    pub fn retain_folders(&self, account_id: &str, keep_paths: &[String]) -> Result<()> {
        if keep_paths.is_empty() {
            return Ok(());
        }
        let mut placeholders = String::new();
        for i in 0..keep_paths.len() {
            if i > 0 {
                placeholders.push(',');
            }
            placeholders.push('?');
        }
        let sql = format!(
            "DELETE FROM folders WHERE account_id = ?1 AND path NOT IN ({placeholders})"
        );
        let mut params: Vec<rusqlite::types::Value> = Vec::with_capacity(1 + keep_paths.len());
        params.push(account_id.to_string().into());
        for p in keep_paths {
            params.push(p.clone().into());
        }
        self.conn.execute(&sql, rusqlite::params_from_iter(params))?;
        Ok(())
    }

    pub fn upsert_message(&self, msg: &Message) -> Result<()> {
        let to = serde_json::to_string(&msg.to).unwrap_or_else(|_| "[]".into());
        let cc = serde_json::to_string(&msg.cc).unwrap_or_else(|_| "[]".into());
        let e = &msg.envelope;
        self.conn.execute(
            r#"
            INSERT INTO messages (
                id, account_id, folder_path, message_id, subject,
                from_name, from_addr, date_rfc3339, seen, snippet,
                body_text, body_html, to_addrs, cc_addrs
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
            ON CONFLICT(id) DO UPDATE SET
                subject = excluded.subject,
                from_name = excluded.from_name,
                from_addr = excluded.from_addr,
                date_rfc3339 = excluded.date_rfc3339,
                seen = excluded.seen,
                snippet = excluded.snippet,
                body_text = CASE
                    WHEN length(excluded.body_text) > 0 THEN excluded.body_text
                    ELSE messages.body_text
                END,
                body_html = CASE
                    WHEN length(excluded.body_html) > 0 THEN excluded.body_html
                    ELSE messages.body_html
                END,
                to_addrs = excluded.to_addrs,
                cc_addrs = excluded.cc_addrs
            "#,
            params![
                e.id,
                e.account_id,
                e.folder_path,
                e.message_id,
                e.subject,
                e.from_name,
                e.from_addr,
                e.date.to_rfc3339(),
                e.seen as i64,
                e.snippet,
                msg.body_text,
                msg.body_html,
                to,
                cc,
            ],
        )?;
        Ok(())
    }

    pub fn list_envelopes(&self, account_id: &str, folder_path: &str) -> Result<Vec<Envelope>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, account_id, folder_path, message_id, subject,
                   from_name, from_addr, date_rfc3339, seen, snippet
            FROM messages
            WHERE account_id = ?1 AND folder_path = ?2
            ORDER BY date_rfc3339 DESC
            "#,
        )?;
        let rows = stmt.query_map(params![account_id, folder_path], |row| {
            let date_s: String = row.get(7)?;
            let date = DateTime::parse_from_rfc3339(&date_s)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(Envelope {
                id: row.get(0)?,
                account_id: row.get(1)?,
                folder_path: row.get(2)?,
                message_id: row.get(3)?,
                subject: row.get(4)?,
                from_name: row.get(5)?,
                from_addr: row.get(6)?,
                date,
                seen: row.get::<_, i64>(8)? != 0,
                snippet: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_message(&self, id: &MessageId) -> Result<Option<Message>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, account_id, folder_path, message_id, subject,
                   from_name, from_addr, date_rfc3339, seen, snippet,
                   body_text, to_addrs, cc_addrs, body_html
            FROM messages WHERE id = ?1
            "#,
        )?;
        let msg = stmt
            .query_row(params![id], |row| {
                let date_s: String = row.get(7)?;
                let date = DateTime::parse_from_rfc3339(&date_s)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let to_s: String = row.get(11)?;
                let cc_s: String = row.get(12)?;
                let to: Vec<String> = serde_json::from_str(&to_s).unwrap_or_default();
                let cc: Vec<String> = serde_json::from_str(&cc_s).unwrap_or_default();
                let body_html: String = row.get(13).unwrap_or_default();
                Ok(Message {
                    envelope: Envelope {
                        id: row.get(0)?,
                        account_id: row.get(1)?,
                        folder_path: row.get(2)?,
                        message_id: row.get(3)?,
                        subject: row.get(4)?,
                        from_name: row.get(5)?,
                        from_addr: row.get(6)?,
                        date,
                        seen: row.get::<_, i64>(8)? != 0,
                        snippet: row.get(9)?,
                    },
                    body_text: row.get(10)?,
                    body_html,
                    to,
                    cc,
                })
            })
            .optional()?;
        Ok(msg)
    }

    pub fn mark_seen(&self, id: &MessageId, seen: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET seen = ?1 WHERE id = ?2",
            params![seen as i64, id],
        )?;
        Ok(())
    }

    pub fn search_local(&self, account_id: &str, query: &str) -> Result<Vec<Envelope>> {
        let q = format!("%{query}%");
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, account_id, folder_path, message_id, subject,
                   from_name, from_addr, date_rfc3339, seen, snippet
            FROM messages
            WHERE account_id = ?1
              AND (subject LIKE ?2 OR from_name LIKE ?2 OR from_addr LIKE ?2
                   OR snippet LIKE ?2 OR body_text LIKE ?2)
            ORDER BY date_rfc3339 DESC
            LIMIT 200
            "#,
        )?;
        let rows = stmt.query_map(params![account_id, q], |row| {
            let date_s: String = row.get(7)?;
            let date = DateTime::parse_from_rfc3339(&date_s)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(Envelope {
                id: row.get(0)?,
                account_id: row.get(1)?,
                folder_path: row.get(2)?,
                message_id: row.get(3)?,
                subject: row.get(4)?,
                from_name: row.get(5)?,
                from_addr: row.get(6)?,
                date,
                seen: row.get::<_, i64>(8)? != 0,
                snippet: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn ailiak_task_for_message(&self, message_id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT ailiak_task_id FROM ailiak_links WHERE message_id = ?1")?;
        let id = stmt
            .query_row(params![message_id], |row| row.get(0))
            .optional()?;
        Ok(id)
    }

    pub fn record_ailiak_link(&self, message_id: &str, task_id: &str) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO ailiak_links (message_id, ailiak_task_id, created_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(message_id) DO UPDATE SET ailiak_task_id = excluded.ailiak_task_id
            "#,
            params![message_id, task_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn message_count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn account_ids_with_mail(&self) -> Result<Vec<AccountId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT account_id FROM folders ORDER BY account_id")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample() -> Message {
        Message {
            envelope: Envelope {
                id: "m1".into(),
                account_id: "demo".into(),
                folder_path: "INBOX".into(),
                message_id: "<m1@example.com>".into(),
                subject: "Hello".into(),
                from_name: "Ada".into(),
                from_addr: "ada@example.com".into(),
                date: Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap(),
                seen: false,
                snippet: "Hi there".into(),
            },
            body_text: "Hi there\n\n— Ada".into(),
            body_html: String::new(),
            to: vec!["you@example.com".into()],
            cc: vec![],
        }
    }

    #[test]
    fn roundtrip_message() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_folder(&Folder {
                account_id: "demo".into(),
                name: "Inbox".into(),
                path: "INBOX".into(),
                unread: 1,
                hidden: false,
            })
            .unwrap();
        store.upsert_message(&sample()).unwrap();
        let list = store.list_envelopes("demo", "INBOX").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].subject, "Hello");
        let msg = store.get_message(&"m1".into()).unwrap().unwrap();
        assert!(msg.body_text.contains("Ada"));
    }

    #[test]
    fn roundtrip_preserves_body_html() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_folder(&Folder {
                account_id: "demo".into(),
                name: "Inbox".into(),
                path: "INBOX".into(),
                unread: 0,
                hidden: false,
            })
            .unwrap();
        let mut msg = sample();
        msg.body_html = "<p>Hi <a href=\"https://example.com\">there</a></p>".into();
        store.upsert_message(&msg).unwrap();

        let got = store.get_message(&"m1".into()).unwrap().unwrap();
        assert_eq!(got.body_html, msg.body_html);
        assert!(got.body_text.contains("Ada"));

        // Empty HTML on re-upsert must not wipe a stored HTML body.
        let mut again = sample();
        again.body_html = String::new();
        again.body_text = "updated plain".into();
        store.upsert_message(&again).unwrap();
        let got = store.get_message(&"m1".into()).unwrap().unwrap();
        assert!(
            got.body_html.contains("https://example.com"),
            "body_html cleared unexpectedly: {}",
            got.body_html
        );
        assert_eq!(got.body_text, "updated plain");
    }
}
