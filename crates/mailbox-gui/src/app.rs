use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::{bail, Result};
use eframe::App;
use egui::RichText;
use mailbox_ailiak::{
    build_payload, short_task_number, todo_defaults, AiliakClient, DeskTask,
};
use mailbox_core::config::{AppConfig, MailAccountConfig, MailAuthMethod, MailProtocol};
use mailbox_core::oauth::{self, MailCredential, OAuthProvider, OAuthSignInResult};
use mailbox_core::oauth_clients;
use mailbox_core::paths::AppPaths;
use mailbox_core::secrets;
use mailbox_core::seed::{demo_account_id, seed_demo_if_empty};
use mailbox_core::store::Store;
use mailbox_core::sync::{sync_account, test_connection, SyncOptions};
use mailbox_core::types::{Envelope, Folder, Message, MessageId};

use crate::avatars::{AvatarCache, AvatarMsg};
use crate::brand::AiliakLogo;
use crate::calendar::CalendarState;
use crate::settings::{AccountWizard, AccountWizardStep, SettingsSection};
use crate::theme;
use mailbox_core::providers::MailProvider;

enum BgMsg {
    AiliakOk {
        task_id: String,
        for_date: Option<String>,
    },
    WhoAmI(String),
    SyncOk {
        account_id: String,
        folders: usize,
        messages: usize,
        folder_path: String,
        detail: String,
    },
    /// Mail connection test finished (OK message or error text).
    MailTest(Result<String, String>),
    /// Browser OAuth sign-in finished.
    OAuthSignIn(Result<OAuthSignInResult, String>),
    DeskTasks(Vec<DeskTask>),
    Error(String),
}

pub struct MailboxApp {
    pub(crate) paths: AppPaths,
    pub(crate) config: AppConfig,
    pub(crate) store: Store,
    pub(crate) account_id: String,
    pub(crate) folders: Vec<Folder>,
    pub(crate) envelopes: Vec<Envelope>,
    pub(crate) selected_folder: String,
    /// When set, message list is filtered to this Stak (sender/domain stack).
    pub(crate) selected_stak: Option<String>,
    pub(crate) selected_message: Option<MessageId>,
    pub(crate) open_message: Option<Message>,
    /// Opt-in remote images for the open message (privacy default: off).
    pub(crate) load_remote_images: bool,
    pub(crate) mail_images: crate::mail_reader::MailImageCache,
    /// Cached structured body for the open message (avoids re-parse every frame).
    pub(crate) mail_body_cache: Option<(MessageId, bool, mailbox_core::MailBody)>,
    pub(crate) mail_webview: crate::mail_webview::MailWebView,
    /// Set each frame by the reader when HTML should fill a screen rect.
    pub(crate) mail_webview_req: Option<crate::mail_webview::MailWebViewRequest>,
    pub(crate) search: String,
    /// Request keyboard focus on the top-bar search field (set by `/`).
    pub(crate) focus_search: bool,
    pub(crate) status: String,
    /// Latest Test connection result (shown under the button in Accounts).
    pub(crate) mail_test_feedback: String,
    /// Shown in the empty message list (why there is nothing to read).
    pub(crate) list_empty_hint: String,
    pub(crate) show_settings: bool,
    pub(crate) settings_section: SettingsSection,
    pub(crate) show_help: bool,
    pub(crate) show_calendar: bool,
    pub(crate) show_todo: bool,
    /// Folder rail: icons only when true.
    pub(crate) folders_collapsed: bool,
    /// Mid-column width — owned by us (not egui PanelState).
    pub(crate) list_width: f32,
    /// Left edge of the list panel (for manual resize math).
    pub(crate) list_panel_left: f32,
    pub(crate) todo_line: String,
    pub(crate) todo_detail: String,
    pub(crate) todo_message_id: String,
    pub(crate) calendar: CalendarState,
    pub(crate) ailiak_token_input: String,
    pub(crate) account_name_input: String,
    pub(crate) account_email_input: String,
    pub(crate) mail_protocol: MailProtocol,
    pub(crate) imap_host_input: String,
    pub(crate) mail_port_input: String,
    pub(crate) mail_tls: bool,
    pub(crate) imap_user_input: String,
    pub(crate) imap_password_input: String,
    /// When true, mail password field shows plaintext.
    pub(crate) show_mail_password: bool,
    pub(crate) smtp_host_input: String,
    pub(crate) smtp_port: u16,
    /// Account id currently edited in Settings (may be a new draft).
    pub(crate) editing_account_id: String,
    /// Multi-step add-account flow (`None` = manage existing).
    pub(crate) account_wizard: Option<AccountWizard>,
    pub(crate) ailiak_logo: AiliakLogo,
    pub(crate) avatars: AvatarCache,
    avatar_tx: Sender<AvatarMsg>,
    avatar_rx: Receiver<AvatarMsg>,
    bg_tx: Sender<BgMsg>,
    bg_rx: Receiver<BgMsg>,
}

