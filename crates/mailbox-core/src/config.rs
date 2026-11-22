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
    /// Password is never stored here — see `secrets` module / OS keyring.
    pub username: String,
}

impl MailAccountConfig {
    /// Gmail IMAP/SMTP defaults. Auth with an [App Password](https://myaccount.google.com/apppasswords).
    pub fn gmail(email: impl Into<String>) -> Self {
        let email = email.into();
        Self {
            id: "gmail".into(),
            name: "Gmail".into(),
            email: email.clone(),
            protocol: MailProtocol::Imap,
            imap_host: "imap.gmail.com".into(),
            imap_port: 993,
            imap_tls: true,
            smtp_host: "smtp.gmail.com".into(),
            smtp_port: 587,
            smtp_tls: true,
            username: email,
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
        }
    }

    pub fn is_gmail(&self) -> bool {
        self.protocol == MailProtocol::Imap
            && (self.imap_host.eq_ignore_ascii_case("imap.gmail.com")
                || self.email.to_lowercase().ends_with("@gmail.com")
                || self.email.to_lowercase().ends_with("@googlemail.com"))
    }

    pub fn is_pop3(&self) -> bool {
        self.protocol == MailProtocol::Pop3
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
}

fn default_demo() -> bool {
    true
}

fn default_accent() -> String {
    "teal".into()
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            use_demo_data: true,
            theme: ThemeMode::System,
            accent: default_accent(),
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

    pub fn primary_account(&self) -> Option<&MailAccountConfig> {
        self.accounts.first()
    }

    pub fn account_by_id(&self, id: &str) -> Option<&MailAccountConfig> {
        self.accounts.iter().find(|a| a.id == id)
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
}
