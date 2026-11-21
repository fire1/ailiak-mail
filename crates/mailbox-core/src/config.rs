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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailAccountConfig {
    pub id: String,
    pub name: String,
    pub email: String,
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
            imap_host: "imap.gmail.com".into(),
            imap_port: 993,
            imap_tls: true,
            smtp_host: "smtp.gmail.com".into(),
            smtp_port: 587,
            smtp_tls: true,
            username: email,
        }
    }

    pub fn is_gmail(&self) -> bool {
        self.imap_host.eq_ignore_ascii_case("imap.gmail.com")
            || self.email.to_lowercase().ends_with("@gmail.com")
            || self.email.to_lowercase().ends_with("@googlemail.com")
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
}

fn default_demo() -> bool {
    true
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            use_demo_data: true,
            theme: ThemeMode::System,
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
}
