//! Shared Mailbox domain: config, paths, models, SQLite cache, secrets.
//!
//! UI crates must talk to mail only through this crate — never IMAP/POP3 directly.

pub mod config;
pub mod imap;
pub mod mail_body;
pub mod message_parse;
pub mod mime_inline;
pub use mail_body::{
    analyze_remote_image_threats, build_mail_body, build_mail_html_document,
    classify_mail_navigation, classify_remote_image_threats, mail_html_has_remote_images, safe_url,
    sanitize_mail_html, BodyBlock, ImageThreatKind, InlineSeg, MailBody, MailNavAction,
    RemoteImageRisk,
};
pub use message_parse::{html_to_plain, missing_plain_placeholder, readable_body};
pub use mime_inline::{
    has_cid_refs, inline_parts, message_has_real_html, resolve_inline_images, rewrite_cids,
};
pub mod oauth;
pub mod oauth_clients;
pub mod paths;
pub mod pop3;
pub mod providers;
pub mod secrets;
pub mod seed;
pub mod smtp;
pub mod store;
pub mod sync;
pub mod types;
pub mod folder_role;

pub use config::{
    folders_panel_help, looks_like_abv, mail_host, normalize_mail_addr,
    pop3_account_form_help, pop3_sync_limitation_note, remote_image_domain, AiliakConfig,
    AppConfig, MailAccountConfig, MailAuthMethod, MailProtocol, OAuthConfig, SoundTheme,
    ThemeMode, UiConfig,
};
pub use oauth::{
    ensure_access_token, oauth_sign_in, resolve_mail_credential, store_oauth_tokens, MailCredential,
    OAuthClientCreds, OAuthProvider, OAuthSignInResult,
};
pub use oauth_clients::{creds_ready as oauth_creds_ready, has_embedded_app, resolve_creds as resolve_oauth_creds};
pub use providers::{is_personal_mailbox_domain, MailProvider, ProviderPreset};
pub use paths::AppPaths;
pub use smtp::{
    reply_references, reply_subject, send as send_mail, OutgoingAttachment, OutgoingMail,
};
pub use store::Store;
pub use sync::{
    mark_flagged_on_server, mark_seen_on_server, move_messages, sync_account, test_connection,
    SyncOptions, SyncReport,
};
pub use folder_role::{
    build_label_tree, classify_mailbox, is_all_mail, LabelNode, MailboxRole,
};
pub use types::{
    date_group_label, is_snoozed_folder, list_date_label, resolve_special_folder, AccountId,
    Envelope, Folder, Message, MessageId, SpecialFolderKind, SNOOZED_FOLDER_PATH,
};
