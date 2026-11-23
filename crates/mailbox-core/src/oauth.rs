//! OAuth2 (PKCE + loopback) for IMAP XOAUTH2 — Google, Microsoft, Yahoo, Yandex.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::config::MailAuthMethod;
use crate::providers::MailProvider;
use crate::secrets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProvider {
    Google,
    Microsoft,
    Yahoo,
    Yandex,
}

impl OAuthProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Microsoft => "microsoft",
            Self::Yahoo => "yahoo",
            Self::Yandex => "yandex",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Microsoft => "Microsoft",
            Self::Yahoo => "Yahoo",
            Self::Yandex => "Yandex",
        }
    }

    pub fn auth_method(self) -> MailAuthMethod {
        match self {
            Self::Google => MailAuthMethod::GoogleOAuth,
            Self::Microsoft => MailAuthMethod::MicrosoftOAuth,
            Self::Yahoo => MailAuthMethod::YahooOAuth,
            Self::Yandex => MailAuthMethod::YandexOAuth,
        }
    }

    pub fn from_auth_method(auth: MailAuthMethod) -> Option<Self> {
        match auth {
            MailAuthMethod::Password => None,
            MailAuthMethod::GoogleOAuth => Some(Self::Google),
            MailAuthMethod::MicrosoftOAuth => Some(Self::Microsoft),
            MailAuthMethod::YahooOAuth => Some(Self::Yahoo),
            MailAuthMethod::YandexOAuth => Some(Self::Yandex),
        }
    }

    pub fn from_mail_provider(p: MailProvider) -> Option<Self> {
        match p {
            MailProvider::Gmail => Some(Self::Google),
            MailProvider::Outlook | MailProvider::Office365 => Some(Self::Microsoft),
            MailProvider::Yahoo => Some(Self::Yahoo),
            MailProvider::Yandex => Some(Self::Yandex),
            MailProvider::Icloud
            | MailProvider::Gmx
            | MailProvider::Imap
            | MailProvider::Pop3 => None,
        }
    }

    fn auth_url(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            Self::Yahoo => "https://api.login.yahoo.com/oauth2/request_auth",
            Self::Yandex => "https://oauth.yandex.com/authorize",
        }
    }

    fn token_url(self) -> &'static str {
        match self {
            Self::Google => "https://oauth2.googleapis.com/token",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            Self::Yahoo => "https://api.login.yahoo.com/oauth2/get_token",
            Self::Yandex => "https://oauth.yandex.com/token",
        }
    }

    fn scope(self) -> &'static str {
        match self {
            Self::Google => "https://mail.google.com/ openid email profile",
            Self::Microsoft => {
                "offline_access openid email profile https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send"
            }
            Self::Yahoo => "openid mail-w",
            Self::Yandex => "mail:imap_full mail:smtp login:email login:info",
        }
    }

    pub fn needs_client_secret(self) -> bool {
        matches!(self, Self::Yahoo | Self::Yandex)
    }

    fn uses_pkce(self) -> bool {
        // Google + Microsoft: public PKCE clients. Yahoo supports PKCE; Yandex uses classic code flow.
        matches!(self, Self::Google | Self::Microsoft | Self::Yahoo)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OAuthTokenBundle {
    pub provider: String,
    pub refresh_token: String,
    pub access_token: String,
    /// Unix seconds when `access_token` expires.
    pub expires_at_unix: i64,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct OAuthSignInResult {
    pub email: String,
    pub name: String,
    pub tokens: OAuthTokenBundle,
    pub oauth: OAuthProvider,
    /// Wizard provider used to pick IMAP hosts (Outlook vs Office 365, etc.).
    pub mail_provider: MailProvider,
    /// Profile photo URL from the provider (e.g. Google `picture`), if any.
    pub picture_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthClientCreds {
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct UserInfo {
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    default_email: Option<String>,
    #[serde(default)]
    mail: Option<String>,
    #[serde(default)]
    user_principal_name: Option<String>,
    #[serde(default)]
    login: Option<String>,
    /// Google userinfo / OpenID `picture`.
    #[serde(default)]
    picture: Option<String>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn random_verifier() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| anyhow!("generate PKCE verifier: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn challenge_s256(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

/// Open a URL in the system browser.
pub fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("xdg-open")?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .context("open")?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .context("start")?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = url;
        bail!("opening a browser is not supported on this OS");
    }
}

fn parse_code_from_request(req: &str) -> Result<String> {
    let line = req.lines().next().unwrap_or("");
    let path = line.split_whitespace().nth(1).unwrap_or("");
    let query = path.split('?').nth(1).unwrap_or("");
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        let k = kv.next().unwrap_or("");
        let v = kv.next().unwrap_or("");
        if k == "code" {
            return Ok(urlencoding_decode(v));
        }
        if k == "error" {
            let desc = query
                .split('&')
                .find_map(|p| {
                    let mut kv = p.splitn(2, '=');
                    (kv.next() == Some("error_description")).then(|| kv.next().unwrap_or(""))
                })
                .unwrap_or(v);
            bail!(
                "Sign-in cancelled or denied: {}",
                urlencoding_decode(desc)
            );
        }
    }
    bail!("no authorization code in redirect (did you close the browser?)");
}

fn urlencoding_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn exchange_code(
    provider: OAuthProvider,
    creds: &OAuthClientCreds,
    code: &str,
    verifier: Option<&str>,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let client = reqwest::blocking::Client::new();
    let mut body = format!(
        "client_id={}&code={}&grant_type=authorization_code&redirect_uri={}",
        urlencoding_encode(&creds.client_id),
        urlencoding_encode(code),
        urlencoding_encode(redirect_uri),
    );
    if let Some(v) = verifier {
        body.push_str(&format!("&code_verifier={}", urlencoding_encode(v)));
    }
    if let Some(secret) = creds.client_secret.as_deref().filter(|s| !s.is_empty()) {
        body.push_str(&format!("&client_secret={}", urlencoding_encode(secret)));
    }

    let mut req = client
        .post(provider.token_url())
        .header("Content-Type", "application/x-www-form-urlencoded");

    // Yahoo expects HTTP Basic with client_id:client_secret on token requests.
    if provider == OAuthProvider::Yahoo {
        if let Some(secret) = creds.client_secret.as_deref().filter(|s| !s.is_empty()) {
            req = req.basic_auth(&creds.client_id, Some(secret));
        }
    }

    let resp = req
        .body(body)
        .send()
        .context("token exchange request")?;
    let status = resp.status();
    let text = resp.text().context("read token exchange body")?;
    if !status.is_success() {
        bail!("token exchange HTTP {status}: {text}");
    }
    serde_json::from_str(&text).context("parse token response")
}

fn refresh_access_token(
    provider: OAuthProvider,
    creds: &OAuthClientCreds,
    refresh_token: &str,
) -> Result<TokenResponse> {
    let client = reqwest::blocking::Client::new();
    let mut body = format!(
        "client_id={}&refresh_token={}&grant_type=refresh_token",
        urlencoding_encode(&creds.client_id),
        urlencoding_encode(refresh_token),
    );
    if let Some(secret) = creds.client_secret.as_deref().filter(|s| !s.is_empty()) {
        body.push_str(&format!("&client_secret={}", urlencoding_encode(secret)));
    }

    let mut req = client
        .post(provider.token_url())
        .header("Content-Type", "application/x-www-form-urlencoded");
    if provider == OAuthProvider::Yahoo {
        if let Some(secret) = creds.client_secret.as_deref().filter(|s| !s.is_empty()) {
            req = req.basic_auth(&creds.client_id, Some(secret));
        }
    }

    let resp = req
        .body(body)
        .send()
        .context("refresh token request")?;
    let status = resp.status();
    let text = resp.text().context("read refresh body")?;
    if !status.is_success() {
        bail!("refresh token HTTP {status}: {text}");
    }
    serde_json::from_str(&text).context("parse refresh response")
}

fn fetch_profile(provider: OAuthProvider, access_token: &str) -> Result<UserInfo> {
    let client = reqwest::blocking::Client::new();
    let url = match provider {
        OAuthProvider::Google => "https://www.googleapis.com/oauth2/v2/userinfo",
        OAuthProvider::Microsoft => "https://graph.microsoft.com/v1.0/me",
        OAuthProvider::Yahoo => "https://api.login.yahoo.com/openid/v1/userinfo",
        OAuthProvider::Yandex => "https://login.yandex.ru/info?format=json",
    };
    let resp = client
        .get(url)
        .bearer_auth(access_token)
        .send()
        .context("profile request")?
        .error_for_status()
        .context("profile HTTP error")?;
    resp.json::<UserInfo>().context("parse profile")
}

fn id_token_claims(id_token: &str) -> Option<serde_json::Value> {
    let payload = id_token.split('.').nth(1)?;
    let padded = match payload.len() % 4 {
        2 => format!("{payload}=="),
        3 => format!("{payload}="),
        _ => payload.to_string(),
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| STANDARD.decode(&padded))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn email_from_id_token(id_token: &str) -> Option<String> {
    let v = id_token_claims(id_token)?;
    v.get("email")
        .or_else(|| v.get("preferred_username"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

fn picture_from_id_token(id_token: &str) -> Option<String> {
    let v = id_token_claims(id_token)?;
    v.get("picture")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn pick_email(info: &UserInfo, id_token: Option<&str>) -> Option<String> {
    info.email
        .clone()
        .or_else(|| info.mail.clone())
        .or_else(|| info.default_email.clone())
        .or_else(|| info.user_principal_name.clone())
        .or_else(|| info.login.clone())
        .or_else(|| id_token.and_then(email_from_id_token))
}

fn pick_name(info: &UserInfo, email: &str) -> String {
    info.name
        .clone()
        .or_else(|| info.display_name.clone())
        .unwrap_or_else(|| {
            email
                .split('@')
                .next()
                .unwrap_or("Mail")
                .to_string()
        })
}

/// Interactive OAuth sign-in: opens the browser, waits for loopback redirect, returns tokens.
pub fn oauth_sign_in(
    oauth: OAuthProvider,
    mail_provider: MailProvider,
    creds: &OAuthClientCreds,
) -> Result<OAuthSignInResult> {
    let client_id = creds.client_id.trim();
    if client_id.is_empty() {
        bail!(
            "{} OAuth Client ID is not set. Create an app in the provider console and paste the Client ID in Settings.",
            oauth.label()
        );
    }
    if oauth.needs_client_secret()
        && creds
            .client_secret
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
    {
        bail!(
            "{} also needs a Client Secret — paste it in Settings (Yahoo/Yandex apps provide one).",
            oauth.label()
        );
    }

    let verifier = if oauth.uses_pkce() {
        Some(random_verifier()?)
    } else {
        None
    };
    let challenge = verifier.as_deref().map(challenge_s256);

    let listener = TcpListener::bind("127.0.0.1:0").context("bind OAuth loopback")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");

    let mut auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}",
        oauth.auth_url(),
        urlencoding_encode(client_id),
        urlencoding_encode(&redirect_uri),
        urlencoding_encode(oauth.scope()),
    );
    match oauth {
        OAuthProvider::Google => {
            auth_url.push_str("&access_type=offline&prompt=consent");
        }
        OAuthProvider::Microsoft => {
            auth_url.push_str("&prompt=select_account");
        }
        OAuthProvider::Yahoo => {}
        OAuthProvider::Yandex => {
            auth_url.push_str("&force_confirm=yes");
        }
    }
    if let Some(ch) = challenge.as_deref() {
        auth_url.push_str(&format!(
            "&code_challenge={}&code_challenge_method=S256",
            urlencoding_encode(ch)
        ));
    }

    open_url(&auth_url).with_context(|| format!("open {} sign-in in browser", oauth.label()))?;

    let (mut stream, _) = listener.accept().with_context(|| {
        format!(
            "waiting for {} sign-in (complete it in your browser)",
            oauth.label()
        )
    })?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .ok();
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).context("read OAuth redirect")?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let code = parse_code_from_request(&req)?;

    // Exchange before answering the browser so success/failure is accurate (not a false “done”).
    let finish = (|| -> Result<OAuthSignInResult> {
        let token = exchange_code(
            oauth,
            creds,
            &code,
            verifier.as_deref(),
            &redirect_uri,
        )?;
        let refresh = token.refresh_token.clone().ok_or_else(|| {
            anyhow!(
                "{} did not return a refresh token — try signing in again (revoke app access in the account and retry)",
                oauth.label()
            )
        })?;

        let info = fetch_profile(oauth, &token.access_token).unwrap_or(UserInfo {
            email: None,
            name: None,
            display_name: None,
            default_email: None,
            mail: None,
            user_principal_name: None,
            login: None,
            picture: None,
        });
        let email = pick_email(&info, token.id_token.as_deref()).ok_or_else(|| {
            anyhow!("could not read email from {} account", oauth.label())
        })?;
        let name = pick_name(&info, &email);
        let picture_url = info
            .picture
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| token.id_token.as_deref().and_then(picture_from_id_token));

        let expires_at_unix = now_unix() + token.expires_in.max(60) - 60;
        let tokens = OAuthTokenBundle {
            provider: oauth.as_str().into(),
            refresh_token: refresh,
            access_token: token.access_token,
            expires_at_unix,
            email: email.clone(),
        };

        Ok(OAuthSignInResult {
            email,
            name,
            tokens,
            oauth,
            mail_provider,
            picture_url,
        })
    })();

    let html = match &finish {
        Ok(r) => format!(
            "<h1>Signed in to {}</h1><p>Signed in as <strong>{}</strong>. You can close this window and return to AiliakMail.</p>",
            oauth.label(),
            html_escape(&r.email)
        ),
        Err(e) => format!(
            "<h1>Sign-in failed</h1><p>{}</p><p>You can close this window and check the status in AiliakMail.</p>",
            html_escape(&e.to_string())
        ),
    };
    let body = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
<!DOCTYPE html><html><body style=\"font-family:system-ui;padding:2rem\">{html}</body></html>"
    );
    let _ = stream.write_all(body.as_bytes());

    if let Err(ref e) = finish {
        tracing::error!(error = %e, "OAuth sign-in failed after browser redirect");
    }

    finish
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Persist OAuth tokens for an account.
pub fn store_oauth_tokens(account_id: &str, tokens: &OAuthTokenBundle) -> Result<()> {
    secrets::set_oauth_tokens(account_id, tokens)
}

/// Google userinfo `picture` URL for the signed-in user (needs `profile` scope).
pub fn fetch_google_picture_url(access_token: &str) -> Result<Option<String>> {
    let info = fetch_profile(OAuthProvider::Google, access_token)?;
    Ok(info
        .picture
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty()))
}

/// Ensure a fresh access token (refresh if needed).
pub fn ensure_access_token(
    account_id: &str,
    oauth: OAuthProvider,
    creds: &OAuthClientCreds,
) -> Result<String> {
    let mut bundle = secrets::get_oauth_tokens(account_id)?.ok_or_else(|| {
        anyhow!(
            "No {} sign-in for this account — Sign in again",
            oauth.label()
        )
    })?;
    if bundle.provider != oauth.as_str() {
        bail!(
            "account tokens are for {}, expected {}",
            bundle.provider,
            oauth.as_str()
        );
    }
    if now_unix() < bundle.expires_at_unix - 30 && !bundle.access_token.is_empty() {
        return Ok(bundle.access_token);
    }
    if creds.client_id.trim().is_empty() {
        bail!(
            "{} OAuth Client ID missing — set it in Settings to refresh the session",
            oauth.label()
        );
    }
    let token = refresh_access_token(oauth, creds, &bundle.refresh_token)?;
    bundle.access_token = token.access_token;
    bundle.expires_at_unix = now_unix() + token.expires_in.max(60) - 60;
    if let Some(r) = token.refresh_token {
        bundle.refresh_token = r;
    }
    secrets::set_oauth_tokens(account_id, &bundle)?;
    Ok(bundle.access_token)
}

/// How to authenticate to the mail server for sync / test.
#[derive(Debug, Clone)]
pub enum MailCredential {
    Password(String),
    OAuth2 { access_token: String },
}

/// Resolve password / OAuth access token for mail I/O.
///
/// May hit the keyring and (for OAuth refresh) the network — call from a worker
/// thread, never from the egui UI loop.
pub fn resolve_mail_credential(
    account: &crate::config::MailAccountConfig,
    oauth_config: &crate::config::OAuthConfig,
    password_override: Option<&str>,
) -> Result<MailCredential> {
    let normalize = |s: &str| -> String { s.chars().filter(|c| !c.is_whitespace()).collect() };
    match account.auth {
        MailAuthMethod::Password => {
            if let Some(p) = password_override.map(str::trim).filter(|s| !s.is_empty()) {
                return Ok(MailCredential::Password(normalize(p)));
            }
            match secrets::get_mail_password(&account.id)? {
                Some(p) => Ok(MailCredential::Password(normalize(&p))),
                None => bail!("No password in keyring — paste your mail password in Settings"),
            }
        }
        auth if auth.is_oauth() => {
            let oauth = OAuthProvider::from_auth_method(auth)
                .ok_or_else(|| anyhow!("Unknown OAuth provider on this account"))?;
            let creds = crate::oauth_clients::resolve_creds(oauth, oauth_config).ok_or_else(|| {
                anyhow!(
                    "{} app authorization is not configured in this build",
                    oauth.label()
                )
            })?;
            let access_token = ensure_access_token(&account.id, oauth, &creds)?;
            Ok(MailCredential::OAuth2 { access_token })
        }
        _ => bail!("Unsupported auth method"),
    }
}

/// SASL XOAUTH2 initial client response (async-imap Authenticator).
pub struct XoAuth2 {
    pub user: String,
    pub access_token: String,
}

impl async_imap::Authenticator for XoAuth2 {
    type Response = Vec<u8>;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.user, self.access_token
        )
        .into_bytes()
    }
}

// Back-compat helpers used by older call sites.
pub type GoogleOAuthResult = OAuthSignInResult;

pub fn google_sign_in(client_id: &str) -> Result<OAuthSignInResult> {
    oauth_sign_in(
        OAuthProvider::Google,
        MailProvider::Gmail,
        &OAuthClientCreds {
            client_id: client_id.into(),
            client_secret: None,
        },
    )
}

pub fn store_google_tokens(account_id: &str, tokens: &OAuthTokenBundle) -> Result<()> {
    store_oauth_tokens(account_id, tokens)
}

pub fn ensure_google_access_token(account_id: &str, client_id: &str) -> Result<String> {
    ensure_access_token(
        account_id,
        OAuthProvider::Google,
        &OAuthClientCreds {
            client_id: client_id.into(),
            client_secret: None,
        },
    )
}
