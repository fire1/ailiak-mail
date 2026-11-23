use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths::AppPaths;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub accounts: Vec<MailAccountConfig>,
    #[serde(default)]
    pub ailiak: AiliakConfig,
    #[serde(default)]
    pub ui: UiConfig,
    /// OAuth client IDs for Sign in with Google / Microsoft (desktop PKCE).
    #[serde(default)]
    pub oauth: OAuthConfig,
}

/// How this account authenticates to the mail server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MailAuthMethod {
    #[default]
    Password,
    /// Google OAuth2 + IMAP XOAUTH2 (browser sign-in, including 2FA).
    GoogleOAuth,
    /// Microsoft (Outlook.com / Microsoft 365) OAuth2 + IMAP XOAUTH2.
    MicrosoftOAuth,
    /// Yahoo Mail OAuth2 + IMAP XOAUTH2.
    YahooOAuth,
    /// Yandex Mail OAuth2 + IMAP XOAUTH2.
    YandexOAuth,
}

impl MailAuthMethod {
    pub fn is_oauth(self) -> bool {
        !matches!(self, Self::Password)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuthConfig {
    /// Google Cloud OAuth 2.0 Client ID (type: Desktop app).
    #[serde(default)]
    pub google_client_id: String,
    /// Google Desktop client secret (token exchange; not a true secret for installed apps).
    #[serde(default)]
    pub google_client_secret: String,
    /// Azure app registration Application (client) ID — public client / mobile & desktop.
    #[serde(default)]
    pub microsoft_client_id: String,
    /// Yahoo OAuth Client ID.
    #[serde(default)]
    pub yahoo_client_id: String,
    /// Yahoo OAuth Client Secret (required by Yahoo).
    #[serde(default)]
    pub yahoo_client_secret: String,
    /// Yandex OAuth Client ID.
    #[serde(default)]
    pub yandex_client_id: String,
    /// Yandex OAuth Client Secret.
    #[serde(default)]
    pub yandex_client_secret: String,
}

/// Incoming mail protocol. Existing configs without this field default to IMAP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MailProtocol {
    #[default]
    Imap,
    Pop3,
}

impl MailProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Imap => "imap",
            Self::Pop3 => "pop3",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Imap => "IMAP",
            Self::Pop3 => "POP3",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailAccountConfig {
    pub id: String,
    pub name: String,
    pub email: String,
    /// `imap` (default) or `pop3`. Older servers often only speak POP3.
    #[serde(default)]
    pub protocol: MailProtocol,
    /// Incoming server host (IMAP or POP3 — field name kept for config compatibility).
    pub imap_host: String,
    #[serde(default = "default_imap_port")]
    pub imap_port: u16,
    #[serde(default = "default_true")]
    pub imap_tls: bool,
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    #[serde(default = "default_true")]
    pub smtp_tls: bool,
    /// Password / OAuth tokens are never stored here — see `secrets` / OS keyring.
    pub username: String,
    #[serde(default)]
    pub auth: MailAuthMethod,
}

impl MailAccountConfig {
    /// Gmail IMAP/SMTP defaults. Prefer Sign in with Google; App Password still works.
    pub fn gmail(email: impl Into<String>) -> Self {
        Self::from_provider(crate::providers::MailProvider::Gmail, email)
    }

    /// Build an account draft from a [`crate::providers::MailProvider`] preset.
    pub fn from_provider(
        provider: crate::providers::MailProvider,
        email: impl Into<String>,
    ) -> Self {
        let email = email.into();
        let p = provider.preset();
        Self {
            id: p.label.to_lowercase().replace(' ', "-"),
            name: p.label.into(),
            email: email.clone(),
            protocol: p.protocol,
            imap_host: p.imap_host.into(),
            imap_port: p.imap_port,
            imap_tls: p.imap_tls,
            smtp_host: p.smtp_host.into(),
            smtp_port: p.smtp_port,
            smtp_tls: p.smtp_tls,
            username: email,
            auth: MailAuthMethod::Password,
        }
    }

    /// Generic POP3S defaults (port 995, TLS). Host left blank for the user to fill.
    pub fn pop3_template(email: impl Into<String>) -> Self {
        let email = email.into();
        Self {
            id: "pop3".into(),
            name: "POP3".into(),
            email: email.clone(),
            protocol: MailProtocol::Pop3,
            imap_host: String::new(),
            imap_port: 995,
            imap_tls: true,
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_tls: true,
            username: email,
            auth: MailAuthMethod::Password,
        }
    }

    pub fn is_gmail(&self) -> bool {
        self.protocol == MailProtocol::Imap
            && matches!(
                crate::providers::MailProvider::infer_from_host_email(&self.imap_host, &self.email),
                Some(crate::providers::MailProvider::Gmail)
            )
    }

