use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::folder_role::{classify_mailbox, MailboxRole};
use crate::types::{AccountId, Envelope, Folder, Message, MessageId};

fn envelope_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Envelope> {
    let date_s: String = row.get(7)?;
    let date = DateTime::parse_from_rfc3339(&date_s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let message_id: String = row.get(3)?;
    let in_reply_to: String = row.get(11).unwrap_or_default();
    let references: String = row.get(12).unwrap_or_default();
    let thread_id: String = row.get(13).unwrap_or_else(|_| {
        Envelope::compute_thread_id(&message_id, &in_reply_to, &references)
    });
    Ok(Envelope {
        id: row.get(0)?,
        account_id: row.get(1)?,
        folder_path: row.get(2)?,
        message_id,
        subject: row.get(4)?,
        from_name: row.get(5)?,
        from_addr: row.get(6)?,
        date,
        seen: row.get::<_, i64>(8)? != 0,
        flagged: row.get::<_, i64>(9).unwrap_or(0) != 0,
        snippet: row.get(10)?,
        labels: Vec::new(),
        in_reply_to,
        references,
        thread_id,
    })
}

fn label_display_name(path: &str, name: &str) -> String {
    let n = name.trim();
    if !n.is_empty() {
        return n.to_string();
    }
    path.rsplit(['/', '.'])
        .next()
        .unwrap_or(path)
        .trim()
        .to_string()
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open for the UI thread — short busy wait so the egui loop never ANRs on SQLITE_BUSY.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_busy_ms(path, 50)
    }

    /// Open for background sync/move workers — wait longer for writers.
    pub fn open_for_worker(path: &Path) -> Result<Self> {
        Self::open_with_busy_ms(path, 10_000)
    }

    pub fn open_in_memory() -> Result<Self> {
        let store = Self {
            conn: Connection::open_in_memory()?,
        };
        store.configure_connection(50)?;
        store.migrate()?;
        Ok(store)
    }

    fn open_with_busy_ms(path: &Path, busy_ms: u32) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("open sqlite {}", path.display()))?;
        let store = Self { conn };
        store.configure_connection(busy_ms)?;
        store.migrate()?;
        Ok(store)
    }

    /// WAL so UI + workers can share the DB. Keep UI `busy_ms` tiny to avoid freezes.
    fn configure_connection(&self, busy_ms: u32) -> Result<()> {
        self.conn.execute_batch(&format!(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA busy_timeout = {busy_ms};
            PRAGMA synchronous = NORMAL;
            "#
        ))?;
        Ok(())
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
                flagged INTEGER NOT NULL DEFAULT 0,
                snoozed INTEGER NOT NULL DEFAULT 0,
                snippet TEXT NOT NULL,
                body_text TEXT NOT NULL,
                body_html TEXT NOT NULL DEFAULT '',
                to_addrs TEXT NOT NULL DEFAULT '[]',
                cc_addrs TEXT NOT NULL DEFAULT '[]',
                in_reply_to TEXT NOT NULL DEFAULT '',
                references_hdr TEXT NOT NULL DEFAULT '',
                thread_id TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_messages_folder
                ON messages(account_id, folder_path, date_rfc3339 DESC);
            CREATE TABLE IF NOT EXISTS ailiak_links (
                message_id TEXT PRIMARY KEY,
                ailiak_task_id TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sender_notes (
                sender_addr TEXT PRIMARY KEY,
                note TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )?;
        // Older DBs created before `hidden` / `body_html` / `snoozed` existed.
        // ALTER must run before indexes that reference those columns.
        let _ = self
            .conn
            .execute("ALTER TABLE folders ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0", []);
        // HTML body for the safe reader (plain text remains in body_text).
        let _ = self
            .conn
            .execute("ALTER TABLE messages ADD COLUMN body_html TEXT NOT NULL DEFAULT ''", []);
        let _ = self
            .conn
            .execute("ALTER TABLE messages ADD COLUMN snoozed INTEGER NOT NULL DEFAULT 0", []);
        let _ = self.conn.execute(
            "ALTER TABLE messages ADD COLUMN in_reply_to TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE messages ADD COLUMN references_hdr TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE messages ADD COLUMN thread_id TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE messages ADD COLUMN flagged INTEGER NOT NULL DEFAULT 0",
            [],
        );
        self.conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_snoozed
                ON messages(account_id, snoozed, date_rfc3339 DESC);
            CREATE INDEX IF NOT EXISTS idx_messages_thread
                ON messages(account_id, thread_id, date_rfc3339 ASC);
            CREATE INDEX IF NOT EXISTS idx_messages_from
                ON messages(account_id, from_addr);
            CREATE INDEX IF NOT EXISTS idx_messages_msgid
                ON messages(account_id, message_id);
            "#,
        )?;
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

    /// Insert or refresh a cached message. Returns `true` when the row was newly inserted.
    pub fn upsert_message(&self, msg: &Message) -> Result<bool> {
        let to = serde_json::to_string(&msg.to).unwrap_or_else(|_| "[]".into());
        let cc = serde_json::to_string(&msg.cc).unwrap_or_else(|_| "[]".into());
        let e = &msg.envelope;
        let existed: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM messages WHERE id = ?1",
                params![e.id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        self.conn.execute(
            r#"
            INSERT INTO messages (
                id, account_id, folder_path, message_id, subject,
                from_name, from_addr, date_rfc3339, seen, flagged, snippet,
                body_text, body_html, to_addrs, cc_addrs,
                in_reply_to, references_hdr, thread_id
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
            ON CONFLICT(id) DO UPDATE SET
                subject = excluded.subject,
                from_name = excluded.from_name,
                from_addr = excluded.from_addr,
                date_rfc3339 = excluded.date_rfc3339,
                seen = excluded.seen,
                flagged = excluded.flagged,
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
                cc_addrs = excluded.cc_addrs,
                in_reply_to = excluded.in_reply_to,
                references_hdr = excluded.references_hdr,
                thread_id = excluded.thread_id
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
                e.flagged as i64,
                e.snippet,
                msg.body_text,
                msg.body_html,
                to,
                cc,
                e.in_reply_to,
                e.references,
                e.thread_id,
            ],
        )?;
        Ok(!existed)
    }

    pub fn list_envelopes(&self, account_id: &str, folder_path: &str) -> Result<Vec<Envelope>> {
        if crate::types::is_snoozed_folder(folder_path) {
            return self.list_snoozed_envelopes(account_id);
        }
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, account_id, folder_path, message_id, subject,
                   from_name, from_addr, date_rfc3339, seen, flagged, snippet,
                   in_reply_to, references_hdr, thread_id
            FROM messages
            WHERE account_id = ?1 AND folder_path = ?2 AND snoozed = 0
            ORDER BY date_rfc3339 DESC
            "#,
        )?;
        let rows = stmt.query_map(params![account_id, folder_path], envelope_from_row)?;
        let mut envs = rows.collect::<Result<Vec<_>, _>>()?;
        self.enrich_envelopes(&mut envs)?;
        Ok(envs)
    }

    /// All snoozed messages for an account (local “Snoozed” view).
    pub fn list_snoozed_envelopes(&self, account_id: &str) -> Result<Vec<Envelope>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, account_id, folder_path, message_id, subject,
                   from_name, from_addr, date_rfc3339, seen, flagged, snippet,
                   in_reply_to, references_hdr, thread_id
            FROM messages
            WHERE account_id = ?1 AND snoozed = 1
            ORDER BY date_rfc3339 DESC
            "#,
        )?;
        let rows = stmt.query_map(params![account_id], envelope_from_row)?;
        let mut envs = rows.collect::<Result<Vec<_>, _>>()?;
        self.enrich_envelopes(&mut envs)?;
        Ok(envs)
    }

    /// Attach Gmail-style labels + starred from sibling folder rows (same Message-ID).
    fn enrich_envelopes(&self, envs: &mut [Envelope]) -> Result<()> {
        if envs.is_empty() {
            return Ok(());
        }
        let account_id = envs[0].account_id.clone();
        let mut ids: Vec<String> = envs
            .iter()
            .map(|e| e.message_id.clone())
            .filter(|id| !id.is_empty())
            .collect();
        ids.sort();
        ids.dedup();
        if ids.is_empty() {
            return Ok(());
        }

        let mut placeholders = String::new();
        for i in 0..ids.len() {
            if i > 0 {
                placeholders.push(',');
            }
            placeholders.push('?');
        }
        let sql = format!(
            r#"
            SELECT m.message_id, m.folder_path, COALESCE(f.name, ''), m.flagged
            FROM messages m
            LEFT JOIN folders f
              ON f.account_id = m.account_id AND f.path = m.folder_path
            WHERE m.account_id = ?1 AND m.message_id IN ({placeholders})
            "#
        );
        let mut params: Vec<rusqlite::types::Value> = Vec::with_capacity(1 + ids.len());
        params.push(account_id.into());
        for id in &ids {
            params.push(id.clone().into());
        }
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3).unwrap_or(0) != 0,
            ))
        })?;

        let mut by_mid: HashMap<String, Vec<(String, String, bool)>> = HashMap::new();
        for row in rows {
            let (mid, path, name, flagged) = row?;
            by_mid
                .entry(mid)
                .or_default()
                .push((path, name, flagged));
        }

        for env in envs.iter_mut() {
            let Some(entries) = by_mid.get(&env.message_id) else {
                continue;
            };
            let mut labels: Vec<String> = Vec::new();
            let mut flagged = env.flagged;
            for (path, name, fl) in entries {
                if *fl {
                    flagged = true;
                }
                let role = classify_mailbox(path, name);
                match role {
                    MailboxRole::Starred => flagged = true,
                    MailboxRole::Important | MailboxRole::User => {
                        if path != &env.folder_path {
                            let label = label_display_name(path, name);
                            if !label.is_empty() && !labels.iter().any(|l| l == &label) {
                                labels.push(label);
                            }
                        }
                    }
                    _ => {}
                }
            }
            labels.sort();
            env.flagged = flagged;
            env.labels = labels;
        }
        Ok(())
    }

    /// Messages in a thread, oldest first (for chat view).
    pub fn list_thread_messages(
        &self,
        account_id: &str,
        thread_id: &str,
    ) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id FROM messages
            WHERE account_id = ?1 AND thread_id = ?2
            ORDER BY date_rfc3339 ASC
            "#,
        )?;
        let ids: Vec<String> = stmt
            .query_map(params![account_id, thread_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(msg) = self.get_message(&id)? {
                out.push(msg);
            }
        }
        Ok(out)
    }

    /// Messages exchanged with a peer address (from them or to them), across folders.
    /// Trash and Spam are excluded so a trashed conversation leaves People/chat.
    pub fn list_messages_with_peer(
        &self,
        account_id: &str,
        peer_addr: &str,
    ) -> Result<Vec<Message>> {
        let peer = peer_addr.trim().to_lowercase();
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, from_addr, to_addrs, cc_addrs, folder_path, date_rfc3339
            FROM messages
            WHERE account_id = ?1 AND snoozed = 0
            ORDER BY date_rfc3339 ASC
            "#,
        )?;
        let candidates: Vec<(String, String, String, String, String)> = stmt
            .query_map(params![account_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        for (id, from, to_s, cc_s, folder_path) in candidates {
            let role = crate::folder_role::classify_mailbox(&folder_path, &folder_path);
            if matches!(
                role,
                crate::folder_role::MailboxRole::Trash | crate::folder_role::MailboxRole::Spam
            ) {
                continue;
            }
            let from_l = from.to_lowercase();
            if from_l == peer || from_l.contains(&peer) {
                if let Some(msg) = self.get_message(&id)? {
                    out.push(msg);
                }
                continue;
            }
            let to: Vec<String> = serde_json::from_str(&to_s).unwrap_or_default();
            let cc: Vec<String> = serde_json::from_str(&cc_s).unwrap_or_default();
            let hit = to
                .iter()
                .chain(cc.iter())
                .any(|a| a.to_lowercase().contains(&peer));
            if hit {
                if let Some(msg) = self.get_message(&id)? {
                    out.push(msg);
                }
            }
        }
        Ok(out)
    }

    pub fn snoozed_counts(&self, account_id: &str) -> Result<(u32, u32)> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT COUNT(*),
                   COALESCE(SUM(CASE WHEN seen = 0 THEN 1 ELSE 0 END), 0)
            FROM messages
            WHERE account_id = ?1 AND snoozed = 1
            "#,
        )?;
        let (total, unread) = stmt.query_row(params![account_id], |row| {
            Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u32))
        })?;
        Ok((total, unread))
    }

    pub fn is_snoozed(&self, id: &MessageId) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT snoozed FROM messages WHERE id = ?1")?;
        let v = stmt
            .query_row(params![id], |row| row.get::<_, i64>(0))
            .optional()?;
        Ok(v.unwrap_or(0) != 0)
    }

    /// Hide / restore a message locally. Does not change `seen` by itself.
    /// Unread snoozed mail is removed from the source folder’s unread badge.
    pub fn set_snoozed(&self, id: &MessageId, snoozed: bool) -> Result<bool> {
        let Some(msg) = self.get_message(id)? else {
            return Ok(false);
        };
        let was = self.is_snoozed(id)?;
        if was == snoozed {
            return Ok(false);
        }
        self.conn.execute(
            "UPDATE messages SET snoozed = ?1 WHERE id = ?2",
            params![snoozed as i64, id],
        )?;
        if !msg.envelope.seen {
            if snoozed {
                self.conn.execute(
                    "UPDATE folders SET unread = MAX(0, unread - 1) WHERE account_id = ?1 AND path = ?2",
                    params![msg.envelope.account_id, msg.envelope.folder_path],
                )?;
            } else {
                self.conn.execute(
                    "UPDATE folders SET unread = unread + 1 WHERE account_id = ?1 AND path = ?2",
                    params![msg.envelope.account_id, msg.envelope.folder_path],
                )?;
            }
        }
        Ok(true)
    }

    pub fn get_message(&self, id: &MessageId) -> Result<Option<Message>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, account_id, folder_path, message_id, subject,
                   from_name, from_addr, date_rfc3339, seen, flagged, snippet,
                   body_text, to_addrs, cc_addrs, body_html,
                   in_reply_to, references_hdr, thread_id
            FROM messages WHERE id = ?1
            "#,
        )?;
        let mut msg = stmt
            .query_row(params![id], |row| {
                let date_s: String = row.get(7)?;
                let date = DateTime::parse_from_rfc3339(&date_s)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let to_s: String = row.get(12)?;
                let cc_s: String = row.get(13)?;
                let to: Vec<String> = serde_json::from_str(&to_s).unwrap_or_default();
                let cc: Vec<String> = serde_json::from_str(&cc_s).unwrap_or_default();
                let body_html: String = row.get(14).unwrap_or_default();
                let message_id: String = row.get(3)?;
                let in_reply_to: String = row.get(15).unwrap_or_default();
                let references: String = row.get(16).unwrap_or_default();
                let thread_id: String = row.get(17).unwrap_or_else(|_| {
                    Envelope::compute_thread_id(&message_id, &in_reply_to, &references)
                });
                Ok(Message {
                    envelope: Envelope {
                        id: row.get(0)?,
                        account_id: row.get(1)?,
                        folder_path: row.get(2)?,
                        message_id,
                        subject: row.get(4)?,
                        from_name: row.get(5)?,
                        from_addr: row.get(6)?,
                        date,
                        seen: row.get::<_, i64>(8)? != 0,
                        flagged: row.get::<_, i64>(9).unwrap_or(0) != 0,
                        snippet: row.get(10)?,
                        labels: Vec::new(),
                        in_reply_to,
                        references,
                        thread_id,
                    },
                    body_text: row.get(11)?,
                    body_html,
                    to,
                    cc,
                })
            })
            .optional()?;
        if let Some(ref mut m) = msg {
            let mut one = [m.envelope.clone()];
            self.enrich_envelopes(&mut one)?;
            m.envelope = one.into_iter().next().unwrap();
        }
        Ok(msg)
    }

    /// Set or clear `\Flagged` / star on all local rows sharing this Message-ID.
    /// When clearing, drops local copies that live only in Starred mailboxes so
    /// list enrichment does not immediately re-star the message.
    pub fn set_flagged(&self, id: &MessageId, flagged: bool) -> Result<bool> {
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT account_id, message_id FROM messages WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((account_id, message_id)) = row else {
            return Ok(false);
        };
        let updated = if message_id.is_empty() {
            self.conn.execute(
                "UPDATE messages SET flagged = ?1 WHERE id = ?2",
                params![flagged as i64, id],
            )?
        } else {
            self.conn.execute(
                "UPDATE messages SET flagged = ?1 WHERE account_id = ?2 AND message_id = ?3",
                params![flagged as i64, account_id, message_id],
            )?
        };
        if !flagged && !message_id.is_empty() {
            let folders = self.list_all_folders(&account_id)?;
            for f in folders {
                if classify_mailbox(&f.path, &f.name) == MailboxRole::Starred {
                    self.conn.execute(
                        "DELETE FROM messages WHERE account_id = ?1 AND message_id = ?2 AND folder_path = ?3",
                        params![account_id, message_id, f.path],
                    )?;
                }
            }
        }
        Ok(updated > 0)
    }

    /// Set local seen flag. Returns `true` when the value actually changed.
    /// Also adjusts the folder unread badge count.
    pub fn mark_seen(&self, id: &MessageId, seen: bool) -> Result<bool> {
        let Some(msg) = self.get_message(id)? else {
            return Ok(false);
        };
        if msg.envelope.seen == seen {
            return Ok(false);
        }
        self.conn.execute(
            "UPDATE messages SET seen = ?1 WHERE id = ?2",
            params![seen as i64, id],
        )?;
        if seen {
            self.conn.execute(
                "UPDATE folders SET unread = MAX(0, unread - 1) WHERE account_id = ?1 AND path = ?2",
                params![msg.envelope.account_id, msg.envelope.folder_path],
            )?;
        } else {
            self.conn.execute(
                "UPDATE folders SET unread = unread + 1 WHERE account_id = ?1 AND path = ?2",
                params![msg.envelope.account_id, msg.envelope.folder_path],
            )?;
        }
        Ok(true)
    }

    pub fn delete_message(&self, id: &MessageId) -> Result<()> {
        self.conn
            .execute("DELETE FROM messages WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Move a cached message into another folder (updates `folder_path` + id).
    pub fn move_message_local(
        &self,
        id: &MessageId,
        account_id: &str,
        from_folder: &str,
        to_folder: &str,
    ) -> Result<MessageId> {
        let uid = crate::types::parse_imap_uid(id, account_id, from_folder).unwrap_or(0);
        let new_id = format!("{account_id}:{to_folder}:{uid}");
        if new_id == *id {
            self.conn.execute(
                "UPDATE messages SET folder_path = ?1 WHERE id = ?2",
                params![to_folder, id],
            )?;
            return Ok(id.clone());
        }
        // Prefer rename when destination id is free; otherwise delete source after copy.
        let updated = self.conn.execute(
            r#"
            UPDATE messages
            SET id = ?1, folder_path = ?2
            WHERE id = ?3
              AND NOT EXISTS (SELECT 1 FROM messages WHERE id = ?1)
            "#,
            params![new_id, to_folder, id],
        )?;
        if updated == 0 {
            // Collision or missing row — fall back to folder_path-only update.
            self.conn.execute(
                "UPDATE messages SET folder_path = ?1 WHERE id = ?2",
                params![to_folder, id],
            )?;
            return Ok(id.clone());
        }
        Ok(new_id)
    }

    /// Frequent From addresses for composer To/Cc suggestions (Trash/Spam excluded).
    /// Returns `(display_name, email)` ordered by how often they appear.
    pub fn list_recipient_hints(
        &self,
        account_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        let limit = limit.clamp(1, 500);
        let mut stmt = self.conn.prepare(
            r#"
            SELECT trim(from_addr), max(from_name), COUNT(*) AS c
            FROM messages
            WHERE account_id = ?1
              AND snoozed = 0
              AND length(trim(from_addr)) > 0
              AND lower(folder_path) NOT LIKE '%trash%'
              AND lower(folder_path) NOT LIKE '%spam%'
              AND lower(folder_path) NOT LIKE '%junk%'
            GROUP BY lower(trim(from_addr))
            ORDER BY c DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![account_id, limit as i64], |row| {
            let addr: String = row.get(0)?;
            let name: String = row.get::<_, String>(1).unwrap_or_default();
            Ok((name.trim().to_string(), addr.trim().to_string()))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (name, addr) = row?;
            if addr.contains('@') {
                out.push((name, addr));
            }
        }
        Ok(out)
    }

    pub fn search_local(&self, account_id: &str, query: &str) -> Result<Vec<Envelope>> {
        let q = format!("%{query}%");
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, account_id, folder_path, message_id, subject,
                   from_name, from_addr, date_rfc3339, seen, flagged, snippet,
                   in_reply_to, references_hdr, thread_id
            FROM messages
            WHERE account_id = ?1
              AND snoozed = 0
              AND (subject LIKE ?2 OR from_name LIKE ?2 OR from_addr LIKE ?2
                   OR snippet LIKE ?2 OR body_text LIKE ?2)
            ORDER BY date_rfc3339 DESC
            LIMIT 200
            "#,
        )?;
        let rows = stmt.query_map(params![account_id, q], envelope_from_row)?;
        let mut envs = rows.collect::<Result<Vec<_>, _>>()?;
        self.enrich_envelopes(&mut envs)?;
        Ok(envs)
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

    /// Load a note keyed by sender email (case-insensitive).
    pub fn get_sender_note(&self, sender_addr: &str) -> Result<Option<String>> {
        let key = sender_addr.trim().to_lowercase();
        if key.is_empty() {
            return Ok(None);
        }
        let mut stmt = self
            .conn
            .prepare("SELECT note FROM sender_notes WHERE sender_addr = ?1")?;
        let note = stmt
            .query_row(params![key], |row| row.get(0))
            .optional()?;
        Ok(note)
    }

    /// Upsert a per-sender note. Empty note deletes the row.
    pub fn set_sender_note(&self, sender_addr: &str, note: &str) -> Result<()> {
        let key = sender_addr.trim().to_lowercase();
        if key.is_empty() {
            return Ok(());
        }
        if note.is_empty() {
            self.conn
                .execute("DELETE FROM sender_notes WHERE sender_addr = ?1", params![key])?;
            return Ok(());
        }
        self.conn.execute(
            r#"
            INSERT INTO sender_notes (sender_addr, note, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(sender_addr) DO UPDATE SET
                note = excluded.note,
                updated_at = excluded.updated_at
            "#,
            params![key, note, Utc::now().to_rfc3339()],
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
                flagged: false,
                snippet: "Hi there".into(),
                labels: Vec::new(),
                in_reply_to: String::new(),
                references: String::new(),
                thread_id: String::new(),
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
    fn set_flagged_clears_starred_copies() {
        let store = Store::open_in_memory().unwrap();
        for (name, path) in [("Inbox", "INBOX"), ("Starred", "[Gmail]/Starred")] {
            store
                .upsert_folder(&Folder {
                    account_id: "demo".into(),
                    name: name.into(),
                    path: path.into(),
                    unread: 0,
                    hidden: false,
                })
                .unwrap();
        }
        store.upsert_message(&sample()).unwrap();
        let mut starred = sample();
        starred.envelope.id = "m1-star".into();
        starred.envelope.folder_path = "[Gmail]/Starred".into();
        starred.envelope.flagged = true;
        store.upsert_message(&starred).unwrap();
        assert!(store.list_envelopes("demo", "INBOX").unwrap()[0].flagged);

        assert!(store.set_flagged(&"m1".into(), false).unwrap());
        let inbox = store.list_envelopes("demo", "INBOX").unwrap();
        assert!(!inbox[0].flagged);
        assert!(store
            .list_envelopes("demo", "[Gmail]/Starred")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn sibling_folder_becomes_label_and_star() {
        let store = Store::open_in_memory().unwrap();
        for (name, path) in [("Inbox", "INBOX"), ("Work", "Work"), ("Starred", "[Gmail]/Starred")]
        {
            store
                .upsert_folder(&Folder {
                    account_id: "demo".into(),
                    name: name.into(),
                    path: path.into(),
                    unread: 0,
                    hidden: false,
                })
                .unwrap();
        }
        store.upsert_message(&sample()).unwrap();
        let mut labeled = sample();
        labeled.envelope.id = "m1-work".into();
        labeled.envelope.folder_path = "Work".into();
        store.upsert_message(&labeled).unwrap();
        let mut starred = sample();
        starred.envelope.id = "m1-star".into();
        starred.envelope.folder_path = "[Gmail]/Starred".into();
        starred.envelope.flagged = true;
        store.upsert_message(&starred).unwrap();

        let list = store.list_envelopes("demo", "INBOX").unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].flagged);
        assert_eq!(list[0].labels, vec!["Work".to_string()]);
    }

    #[test]
    fn snooze_hides_from_folder_and_keeps_unread() {
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
        assert_eq!(store.list_envelopes("demo", "INBOX").unwrap().len(), 1);
        assert!(store.set_snoozed(&"m1".into(), true).unwrap());
        assert!(store.list_envelopes("demo", "INBOX").unwrap().is_empty());
        assert_eq!(
            store.list_envelopes("demo", crate::types::SNOOZED_FOLDER_PATH)
                .unwrap()
                .len(),
            1
        );
        assert!(!store.get_message(&"m1".into()).unwrap().unwrap().envelope.seen);
        assert_eq!(store.list_folders("demo").unwrap()[0].unread, 0);
        assert!(store.set_snoozed(&"m1".into(), false).unwrap());
        assert_eq!(store.list_envelopes("demo", "INBOX").unwrap().len(), 1);
        assert_eq!(store.list_folders("demo").unwrap()[0].unread, 1);
    }

    #[test]
    fn mark_seen_updates_folder_unread() {
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
        assert!(store.mark_seen(&"m1".into(), true).unwrap());
        assert!(!store.mark_seen(&"m1".into(), true).unwrap());
        let folders = store.list_folders("demo").unwrap();
        assert_eq!(folders[0].unread, 0);
        assert!(store.get_message(&"m1".into()).unwrap().unwrap().envelope.seen);
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
