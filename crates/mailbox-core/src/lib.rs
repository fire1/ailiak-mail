//! Shared Mailbox domain: config, paths, models, SQLite cache, secrets.
//!
//! UI crates must talk to mail only through this crate — never IMAP/POP3 directly.

pub mod config;
pub mod imap;
pub mod message_parse;
pub mod paths;
pub mod pop3;
pub mod secrets;
pub mod seed;
pub mod store;
pub mod sync;
pub mod types;

pub use config::{AiliakConfig, AppConfig, MailAccountConfig, MailProtocol, ThemeMode};
pub use paths::AppPaths;
pub use store::Store;
pub use sync::{sync_account, test_connection, SyncOptions, SyncReport};
pub use types::{date_group_label, AccountId, Envelope, Folder, Message, MessageId};