    pub fn is_pop3(&self) -> bool {
        self.protocol == MailProtocol::Pop3
    }

    /// True when this account looks like abv.bg (POP3-only; no IMAP folders).
    pub fn looks_like_abv(&self) -> bool {
        looks_like_abv(&self.imap_host, &self.email)
    }

    pub fn mail_host(&self) -> &str {
        &self.imap_host
    }

    pub fn mail_port(&self) -> u16 {
        self.imap_port
    }

    pub fn mail_tls(&self) -> bool {
        self.imap_tls
    }

    /// Apply typical ports when switching protocol / TLS.
    pub fn apply_protocol_defaults(&mut self) {
        match (self.protocol, self.imap_tls) {
            (MailProtocol::Imap, true) => self.imap_port = 993,
            (MailProtocol::Imap, false) => self.imap_port = 143,
            (MailProtocol::Pop3, true) => self.imap_port = 995,
            (MailProtocol::Pop3, false) => self.imap_port = 110,
        }
    }
}

/// True when host/email looks like abv.bg (Bulgarian ISP — POP3 only).
pub fn looks_like_abv(host: &str, email: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    let email = email.trim().to_ascii_lowercase();
    host.contains("abv.bg") || email.ends_with("@abv.bg")
}

/// Short note embedded in POP3 sync success/detail strings.
pub fn pop3_sync_limitation_note(host: &str, email: &str) -> &'static str {
    if looks_like_abv(host, email) {
        "ABV is POP3-only (no IMAP) — Sent, Spam, Drafts, and custom folders stay in ABV webmail"
    } else {
        "POP3 cannot list server folders — only Inbox is available"
    }
}

/// Longer help under the account edit form when protocol is POP3.
pub fn pop3_account_form_help(host: &str, email: &str) -> &'static str {
    if looks_like_abv(host, email) {
        "ABV supports POP3 only (no IMAP). Sync downloads Inbox; Sent, Spam, Drafts, and custom folders stay in ABV webmail. Mail is left on the server."
    } else {
        "POP3 maps to a single Inbox — server folders cannot be listed. Mail is left on the server (no delete)."
    }
}

/// Help text for Settings → Folders (POP3 / ABV vs IMAP).
pub fn folders_panel_help(is_pop3: bool, is_abv: bool) -> &'static str {
    if is_abv {
        "ABV has no IMAP — Sync only downloads Inbox. Sent, Spam, Drafts, and custom folders stay in ABV webmail."
    } else if is_pop3 {
        "POP3 cannot list server folders. Sync downloads Inbox only; hide/show applies to local folders."
    } else {
        "IMAP lists every mailbox on Sync. Hide folders you don’t want in the left rail."
    }
}

/// Tooltip for the nav-rail sync button.
pub fn nav_sync_tip(is_pop3: bool, is_abv: bool) -> &'static str {
    if is_pop3 && is_abv {
        "Sync Inbox — POP3 cannot list server folders (ABV webmail folders stay online)"
    } else if is_pop3 {
        "Sync Inbox — POP3 cannot list server folders"
    } else {
        "Sync all folders — list and download every IMAP folder (Shift+G)"
    }
}

fn default_imap_port() -> u16 {
    993
}
fn default_smtp_port() -> u16 {
    587
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiliakConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ailiak_base")]
    pub base_url: String,
    /// `today` | `null` (unscheduled)
    #[serde(default = "default_for_date")]
    pub default_for_date: String,
    #[serde(default = "default_hashtag")]
    pub hashtag: String,
}

fn default_ailiak_base() -> String {
    "https://api.ailiak.com".into()
}
fn default_for_date() -> String {
    "today".into()
}
fn default_hashtag() -> String {
    "mail".into()
}

impl Default for AiliakConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_ailiak_base(),
            default_for_date: default_for_date(),
            hashtag: default_hashtag(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_demo")]
    pub use_demo_data: bool,
    /// `system` (default) follows the OS; or force `light` / `dark`.
    #[serde(default)]
    pub theme: ThemeMode,
    /// Accent id from Ailiak.com mood options (`teal`, `cyan`, …).
    #[serde(default = "default_accent")]
    pub accent: String,
    /// Account id opened on launch. Empty → first configured account.
    #[serde(default)]
    pub default_account_id: String,
    /// Minutes between automatic syncs. `0` disables auto sync. Default: 60 (1 hour).
    #[serde(default = "default_auto_sync_mins")]
    pub auto_sync_mins: u32,
    /// Lowercased `From` addresses allowed to load remote images (privacy allowlist).
    #[serde(default)]
    pub remote_image_senders: Vec<String>,
}

