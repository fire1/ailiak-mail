//! Built-in AiliakMail OAuth application credentials (desktop / public clients).
//!
//! Users authorize **AiliakMail** in the browser — they do not create Cloud Console
//! projects or paste Client IDs. Maintainers register the apps once and either:
//! - set the constants below, or
//! - inject at build time: `AILIAKMAIL_OAUTH_GOOGLE_CLIENT_ID=… cargo build --release`
//!
//! Priority when resolving credentials:
//! 1. User config override (`[oauth]` in config.toml) — power users / forks
//! 2. Runtime env (`AILIAKMAIL_GOOGLE_CLIENT_ID`, …)
//! 3. Embedded / build-time values from this module

use crate::config::OAuthConfig;
use crate::oauth::{OAuthClientCreds, OAuthProvider};

fn non_empty(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn env_first(keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Ok(v) = std::env::var(k) {
            let t = v.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn build_time(name: &str) -> Option<&'static str> {
    // Names must be string literals for option_env! — match explicitly.
    let v = match name {
        "AILIAKMAIL_OAUTH_GOOGLE_CLIENT_ID" => option_env!("AILIAKMAIL_OAUTH_GOOGLE_CLIENT_ID"),
        "AILIAKMAIL_OAUTH_GOOGLE_CLIENT_SECRET" => {
            option_env!("AILIAKMAIL_OAUTH_GOOGLE_CLIENT_SECRET")
        }
        "AILIAKMAIL_OAUTH_MICROSOFT_CLIENT_ID" => {
            option_env!("AILIAKMAIL_OAUTH_MICROSOFT_CLIENT_ID")
        }
        "AILIAKMAIL_OAUTH_YAHOO_CLIENT_ID" => option_env!("AILIAKMAIL_OAUTH_YAHOO_CLIENT_ID"),
        "AILIAKMAIL_OAUTH_YAHOO_CLIENT_SECRET" => {
            option_env!("AILIAKMAIL_OAUTH_YAHOO_CLIENT_SECRET")
        }
        "AILIAKMAIL_OAUTH_YANDEX_CLIENT_ID" => option_env!("AILIAKMAIL_OAUTH_YANDEX_CLIENT_ID"),
        "AILIAKMAIL_OAUTH_YANDEX_CLIENT_SECRET" => {
            option_env!("AILIAKMAIL_OAUTH_YANDEX_CLIENT_SECRET")
        }
        _ => None,
    };
    v.and_then(non_empty)
}

/// Optional compile-time embeds for release builds only.
/// Leave empty in git — inject via `.env` / CI secrets / `cargo:rustc-env` (see build.rs).
/// Never commit real Client IDs or secrets here (GitHub push protection will block).
mod embedded {
    pub const GOOGLE_CLIENT_ID: &str = "";
    pub const GOOGLE_CLIENT_SECRET: &str = "";
    pub const MICROSOFT_CLIENT_ID: &str = "";
    pub const YAHOO_CLIENT_ID: &str = "";
    pub const YAHOO_CLIENT_SECRET: &str = "";
    pub const YANDEX_CLIENT_ID: &str = "";
    pub const YANDEX_CLIENT_SECRET: &str = "";
}

fn embedded_id(provider: OAuthProvider) -> Option<&'static str> {
    let (build_key, const_id) = match provider {
        OAuthProvider::Google => (
            "AILIAKMAIL_OAUTH_GOOGLE_CLIENT_ID",
            embedded::GOOGLE_CLIENT_ID,
        ),
        OAuthProvider::Microsoft => (
            "AILIAKMAIL_OAUTH_MICROSOFT_CLIENT_ID",
            embedded::MICROSOFT_CLIENT_ID,
        ),
        OAuthProvider::Yahoo => (
            "AILIAKMAIL_OAUTH_YAHOO_CLIENT_ID",
            embedded::YAHOO_CLIENT_ID,
        ),
        OAuthProvider::Yandex => (
            "AILIAKMAIL_OAUTH_YANDEX_CLIENT_ID",
            embedded::YANDEX_CLIENT_ID,
        ),
    };
    build_time(build_key).or_else(|| non_empty(const_id))
}

fn embedded_secret(provider: OAuthProvider) -> Option<&'static str> {
    match provider {
        OAuthProvider::Google => build_time("AILIAKMAIL_OAUTH_GOOGLE_CLIENT_SECRET")
            .or_else(|| non_empty(embedded::GOOGLE_CLIENT_SECRET)),
        OAuthProvider::Microsoft => None,
        OAuthProvider::Yahoo => build_time("AILIAKMAIL_OAUTH_YAHOO_CLIENT_SECRET")
            .or_else(|| non_empty(embedded::YAHOO_CLIENT_SECRET)),
        OAuthProvider::Yandex => build_time("AILIAKMAIL_OAUTH_YANDEX_CLIENT_SECRET")
            .or_else(|| non_empty(embedded::YANDEX_CLIENT_SECRET)),
    }
}

