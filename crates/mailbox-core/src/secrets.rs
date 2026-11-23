//! Mail / Ailiak secrets.
//!
//! Order when reading a mail password:
//! 1. OS keyring (keyring crate — Linux keyutils / macOS Keychain / Windows Credential Manager)
//! 2. Session memory (same process, after Save)
//! 3. Local credential file under the config dir (0600) — survives restarts when keyring is mock/unavailable

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};

use crate::oauth::OAuthTokenBundle;
use crate::paths::AppPaths;

const SERVICE: &str = "ailiakmail";
const LEGACY_SERVICE: &str = "mailbox";

#[derive(Default, Serialize, Deserialize)]
struct FileStore {
    #[serde(default)]
    mail_passwords: HashMap<String, String>,
    #[serde(default)]
    ailiak_token: Option<String>,
    #[serde(default)]
    oauth: HashMap<String, String>,
}

fn session_passwords() -> &'static Mutex<HashMap<String, String>> {
    static MAP: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn file_path() -> Result<PathBuf> {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    if let Some(p) = PATH.get() {
        return Ok(p.clone());
    }
    let paths = AppPaths::resolve()?;
    paths.ensure()?;
    let path = paths.config_dir.join("credentials.json");
    let _ = PATH.set(path.clone());
    Ok(path)
}

fn load_file() -> FileStore {
    let Ok(path) = file_path() else {
        return FileStore::default();
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return FileStore::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_file(store: &FileStore) -> Result<()> {
    let path = file_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(store).context("serialize credentials")?;
    fs::write(&path, raw).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn entry(service: &str, key: &str) -> Result<Entry> {
    Entry::new(service, key).context("open keyring entry")
}

pub fn set_mail_password(account_id: &str, password: &str) -> Result<()> {
    if let Ok(mut map) = session_passwords().lock() {
        map.insert(account_id.to_string(), password.to_string());
    }

    let key = format!("mail_password_{account_id}");
    match entry(SERVICE, &key).and_then(|e| {
        e.set_password(password)
            .context("store mail password in keyring")
    }) {
        Ok(()) => {}
        Err(e) => {
            tracing::warn!(error = %e, "OS keyring write failed; using local credential file");
        }
    }

    let mut store = load_file();
    store
        .mail_passwords
        .insert(account_id.to_string(), password.to_string());
    save_file(&store).context("persist mail password to credential file")?;
    Ok(())
}

pub fn get_mail_password(account_id: &str) -> Result<Option<String>> {
    let key = format!("mail_password_{account_id}");
    match entry(SERVICE, &key)?.get_password() {
        Ok(p) if !p.is_empty() => return Ok(Some(p)),
        Ok(_) | Err(keyring::Error::NoEntry) => {}
        Err(e) => {
            tracing::debug!(error = %e, "OS keyring read failed");
        }
    }

    if let Ok(map) = session_passwords().lock() {
        if let Some(p) = map.get(account_id).filter(|p| !p.is_empty()) {
            return Ok(Some(p.clone()));
        }
    }

    match entry(LEGACY_SERVICE, &key)?.get_password() {
        Ok(p) if !p.is_empty() => return Ok(Some(p)),
        _ => {}
    }

    let store = load_file();
    Ok(store.mail_passwords.get(account_id).cloned())
}

pub fn delete_mail_password(account_id: &str) -> Result<()> {
    if let Ok(mut map) = session_passwords().lock() {
        map.remove(account_id);
    }
    let key = format!("mail_password_{account_id}");
    for service in [SERVICE, LEGACY_SERVICE] {
        match entry(service, &key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => tracing::debug!(error = %e, "keyring delete"),
        }
    }
    let mut store = load_file();
    store.mail_passwords.remove(account_id);
    let _ = save_file(&store);
    Ok(())
}

/// True when a password is available (OS keyring, session, or credential file).
///
/// May talk to the OS keyring (DBus / keyutils) — **do not call from the UI paint path**.
pub fn has_mail_password(account_id: &str) -> bool {
    matches!(get_mail_password(account_id), Ok(Some(p)) if !p.is_empty())
}

/// Fast UI-safe check: session memory + local credential file only (no keyring/DBus).
///
/// Safe to call while painting. Matches [`set_mail_password`], which always mirrors to the file.
pub fn has_mail_password_local(account_id: &str) -> bool {
    if account_id.is_empty() {
        return false;
    }
    if let Ok(map) = session_passwords().lock() {
        if map.get(account_id).is_some_and(|p| !p.is_empty()) {
            return true;
        }
    }
    load_file()
        .mail_passwords
        .get(account_id)
        .is_some_and(|p| !p.is_empty())
}

pub fn set_ailiak_token(token: &str) -> Result<()> {
    match entry(SERVICE, "ailiak_token")?.set_password(token) {
        Ok(()) => {}
        Err(e) => tracing::warn!(error = %e, "OS keyring token write failed"),
    }
    let mut store = load_file();
    store.ailiak_token = Some(token.to_string());
    save_file(&store)?;
    Ok(())
}

pub fn get_ailiak_token() -> Result<Option<String>> {
    match entry(SERVICE, "ailiak_token")?.get_password() {
        Ok(p) if !p.is_empty() => return Ok(Some(p)),
        Ok(_) | Err(keyring::Error::NoEntry) => {}
        Err(_) => {}
    }
    match entry(LEGACY_SERVICE, "ailiak_token")?.get_password() {
        Ok(p) if !p.is_empty() => return Ok(Some(p)),
        _ => {}
    }
    Ok(load_file().ailiak_token.filter(|t| !t.is_empty()))
}

pub fn delete_ailiak_token() -> Result<()> {
    for service in [SERVICE, LEGACY_SERVICE] {
        match entry(service, "ailiak_token")?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_) => {}
        }
    }
    let mut store = load_file();
    store.ailiak_token = None;
    let _ = save_file(&store);
    Ok(())
}

pub fn set_oauth_tokens(account_id: &str, tokens: &OAuthTokenBundle) -> Result<()> {
    let key = format!("mail_oauth_{account_id}");
    let raw = serde_json::to_string(tokens).context("serialize oauth tokens")?;
    match entry(SERVICE, &key)?.set_password(&raw) {
        Ok(()) => {}
        Err(e) => tracing::warn!(error = %e, "OS keyring oauth write failed"),
    }
    let mut store = load_file();
    store.oauth.insert(account_id.to_string(), raw);
    save_file(&store)?;
    Ok(())
}

pub fn get_oauth_tokens(account_id: &str) -> Result<Option<OAuthTokenBundle>> {
    let key = format!("mail_oauth_{account_id}");
    if let Ok(raw) = entry(SERVICE, &key)?.get_password() {
        if let Ok(tokens) = serde_json::from_str(&raw) {
            return Ok(Some(tokens));
        }
    }
    let store = load_file();
    Ok(store
        .oauth
        .get(account_id)
        .and_then(|raw| serde_json::from_str(raw).ok()))
}

pub fn delete_oauth_tokens(account_id: &str) -> Result<()> {
    let key = format!("mail_oauth_{account_id}");
    match entry(SERVICE, &key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(_) => {}
    }
    let mut store = load_file();
    store.oauth.remove(account_id);
    let _ = save_file(&store);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mail_password_round_trip() {
        let id = "test-account-roundtrip";
        let _ = delete_mail_password(id);
        set_mail_password(id, "s3cret!").unwrap();
        assert_eq!(get_mail_password(id).unwrap().as_deref(), Some("s3cret!"));
        assert!(has_mail_password(id));
        delete_mail_password(id).unwrap();
    }
}
