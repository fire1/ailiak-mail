//! Shared Mailbox domain: config, paths, models, SQLite cache, secrets.
//!
//! UI crates must talk to mail only through this crate — never IMAP/POP3 directly.

pub mod config;
pub mod imap;
pub mod mail_body;
pub mod message_parse;
pub use mail_body::{
    build_mail_body, build_mail_html_document, classify_mail_navigation, mail_html_has_remote_images,
    safe_url, sanitize_mail_html, BodyBlock, InlineSeg, MailBody, MailNavAction,
};
pub use message_parse::readable_body;
pub mod oauth;
pub mod oauth_clients;
pub mod paths;
pub mod pop3;
pub mod providers;
pub mod secrets;
pub mod seed;
pub mod store;
pub mod sync;
pub mod types;

pub use config::{
    AiliakConfig, AppConfig, MailAccountConfig, MailAuthMethod, MailProtocol, OAuthConfig,
    ThemeMode,
};
pub use oauth::{
    ensure_access_token, oauth_sign_in, store_oauth_tokens, MailCredential, OAuthClientCreds,
    OAuthProvider, OAuthSignInResult,
};
pub use oauth_clients::{creds_ready as oauth_creds_ready, has_embedded_app, resolve_creds as resolve_oauth_creds};
pub use providers::{MailProvider, ProviderPreset};
pub use paths::AppPaths;
pub use store::Store;
pub use sync::{move_messages, sync_account, test_connection, SyncOptions, SyncReport};
pub use types::{
    date_group_label, resolve_special_folder, AccountId, Envelope, Folder, Message, MessageId,
    SpecialFolderKind,
};
