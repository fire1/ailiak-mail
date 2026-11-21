//! Shared Mailbox domain: config, paths, models, SQLite cache, secrets.
//!
//! UI crates must talk to mail only through this crate — never IMAP directly.

pub mod config;
pub mod imap;
pub mod paths;
pub mod secrets;
pub mod seed;
pub mod store;
pub mod sync;
pub mod types;

pub use config::{AiliakConfig, AppConfig, MailAccountConfig, ThemeMode};
pub use paths::AppPaths;
pub use store::Store;
pub use sync::{sync_account, test_connection, SyncOptions, SyncReport};
pub use types::{AccountId, Envelope, Folder, Message, MessageId};