impl MailboxApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
        // Theme styles installed after config load (below).

        let paths = AppPaths::resolve()?;
        paths.ensure()?;
        let config = AppConfig::load_or_init(&paths)?;
        theme::install(&cc.egui_ctx, config.ui.theme, &config.ui.accent);
        let store = Store::open(&paths.db_file())?;

        if config.ui.use_demo_data && seed_demo_if_empty(&store)? {
            tracing::info!("seeded demo mail into {}", paths.db_file().display());
        }

        let account_id = config
            .primary_account()
            .map(|a| a.id.clone())
            .or_else(|| {
                store
                    .account_ids_with_mail()
                    .ok()
                    .and_then(|v| v.into_iter().next())
            })
            .unwrap_or_else(|| demo_account_id().to_string());

        let status = if config.primary_account().is_some() {
            let proto = config.primary_account().unwrap().protocol.label();
            format!("Ready — Sync to pull mail ({proto})")
        } else {
            "Ready — demo mail, or Add account in Settings".into()
        };

        let (bg_tx, bg_rx) = mpsc::channel();
        let (avatar_tx, avatar_rx) = mpsc::channel();
        let accent = theme::resolve_accent(&config.ui.accent).color;
        let dark = theme::resolve_theme_ctx(&cc.egui_ctx, config.ui.theme)
            == egui::Theme::Dark;
        let ailiak_logo = AiliakLogo::load(&cc.egui_ctx, accent, dark);
        let editing_account_id = account_id.clone();

        let mut app = Self {
            paths,
            config,
            store,
            account_id,
            folders: Vec::new(),
            envelopes: Vec::new(),
            selected_folder: "INBOX".into(),
            selected_stak: None,
            selected_message: None,
            open_message: None,
            load_remote_images: false,
            mail_images: crate::mail_reader::MailImageCache::default(),
            mail_body_cache: None,
            mail_webview: crate::mail_webview::MailWebView::default(),
            mail_webview_req: None,
            search: String::new(),
            focus_search: false,
            status,
            mail_test_feedback: String::new(),
            list_empty_hint: String::new(),
            show_settings: false,
            settings_section: SettingsSection::Appearance,
            show_help: false,
            show_calendar: false,
            show_todo: false,
            folders_collapsed: false,
            list_width: 340.0,
            list_panel_left: 0.0,
            todo_line: String::new(),
            todo_detail: String::new(),
            todo_message_id: String::new(),
            calendar: CalendarState::default(),
            ailiak_token_input: String::new(),
            account_name_input: String::new(),
            account_email_input: String::new(),
            mail_protocol: MailProtocol::Imap,
            imap_host_input: String::new(),
            mail_port_input: "993".into(),
            mail_tls: true,
            imap_user_input: String::new(),
            imap_password_input: String::new(),
            show_mail_password: false,
            smtp_host_input: String::new(),
            smtp_port: 587,
            editing_account_id,
            account_wizard: None,
            ailiak_logo,
            avatars: AvatarCache::default(),
            avatar_tx,
            avatar_rx,
            bg_tx,
            bg_rx,
        };
        app.reload_settings_fields();
        app.refresh_folders()?;
        app.refresh_envelopes()?;
        if let Some(first) = app.envelopes.first().map(|e| e.id.clone()) {
            app.open_id(&first)?;
        }
        if app.config.ailiak.enabled {
            app.refresh_desk_tasks();
        }
        Ok(app)
    }

    pub(crate) fn apply_provider_preset(&mut self, provider: MailProvider) {
        let email = self.account_email_input.trim().to_string();
        let draft = MailAccountConfig::from_provider(provider, email.clone());
        let preset = provider.preset();
        self.mail_protocol = preset.protocol;
        self.imap_host_input = draft.imap_host;
        self.mail_port_input = draft.imap_port.to_string();
        self.mail_tls = draft.imap_tls;
        self.smtp_host_input = draft.smtp_host;
        self.smtp_port = draft.smtp_port;
        if self.account_name_input.trim().is_empty() {
            self.account_name_input = preset.label.to_string();
        }
        if self.imap_user_input.trim().is_empty() && !email.is_empty() {
            self.imap_user_input = email;
        }
        self.status = format!("{} defaults ready — enter email and password", preset.label);
    }

    pub(crate) fn wizard_select_provider(&mut self, provider: MailProvider) {
        self.apply_provider_preset(provider);
        if let Some(wiz) = self.account_wizard.as_mut() {
            wiz.provider = Some(provider);
            wiz.step = AccountWizardStep::Credentials;
        }
    }

    pub(crate) fn cancel_account_wizard(&mut self) {
        self.account_wizard = None;
        self.show_mail_password = false;
        self.reload_settings_fields();
        self.status = "Account setup cancelled".into();
    }

    fn fresh_account_id(email: &str) -> String {
        let base = email
            .split('@')
            .next()
            .unwrap_or("account")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(12)
            .collect::<String>()
            .to_lowercase();
        let base = if base.is_empty() {
            "account".to_string()
        } else {
            base
        };
        let suffix = &uuid::Uuid::new_v4().to_string()[..8];
        format!("{base}-{suffix}")
    }

    pub(crate) fn selected_account(&self) -> Option<&MailAccountConfig> {
        self.config
            .accounts
            .iter()
            .find(|a| a.id == self.account_id)
    }

    fn account_by_id(&self, id: &str) -> Option<&MailAccountConfig> {
        self.config.accounts.iter().find(|a| a.id == id)
    }

    fn current_account_draft(&self) -> Option<MailAccountConfig> {
        let email = self.account_email_input.trim();
        if email.is_empty() && self.imap_host_input.trim().is_empty() {
            return self
                .account_by_id(&self.editing_account_id)
                .cloned()
                .or_else(|| self.selected_account().cloned())
                .or_else(|| self.config.primary_account().cloned());
        }
        let id = if !self.editing_account_id.is_empty() {
            self.editing_account_id.clone()
        } else if let Some(a) = self.selected_account() {
            a.id.clone()
        } else {
            Self::fresh_account_id(email)
        };
        let port = self
            .mail_port_input
            .trim()
            .parse::<u16>()
            .unwrap_or(match (self.mail_protocol, self.mail_tls) {
                (MailProtocol::Pop3, true) => 995,
                (MailProtocol::Pop3, false) => 110,
                (MailProtocol::Imap, true) => 993,
                (MailProtocol::Imap, false) => 143,
            });
        let auth = self
            .config
            .account_by_id(&id)
            .map(|a| a.auth)
            .unwrap_or(MailAuthMethod::Password);
        Some(MailAccountConfig {
            id,
            name: if self.account_name_input.trim().is_empty() {
                email.to_string()
            } else {
                self.account_name_input.trim().to_string()
            },
            email: email.to_string(),
            protocol: self.mail_protocol,
            imap_host: self.imap_host_input.trim().to_string(),
            imap_port: port,
            imap_tls: self.mail_tls,
            smtp_host: if self.smtp_host_input.trim().is_empty() {
                self.imap_host_input.trim().to_string()
            } else {
                self.smtp_host_input.trim().to_string()
            },
            smtp_port: if self.smtp_port == 0 { 587 } else { self.smtp_port },
            smtp_tls: true,
            username: if self.imap_user_input.trim().is_empty() {
                email.to_string()
            } else {
                self.imap_user_input.trim().to_string()
            },
            auth,
        })
    }

    pub(crate) fn oauth_ready(&self, provider: OAuthProvider) -> bool {
        oauth_clients::creds_ready(provider, &self.config.oauth)
    }

    /// Built-in AiliakMail app credentials are present (no Client ID paste needed).
    pub(crate) fn oauth_app_embedded(&self, provider: OAuthProvider) -> bool {
        oauth_clients::has_embedded_app(provider)
    }

    fn resolve_mail_credential(
        &self,
        account: &MailAccountConfig,
    ) -> Result<MailCredential, String> {
        match account.auth {
            MailAuthMethod::Password => Ok(MailCredential::Password(
                self.resolve_mail_password(&account.id)?,
            )),
            auth if auth.is_oauth() => {
                let oauth = OAuthProvider::from_auth_method(auth).ok_or_else(|| {
                    "Unknown OAuth provider on this account".to_string()
                })?;
                let creds = oauth_clients::resolve_creds(oauth, &self.config.oauth).ok_or_else(
                    || {
                        format!(
                            "{} app authorization is not configured in this build",
                            oauth.label()
                        )
                    },
                )?;
                oauth::ensure_access_token(&account.id, oauth, &creds)
                    .map(|access_token| MailCredential::OAuth2 { access_token })
                    .map_err(|e| e.to_string())
            }
            _ => Err("Unsupported auth method".into()),
        }
    }

    /// Open browser app authorization (handles 2FA there) and finish account setup on success.
    pub(crate) fn start_oauth_sign_in(&mut self, mail_provider: MailProvider) {
        let Some(oauth) = OAuthProvider::from_mail_provider(mail_provider) else {
            let msg = "This provider does not support browser authorization — use an app password."
                .to_string();
            self.status = msg.clone();
            self.mail_test_feedback = msg;
            return;
        };
        let Some(creds) = oauth_clients::resolve_creds(oauth, &self.config.oauth) else {
            let msg = if oauth_clients::has_embedded_app(oauth) {
                format!("{} credentials missing — check config overrides.", oauth.label())
            } else {
                format!(
                    "AiliakMail is not registered with {} in this build. Use an app password, or ask the maintainer to embed OAuth app credentials (docs/oauth-apps.md).",
                    oauth.label()
                )
            };
            self.status = msg.clone();
            self.mail_test_feedback = msg;
            return;
        };
        if oauth.needs_client_secret()
            && creds
                .client_secret
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            let msg = format!(
                "AiliakMail {} app secret is missing in this build — use an app password for now.",
                oauth.label()
            );
            self.status = msg.clone();
            self.mail_test_feedback = msg;
            return;
        }
        let tx = self.bg_tx.clone();
        let pending = format!(
            "Authorize AiliakMail with {} in your browser…",
            oauth.label()
        );
        self.status = pending.clone();
        self.mail_test_feedback = pending;
        thread::spawn(move || {
            let result =
                oauth::oauth_sign_in(oauth, mail_provider, &creds).map_err(|e| e.to_string());
            let _ = tx.send(BgMsg::OAuthSignIn(result));
        });
    }

    fn apply_oauth_sign_in(&mut self, result: OAuthSignInResult) {
        let email = result.email.trim().to_string();
        let id = self
            .config
            .accounts
            .iter()
            .find(|a| a.email.eq_ignore_ascii_case(&email))
            .map(|a| a.id.clone())
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| Self::fresh_account_id(&email));
        if let Err(e) = oauth::store_oauth_tokens(&id, &result.tokens) {
            let msg = format!("Could not store OAuth tokens: {e}");
            self.status = msg.clone();
            self.mail_test_feedback = msg;
            return;
        }
        if let Some(url) = result.picture_url.as_deref() {
            match AvatarCache::set_from_url(&self.paths.avatars_dir(), &email, url) {
                Ok(()) => self.avatars.invalidate(&email),
                Err(e) => tracing::warn!(error = %e, "could not fetch OAuth profile photo"),
            }
        }
        let existing_name = self
            .config
            .account_by_id(&id)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        let mut account = MailAccountConfig::from_provider(result.mail_provider, email.clone());
        account.id = id.clone();
        account.name = if !self.account_name_input.trim().is_empty() {
            self.account_name_input.trim().to_string()
        } else if !existing_name.is_empty() {
            existing_name
        } else {
            result.name.clone()
        };
        account.auth = result.oauth.auth_method();
        self.account_name_input = account.name.clone();
        self.account_email_input = email;
        self.imap_user_input = account.username.clone();
        self.imap_host_input = account.imap_host.clone();
        self.smtp_host_input = account.smtp_host.clone();
        self.mail_port_input = account.imap_port.to_string();
        self.mail_tls = account.imap_tls;
        self.mail_protocol = account.protocol;
        self.imap_password_input.clear();
        self.config.upsert_account(account);
        self.config.ui.use_demo_data = false;
        self.account_id = id.clone();
        self.editing_account_id = id;
        self.account_wizard = None;
        if let Err(e) = self.config.save(&self.paths.config_file()) {
            self.status = format!("Signed in but config save failed: {e}");
            self.mail_test_feedback = self.status.clone();
            return;
        }
        let _ = self.refresh_folders();
        let _ = self.refresh_envelopes();
        let msg = format!(
            "Signed in as {} via {} — syncing mail…",
            result.tokens.email,
            result.oauth.label()
        );
        self.status = msg.clone();
        self.mail_test_feedback = msg;
        // Pull folders + Inbox immediately so sign-in feels complete.
        self.start_sync(true);
    }

    pub(crate) fn switch_account(&mut self, id: &str) {
        if self.account_id == id {
            self.editing_account_id = id.to_string();
            self.reload_settings_fields();
            return;
        }
        self.account_id = id.to_string();
        self.editing_account_id = id.to_string();
        self.selected_stak = None;
        self.selected_message = None;
        self.open_message = None;
        self.mail_webview.destroy();
        self.mail_webview_req = None;
        self.search.clear();
        self.reload_settings_fields();
        if let Err(e) = self.refresh_folders() {
            self.status = e.to_string();
            self.list_empty_hint = e.to_string();
        } else if let Err(e) = self.refresh_envelopes() {
            self.status = e.to_string();
            self.list_empty_hint = e.to_string();
        } else if let Some(first) = self.envelopes.first().map(|e| e.id.clone()) {
            self.list_empty_hint.clear();
            let _ = self.open_id(&first);
        } else {
            let proto = self
                .selected_account()
                .map(|a| a.protocol.label())
                .unwrap_or("Mail");
            self.list_empty_hint = if self.folders.is_empty() {
                format!(
                    "No mail downloaded yet for this account. Press Sync (g) to pull from the {proto} server."
                )
            } else {
                format!(
                    "Inbox is empty in the local cache. Press Sync (g) to refresh from the {proto} server."
                )
            };
        }
        let label = self
            .selected_account()
            .map(|a| {
                if a.name.is_empty() {
                    a.email.clone()
                } else {
                    a.name.clone()
                }
            })
            .unwrap_or_else(|| id.to_string());
        self.status = format!("Switched to {label}");
    }

    pub(crate) fn open_settings(&mut self, section: SettingsSection) {
        self.settings_section = section;
        self.show_settings = true;
        self.show_help = false;
        self.show_todo = false;
    }

    pub(crate) fn start_new_account(&mut self) {
        self.editing_account_id = Self::fresh_account_id("");
        self.account_name_input.clear();
        self.account_email_input.clear();
        self.mail_protocol = MailProtocol::Imap;
        self.imap_host_input.clear();
        self.mail_port_input = "993".into();
        self.mail_tls = true;
        self.imap_user_input.clear();
        self.imap_password_input.clear();
        self.show_mail_password = false;
        self.mail_test_feedback.clear();
        self.smtp_host_input.clear();
        self.account_wizard = Some(AccountWizard::default());
        self.open_settings(SettingsSection::Accounts);
        self.status = "Add account — choose a provider".into();
    }

    fn resolve_mail_password(&self, account_id: &str) -> Result<String, String> {
        // Strip whitespace so Google/Yahoo/Apple app passwords pasted with spaces still work.
        let normalize = |s: &str| -> String {
            s.chars().filter(|c| !c.is_whitespace()).collect()
        };
        if !self.imap_password_input.is_empty() {
            return Ok(normalize(&self.imap_password_input));
        }
        match secrets::get_mail_password(account_id) {
            Ok(Some(p)) => Ok(normalize(&p)),
            Ok(None) => Err(
                "No password in keyring — paste your mail password in Settings".into(),
            ),
            Err(e) => Err(format!("Keyring: {e}")),
        }
    }

    fn friendly_mail_test_error(raw: &str) -> String {
        let lower = raw.to_ascii_lowercase();
        if lower.contains("application-specific password") || lower.contains("app password") {
            return "Gmail needs a Google App Password (16 characters), not your normal password. \
Turn on 2-Step Verification, then create an App Password at https://myaccount.google.com/apppasswords \
and paste it here."
                .into();
        }
        format!("Error: {raw}")
    }

    pub(crate) fn test_mail_connection(&mut self) {
        let Some(account) = self.current_account_draft() else {
            let msg = "Enter email and server host first".to_string();
            self.status = msg.clone();
            self.mail_test_feedback = msg;
            return;
        };
        let credential = match self.resolve_mail_credential(&account) {
            Ok(c) => c,
            Err(e) => {
                self.status = e.clone();
                self.mail_test_feedback = e;
                return;
            }
        };
        let tx = self.bg_tx.clone();
        let label = if account.email.is_empty() {
            account.imap_host.clone()
        } else {
            account.email.clone()
        };
        let proto = account.protocol.label();
        let pending = format!("Testing {proto} login for {label}…");
        self.status = pending.clone();
        self.mail_test_feedback = pending;
        thread::spawn(move || match test_connection(&account, &credential) {
            Ok(detail) => {
                let _ = tx.send(BgMsg::MailTest(Ok(format!("{detail} · {label}"))));
            }
            Err(e) => {
                let _ = tx.send(BgMsg::MailTest(Err(e.to_string())));
            }
        });
    }

    /// Sync mail. `sync_all` downloads every IMAP folder (POP3 is always Inbox only).
    pub(crate) fn start_sync(&mut self, sync_all: bool) {
        let Some(account) = self
            .selected_account()
            .cloned()
            .or_else(|| self.current_account_draft())
        else {
            self.status = "Add a mail account in Settings first".into();
            self.open_settings(SettingsSection::Accounts);
            return;
        };
        let credential = match self.resolve_mail_credential(&account) {
            Ok(c) => c,
            Err(e) => {
                self.status = e;
                self.open_settings(SettingsSection::Accounts);
                return;
            }
        };

        // Persist password if typed this session (password auth only)
        if account.auth == MailAuthMethod::Password && !self.imap_password_input.is_empty() {
            if let MailCredential::Password(ref password) = credential {
                if let Err(e) = secrets::set_mail_password(&account.id, password) {
                    self.status = format!("Keyring: {e}");
                    return;
                }
            }
            self.imap_password_input.clear();
        }

        let db_path = self.paths.db_file();
        let folder = self.selected_folder.clone();
        let tx = self.bg_tx.clone();
        let is_pop3 = account.protocol == MailProtocol::Pop3;
        self.status = if is_pop3 {
            "Syncing Inbox (POP3)…".into()
        } else if sync_all {
            "Syncing all folders…".into()
        } else {
            format!("Syncing {}…", folder)
        };
        thread::spawn(move || {
            let result = (|| -> Result<_> {
                let store = Store::open(&db_path)?;
                let report = sync_account(
                    &store,
                    &account,
                    &credential,
                    SyncOptions {
                        max_messages: 40,
                        folder_path: folder,
                        sync_all_folders: sync_all,
                    },
                )?;
                Ok((account.id, report))
            })();
            match result {
                Ok((account_id, report)) => {
                    let _ = tx.send(BgMsg::SyncOk {
                        account_id,
                        folders: report.folders,
                        messages: report.messages,
                        folder_path: report.folder_path,
                        detail: report.detail,
                    });
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::Error(e.to_string()));
                }
            }
        });
    }

    pub(crate) fn set_folder_hidden(&mut self, path: &str, hidden: bool) {
        if let Err(e) = self.store.set_folder_hidden(&self.account_id, path, hidden) {
            self.status = e.to_string();
            return;
        }
        if hidden && self.selected_folder == path {
            if let Ok(visible) = self.store.list_folders(&self.account_id) {
                self.selected_folder = visible
                    .first()
                    .map(|f| f.path.clone())
                    .unwrap_or_else(|| "INBOX".into());
            }
        }
        if let Err(e) = self.refresh_folders() {
            self.status = e.to_string();
        } else if let Err(e) = self.refresh_envelopes() {
            self.status = e.to_string();
        }
    }

    pub(crate) fn reload_settings_fields(&mut self) {
        let account = self
            .account_by_id(&self.editing_account_id)
            .or_else(|| self.selected_account())
            .or_else(|| self.config.primary_account())
            .cloned();
        if let Some(a) = account {
            self.editing_account_id = a.id;
            self.account_name_input = a.name;
            self.account_email_input = a.email;
            self.mail_protocol = a.protocol;
            self.imap_host_input = a.imap_host;
            self.mail_port_input = a.imap_port.to_string();
            self.mail_tls = a.imap_tls;
            self.imap_user_input = a.username;
            self.smtp_host_input = a.smtp_host;
            self.smtp_port = a.smtp_port;
        }
        self.ailiak_token_input.clear();
    }

    fn refresh_folders(&mut self) -> Result<()> {
        self.folders = self.store.list_folders(&self.account_id)?;
        if self.folders.is_empty() {
            self.selected_folder = "INBOX".into();
        } else if !self.folders.iter().any(|f| f.path == self.selected_folder) {
            self.selected_folder = self.folders[0].path.clone();
        }
        Ok(())
    }

    pub(crate) fn refresh_envelopes(&mut self) -> Result<()> {
        self.envelopes = if self.search.trim().is_empty() {
            self.store
                .list_envelopes(&self.account_id, &self.selected_folder)?
        } else {
            self.store
                .search_local(&self.account_id, self.search.trim())?
        };
        Ok(())
    }

    pub(crate) fn open_id(&mut self, id: &MessageId) -> Result<()> {
        // Same message already open — skip SQLite + list reload (fast click path).
        if self.selected_message.as_ref() == Some(id) && self.open_message.is_some() {
            return Ok(());
        }

        if let Some(mut msg) = self.store.get_message(id)? {
            if !msg.envelope.seen {
                self.store.mark_seen(id, true)?;
                msg.envelope.seen = true;
                // Update list row in place — avoid reloading the whole folder.
                if let Some(env) = self.envelopes.iter_mut().find(|e| e.id == *id) {
                    env.seen = true;
                }
            }
            self.load_remote_images = false;
            self.mail_images.clear();
            self.mail_body_cache = None;
            self.mail_webview_req = None;
            self.mail_webview.last_error = None; // retry create on next HTML message
            self.selected_message = Some(id.clone());
            self.open_message = Some(msg);
        }
        Ok(())
    }

    /// Structured body for the reader; rebuilt only when message or image opt-in changes.
    pub(crate) fn cached_mail_body(&mut self) -> Option<&mailbox_core::MailBody> {
        let msg = self.open_message.as_ref()?;
        let id = msg.envelope.id.clone();
        let load = self.load_remote_images;
        let hit = matches!(
            &self.mail_body_cache,
            Some((cid, cload, _)) if *cid == id && *cload == load
        );
        if !hit {
            let body = mailbox_core::build_mail_body(&msg.body_text, &msg.body_html, load);
            self.mail_body_cache = Some((id, load, body));
        }
        self.mail_body_cache.as_ref().map(|(_, _, b)| b)
    }

    fn poll_bg(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                BgMsg::AiliakOk { task_id, for_date } => {
                    self.show_todo = false;
                    let day = for_date.unwrap_or_else(|| "today".into());
                    self.status = format!("Todo created {task_id} · Desk {day}");
                    self.refresh_desk_tasks();
                }
                BgMsg::WhoAmI(s) => {
                    self.status = format!("Ailiak: signed in as {s}");
                }
                BgMsg::MailTest(Ok(s)) => {
                    self.status = s.clone();
                    self.mail_test_feedback = s;
                }
                BgMsg::MailTest(Err(e)) => {
                    let msg = Self::friendly_mail_test_error(&e);
                    self.status = msg.clone();
                    self.mail_test_feedback = msg;
                }
                BgMsg::OAuthSignIn(Ok(result)) => {
                    self.apply_oauth_sign_in(result);
                }
                BgMsg::OAuthSignIn(Err(e)) => {
                    let msg = format!("Sign-in: {e}");
                    self.status = msg.clone();
                    self.mail_test_feedback = msg;
                }
                BgMsg::SyncOk {
                    account_id,
                    folders,
                    messages,
                    folder_path,
                    detail,
                } => {
                    self.account_id = account_id;
                    self.selected_folder = folder_path.clone();
                    self.status = if detail.is_empty() {
                        format!(
                            "Synced {messages} messages from {folder_path} ({folders} folders)"
                        )
                    } else {
                        format!("Synced {messages} from {folder_path} — {detail}")
                    };
                    self.list_empty_hint = if messages == 0 {
                        if detail.is_empty() {
                            format!(
                                "No messages in {folder_path}. Press Sync (g) again, or check Test connection in Settings."
                            )
                        } else {
                            detail
                        }
                    } else {
                        String::new()
                    };
                    if let Err(e) = self.refresh_folders() {
                        self.status = e.to_string();
                        self.list_empty_hint = e.to_string();
                    } else if let Err(e) = self.refresh_envelopes() {
                        self.status = e.to_string();
                        self.list_empty_hint = e.to_string();
                    } else if let Some(first) = self.envelopes.first().map(|e| e.id.clone()) {
                        let _ = self.open_id(&first);
                    }
                }
                BgMsg::DeskTasks(tasks) => {
                    self.calendar.loading = false;
                    self.calendar.tasks = tasks;
                }
                BgMsg::Error(e) => {
                    self.calendar.loading = false;
                    let msg = format!("Error: {e}");
                    self.status = msg.clone();
                    self.list_empty_hint = msg;
                }
            }
        }
    }

    fn poll_avatars(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.avatar_rx.try_recv() {
            self.avatars.apply_msg(ctx, msg);
            ctx.request_repaint();
        }
    }

    pub(crate) fn resolve_avatar(
        &mut self,
        ctx: &egui::Context,
        email: &str,
        name: &str,
    ) -> crate::avatars::AvatarView {
        self.maybe_fetch_google_account_photo(email);
        self.avatars.resolve(
            ctx,
            &self.paths.avatars_dir(),
            email,
            name,
            &self.avatar_tx,
        )
    }

    /// If this is a Google OAuth account with no local photo yet, pull Google’s profile picture.
    fn maybe_fetch_google_account_photo(&mut self, email: &str) {
        let key = AvatarCache::normalize_email(email);
        if key.is_empty() {
            return;
        }
        let local = AvatarCache::local_path(&self.paths.avatars_dir(), &key);
        if local.exists() || self.avatars.has_image(&key) {
            return;
        }
        let Some(account) = self
            .config
            .accounts
            .iter()
            .find(|a| AvatarCache::normalize_email(&a.email) == key)
            .cloned()
        else {
            return;
        };
        if account.auth != MailAuthMethod::GoogleOAuth {
            return;
        }
        if !self.avatars.begin_oauth_photo_try(&key) {
            return;
        }
        let Ok(cred) = self.resolve_mail_credential(&account) else {
            return;
        };
        let MailCredential::OAuth2 { access_token } = cred else {
            return;
        };
        let dir = self.paths.avatars_dir();
        let tx = self.avatar_tx.clone();
        thread::spawn(move || {
            match oauth::fetch_google_picture_url(&access_token) {
                Ok(Some(url)) => AvatarCache::spawn_url_fetch(key, url, dir, tx),
                Ok(None) | Err(_) => {
                    let _ = tx.send(AvatarMsg::Miss { email: key });
                }
            }
        });
    }

    pub(crate) fn choose_account_avatar(&mut self) {
        let email = self.account_email_input.trim().to_string();
        if email.is_empty() {
            self.status = "Set the account email before choosing a photo".into();
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg"])
            .pick_file()
        else {
            return;
        };
        match AvatarCache::set_local_override(&self.paths.avatars_dir(), &email, &path) {
            Ok(()) => {
                self.avatars.invalidate(&email);
                self.status = "Account photo saved".into();
            }
            Err(e) => {
                self.status = format!("Avatar: {e}");
            }
        }
    }

    pub(crate) fn clear_account_avatar(&mut self) {
        let email = self.account_email_input.trim().to_string();
        if email.is_empty() {
            self.status = "No email to clear photo for".into();
            return;
        }
        match AvatarCache::clear_local_override(&self.paths.avatars_dir(), &email) {
            Ok(()) => {
                self.avatars.invalidate(&email);
                self.status = "Account photo cleared (Gravatar / initials)".into();
            }
            Err(e) => {
                self.status = format!("Avatar: {e}");
            }
        }
    }

    /// Todo and Calendar share one right-side panel — opening one closes the other.
    pub(crate) fn open_calendar_panel(&mut self) {
        self.show_todo = false;
        self.show_calendar = true;
        self.refresh_desk_tasks();
    }

    pub(crate) fn toggle_calendar(&mut self) {
        if self.show_calendar {
            self.show_calendar = false;
        } else {
            self.open_calendar_panel();
        }
    }

    /// Toggle the Todo side panel (replaces Calendar when opening).
    pub(crate) fn toggle_todo(&mut self) {
        if self.show_todo {
            self.show_todo = false;
            return;
        }
        self.open_todo();
    }

    pub(crate) fn open_todo(&mut self) {
        let Some(msg) = self.open_message.clone() else {
            self.status = "Open a message first".into();
            return;
        };
        if !self.config.ailiak.enabled {
            self.status = "Enable Ailiak in Settings first".into();
            self.open_settings(SettingsSection::Ailiak);
            return;
        }
        match self.store.ailiak_task_for_message(&msg.envelope.message_id) {
            Ok(Some(existing)) => {
                self.status = format!("Already on Desk as {existing}");
                return;
            }
            Ok(None) => {}
            Err(e) => {
                self.status = format!("Dedup check failed: {e}");
                return;
            }
        }
        let (line, detail) = todo_defaults(&msg, &self.config.ailiak);
        self.todo_line = line;
        self.todo_detail = detail;
        self.todo_message_id = msg.envelope.message_id.clone();
        self.show_calendar = false;
        self.show_todo = true;
    }

    pub(crate) fn submit_todo(&mut self) {
        if self.todo_message_id.is_empty() {
            self.status = "No message linked for Todo".into();
            return;
        }
        let token = match secrets::get_ailiak_token() {
            Ok(Some(t)) => t,
            Ok(None) => {
                self.status = "No Ailiak token — add one in Settings".into();
                self.open_settings(SettingsSection::Ailiak);
                return;
            }
            Err(e) => {
                self.status = format!("Keyring: {e}");
                return;
            }
        };

        let cfg = self.config.ailiak.clone();
        let base = cfg.base_url.clone();
        let message_id = self.todo_message_id.clone();
        let line = self.todo_line.clone();
        let detail = self.todo_detail.clone();
        let ui_day = self.calendar.todo_for_date();
        let task_number = Some(short_task_number(&message_id));
        let payload = build_payload(&line, &detail, ui_day, task_number, &cfg);
        let for_date_status = payload.for_date.clone();
        let tx = self.bg_tx.clone();
        let db_path = self.paths.db_file();

        self.status = "Creating Todo on Desk…".into();
        thread::spawn(move || {
            let result = (|| -> Result<(String, Option<String>)> {
                let mut client = AiliakClient::new(base, token)?;
                let created = client.create_task(&payload)?;
                let store = Store::open(&db_path)?;
                store.record_ailiak_link(&message_id, &created.id)?;
                Ok((created.id, for_date_status))
            })();
            match result {
                Ok((task_id, for_date)) => {
                    let _ = tx.send(BgMsg::AiliakOk { task_id, for_date });
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::Error(e.to_string()));
                }
            }
        });
    }

    pub(crate) fn refresh_desk_tasks(&mut self) {
        if !self.config.ailiak.enabled {
            return;
        }
        let token = match secrets::get_ailiak_token() {
            Ok(Some(t)) => t,
            _ => return,
        };
        let base = self.config.ailiak.base_url.clone();
        let tag = self.config.ailiak.hashtag.clone();
        let tx = self.bg_tx.clone();
        self.calendar.loading = true;
        thread::spawn(move || {
            let result = (|| -> Result<Vec<DeskTask>> {
                let mut client = AiliakClient::new(base, token)?;
                // Prefer #mail tasks; fall back to all open if empty.
                let tagged = client.list_open_tasks(None, Some(tag.trim_start_matches('#')))?;
                if tagged.is_empty() {
                    client.list_open_tasks(None, None)
                } else {
                    Ok(tagged)
                }
            })();
            match result {
                Ok(tasks) => {
                    let _ = tx.send(BgMsg::DeskTasks(tasks));
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::Error(e.to_string()));
                }
            }
        });
    }

    pub(crate) fn test_ailiak(&mut self) {
        let token = if !self.ailiak_token_input.trim().is_empty() {
            self.ailiak_token_input.trim().to_string()
        } else {
            match secrets::get_ailiak_token() {
                Ok(Some(t)) => t,
                Ok(None) => {
                    self.status = "Paste an ailiak_… token first".into();
                    return;
                }
                Err(e) => {
                    self.status = format!("Keyring: {e}");
                    return;
                }
            }
        };
        let base = self.config.ailiak.base_url.clone();
        let tx = self.bg_tx.clone();
        self.status = "Testing Ailiak…".into();
        thread::spawn(move || {
            let result = (|| -> Result<String> {
                let mut client = AiliakClient::new(base, token)?;
                let who = client.whoami()?;
                Ok(who.display)
            })();
            match result {
                Ok(display) => {
                    let _ = tx.send(BgMsg::WhoAmI(display));
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::Error(e.to_string()));
                }
            }
        });
    }

    pub(crate) fn save_settings(&mut self) -> Result<()> {
        let mut should_sync = false;
        if let Some(mut account) = self.current_account_draft() {
            if !account.imap_host.is_empty()
                && (!account.email.is_empty() || !account.username.is_empty())
            {
                if account.id.is_empty() {
                    account.id = Self::fresh_account_id(&account.email);
                }
                let id = account.id.clone();
                if !self.imap_password_input.is_empty() {
                    let password = self.imap_password_input.clone();
                    secrets::set_mail_password(&id, &password)?;
                    // Confirm we can read it back before clearing the field.
                    if !secrets::has_mail_password(&id) {
                        bail!(
                            "Password could not be stored securely — sync would fail. \
Try again, or check that a keyring/secret-service is running."
                        );
                    }
                    self.imap_password_input.clear();
                    account.auth = MailAuthMethod::Password;
                    should_sync = true;
                } else if account.auth.is_oauth() || secrets::has_mail_password(&id) {
                    should_sync = true;
                }
                self.config.upsert_account(account);
                self.account_id = id.clone();
                self.editing_account_id = id;
                self.config.ui.use_demo_data = false;
                let _ = self.refresh_folders();
                let _ = self.refresh_envelopes();
            }
        }

        if !self.ailiak_token_input.trim().is_empty() {
            secrets::set_ailiak_token(self.ailiak_token_input.trim())?;
            self.config.ailiak.enabled = true;
            self.ailiak_token_input.clear();
        }

        self.config.save(&self.paths.config_file())?;
        if should_sync {
            self.status = "Account saved — syncing mail…".into();
            self.list_empty_hint = "Syncing…".into();
            self.start_sync(true);
        } else {
            self.status = format!("Saved {}", self.paths.config_file().display());
            if self.envelopes.is_empty() {
                self.list_empty_hint =
                    "Account saved. Add/re-enter the password, then Sync (g) or Test connection."
                        .into();
            }
        }
        Ok(())
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_help;
        egui::Window::new("Help")
            .open(&mut open)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("Keyboard").strong());
                ui.monospace("j / ↓     next message");
                ui.monospace("k / ↑     previous message");
                ui.monospace("/         focus search");
                ui.monospace("g         sync selected folder");
                ui.monospace("Shift+G   sync all folders");
                ui.monospace("a         Todo (mail → Ailiak)");
                ui.monospace("c         calendar (Desk days)");
                ui.monospace(",         settings page");
                ui.monospace("Esc       leave settings");
                ui.monospace("?         this help");
                ui.add_space(8.0);
                ui.label(RichText::new("Todo / slash").strong());
                ui.label(
                    "In the Todo panel: /from tomorrow, /todo, /estimate 1h, /spent 30m, /done, #mail",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Gmail / POP3").strong());
                ui.label(
                    "Settings → Accounts → Add account → pick Gmail / Outlook / … → email & password.",
                );
                ui.label(
                    "Custom servers: choose IMAP or POP3, then enter host with email & password.",
                );
                ui.label(
                    "Folders: Sync lists IMAP mailboxes; Settings → Folders to hide ones you don’t need. POP3 is Inbox only.",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Theme").strong());
                ui.label("Settings → Appearance: System (default), Light, or Dark.");
                ui.add_space(8.0);
                ui.label(RichText::new("Principles").strong());
                ui.label("Mail stays in AiliakMail. Ailiak is the desk for work that needs tracking.");
            });
        self.show_help = open;
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.show_settings {
            let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            if escape && !ctx.wants_keyboard_input() {
                self.show_settings = false;
            }
            return;
        }
        if self.show_help {
            return;
        }

        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escape && !ctx.wants_keyboard_input() && (self.show_todo || self.show_calendar) {
            self.show_todo = false;
            self.show_calendar = false;
            return;
        }

        let slash = ctx.input(|i| {
            i.key_pressed(egui::Key::Slash)
                && !i.modifiers.shift
                && !i.modifiers.ctrl
                && !i.modifiers.alt
                && !i.modifiers.command
        });
        if slash && !ctx.wants_keyboard_input() {
            self.focus_search = true;
            return;
        }

        // Don't steal j/k/g/… while typing in search (or any other field).
        if ctx.wants_keyboard_input() {
            return;
        }

        ctx.input(|i| {
            if i.modifiers.shift && i.key_pressed(egui::Key::Slash) {
                self.show_help = true;
            }
            if i.key_pressed(egui::Key::Comma) {
                self.open_settings(SettingsSection::Appearance);
            }
            if i.key_pressed(egui::Key::A) && !i.modifiers.any() {
                self.toggle_todo();
            }
            if i.key_pressed(egui::Key::C) && !i.modifiers.any() {
                self.toggle_calendar();
            }
            if i.key_pressed(egui::Key::G) {
                self.start_sync(i.modifiers.shift);
            }
        });

        let delta = ctx.input(|i| {
            if i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown) {
                Some(1isize)
            } else if i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp) {
                Some(-1)
            } else {
                None
            }
        });
        if let Some(delta) = delta {
            if self.envelopes.is_empty() {
                return;
            }
            let cur = self
                .selected_message
                .as_ref()
                .and_then(|id| self.envelopes.iter().position(|e| &e.id == id))
                .unwrap_or(0);
            let next = (cur as isize + delta).clamp(0, self.envelopes.len() as isize - 1) as usize;
            let id = self.envelopes[next].id.clone();
            let _ = self.open_id(&id);
        }
    }
}

impl App for MailboxApp {
    /// eframe defaults to a dark semi-transparent clear color; any hairline gap
    /// between SidePanel and CentralPanel would show as a muddy strip.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll_bg();
        self.poll_avatars(ctx);
        self.mail_images.poll(ctx);
        crate::mail_webview::MailWebView::pump_platform();
        theme::sync_system(ctx, self.config.ui.theme);
        self.handle_keys(ctx);

        // Reader sets this while painting; clear first so a missing request hides the view.
        self.mail_webview_req = None;

        if self.show_help {
            self.help_window(ctx);
        }

        self.show_shell(ctx);

        let hide_webview = self.show_settings || self.show_help;
        if hide_webview {
            self.mail_webview.hide();
        } else if let Some(req) = self.mail_webview_req.take() {
            self.mail_webview.show_html(
                frame,
                &req.msg_id,
                &req.body_html,
                req.load_images,
                req.bounds,
            );
        } else {
            self.mail_webview.hide();
        }
        self.mail_webview.poll_links();

        if (!self.status.contains("Ready") && self.status.ends_with('…')) || self.calendar.loading
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}
