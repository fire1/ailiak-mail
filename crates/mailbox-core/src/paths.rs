use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;

/// XDG-style locations for config and cached mail data.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Result<Self> {
        let dirs = ProjectDirs::from("com", "ailiak", "AiliakMail")
            .context("could not resolve XDG project directories")?;
        let mut paths = Self {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_dir().to_path_buf(),
        };
        paths.migrate_legacy_mailbox_dirs()?;
        Ok(paths)
    }

    /// Prefer AiliakMail dirs; if empty, reuse legacy `mailbox` XDG paths when present.
    fn migrate_legacy_mailbox_dirs(&mut self) -> Result<()> {
        let legacy = ProjectDirs::from("com", "mailbox", "mailbox");
        let Some(legacy) = legacy else {
            return Ok(());
        };
        let legacy_config = legacy.config_dir();
        let legacy_data = legacy.data_dir();

        if !self.config_dir.exists()
            && legacy_config.join("config.toml").is_file()
        {
            self.config_dir = legacy_config.to_path_buf();
        }
        if !self.data_dir.exists() && legacy_data.join("mail.db").is_file() {
            self.data_dir = legacy_data.to_path_buf();
        }
        Ok(())
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("create config dir {}", self.config_dir.display()))?;
        fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("create data dir {}", self.data_dir.display()))?;
        fs::create_dir_all(self.avatars_dir())
            .with_context(|| format!("create avatars dir {}", self.avatars_dir().display()))?;
        Ok(())
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("mail.db")
    }

    /// Local profile-picture overrides (`{sha256(email)}.png`).
    pub fn avatars_dir(&self) -> PathBuf {
        self.data_dir.join("avatars")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_under_ailiakmail_or_legacy() {
        let p = AppPaths::resolve().unwrap();
        let cfg = p.config_dir.to_string_lossy().to_lowercase();
        assert!(
            cfg.contains("ailiakmail") || cfg.contains("mailbox"),
            "unexpected config dir {}",
            p.config_dir.display()
        );
    }
}