/// True when this build ships working AiliakMail app credentials for `provider`
/// (no user Client ID paste required).
pub fn has_embedded_app(provider: OAuthProvider) -> bool {
    let Some(_) = embedded_id(provider) else {
        return false;
    };
    match provider {
        OAuthProvider::Yahoo | OAuthProvider::Yandex => embedded_secret(provider).is_some(),
        OAuthProvider::Google | OAuthProvider::Microsoft => true,
    }
}

/// Resolve OAuth client credentials for browser app authorization.
pub fn resolve_creds(provider: OAuthProvider, config: &OAuthConfig) -> Option<OAuthClientCreds> {
    let (runtime_id_keys, runtime_secret_keys, cfg_id, cfg_secret) = match provider {
        OAuthProvider::Google => (
            &[
                "AILIAKMAIL_OAUTH_GOOGLE_CLIENT_ID",
                "AILIAKMAIL_GOOGLE_CLIENT_ID",
            ][..],
            &[
                "AILIAKMAIL_OAUTH_GOOGLE_CLIENT_SECRET",
                "AILIAKMAIL_GOOGLE_CLIENT_SECRET",
            ][..],
            config.google_client_id.as_str(),
            Some(config.google_client_secret.as_str()),
        ),
        OAuthProvider::Microsoft => (
            &[
                "AILIAKMAIL_OAUTH_MICROSOFT_CLIENT_ID",
                "AILIAKMAIL_MICROSOFT_CLIENT_ID",
            ][..],
            &[][..],
            config.microsoft_client_id.as_str(),
            None,
        ),
        OAuthProvider::Yahoo => (
            &[
                "AILIAKMAIL_OAUTH_YAHOO_CLIENT_ID",
                "AILIAKMAIL_YAHOO_CLIENT_ID",
            ][..],
            &[
                "AILIAKMAIL_OAUTH_YAHOO_CLIENT_SECRET",
                "AILIAKMAIL_YAHOO_CLIENT_SECRET",
            ][..],
            config.yahoo_client_id.as_str(),
            Some(config.yahoo_client_secret.as_str()),
        ),
        OAuthProvider::Yandex => (
            &[
                "AILIAKMAIL_OAUTH_YANDEX_CLIENT_ID",
                "AILIAKMAIL_YANDEX_CLIENT_ID",
            ][..],
            &[
                "AILIAKMAIL_OAUTH_YANDEX_CLIENT_SECRET",
                "AILIAKMAIL_YANDEX_CLIENT_SECRET",
            ][..],
            config.yandex_client_id.as_str(),
            Some(config.yandex_client_secret.as_str()),
        ),
    };

    let client_id = non_empty(cfg_id)
        .map(|s| s.to_string())
        .or_else(|| env_first(runtime_id_keys))
        .or_else(|| embedded_id(provider).map(|s| s.to_string()))?;

    let client_secret = cfg_secret
        .and_then(non_empty)
        .map(|s| s.to_string())
        .or_else(|| env_first(runtime_secret_keys))
        .or_else(|| embedded_secret(provider).map(|s| s.to_string()));

    Some(OAuthClientCreds {
        client_id,
        client_secret,
    })
}

/// Credentials are complete enough to start browser sign-in.
pub fn creds_ready(provider: OAuthProvider, config: &OAuthConfig) -> bool {
    let Some(creds) = resolve_creds(provider, config) else {
        return false;
    };
    match provider {
        OAuthProvider::Yahoo | OAuthProvider::Yandex => creds
            .client_secret
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty()),
        OAuthProvider::Google | OAuthProvider::Microsoft => true,
    }
}