/// Normalize a mail address for allowlist lookup (trim + lowercase).
pub fn normalize_mail_addr(addr: &str) -> String {
    addr.trim().to_lowercase()
}

impl UiConfig {
    pub fn allows_remote_images(&self, from_addr: &str) -> bool {
        let key = normalize_mail_addr(from_addr);
        !key.is_empty() && self.remote_image_senders.iter().any(|s| s == &key)
    }

    /// Remember this sender so later messages auto-load remote images.
    pub fn allow_remote_images_sender(&mut self, from_addr: &str) {
        let key = normalize_mail_addr(from_addr);
        if key.is_empty() {
            return;
        }
        if !self.remote_image_senders.iter().any(|s| s == &key) {
            self.remote_image_senders.push(key);
        }
    }

    /// Forget this sender; remote images stay blocked again.
    pub fn block_remote_images_sender(&mut self, from_addr: &str) {
        let key = normalize_mail_addr(from_addr);
        self.remote_image_senders.retain(|s| s != &key);
    }
}

fn default_demo() -> bool {
    true
}

fn default_accent() -> String {
    "teal".into()
}

fn default_auto_sync_mins() -> u32 {
    60
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            use_demo_data: true,
            theme: ThemeMode::System,
            accent: default_accent(),
            default_account_id: String::new(),
            auto_sync_mins: default_auto_sync_mins(),
            remote_image_senders: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn load_or_init(paths: &AppPaths) -> Result<Self> {
        paths.ensure()?;
        let path = paths.config_file();
        if path.exists() {
            Self::load(&path)
        } else {
            let cfg = Self::default();
            cfg.save(&path)?;
            Ok(cfg)
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let raw =
            fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
        let cfg: AppConfig = toml::from_str(&raw).context("parse config.toml")?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self).context("serialize config")?;
        fs::write(path, raw).with_context(|| format!("write config {}", path.display()))?;
        Ok(())
    }

    /// Account used at startup: `ui.default_account_id` when valid, else first account.
    pub fn primary_account(&self) -> Option<&MailAccountConfig> {
        let preferred = self.ui.default_account_id.trim();
        if !preferred.is_empty() {
            if let Some(a) = self.account_by_id(preferred) {
                return Some(a);
            }
        }
        self.accounts.first()
    }

    pub fn account_by_id(&self, id: &str) -> Option<&MailAccountConfig> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn is_default_account(&self, id: &str) -> bool {
        self.primary_account().is_some_and(|a| a.id == id)
    }

    /// Persist which account opens on launch.
    pub fn set_default_account_id(&mut self, id: impl Into<String>) {
        self.ui.default_account_id = id.into();
    }

    /// Insert or replace an account keyed by `id`.
    pub fn upsert_account(&mut self, account: MailAccountConfig) {
        if let Some(pos) = self.accounts.iter().position(|a| a.id == account.id) {
            self.accounts[pos] = account;
        } else {
            self.accounts.push(account);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str, email: &str) -> MailAccountConfig {
        MailAccountConfig {
            id: id.into(),
            name: email.into(),
            email: email.into(),
            protocol: MailProtocol::Imap,
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            imap_tls: true,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            smtp_tls: true,
            username: email.into(),
            auth: MailAuthMethod::Password,
        }
    }

    #[test]
    fn upsert_account_inserts_and_replaces() {
        let mut cfg = AppConfig::default();
        cfg.upsert_account(sample("a1", "one@example.com"));
        assert_eq!(cfg.accounts.len(), 1);
        cfg.upsert_account(sample("a2", "two@example.com"));
        assert_eq!(cfg.accounts.len(), 2);
        cfg.upsert_account(MailAccountConfig {
            name: "One renamed".into(),
            ..sample("a1", "one@example.com")
        });
        assert_eq!(cfg.accounts.len(), 2);
        assert_eq!(cfg.account_by_id("a1").unwrap().name, "One renamed");
        assert_eq!(cfg.primary_account().unwrap().id, "a1");
    }

    #[test]
    fn primary_account_prefers_default_id() {
        let mut cfg = AppConfig::default();
        cfg.upsert_account(sample("a1", "one@example.com"));
        cfg.upsert_account(sample("a2", "two@example.com"));
        assert_eq!(cfg.primary_account().unwrap().id, "a1");
        cfg.set_default_account_id("a2");
        assert_eq!(cfg.primary_account().unwrap().id, "a2");
        assert!(cfg.is_default_account("a2"));
        cfg.set_default_account_id("missing");
        assert_eq!(
            cfg.primary_account().unwrap().id,
            "a1",
            "invalid default falls back to first"
        );
    }
}
