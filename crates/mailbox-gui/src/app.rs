use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use eframe::App;
use egui::RichText;
use mailbox_ailiak::{
    build_payload, insert_selection_into_compose, short_task_number, AiliakClient, DeskTask,
};
use mailbox_core::config::{AppConfig, MailAccountConfig, MailAuthMethod, MailProtocol};
use mailbox_core::oauth::{self, open_url, MailCredential, OAuthProvider, OAuthSignInResult};
use mailbox_core::oauth_clients;
use mailbox_core::paths::AppPaths;
use mailbox_core::secrets;
use mailbox_core::seed::{demo_account_id, seed_demo_if_empty};
use mailbox_core::store::Store;
use mailbox_core::sync::{
    mark_flagged_on_server, mark_seen_on_server, move_messages, sync_account, test_connection,
    SyncOptions,
};
use mailbox_core::types::{Envelope, Folder, Message, MessageId, SpecialFolderKind};

use crate::avatars::{AvatarCache, AvatarMsg};
use crate::brand::AiliakLogo;
use crate::busy::BusyFlags;
use crate::calendar::CalendarState;
use crate::logo_dev::LogoCache;
use crate::settings::{AccountWizard, AccountWizardStep, SettingsSection};
use crate::theme;
use mailbox_core::providers::MailProvider;

/// Full Ailiak Desk in the system browser (escape hatch; Options Todo uses the API).
pub(crate) const DESK_URL: &str = "https://ailiak.com";
/// Feedback nav entry → mail composer (To locked to [`FEEDBACK_TO_ADDR`]).
pub(crate) const FEEDBACK_UI_ENABLED: bool = true;
/// Where Feedback compose sends mail.
pub(crate) const FEEDBACK_TO_ADDR: &str = "anzaprianov@gmail.com";
/// Legacy GitHub issue URL (unused — feedback uses SMTP compose).
#[allow(dead_code)]
pub(crate) const FEEDBACK_ISSUES_URL: &str =
    "https://github.com/fire1/ailiak-mail/issues/new";

/// Let the first frames paint before kicking a network sync.
const STARTUP_SYNC_DELAY: Duration = Duration::from_secs(2);
/// Skip startup sync if we already synced within this window (rapid relaunch).
const STARTUP_SYNC_COOLDOWN: Duration = Duration::from_secs(90);

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
        new_messages: usize,
        new_from: String,
        new_subject: String,
        folder_path: String,
        detail: String,
    },
    /// Mail connection test finished (OK message or error text).
    MailTest(Result<String, String>),
    /// Browser OAuth sign-in finished.
    OAuthSignIn(Result<OAuthSignInResult, String>),
    DeskTasks(Vec<DeskTask>),
    /// Archive / Trash finished (count moved).
    MailMoved {
        kind: SpecialFolderKind,
        count: usize,
    },
    /// SMTP send finished.
    SendOk,
    Error(String),
}

/// Mid-list bulk action awaiting an explicit confirm (whole folder/label/list).
#[derive(Clone, Copy)]
pub(crate) enum PendingBulkKind {
    Move(SpecialFolderKind),
    MarkRead,
}

impl PendingBulkKind {
    fn verb(self) -> &'static str {
        match self {
            Self::Move(k) => k.label(),
            Self::MarkRead => "Mark as read",
        }
    }

    fn danger(self) -> bool {
        matches!(self, Self::Move(SpecialFolderKind::Trash))
    }
}

/// Mid-list Archive/Trash/Mark-read awaiting confirm.
#[derive(Clone)]
pub(crate) struct PendingBulkMove {
    pub kind: PendingBulkKind,
    pub ids: Vec<MessageId>,
    /// Human scope, e.g. "Inbox" or "GitHub · Inbox".
    pub scope: String,
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
    /// Cached Staks for the current envelope list (rebuilt on refresh / mark-seen).
    pub(crate) staks_cache: Vec<crate::staks::Stak>,
    /// Selected person email (People chat mode).
    pub(crate) selected_person: Option<String>,
    /// People derived from current envelopes.
    pub(crate) people_cache: Vec<crate::people::Person>,
    /// Messages in the open People conversation (oldest → newest).
    pub(crate) chat_messages: Vec<Message>,
    /// Chat dock reply draft.
    pub(crate) chat_reply: String,
    pub(crate) chat_reply_focused: bool,
    pub(crate) chat_attachments: Vec<crate::compose::ComposeAttachment>,
    /// Floating compose draft (None = closed).
    pub(crate) compose: Option<crate::compose::ComposeDraft>,
    /// Full-card “Message sent” notice after SMTP success (compose form cleared).
    pub(crate) compose_sent: Option<crate::compose::ComposeSentNotice>,
    /// `(name, email)` suggestions for compose To/Cc/Bcc.
    pub(crate) compose_hints: Vec<(String, String)>,
    /// Focus the To field once when the full composer opens.
    pub(crate) compose_autofocus_to: bool,
    /// Focus Subject once (e.g. Feedback compose with locked To).
    pub(crate) compose_autofocus_subject: bool,
    /// Body webview owns keyboard — keep egui from parking focus on To.
    pub(crate) compose_body_focused: bool,
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
    /// Consecutive frames without a mail webview request (debounce hide).
    mail_webview_miss_frames: u8,
    /// WYSIWYG compose body (contenteditable wry view).
    pub(crate) compose_webview: crate::compose_webview::ComposeWebView,
    /// Set each frame while the composer shows an editor rect.
    pub(crate) compose_webview_req: Option<crate::compose_webview::ComposeWebViewRequest>,
    /// Waiting for a definitive HTML flush before SMTP send.
    pending_compose_send: bool,
    /// Frames waited for webview flush (timeout → send with synced draft).
    pending_compose_send_frames: u8,
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
    pub(crate) show_feedback: bool,
    pub(crate) feedback_title: String,
    pub(crate) feedback_body: String,
    /// Reader meta (To / Cc / Date) expanded under From.
    pub(crate) reader_headers_expanded: bool,
    /// Reader subject shown in full (click a truncated title to expand).
    pub(crate) reader_subject_expanded: bool,
    pub(crate) show_calendar: bool,
    pub(crate) show_options: bool,
    /// Last right desk panel was Options (vs Calendar) — used while close-animating.
    pub(crate) desk_was_options: bool,
    /// Folder rail: icons only when true.
    pub(crate) folders_collapsed: bool,
    /// Expanded left-nav width (owned like `list_width`; collapsed uses a fixed rail).
    pub(crate) nav_width: f32,
    /// Nav accordion: Staks (brand/domain stacks) section open.
    pub(crate) staks_section_open: bool,
    /// Nav accordion: People section open.
    pub(crate) people_section_open: bool,
    /// Nav accordion: Folders (system) section open.
    pub(crate) folders_section_open: bool,
    /// Nav accordion: Labels (user) section open.
    pub(crate) labels_section_open: bool,
    /// Expanded parent paths in the Labels tree.
    pub(crate) label_expanded: std::collections::HashSet<String>,
    /// Avatar click → mid-list multi-select (checkboxes on messages).
    pub(crate) list_multi_select: bool,
    /// Message ids checked while `list_multi_select` is on.
    pub(crate) list_selected: std::collections::HashSet<MessageId>,
    /// Confirm before mid-panel Archive/Trash clears a whole list.
    pub(crate) pending_bulk_move: Option<PendingBulkMove>,
    /// Visible-list index where Shift+arrow selection started.
    list_select_anchor: Option<usize>,
    /// Mid-column width — owned by us (not egui PanelState).
    pub(crate) list_width: f32,
    /// Last shell width used to proportion `list_width` (~30% mail list / ≥50% mail body).
    pub(crate) last_shell_width: f32,
    /// Left edge of the list panel (for manual resize math).
    pub(crate) list_panel_left: f32,
    /// Single Ailiak slash-command compose field (Options → Todo).
    pub(crate) todo_compose: String,
    /// Visible row count for the Task textarea (default 5; drag to stretch).
    pub(crate) todo_compose_rows: f32,
    pub(crate) todo_message_id: String,
    /// Per-sender note shown in Options (persisted in SQLite).
    pub(crate) options_note: String,
    pub(crate) options_note_sender: String,
    /// Waiting on mail webview `getSelection()` for Todo insert.
    pub(crate) pending_todo_selection: bool,
    /// Options → Task: slash-command help expanded under the textarea.
    pub(crate) options_task_help_open: bool,
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
    /// Cached "password on file" for the editing account (never query keyring while painting).
    pub(crate) mail_password_saved: bool,
    pub(crate) smtp_host_input: String,
    pub(crate) smtp_port: u16,
    /// Account id currently edited in Settings (may be a new draft).
    pub(crate) editing_account_id: String,
    /// Multi-step add-account flow (`None` = manage existing).
    pub(crate) account_wizard: Option<AccountWizard>,
    pub(crate) ailiak_logo: AiliakLogo,
    pub(crate) avatars: AvatarCache,
    pub(crate) logos: LogoCache,
    avatar_tx: Sender<AvatarMsg>,
    avatar_rx: Receiver<AvatarMsg>,
    bg_tx: Sender<BgMsg>,
    bg_rx: Receiver<BgMsg>,
    /// Toolbar / settings spinners for in-flight work.
    pub(crate) busy: BusyFlags,
    /// Last sync start (manual or auto) — drives the auto-sync timer.
    last_sync_at: Instant,
    /// When set, pull the current folder once after this instant (gentle boot sync).
    startup_sync_at: Option<Instant>,
    /// Webview torn down; send `Close` next frame so wry Drop does not hang.
    exit_after_webview: bool,
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
        let options_open = config.ui.options_open;
        let nav_width = config.ui.nav_width.clamp(140.0, 200.0);

        let mut app = Self {
            paths,
            config,
            store,
            account_id,
            folders: Vec::new(),
            envelopes: Vec::new(),
            selected_folder: "INBOX".into(),
            selected_stak: None,
            staks_cache: Vec::new(),
            selected_person: None,
            people_cache: Vec::new(),
            chat_messages: Vec::new(),
            chat_reply: String::new(),
            chat_reply_focused: false,
            chat_attachments: Vec::new(),
            compose: None,
            compose_sent: None,
            compose_hints: Vec::new(),
            compose_autofocus_to: false,
            compose_autofocus_subject: false,
            compose_body_focused: false,
            selected_message: None,
            open_message: None,
            load_remote_images: false,
            mail_images: crate::mail_reader::MailImageCache::default(),
            mail_body_cache: None,
            mail_webview: crate::mail_webview::MailWebView::default(),
            mail_webview_req: None,
            mail_webview_miss_frames: 0,
            compose_webview: crate::compose_webview::ComposeWebView::default(),
            compose_webview_req: None,
            pending_compose_send: false,
            pending_compose_send_frames: 0,
            search: String::new(),
            focus_search: false,
            status,
            mail_test_feedback: String::new(),
            list_empty_hint: String::new(),
            show_settings: false,
            settings_section: SettingsSection::Appearance,
            show_help: false,
            show_feedback: false,
            feedback_title: String::new(),
            feedback_body: String::new(),
            reader_headers_expanded: false,
            reader_subject_expanded: false,
            show_calendar: false,
            show_options: options_open,
            desk_was_options: options_open,
            folders_collapsed: false,
            nav_width,
            staks_section_open: false,
            people_section_open: false,
            folders_section_open: true,
            labels_section_open: false,
            label_expanded: std::collections::HashSet::new(),
            list_multi_select: false,
            pending_bulk_move: None,
            list_selected: std::collections::HashSet::new(),
            list_select_anchor: None,
            list_width: 340.0,
            last_shell_width: 0.0,
            list_panel_left: 0.0,
            todo_compose: String::new(),
            todo_compose_rows: 5.0,
            todo_message_id: String::new(),
            options_note: String::new(),
            options_note_sender: String::new(),
            pending_todo_selection: false,
            options_task_help_open: false,
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
            mail_password_saved: false,
            smtp_host_input: String::new(),
            smtp_port: 587,
            editing_account_id,
            account_wizard: None,
            ailiak_logo,
            avatars: AvatarCache::default(),
            logos: LogoCache::default(),
            avatar_tx,
            avatar_rx,
            bg_tx,
            bg_rx,
            busy: BusyFlags::default(),
            exit_after_webview: false,
            last_sync_at: Instant::now(),
            startup_sync_at: None,
        };
        app.reload_settings_fields();
        app.refresh_folders()?;
        app.refresh_envelopes()?;
        // Warm SVG textures before Settings → Add account (avoids a long hitch on first open).
        crate::provider_logos::preload(&cc.egui_ctx);
        if let Some(first) = app.envelopes.first().map(|e| e.id.clone()) {
            app.open_id(&first)?;
        }
        if app.config.ailiak.enabled {
            app.refresh_desk_tasks();
        }
        app.schedule_startup_sync();
        Ok(app)
    }

    /// Queue a deferred current-folder sync after launch (not `sync_all`).
    fn schedule_startup_sync(&mut self) {
        if self.config.accounts.is_empty() {
            return;
        }
        if let Some(last) = self.paths.read_last_sync_unix() {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if now.saturating_sub(last) < STARTUP_SYNC_COOLDOWN.as_secs() {
                tracing::debug!(
                    cooldown_secs = STARTUP_SYNC_COOLDOWN.as_secs(),
                    "skipping startup sync — recent sync stamp"
                );
                return;
            }
        }
        self.startup_sync_at = Some(Instant::now() + STARTUP_SYNC_DELAY);
    }

    fn note_sync_stamp(&self) {
        if let Ok(secs) = SystemTime::now().duration_since(UNIX_EPOCH) {
            self.paths.write_last_sync_unix(secs.as_secs());
        }
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

    /// Password typed in Settings this session (if any). OAuth/network resolve runs off-thread.
    fn password_override(&self) -> Option<String> {
        let p = self.imap_password_input.trim();
        if p.is_empty() {
            None
        } else {
            Some(p.to_string())
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
        self.busy.oauth = true;
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
        if let Some(url) = result.picture_url.as_deref().filter(|u| !u.is_empty()) {
            // Off UI thread — HTTP download must not freeze the egui loop.
            AvatarCache::spawn_url_fetch(
                email.clone(),
                url.to_string(),
                self.paths.avatars_dir(),
                self.avatar_tx.clone(),
            );
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

    /// Nav account avatar: switch account (same id just refreshes edit fields).
    pub(crate) fn on_account_avatar_click(&mut self, id: &str) {
        if self.account_id != id {
            self.clear_list_multi_select();
        }
        self.switch_account(id);
    }

    pub(crate) fn toggle_list_multi_select(&mut self) {
        if self.list_multi_select {
            self.clear_list_multi_select();
            if self.status.contains("Select messages") {
                self.status = "Ready".into();
            }
        } else {
            self.list_multi_select = true;
            self.list_selected.clear();
            self.list_select_anchor = None;
            self.status =
                "Select messages — Shift+↑/↓, then Del to trash (Esc to finish)".into();
        }
    }

    pub(crate) fn clear_list_multi_select(&mut self) {
        self.list_multi_select = false;
        self.list_selected.clear();
        self.list_select_anchor = None;
    }

    pub(crate) fn toggle_list_message_selected(&mut self, id: &MessageId) {
        if self.list_selected.contains(id) {
            self.list_selected.remove(id);
        } else {
            self.list_selected.insert(id.clone());
        }
        self.list_select_anchor = None;
        // Nothing checked → leave multi-select so mid-list shows sender pictures again.
        if self.list_multi_select && self.list_selected.is_empty() {
            self.clear_list_multi_select();
            if self.status.contains("Select messages") || self.status.contains("Selected ") {
                self.status = "Ready".into();
            }
        }
    }

    /// Select every message currently shown in the mid list (Ctrl/Cmd+A).
    pub(crate) fn select_all_visible_messages(&mut self) {
        let indices = self.visible_envelope_indices();
        if indices.is_empty() {
            self.status = "No messages to select".into();
            return;
        }
        self.list_multi_select = true;
        self.list_select_anchor = Some(0);
        self.list_selected.clear();
        for &ei in &indices {
            if let Some(e) = self.envelopes.get(ei) {
                self.list_selected.insert(e.id.clone());
            }
        }
        let n = self.list_selected.len();
        self.status = format!("Selected {n} — Del to trash, Esc to clear");
    }

    /// Envelope indices currently shown in the mid list (respects Stak filter).
    fn visible_envelope_indices(&self) -> Vec<usize> {
        if let Some(stak_id) = self.selected_stak.as_deref() {
            self.envelopes
                .iter()
                .enumerate()
                .filter(|(_, e)| crate::staks::envelope_in_stak(e, stak_id))
                .map(|(i, _)| i)
                .collect()
        } else {
            (0..self.envelopes.len()).collect()
        }
    }

    /// Move focus by `delta` rows. With Shift, enter/extend multi-select range.
    fn navigate_list(&mut self, delta: isize, extend: bool) {
        let indices = self.visible_envelope_indices();
        if indices.is_empty() {
            return;
        }
        let cur_vis = self
            .selected_message
            .as_ref()
            .and_then(|id| {
                indices
                    .iter()
                    .position(|&i| self.envelopes.get(i).is_some_and(|e| &e.id == id))
            })
            .unwrap_or(0);
        let next = (cur_vis as isize + delta).clamp(0, indices.len() as isize - 1) as usize;

        if extend {
            if !self.list_multi_select {
                self.list_multi_select = true;
                self.list_selected.clear();
                self.status =
                    "Select messages — Shift+↑/↓, then Del to trash (Esc to finish)".into();
            }
            if self.list_select_anchor.is_none() {
                self.list_select_anchor = Some(cur_vis);
                // Include the starting row in the range.
                if let Some(&ei) = indices.get(cur_vis) {
                    if let Some(e) = self.envelopes.get(ei) {
                        self.list_selected.insert(e.id.clone());
                    }
                }
            }
            let anchor = self.list_select_anchor.unwrap_or(cur_vis);
            let lo = anchor.min(next);
            let hi = anchor.max(next);
            self.list_selected.clear();
            for vis in lo..=hi {
                if let Some(&ei) = indices.get(vis) {
                    if let Some(e) = self.envelopes.get(ei) {
                        self.list_selected.insert(e.id.clone());
                    }
                }
            }
        } else {
            self.list_select_anchor = None;
        }

        let id = self.envelopes[indices[next]].id.clone();
        let _ = self.open_id(&id);
    }

    pub(crate) fn switch_account(&mut self, id: &str) {
        if self.account_id == id {
            self.editing_account_id = id.to_string();
            self.reload_settings_fields();
            return;
        }
        self.clear_list_multi_select();
        self.account_id = id.to_string();
        self.editing_account_id = id.to_string();
        self.selected_stak = None;
        self.selected_message = None;
        self.open_message = None;
        self.label_expanded.clear();
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
        self.show_feedback = false;
        self.close_options_panel();
        self.show_calendar = false;
    }

    /// Open the system browser on a new issue with the feedback form filled in.
    #[allow(dead_code)]
    pub(crate) fn submit_feedback_to_github(&mut self) {
        let title = self.feedback_title.trim();
        let body = self.feedback_body.trim();
        if title.is_empty() && body.is_empty() {
            self.status = "Write a short title or message first".into();
            return;
        }
        let title = if title.is_empty() {
            "Feedback from AiliakMail".to_string()
        } else {
            title.to_string()
        };
        let mut issue_body = String::new();
        issue_body.push_str(body);
        if !issue_body.is_empty() {
            issue_body.push_str("\n\n");
        }
        issue_body.push_str("---\n");
        issue_body.push_str("_Sent from AiliakMail Feedback_\n");
        let url = format!(
            "{FEEDBACK_ISSUES_URL}?title={}&body={}",
            query_encode(&title),
            query_encode(&issue_body)
        );
        match open_url(&url) {
            Ok(()) => {
                self.status = "Opened GitHub to file your feedback".into();
                self.show_feedback = false;
                self.feedback_title.clear();
                self.feedback_body.clear();
            }
            Err(e) => self.status = format!("Could not open browser: {e}"),
        }
    }

    #[allow(dead_code)]
    fn feedback_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_feedback;
        egui::Window::new("Feedback")
            .open(&mut open)
            .default_width(420.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "GitHub has no issue-by-email (unlike GitLab). Your note opens as a new \
                         issue in the browser — sign in to GitHub to submit.",
                    )
                    .small()
                    .color(theme::mute(ui.ctx())),
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Title").strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.feedback_title)
                        .desired_width(f32::INFINITY)
                        .hint_text("Short summary"),
                );
                ui.add_space(6.0);
                ui.label(RichText::new("Message").strong());
                ui.add(
                    egui::TextEdit::multiline(&mut self.feedback_body)
                        .desired_width(f32::INFINITY)
                        .desired_rows(8)
                        .hint_text("What happened? What did you expect?"),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Open GitHub issue").color(egui::Color32::WHITE),
                            )
                            .fill(theme::accent(ui.ctx())),
                        )
                        .clicked()
                    {
                        self.submit_feedback_to_github();
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_feedback = false;
                    }
                });
            });
        // Window `.open` may clear the flag; keep closed if submit already closed it.
        if !self.show_feedback {
            open = false;
        }
        self.show_feedback = open;
    }

    /// Open Ailiak Desk in the system browser.
    pub(crate) fn open_desk(&mut self) {
        match open_url(DESK_URL) {
            Ok(()) => self.status = format!("Opened {DESK_URL}"),
            Err(e) => self.status = format!("Could not open Desk: {e}"),
        }
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
        let oauth_config = self.config.oauth.clone();
        let password_override = self.password_override();
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
        self.busy.mail_test = true;
        thread::spawn(move || {
            let credential = match oauth::resolve_mail_credential(
                &account,
                &oauth_config,
                password_override.as_deref(),
            ) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(BgMsg::MailTest(Err(e.to_string())));
                    return;
                }
            };
            match test_connection(&account, &credential) {
                Ok(detail) => {
                    let _ = tx.send(BgMsg::MailTest(Ok(format!("{detail} · {label}"))));
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::MailTest(Err(e.to_string())));
                }
            }
        });
    }

    /// Archive or trash the open message (conversation).
    pub(crate) fn move_current_message(&mut self, kind: SpecialFolderKind) {
        if self.selected_person.is_some() {
            self.move_person_conversation(kind);
            return;
        }
        let Some(id) = self.selected_message.clone() else {
            self.status = "Select a message first".into();
            return;
        };
        self.start_move_messages(vec![id], kind);
    }

    /// Archive/trash every active message with the open People peer.
    pub(crate) fn move_person_conversation(&mut self, kind: SpecialFolderKind) {
        let Some(peer) = self.selected_person.clone() else {
            self.status = "Select a person first".into();
            return;
        };
        let ids: Vec<MessageId> = match self.store.list_messages_with_peer(&self.account_id, &peer)
        {
            Ok(msgs) => msgs.into_iter().map(|m| m.envelope.id).collect(),
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        if ids.is_empty() {
            // Nothing left to move — drop them from People immediately.
            self.selected_person = None;
            self.chat_messages.clear();
            self.selected_message = None;
            self.open_message = None;
            self.rebuild_people_cache();
            self.status = format!("No messages left with {peer}");
            return;
        }
        self.start_move_messages(ids, kind);
    }

    /// Archive or trash mid-list selection (multi-select) or every visible row.
    ///
    /// Whole-list / multi-item actions ask for confirm so Inbox (and other
    /// folders/labels) are not cleared by a mis-click on the mid-panel buttons.
    pub(crate) fn move_listed_messages(&mut self, kind: SpecialFolderKind) {
        if self.selected_person.is_some() && !self.list_multi_select {
            self.request_move_person_conversation(kind);
            return;
        }
        let ids: Vec<MessageId> = if self.list_multi_select {
            let visible: std::collections::HashSet<&MessageId> =
                if let Some(stak_id) = self.selected_stak.as_deref() {
                    self.envelopes
                        .iter()
                        .filter(|e| crate::staks::envelope_in_stak(e, stak_id))
                        .map(|e| &e.id)
                        .collect()
                } else {
                    self.envelopes.iter().map(|e| &e.id).collect()
                };
            self.list_selected
                .iter()
                .filter(|id| visible.contains(id))
                .cloned()
                .collect()
        } else if let Some(stak_id) = self.selected_stak.as_deref() {
            self.envelopes
                .iter()
                .filter(|e| crate::staks::envelope_in_stak(e, stak_id))
                .map(|e| e.id.clone())
                .collect()
        } else {
            self.envelopes.iter().map(|e| e.id.clone()).collect()
        };
        if ids.is_empty() {
            self.status = if self.list_multi_select {
                "Check at least one message first".into()
            } else {
                "No messages to move".into()
            };
            return;
        }
        // Single checked row is intentional; whole list / several rows need confirm.
        let needs_confirm = !self.list_multi_select || ids.len() > 1;
        if needs_confirm {
            self.pending_bulk_move = Some(PendingBulkMove {
                kind: PendingBulkKind::Move(kind),
                ids,
                scope: self.bulk_move_scope_label(),
            });
        } else {
            self.start_move_messages(ids, kind);
        }
    }

    /// Mark mid-list selection (or every visible row) as read.
    ///
    /// Whole-list / multi-item actions ask for confirm — same guard as Archive/Trash.
    pub(crate) fn mark_listed_messages_read(&mut self) {
        let ids: Vec<MessageId> = if self.list_multi_select {
            let visible: std::collections::HashSet<&MessageId> =
                if let Some(stak_id) = self.selected_stak.as_deref() {
                    self.envelopes
                        .iter()
                        .filter(|e| crate::staks::envelope_in_stak(e, stak_id))
                        .map(|e| &e.id)
                        .collect()
                } else {
                    self.envelopes.iter().map(|e| &e.id).collect()
                };
            self.list_selected
                .iter()
                .filter(|id| visible.contains(id))
                .cloned()
                .collect()
        } else if let Some(stak_id) = self.selected_stak.as_deref() {
            self.envelopes
                .iter()
                .filter(|e| crate::staks::envelope_in_stak(e, stak_id))
                .map(|e| e.id.clone())
                .collect()
        } else {
            self.envelopes.iter().map(|e| e.id.clone()).collect()
        };
        // Only unread rows need work.
        let ids: Vec<MessageId> = ids
            .into_iter()
            .filter(|id| {
                self.envelopes
                    .iter()
                    .find(|e| e.id == *id)
                    .is_some_and(|e| !e.seen)
            })
            .collect();
        if ids.is_empty() {
            self.status = if self.list_multi_select {
                "Check at least one unread message first".into()
            } else {
                "No unread messages in this list".into()
            };
            return;
        }
        let needs_confirm = !self.list_multi_select || ids.len() > 1;
        if needs_confirm {
            self.pending_bulk_move = Some(PendingBulkMove {
                kind: PendingBulkKind::MarkRead,
                ids,
                scope: self.bulk_move_scope_label(),
            });
        } else {
            self.start_mark_read_messages(ids);
        }
    }

    fn bulk_move_scope_label(&self) -> String {
        if let Some(peer) = self.selected_person.as_deref() {
            let name = self
                .people_cache
                .iter()
                .find(|p| p.addr.eq_ignore_ascii_case(peer))
                .map(|p| crate::people::display_name(p).to_string())
                .unwrap_or_else(|| peer.to_string());
            return format!("conversation with {name}");
        }
        let folder_title = self
            .folders
            .iter()
            .find(|f| f.path == self.selected_folder)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| {
                if self.selected_folder.is_empty() {
                    "this list".into()
                } else {
                    self.selected_folder.clone()
                }
            });
        if let Some(stak_id) = self.selected_stak.as_deref() {
            let label = self
                .staks_cache
                .iter()
                .find(|s| s.id == stak_id)
                .map(|s| s.label.clone())
                .unwrap_or_else(|| stak_id.to_string());
            format!("{label} · {folder_title}")
        } else {
            folder_title
        }
    }

    fn request_move_person_conversation(&mut self, kind: SpecialFolderKind) {
        let Some(peer) = self.selected_person.clone() else {
            self.status = "Select a person first".into();
            return;
        };
        let ids: Vec<MessageId> = match self.store.list_messages_with_peer(&self.account_id, &peer)
        {
            Ok(msgs) => msgs.into_iter().map(|m| m.envelope.id).collect(),
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        if ids.is_empty() {
            self.selected_person = None;
            self.chat_messages.clear();
            self.selected_message = None;
            self.open_message = None;
            self.rebuild_people_cache();
            self.status = format!("No messages left with {peer}");
            return;
        }
        if ids.len() == 1 {
            self.start_move_messages(ids, kind);
            return;
        }
        self.pending_bulk_move = Some(PendingBulkMove {
            kind: PendingBulkKind::Move(kind),
            ids,
            scope: self.bulk_move_scope_label(),
        });
    }

    pub(crate) fn confirm_pending_bulk_move(&mut self) {
        let Some(pending) = self.pending_bulk_move.take() else {
            return;
        };
        match pending.kind {
            PendingBulkKind::Move(kind) => self.start_move_messages(pending.ids, kind),
            PendingBulkKind::MarkRead => self.start_mark_read_messages(pending.ids),
        }
    }

    pub(crate) fn cancel_pending_bulk_move(&mut self) {
        self.pending_bulk_move = None;
    }

    fn show_bulk_confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_bulk_move.clone() else {
            return;
        };
        let n = pending.ids.len();
        let verb = pending.kind.verb();
        let title = format!("Confirm {}", verb.to_ascii_lowercase());
        let body = match pending.kind {
            PendingBulkKind::MarkRead if n == 1 => format!(
                "Mark 1 message as read in {}?",
                pending.scope
            ),
            PendingBulkKind::MarkRead => format!(
                "Mark {n} messages as read in {}?\n\nThis marks everything unread in the current list — cancel if you only meant one message.",
                pending.scope
            ),
            PendingBulkKind::Move(_) if n == 1 => format!(
                "{verb} 1 message in {}?\n\nThis cannot be undone from here.",
                pending.scope
            ),
            PendingBulkKind::Move(_) => format!(
                "{verb} {n} messages in {}?\n\nThis clears the current list view — cancel if you only meant one message.",
                pending.scope
            ),
        };

        let mut confirm = false;
        let mut cancel = false;
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("bulk_confirm_backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(90));
                if resp.clicked() {
                    cancel = true;
                }
            });

        egui::Window::new(title)
            .id(egui::Id::new("bulk_confirm_dialog"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.label(egui::RichText::new(body).color(crate::theme::ink(ctx)));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let confirm_label = match pending.kind {
                            PendingBulkKind::MarkRead => format!("Mark {n} read"),
                            PendingBulkKind::Move(_) => format!("{verb} {n}"),
                        };
                        let confirm_btn = if pending.kind.danger() {
                            ui.add(
                                egui::Button::new(
                                    egui::RichText::new(confirm_label)
                                        .color(egui::Color32::from_rgb(0xc0, 0x3a, 0x3a)),
                                )
                                .fill(crate::theme::chip(ctx)),
                            )
                        } else {
                            ui.button(confirm_label)
                        };
                        if confirm_btn.clicked() {
                            confirm = true;
                        }
                    });
                });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if confirm {
            self.confirm_pending_bulk_move();
        } else if cancel {
            self.cancel_pending_bulk_move();
        }
    }

    /// Mark messages read locally and push IMAP `\Seen` in the background.
    fn start_mark_read_messages(&mut self, ids: Vec<MessageId>) {
        let mut changed = 0usize;
        let mut push: Vec<(MessageId, String)> = Vec::new();
        for id in &ids {
            match self.store.mark_seen(id, true) {
                Ok(true) => {
                    changed += 1;
                    if let Some(env) = self.envelopes.iter_mut().find(|e| e.id == *id) {
                        env.seen = true;
                        push.push((id.clone(), env.folder_path.clone()));
                    } else if let Ok(Some(msg)) = self.store.get_message(id) {
                        push.push((id.clone(), msg.envelope.folder_path));
                    }
                    if self.open_message.as_ref().is_some_and(|m| m.envelope.id == *id) {
                        if let Some(m) = self.open_message.as_mut() {
                            m.envelope.seen = true;
                        }
                    }
                }
                Ok(false) => {}
                Err(e) => {
                    self.status = e.to_string();
                    return;
                }
            }
        }
        if changed > 0 {
            self.rebuild_staks_cache();
            let _ = self.refresh_folders();
            for (id, folder) in push {
                self.push_seen_to_server(id, folder, true);
            }
        }
        self.status = if changed == 0 {
            "No messages marked read".into()
        } else if changed == 1 {
            "Marked 1 message as read".into()
        } else {
            format!("Marked {changed} messages as read")
        };
        if self.list_multi_select {
            self.list_selected.clear();
            self.list_multi_select = false;
            self.list_select_anchor = None;
        }
    }

    fn start_move_messages(&mut self, ids: Vec<MessageId>, kind: SpecialFolderKind) {
        if self.busy.moving() || self.busy.sync {
            return;
        }
        let Some(account) = self.selected_account().cloned() else {
            self.status = "Add a mail account in Settings first".into();
            self.open_settings(SettingsSection::Accounts);
            return;
        };

        let db_path = self.paths.db_file();
        let oauth_config = self.config.oauth.clone();
        let password_override = self.password_override();
        let allow_pop3_local = account.protocol == MailProtocol::Pop3;
        let tx = self.bg_tx.clone();
        let n = ids.len();
        self.busy.set_move(kind);
        self.status = if n == 1 {
            format!("Moving to {}…", kind.label())
        } else {
            format!("Moving {n} to {}…", kind.label())
        };
        thread::spawn(move || {
            let credential = match oauth::resolve_mail_credential(
                &account,
                &oauth_config,
                password_override.as_deref(),
            ) {
                Ok(c) => c,
                Err(e) if allow_pop3_local => {
                    // POP3: allow local Archive/Trash without re-entering password.
                    let _ = e;
                    MailCredential::Password(String::new())
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::Error(e.to_string()));
                    return;
                }
            };
            let result = (|| -> Result<usize> {
                let store = Store::open_for_worker(&db_path)?;
                move_messages(&store, &account, &credential, &ids, kind)
            })();
            match result {
                Ok(count) => {
                    let _ = tx.send(BgMsg::MailMoved { kind, count });
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::Error(e.to_string()));
                }
            }
        });
    }

    /// Sync mail. `sync_all` downloads every IMAP folder (POP3 is always Inbox only).
    pub(crate) fn start_sync(&mut self, sync_all: bool) {
        if self.busy.sync {
            return;
        }
        let Some(account) = self
            .selected_account()
            .cloned()
            .or_else(|| self.current_account_draft())
        else {
            self.status = "Add a mail account in Settings first".into();
            self.open_settings(SettingsSection::Accounts);
            return;
        };

        // Persist typed password locally (keyring only — no network).
        let password_override = self.password_override();
        if account.auth == MailAuthMethod::Password {
            if let Some(ref password) = password_override {
                let normalized: String = password.chars().filter(|c| !c.is_whitespace()).collect();
                if let Err(e) = secrets::set_mail_password(&account.id, &normalized) {
                    self.status = format!("Keyring: {e}");
                    return;
                }
                self.imap_password_input.clear();
            }
        }

        let db_path = self.paths.db_file();
        let folder = self.selected_folder.clone();
        let oauth_config = self.config.oauth.clone();
        let tx = self.bg_tx.clone();
        let is_pop3 = account.protocol == MailProtocol::Pop3;
        self.busy.sync = true;
        self.last_sync_at = Instant::now();
        self.status = if is_pop3 {
            "Syncing Inbox (POP3)…".into()
        } else if sync_all {
            "Syncing all folders…".into()
        } else {
            format!("Syncing {}…", folder)
        };
        thread::spawn(move || {
            let result = (|| -> Result<_> {
                let credential = oauth::resolve_mail_credential(
                    &account,
                    &oauth_config,
                    password_override.as_deref(),
                )?;
                let store = Store::open_for_worker(&db_path)?;
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
                        new_messages: report.new_messages,
                        new_from: report.new_from,
                        new_subject: report.new_subject,
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
            self.mail_password_saved = secrets::has_mail_password_local(&a.id);
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
        } else {
            self.mail_password_saved = false;
        }
        self.ailiak_token_input.clear();
    }

    pub(crate) fn refresh_folders(&mut self) -> Result<()> {
        self.folders = self.store.list_folders(&self.account_id)?;
        // Snoozed virtual folder omitted while the feature is disabled in the UI.
        mailbox_core::folder_role::sort_folders(&mut self.folders);
        // Prefer a visible system folder when the selection vanished or is All Mail (hidden).
        let show_all = self.config.ui.show_all_mail;
        let visible = |f: &mailbox_core::Folder| {
            !f.hidden
                && (show_all || !mailbox_core::is_all_mail(&f.path, &f.name))
        };
        if self.folders.is_empty() {
            self.selected_folder = "INBOX".into();
        } else if !self
            .folders
            .iter()
            .any(|f| f.path == self.selected_folder && visible(f))
        {
            self.selected_folder = self
                .folders
                .iter()
                .find(|f| visible(f))
                .map(|f| f.path.clone())
                .unwrap_or_else(|| "INBOX".into());
        }
        Ok(())
    }

    /// Snooze open message for later — keeps it unread and hides it from the folder list.
    /// Toolbar entry is hidden for now; keep this ready to wire back up.
    #[allow(dead_code)]
    pub(crate) fn snooze_current_message(&mut self) {
        if self.busy.snooze {
            return;
        }
        let Some(id) = self.selected_message.clone() else {
            self.status = "Select a message first".into();
            return;
        };
        self.busy.snooze = true;
        let was_snoozed = self.store.is_snoozed(&id).unwrap_or(false);
        let result = (|| -> Result<()> {
            if was_snoozed {
                self.store.set_snoozed(&id, false)?;
                self.status = "Unsnoozed — back in its folder".into();
            } else {
                // Opening marks read; snooze restores unread so it stays “for later”.
                if self
                    .open_message
                    .as_ref()
                    .is_some_and(|m| m.envelope.seen)
                {
                    if self.store.mark_seen(&id, false)? {
                        if let Some(msg) = self.open_message.as_mut() {
                            msg.envelope.seen = false;
                        }
                        if let Some(env) = self.envelopes.iter_mut().find(|e| e.id == id) {
                            env.seen = false;
                        }
                        let folder = self
                            .open_message
                            .as_ref()
                            .map(|m| m.envelope.folder_path.clone())
                            .unwrap_or_else(|| self.selected_folder.clone());
                        self.push_seen_to_server(id.clone(), folder, false);
                    }
                }
                self.store.set_snoozed(&id, true)?;
                self.status = "Snoozed — left unread for later".into();
            }

            self.selected_message = None;
            self.open_message = None;
            self.mail_body_cache = None;
            self.mail_webview_req = None;
            self.selected_stak = None;
            self.refresh_folders()?;
            self.refresh_envelopes()?;
            if let Some(first) = self.envelopes.first().map(|e| e.id.clone()) {
                let _ = self.open_id(&first);
            }
            Ok(())
        })();
        self.busy.snooze = false;
        if let Err(e) = result {
            self.status = e.to_string();
        }
    }

    pub(crate) fn refresh_envelopes(&mut self) -> Result<()> {
        self.envelopes = if self.search.trim().is_empty() {
            self.store
                .list_envelopes(&self.account_id, &self.selected_folder)?
        } else {
            self.store
                .search_local(&self.account_id, self.search.trim())?
        };
        self.rebuild_staks_cache();
        self.rebuild_people_cache();
        Ok(())
    }

    pub(crate) fn rebuild_staks_cache(&mut self) {
        let had_staks = !self.staks_cache.is_empty();
        self.staks_cache = crate::staks::build_staks(&self.envelopes);
        // Staks cleared → open Folders/Labels so the rail isn’t left empty.
        if had_staks && self.staks_cache.is_empty() {
            self.folders_section_open = true;
            self.labels_section_open = true;
            self.staks_section_open = false;
        }
    }

    pub(crate) fn rebuild_people_cache(&mut self) {
        self.people_cache = crate::people::build_people(&self.envelopes);
        if let Some(peer) = self.selected_person.as_deref() {
            if !self.people_cache.iter().any(|p| p.addr == peer) {
                // Peer no longer in this folder’s People list (e.g. trashed).
                self.selected_person = None;
                self.chat_messages.clear();
            }
        }
    }

    pub(crate) fn account_email(&self) -> String {
        self.config
            .accounts
            .iter()
            .find(|a| a.id == self.account_id)
            .map(|a| a.email.clone())
            .unwrap_or_default()
    }

    pub(crate) fn open_compose_new(&mut self) {
        // Leave People chat so the reader card can host the composer.
        self.selected_person = None;
        self.compose_sent = None;
        let mut draft = crate::compose::ComposeDraft::new_blank();
        draft.from_email = self.account_email();
        // `inline: false` → full reader-card composer (not the reply dock).
        draft.inline = false;
        self.refresh_compose_hints();
        // No field focused on open — user clicks To / Subject / body intentionally.
        self.compose_autofocus_to = false;
        self.compose_autofocus_subject = false;
        self.compose_body_focused = false;
        self.compose = Some(draft);
    }

    /// New message with To prefilled from an Options sender (name + address).
    pub(crate) fn open_compose_to(&mut self, name: &str, addr: &str) {
        let addr = addr.trim();
        if addr.is_empty() {
            return;
        }
        self.selected_person = None;
        self.compose_sent = None;
        let mut draft = crate::compose::ComposeDraft::new_blank();
        draft.from_email = self.account_email();
        draft.inline = false;
        let name = name.trim();
        let to = if name.is_empty() || name.eq_ignore_ascii_case(addr) {
            addr.to_string()
        } else {
            format!("{name} <{addr}>")
        };
        draft.to.push(to);
        self.refresh_compose_hints();
        self.compose_autofocus_to = false;
        self.compose_autofocus_subject = true;
        self.compose_body_focused = false;
        self.compose = Some(draft);
        self.status = format!("New message to {addr}");
    }

    /// Feedback: full composer with To locked to [`FEEDBACK_TO_ADDR`].
    pub(crate) fn open_compose_feedback(&mut self) {
        self.selected_person = None;
        self.compose_sent = None;
        self.show_help = false;
        self.show_feedback = false;
        let mut draft = crate::compose::ComposeDraft::new_feedback(FEEDBACK_TO_ADDR);
        draft.from_email = self.account_email();
        draft.inline = false;
        self.refresh_compose_hints();
        self.compose_autofocus_to = false;
        self.compose_autofocus_subject = false;
        self.compose_body_focused = false;
        self.compose = Some(draft);
        self.status = format!("Feedback — sending to {FEEDBACK_TO_ADDR}");
    }

    pub(crate) fn dismiss_compose_sent(&mut self) {
        self.compose_sent = None;
    }

    /// Load frequent From addresses for the composer suggestion popup.
    pub(crate) fn refresh_compose_hints(&mut self) {
        let me = self.account_email().to_lowercase();
        match self.store.list_recipient_hints(&self.account_id, 150) {
            Ok(hints) => {
                self.compose_hints = hints
                    .into_iter()
                    .filter(|(_, addr)| !addr.eq_ignore_ascii_case(&me))
                    .collect();
            }
            Err(e) => {
                tracing::debug!(error = %e, "recipient hints failed");
                self.compose_hints.clear();
            }
        }
    }

    pub(crate) fn open_reply(&mut self, all: bool) {
        let Some(msg) = self.open_message.clone() else {
            self.status = "Open a message to reply".into();
            return;
        };
        self.compose_sent = None;
        let email = self.account_email();
        self.compose = Some(crate::compose::ComposeDraft::from_reply(&msg, all, &email));
    }

    pub(crate) fn open_person_chat(&mut self, addr: &str) {
        self.selected_person = Some(addr.to_lowercase());
        self.selected_stak = None;
        if self.compose.as_ref().is_some_and(|c| c.inline) {
            self.compose = None;
        }
        self.chat_reply.clear();
        self.chat_reply_focused = false;
        self.chat_attachments.clear();
        match self
            .store
            .list_messages_with_peer(&self.account_id, addr)
        {
            Ok(msgs) => {
                self.chat_messages = msgs;
                if let Some(last) = self.chat_messages.last() {
                    self.selected_message = Some(last.envelope.id.clone());
                    self.open_message = Some(last.clone());
                }
                self.status = format!("Chat with {addr}");
            }
            Err(e) => {
                self.status = e.to_string();
                self.chat_messages.clear();
            }
        }
    }

    pub(crate) fn submit_compose(&mut self) {
        let Some(draft) = self.compose.as_ref() else {
            return;
        };
        if draft.to.is_empty() && draft.to_input.trim().is_empty() {
            self.status = "Add at least one To recipient".into();
            return;
        }
        if !self.config.accounts.iter().any(|a| a.id == self.account_id) {
            self.status = "Add an account in Settings before sending".into();
            return;
        }
        // Pull a definitive HTML snapshot from the editor before SMTP.
        // WebKitGTK callbacks can hang — we time out and use the already-synced draft.
        if self.compose_webview.is_alive() {
            self.pending_compose_send = true;
            self.pending_compose_send_frames = 0;
            self.busy.send = true;
            self.compose_webview.request_flush();
            self.status = "Sending…".into();
            return;
        }
        self.finish_submit_compose();
    }

    fn finish_submit_compose(&mut self) {
        self.pending_compose_send = false;
        self.pending_compose_send_frames = 0;
        let Some(draft) = self.compose.clone() else {
            self.busy.send = false;
            return;
        };
        let Some(account) = self
            .config
            .accounts
            .iter()
            .find(|a| a.id == self.account_id)
            .cloned()
        else {
            self.busy.send = false;
            self.status = "Add an account in Settings before sending".into();
            return;
        };
        let attachments = match load_outgoing_attachments(&draft.attachments) {
            Ok(a) => a,
            Err(e) => {
                self.busy.send = false;
                self.status = e;
                return;
            }
        };
        let html = draft.body_html_for_send();
        let mut draft = draft;
        let mail = mailbox_core::OutgoingMail {
            to: draft.recipients_to(),
            cc: draft.recipients_cc(),
            bcc: draft.recipients_bcc(),
            subject: draft.subject.clone(),
            body_text: draft.body_for_send(),
            body_html: if html.trim().is_empty() {
                None
            } else {
                Some(html)
            },
            in_reply_to: draft.in_reply_to.clone(),
            references: draft.references.clone(),
            attachments,
        };
        if mail.to.is_empty() {
            self.busy.send = false;
            self.status = "Add at least one To recipient".into();
            return;
        }
        self.busy.send = true;
        self.status = "Sending…".into();
        let tx = self.bg_tx.clone();
        let oauth = self.config.oauth.clone();
        thread::spawn(move || {
            let result = (|| {
                let cred = mailbox_core::resolve_mail_credential(&account, &oauth, None)?;
                mailbox_core::send_mail(&account, &cred, &mail)?;
                Ok::<(), anyhow::Error>(())
            })();
            match result {
                Ok(()) => {
                    let _ = tx.send(BgMsg::SendOk);
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::Error(e.to_string()));
                }
            }
        });
    }

    pub(crate) fn apply_compose_webview_result(
        &mut self,
        draft: &crate::compose::ComposeDraft,
        res: &crate::compose::ComposeUiResult,
        ctx: &egui::Context,
    ) {
        if let Some(cmd) = res.exec {
            self.compose_webview.exec_command(cmd);
        }
        if let Some(html) = res.insert_html.as_ref() {
            self.compose_webview.insert_html(html);
        }
        if res.pick_inline_image {
            self.insert_inline_image_from_picker();
        }
        if res.header_focused {
            self.compose_body_focused = false;
        }
        let webview_took_focus = self.compose_webview.take_editor_focused();
        if res.focus_editor || webview_took_focus {
            self.compose_body_focused = true;
        }
        for cmd in self.compose_webview.take_host_cmds() {
            if cmd == "insertImage" {
                self.insert_inline_image_from_picker();
            }
        }
        if let Some(bounds) = res.editor_bounds {
            // Native webview sits above egui — pull it back under recipient suggestions etc.
            let Some(bounds) = crate::compose::inset_editor_bounds_for_overlays(
                bounds,
                &res.egui_overlays,
            ) else {
                return;
            };
            // Webview is a native child above egui — clicks often never reach egui.
            // Use pointer position + IPC focus from the editor to drop To/Subject focus.
            let press_in_editor = ctx.input(|i| {
                let pos = i.pointer.interact_pos().or_else(|| i.pointer.latest_pos());
                i.pointer.primary_pressed() && pos.is_some_and(|p| bounds.contains(p))
            });
            if press_in_editor {
                self.compose_body_focused = true;
            }
            // While the body owns focus, keep egui from parking the caret on To.
            if self.compose_body_focused {
                if let Some(id) = ctx.memory(|m| m.focused()) {
                    ctx.memory_mut(|m| m.surrender_focus(id));
                }
            }
            // Only steal focus from egui → webview. IPC `focus` means the webview
            // already has it; calling focus_editor() again freezes WebKitGTK/egui.
            if press_in_editor || res.focus_editor {
                self.compose_webview.focus_editor();
            }
            let placeholder = if draft.is_reply() {
                if draft.reply_to_name.is_empty() {
                    "Write a reply — type / for styles…".into()
                } else {
                    format!("Reply to {} — type / for styles…", draft.reply_to_name)
                }
            } else {
                "Write, type \"/\" for commands…".into()
            };
            let accent = theme::accent(ctx);
            self.compose_webview_req = Some(crate::compose_webview::ComposeWebViewRequest {
                bounds,
                pixels_per_point: ctx.pixels_per_point(),
                editor_rev: draft.editor_rev,
                initial_html: draft.body_html.clone(),
                // Reader card is always light paper.
                dark: false,
                placeholder,
                accent_css: format!("#{:02x}{:02x}{:02x}", accent.r(), accent.g(), accent.b()),
            });
        }
    }

    /// Pick an image file and insert it as a data-URL `<img>` in the compose body.
    pub(crate) fn insert_inline_image_from_picker(&mut self) {
        // Bookmark caret *before* the dialog steals WebKit focus/selection.
        self.compose_webview.prepare_insert();
        // Park the child view so a fallback in-process dialog cannot trip GDK asserts.
        self.compose_webview.hide();
        crate::mail_webview::MailWebView::pump_platform();
        let Some(path) = crate::file_pick::pick_file(
            "Insert image",
            &[crate::file_pick::Filter {
                name: "Images",
                extensions: &["png", "jpg", "jpeg", "gif", "webp"],
            }],
        ) else {
            self.status = "Image insert cancelled".into();
            return;
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp"
        ) {
            self.status = "Unsupported image type".into();
            return;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.status = format!("Could not read image: {e}");
                return;
            }
        };
        let (mime, encoded, display_w) = match prepare_compose_inline_image(&bytes, &ext) {
            Ok(v) => v,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&encoded);
        let alt_raw = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".into());
        let alt = alt_raw
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;");
        // Explicit width fits common mail clients; max-width keeps fluid layout.
        let html = format!(
            r#"<img src="data:{mime};base64,{b64}" alt="{alt}" width="{display_w}" style="width:{display_w}px;max-width:100%;height:auto;">"#
        );
        self.compose_webview.focus_editor();
        self.compose_webview.insert_html(&html);
        self.status = format!("Inserted {alt_raw} ({display_w}px wide — drag corner to resize)");
    }
}

/// Downscale + re-encode for inline mail HTML. Returns `(mime, bytes, display_width_px)`.
fn prepare_compose_inline_image(bytes: &[u8], ext: &str) -> Result<(String, Vec<u8>, u32), String> {
    use image::codecs::jpeg::JpegEncoder;
    use image::imageops::FilterType;
    use image::{DynamicImage, ImageEncoder, ImageFormat};
    use std::io::Cursor;

    /// Longest pixel edge after decode (keeps detail without huge payloads).
    const MAX_EDGE: u32 = 960;
    /// Default CSS/width attribute — fits typical mail reading panes.
    const MAX_DISPLAY_W: u32 = 560;
    const MAX_OUT: usize = 1_500_000;

    // Keep animated GIFs as-is (re-encode would flatten to one frame).
    if ext == "gif" {
        if bytes.len() > MAX_OUT {
            return Err("GIF too large to inline (max ~1.5 MB)".into());
        }
        let display_w = image::load_from_memory(bytes)
            .map(|i| i.width().min(MAX_DISPLAY_W))
            .unwrap_or(MAX_DISPLAY_W);
        return Ok(("image/gif".into(), bytes.to_vec(), display_w));
    }

    let img = image::load_from_memory(bytes).map_err(|e| format!("Could not decode image: {e}"))?;
    let img = if img.width() > MAX_EDGE || img.height() > MAX_EDGE {
        img.resize(MAX_EDGE, MAX_EDGE, FilterType::Triangle)
    } else {
        img
    };
    let display_w = img.width().min(MAX_DISPLAY_W);

    let rgba = img.to_rgba8();
    let has_alpha = rgba.pixels().any(|p| p.0[3] < 250);

    let (mime, encoded) = if has_alpha {
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(rgba)
            .write_to(&mut out, ImageFormat::Png)
            .map_err(|e| format!("PNG encode failed: {e}"))?;
        ("image/png".into(), out.into_inner())
    } else {
        let rgb = DynamicImage::ImageRgba8(rgba).to_rgb8();
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, 85)
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| format!("JPEG encode failed: {e}"))?;
        ("image/jpeg".into(), out)
    };

    if encoded.len() > MAX_OUT {
        return Err("Image too large to inline after resize (max ~1.5 MB)".into());
    }
    Ok((mime, encoded, display_w))
}

impl MailboxApp {
    fn sync_compose_from_webview(&mut self) {
        let _ = self.compose_webview.poll();
        for cmd in self.compose_webview.take_host_cmds() {
            if cmd == "insertImage" {
                self.insert_inline_image_from_picker();
            }
        }
        let Some(draft) = self.compose.as_mut() else {
            return;
        };
        if !self.compose_webview.is_synced(draft.editor_rev) {
            return;
        }
        draft.body = self.compose_webview.content.text.clone();
        draft.body_html = self.compose_webview.content.html.clone();
        draft.fmt = self.compose_webview.fmt;
        if !draft.body_is_empty() || !draft.attachments.is_empty() {
            draft.dock_focused = true;
        }
    }

    pub(crate) fn submit_chat_reply(&mut self) {
        let body = self.chat_reply.trim().to_string();
        if body.is_empty() && self.chat_attachments.is_empty() {
            return;
        }
        let Some(peer) = self.selected_person.clone() else {
            return;
        };
        let Some(account) = self
            .config
            .accounts
            .iter()
            .find(|a| a.id == self.account_id)
            .cloned()
        else {
            self.status = "Add an account in Settings before sending".into();
            return;
        };
        let (subject, in_reply_to, references) = self
            .chat_messages
            .last()
            .map(|m| {
                (
                    mailbox_core::reply_subject(&m.envelope.subject),
                    Some(m.envelope.message_id.clone()),
                    Some(mailbox_core::reply_references(
                        &m.envelope.references,
                        &m.envelope.message_id,
                    )),
                )
            })
            .unwrap_or_else(|| ("Hi".into(), None, None));
        let attachments = match load_outgoing_attachments(&self.chat_attachments) {
            Ok(a) => a,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        let mail = mailbox_core::OutgoingMail {
            to: vec![peer],
            cc: vec![],
            bcc: vec![],
            subject,
            body_text: body,
            body_html: None,
            in_reply_to,
            references,
            attachments,
        };
        self.busy.send = true;
        self.status = "Sending…".into();
        let tx = self.bg_tx.clone();
        let oauth = self.config.oauth.clone();
        thread::spawn(move || {
            let result = (|| {
                let cred = mailbox_core::resolve_mail_credential(&account, &oauth, None)?;
                mailbox_core::send_mail(&account, &cred, &mail)?;
                Ok::<(), anyhow::Error>(())
            })();
            match result {
                Ok(()) => {
                    let _ = tx.send(BgMsg::SendOk);
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::Error(e.to_string()));
                }
            }
        });
    }

    pub(crate) fn open_id(&mut self, id: &MessageId) -> Result<()> {
        // Same message already open — skip SQLite + list reload (fast click path).
        if self.selected_message.as_ref() == Some(id) && self.open_message.is_some() {
            // Still leave People chat if the mid-list was used to open mail.
            if self.selected_person.is_some() {
                self.selected_person = None;
                self.chat_messages.clear();
                self.chat_reply.clear();
                self.chat_reply_focused = false;
                self.chat_attachments.clear();
            }
            return Ok(());
        }

        // Switching messages dismisses an in-reader reply dock (full-card New stays).
        if self.compose.as_ref().is_some_and(|c| c.inline) {
            self.compose = None;
        }
        // Mid-list selection always shows classic mail — leave People chat.
        if self.selected_person.is_some() {
            self.selected_person = None;
            self.chat_messages.clear();
            self.chat_reply.clear();
            self.chat_reply_focused = false;
            self.chat_attachments.clear();
        }

        if let Some(mut msg) = self.store.get_message(id)? {
            if !msg.envelope.seen {
                let changed = self.store.mark_seen(id, true)?;
                msg.envelope.seen = true;
                // Update list row in place — avoid reloading the whole folder.
                if let Some(env) = self.envelopes.iter_mut().find(|e| e.id == *id) {
                    env.seen = true;
                }
                self.rebuild_staks_cache();
                if changed {
                    let _ = self.refresh_folders();
                    self.push_seen_to_server(
                        id.clone(),
                        msg.envelope.folder_path.clone(),
                        true,
                    );
                }
            }
            self.load_remote_images = self.config.ui.allows_remote_images(&msg.envelope.from_addr);
            self.mail_images.clear();
            self.mail_body_cache = None;
            self.mail_webview_req = None;
            self.mail_webview.last_error = None; // retry create on next HTML message
            self.reader_headers_expanded = false;
            self.reader_subject_expanded = false;
            self.selected_message = Some(id.clone());
            if self.config.ui.notify_opened_mail {
                let from = if msg.envelope.from_name.is_empty() {
                    msg.envelope.from_addr.as_str()
                } else {
                    msg.envelope.from_name.as_str()
                };
                let linked = self
                    .store
                    .ailiak_task_for_message(&msg.envelope.message_id)
                    .ok()
                    .flatten();
                let hint = if let Some(ref task_id) = linked {
                    Some(format!("On Desk · {task_id}"))
                } else if self.config.ailiak.enabled {
                    Some("Press a for Options".into())
                } else {
                    None
                };
                crate::notifications::notify_opened_mail(
                    from,
                    &msg.envelope.subject,
                    hint.as_deref(),
                );
            }
            self.open_message = Some(msg);
        }
        Ok(())
    }

    /// Best-effort IMAP `\Seen` (background). Local cache is already updated.
    fn push_seen_to_server(&self, message_id: MessageId, folder_path: String, seen: bool) {
        let Some(account) = self.selected_account().cloned() else {
            return;
        };
        if account.protocol != MailProtocol::Imap {
            return;
        }
        let oauth_config = self.config.oauth.clone();
        thread::spawn(move || {
            let credential = match oauth::resolve_mail_credential(&account, &oauth_config, None) {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!(error = %e, "mark-seen: no credential");
                    return;
                }
            };
            if let Err(e) =
                mark_seen_on_server(&account, &credential, &message_id, &folder_path, seen)
            {
                tracing::debug!(error = %e, msg = %message_id, "IMAP mark-seen failed");
            }
        });
    }

    /// Toggle star / `\Flagged` on a mid-list row (local + IMAP push).
    pub(crate) fn toggle_starred(&mut self, id: &MessageId) {
        let Some((next, folder_path, row_id, mid)) = self.envelopes.iter().find_map(|e| {
            if e.id == *id {
                Some((
                    !e.flagged,
                    e.folder_path.clone(),
                    e.id.clone(),
                    e.message_id.clone(),
                ))
            } else {
                None
            }
        }) else {
            return;
        };
        match self.store.set_flagged(&row_id, next) {
            Ok(_) => {
                for e in self.envelopes.iter_mut() {
                    if e.id == row_id || (!mid.is_empty() && e.message_id == mid) {
                        e.flagged = next;
                    }
                }
                // Viewing Starred: unstar removes the row from this folder locally.
                if !next
                    && mailbox_core::classify_mailbox(&folder_path, &folder_path)
                        == mailbox_core::MailboxRole::Starred
                {
                    self.envelopes.retain(|e| e.id != row_id);
                    if self.selected_message.as_ref() == Some(&row_id) {
                        self.selected_message = None;
                        self.open_message = None;
                    }
                }
                if let Some(msg) = self.open_message.as_mut() {
                    if msg.envelope.id == row_id
                        || (!mid.is_empty() && msg.envelope.message_id == mid)
                    {
                        msg.envelope.flagged = next;
                    }
                }
                self.status = if next {
                    "Starred".into()
                } else {
                    "Unstarred".into()
                };
                self.push_flagged_to_server(row_id, folder_path, next);
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    /// Best-effort IMAP `\Flagged` (background). Local cache is already updated.
    fn push_flagged_to_server(&self, message_id: MessageId, folder_path: String, flagged: bool) {
        let Some(account) = self.selected_account().cloned() else {
            return;
        };
        if account.protocol != MailProtocol::Imap {
            return;
        }
        let oauth_config = self.config.oauth.clone();
        thread::spawn(move || {
            let credential = match oauth::resolve_mail_credential(&account, &oauth_config, None) {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!(error = %e, "mark-flagged: no credential");
                    return;
                }
            };
            if let Err(e) =
                mark_flagged_on_server(&account, &credential, &message_id, &folder_path, flagged)
            {
                tracing::debug!(error = %e, msg = %message_id, "IMAP mark-flagged failed");
            }
        });
    }

    /// Allow remote images for the open message’s **domain** (persisted; covers the Stak).
    pub(crate) fn allow_remote_images_for_sender(&mut self) {
        let Some(addr) = self
            .open_message
            .as_ref()
            .map(|m| m.envelope.from_addr.clone())
        else {
            return;
        };
        let domain = mailbox_core::remote_image_domain(&addr);
        self.config.ui.allow_remote_images_sender(&addr);
        self.load_remote_images = true;
        self.mail_body_cache = None;
        self.mail_images.clear();
        if let Err(e) = self.config.save(&self.paths.config_file()) {
            self.status = format!("Could not save image allowlist: {e}");
        } else if !domain.is_empty() {
            self.status = format!("Remote images allowed for *.{domain}");
        }
    }

    /// Revoke remote images for the open message’s domain (allowlist undo; settings later).
    #[allow(dead_code)]
    pub(crate) fn block_remote_images_for_sender(&mut self) {
        let Some(addr) = self
            .open_message
            .as_ref()
            .map(|m| m.envelope.from_addr.clone())
        else {
            return;
        };
        let domain = mailbox_core::remote_image_domain(&addr);
        self.config.ui.block_remote_images_sender(&addr);
        self.load_remote_images = false;
        self.mail_body_cache = None;
        self.mail_images.clear();
        if let Err(e) = self.config.save(&self.paths.config_file()) {
            self.status = format!("Could not save image allowlist: {e}");
        } else if !domain.is_empty() {
            self.status = format!("Remote images blocked for *.{domain}");
        }
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
                    self.busy.ailiak = false;
                    self.close_options_panel();
                    let day = for_date.unwrap_or_else(|| "today".into());
                    self.refresh_desk_tasks();
                    // One-shot escape hatch to organize on the full Desk UI.
                    match open_url(DESK_URL) {
                        Ok(()) => {
                            self.status =
                                format!("Todo created {task_id} · Desk {day} · opened {DESK_URL}");
                        }
                        Err(_) => {
                            self.status = format!("Todo created {task_id} · Desk {day}");
                        }
                    }
                }
                BgMsg::SendOk => {
                    self.busy.send = false;
                    self.pending_compose_send = false;
                    self.pending_compose_send_frames = 0;
                    self.compose_body_focused = false;
                    self.compose_autofocus_to = false;
                    self.compose_autofocus_subject = false;
                    // Snapshot recipients/subject, then wipe the draft + native editor.
                    let notice = self
                        .compose
                        .take()
                        .map(|d| crate::compose::ComposeSentNotice::from_draft(&d));
                    self.compose_webview.reset_after_send();
                    self.compose_webview_req = None;
                    self.compose_sent = notice;
                    self.status = "Message sent".into();
                    self.chat_reply.clear();
                    self.chat_reply_focused = false;
                    self.chat_attachments.clear();
                    if let Some(peer) = self.selected_person.clone() {
                        // Chat dock send: refresh the thread; full-card notice still shows.
                        self.open_person_chat(&peer);
                    }
                }
                BgMsg::MailMoved { kind, count } => {
                    self.busy.clear_move();
                    let label = kind.label();
                    self.status = if count == 1 {
                        format!("Moved 1 message to {label}")
                    } else {
                        format!("Moved {count} messages to {label}")
                    };
                    self.list_selected.clear();
                    self.open_message = None;
                    self.selected_message = None;
                    self.mail_webview_req = None;
                    self.mail_body_cache = None;
                    let peer = self.selected_person.clone();
                    if let Err(e) = self.refresh_folders() {
                        self.status = e.to_string();
                    } else if let Err(e) = self.refresh_envelopes() {
                        self.status = e.to_string();
                    }
                    // People conversation: leave the chat when nothing active remains.
                    if let Some(peer) = peer {
                        match self.store.list_messages_with_peer(&self.account_id, &peer) {
                            Ok(msgs) if msgs.is_empty() => {
                                self.selected_person = None;
                                self.chat_messages.clear();
                                self.rebuild_people_cache();
                                self.status = format!("Conversation moved to {label}");
                                if let Some(first) =
                                    self.envelopes.first().map(|e| e.id.clone())
                                {
                                    let _ = self.open_id(&first);
                                }
                            }
                            Ok(msgs) => {
                                self.chat_messages = msgs;
                                self.rebuild_people_cache();
                                if !self.people_cache.iter().any(|p| p.addr == peer) {
                                    // No Inbox (current-folder) presence left — drop from People.
                                    self.selected_person = None;
                                    self.chat_messages.clear();
                                    if let Some(first) =
                                        self.envelopes.first().map(|e| e.id.clone())
                                    {
                                        let _ = self.open_id(&first);
                                    }
                                } else if let Some(last) = self.chat_messages.last() {
                                    self.selected_message = Some(last.envelope.id.clone());
                                    self.open_message = Some(last.clone());
                                }
                            }
                            Err(e) => {
                                self.status = e.to_string();
                                self.selected_person = None;
                                self.chat_messages.clear();
                            }
                        }
                    } else if let Some(first) = self.envelopes.first().map(|e| e.id.clone()) {
                        let _ = self.open_id(&first);
                    }
                }
                BgMsg::WhoAmI(s) => {
                    self.busy.ailiak = false;
                    self.status = format!("Ailiak: signed in as {s}");
                }
                BgMsg::MailTest(Ok(s)) => {
                    self.busy.mail_test = false;
                    self.status = s.clone();
                    self.mail_test_feedback = s;
                }
                BgMsg::MailTest(Err(e)) => {
                    self.busy.mail_test = false;
                    let msg = Self::friendly_mail_test_error(&e);
                    self.status = msg.clone();
                    self.mail_test_feedback = msg;
                }
                BgMsg::OAuthSignIn(Ok(result)) => {
                    self.busy.oauth = false;
                    self.apply_oauth_sign_in(result);
                }
                BgMsg::OAuthSignIn(Err(e)) => {
                    self.busy.oauth = false;
                    let msg = format!("Sign-in: {e}");
                    self.status = msg.clone();
                    self.mail_test_feedback = msg;
                }
                BgMsg::SyncOk {
                    account_id,
                    folders,
                    messages,
                    new_messages,
                    new_from,
                    new_subject,
                    folder_path,
                    detail,
                } => {
                    self.busy.sync = false;
                    self.last_sync_at = Instant::now();
                    self.note_sync_stamp();
                    if new_messages > 0 && self.config.ui.sound_new_mail {
                        crate::sounds::play_new_mail(self.config.ui.sound_theme);
                    }
                    if new_messages > 0 && self.config.ui.notify_new_mail {
                        crate::notifications::notify_new_mail(
                            new_messages,
                            &new_from,
                            &new_subject,
                        );
                    }
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
                    self.busy.ailiak = false;
                    self.calendar.tasks = tasks;
                }
                BgMsg::Error(e) => {
                    self.busy.send = false;
                    self.pending_compose_send = false;
                    self.pending_compose_send_frames = 0;
                    self.calendar.loading = false;
                    if self.busy.sync {
                        self.last_sync_at = Instant::now();
                    }
                    self.busy.clear_network();
                    self.busy.snooze = false;
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

    /// Brand / company mark: prefer crisp bundled SVG; Logo.dev for the rest.
    pub(crate) fn brand_texture(
        &mut self,
        ctx: &egui::Context,
        brand_or_domain: &str,
    ) -> Option<egui::TextureHandle> {
        if let Some(tex) = crate::brand_logos::texture(ctx, brand_or_domain) {
            return Some(tex);
        }
        let dark = ctx.style().visuals.dark_mode;
        self.logos
            .texture(ctx, &self.paths.logos_dir(), brand_or_domain, dark)
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
        let oauth_config = self.config.oauth.clone();
        let dir = self.paths.avatars_dir();
        let tx = self.avatar_tx.clone();
        thread::spawn(move || {
            let access_token = match oauth::resolve_mail_credential(&account, &oauth_config, None) {
                Ok(MailCredential::OAuth2 { access_token }) => access_token,
                _ => {
                    let _ = tx.send(AvatarMsg::Miss { email: key });
                    return;
                }
            };
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
        let Some(path) = crate::file_pick::pick_file(
            "Choose account photo",
            &[crate::file_pick::Filter {
                name: "Images",
                extensions: &["png", "jpg", "jpeg"],
            }],
        ) else {
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

    /// Options and Calendar share one right-side panel — opening one closes the other.
    pub(crate) fn open_calendar_panel(&mut self) {
        self.close_options_panel();
        self.show_calendar = true;
        self.desk_was_options = false;
        self.refresh_desk_tasks();
    }

    pub(crate) fn toggle_calendar(&mut self) {
        if self.show_calendar {
            self.show_calendar = false;
        } else {
            self.open_calendar_panel();
        }
    }

    fn persist_options_open(&mut self, open: bool) {
        if self.config.ui.options_open == open {
            return;
        }
        self.config.ui.options_open = open;
        if let Err(e) = self.config.save(&self.paths.config_file()) {
            self.status = format!("Could not save Options panel state: {e}");
        }
    }

    /// Persist expanded nav width after the user finishes dragging the rail.
    pub(crate) fn persist_nav_width(&mut self) {
        let w = self.nav_width.clamp(140.0, 200.0);
        self.nav_width = w;
        if (self.config.ui.nav_width - w).abs() < 0.5 {
            return;
        }
        self.config.ui.nav_width = w;
        if let Err(e) = self.config.save(&self.paths.config_file()) {
            self.status = format!("Could not save nav width: {e}");
        }
    }

    /// Persist the current Options note (if any) and hide the panel.
    pub(crate) fn close_options_panel(&mut self) {
        self.save_options_note();
        self.show_options = false;
        self.pending_todo_selection = false;
        self.options_task_help_open = false;
        self.persist_options_open(false);
    }

    /// Toggle the Options side panel (replaces Calendar when opening).
    pub(crate) fn toggle_options(&mut self) {
        if self.show_options {
            self.close_options_panel();
            return;
        }
        self.open_options();
    }

    pub(crate) fn open_options(&mut self) {
        let Some(msg) = self.open_message.clone() else {
            self.status = "Open a message first".into();
            return;
        };
        self.load_options_note_for(&msg.envelope.from_addr);
        // Empty until the user inserts selected email text (or types slash commands).
        self.todo_compose.clear();
        self.todo_message_id = msg.envelope.message_id.clone();
        self.show_calendar = false;
        self.show_options = true;
        self.desk_was_options = true;
        self.pending_todo_selection = false;
        self.options_task_help_open = false;
        self.persist_options_open(true);

        if let Ok(Some(existing)) = self.store.ailiak_task_for_message(&msg.envelope.message_id) {
            self.status = format!("Already on Desk as {existing}");
        }
    }

    pub(crate) fn load_options_note_for(&mut self, sender_addr: &str) {
        self.save_options_note();
        let key = sender_addr.trim().to_lowercase();
        self.options_note_sender = key.clone();
        self.options_note = if key.is_empty() {
            String::new()
        } else {
            self.store
                .get_sender_note(&key)
                .ok()
                .flatten()
                .unwrap_or_default()
        };
    }

    pub(crate) fn save_options_note(&mut self) {
        let key = self.options_note_sender.trim().to_lowercase();
        if key.is_empty() {
            return;
        }
        if let Err(e) = self.store.set_sender_note(&key, &self.options_note) {
            self.status = format!("Could not save note: {e}");
        }
    }

    /// Keep Options sender/note in sync when the open message changes.
    pub(crate) fn sync_options_with_open_message(&mut self) {
        if !self.show_options {
            return;
        }
        let Some(msg) = self.open_message.clone() else {
            return;
        };
        let addr = msg.envelope.from_addr.trim().to_lowercase();
        if addr != self.options_note_sender {
            self.load_options_note_for(&msg.envelope.from_addr);
        }
        if msg.envelope.message_id != self.todo_message_id {
            self.todo_compose.clear();
            self.todo_message_id = msg.envelope.message_id.clone();
        }
    }

    pub(crate) fn request_todo_selection_insert(&mut self) {
        if !self.mail_webview.is_alive() {
            self.status = "Select text in the email first".into();
            return;
        }
        self.mail_webview.request_selection();
        if self.mail_webview.has_pending_selection() {
            self.pending_todo_selection = true;
        } else {
            self.status = "Select text in the email first".into();
        }
    }

    pub(crate) fn poll_todo_selection_insert(&mut self) {
        if !self.pending_todo_selection {
            return;
        }
        let Some(sel) = self.mail_webview.take_selection() else {
            return;
        };
        self.pending_todo_selection = false;
        if sel.trim().is_empty() {
            self.status = "Select text in the email first".into();
            return;
        }
        insert_selection_into_compose(&mut self.todo_compose, &sel);
        self.status = "Selection added to Task".into();
    }

    pub(crate) fn submit_todo(&mut self) {
        if self.todo_message_id.is_empty() {
            self.status = "No message linked for Todo".into();
            return;
        }
        if !self.config.ailiak.enabled {
            self.status = "Enable Ailiak in Settings first".into();
            self.open_settings(SettingsSection::Ailiak);
            return;
        }
        match self.store.ailiak_task_for_message(&self.todo_message_id) {
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
        let compose = self.todo_compose.clone();
        let task_number = Some(short_task_number(&message_id));
        // Day comes from slash commands (`/from`, `/date`, `/todo`); no UI day chips.
        let payload = build_payload(&compose, "", None, task_number, &cfg);
        let for_date_status = payload.for_date.clone();
        let tx = self.bg_tx.clone();
        let db_path = self.paths.db_file();

        self.status = "Creating Todo on Desk…".into();
        self.busy.ailiak = true;
        thread::spawn(move || {
            let result = (|| -> Result<(String, Option<String>)> {
                let mut client = AiliakClient::new(base, token)?;
                let created = client.create_task(&payload)?;
                let store = Store::open_for_worker(&db_path)?;
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
        self.busy.ailiak = true;
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
        self.busy.ailiak = true;
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
                    self.mail_password_saved = true;
                    account.auth = MailAuthMethod::Password;
                    should_sync = true;
                } else if account.auth.is_oauth() || secrets::has_mail_password_local(&id) {
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

    /// Deferred boot sync + periodic auto-sync (`ui.auto_sync_mins`).
    fn tick_auto_sync(&mut self, ctx: &egui::Context) {
        if let Some(due) = self.startup_sync_at {
            if self.busy.sync {
                self.startup_sync_at = None;
            } else if Instant::now() >= due {
                self.startup_sync_at = None;
                if !self.config.accounts.is_empty() {
                    tracing::info!("startup sync — current folder only");
                    self.start_sync(false);
                }
                return;
            } else {
                ctx.request_repaint_after((due - Instant::now()).max(Duration::from_millis(200)));
            }
        }

        let mins = self.config.ui.auto_sync_mins;
        if mins == 0 || self.busy.sync || self.config.accounts.is_empty() {
            return;
        }
        let interval = Duration::from_secs(u64::from(mins) * 60);
        let elapsed = self.last_sync_at.elapsed();
        if elapsed >= interval {
            self.start_sync(false);
            return;
        }
        let remaining = interval.saturating_sub(elapsed);
        // Wake often enough to stay close to the interval without busy-looping.
        let wake = remaining.min(Duration::from_secs(60)).max(Duration::from_secs(1));
        ctx.request_repaint_after(wake);
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
                ui.monospace("Shift+↑/↓ extend multi-select in the list");
                ui.monospace("Ctrl+A    select all visible messages");
                ui.monospace("/         focus search");
                ui.monospace("g         sync selected folder");
                ui.monospace("Shift+G   sync all folders");
                ui.monospace("d         open Desk in browser (or click logo)");
                ui.monospace("a         Options (note + task)");
                ui.monospace("c         calendar (Desk days)");
                ui.monospace("n         new message (compose)");
                ui.monospace("r         reply (or focus chat reply)");
                ui.monospace("Shift+R   reply all");
                ui.monospace("e         done / archive (selection or current)");
                ui.monospace("# / Del   trash (selection or current)");
                ui.monospace(",         settings page");
                ui.monospace("Esc       leave settings / multi-select / compose");
                ui.monospace("?         this help");
                ui.add_space(8.0);
                ui.label(RichText::new("Options / Task slash").strong());
                ui.label(
                    "In Options → Task: /from tomorrow, /date, /todo, /estimate 1h, /spent 30m, /details, /done, #mail",
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
        if self.show_help || self.show_feedback {
            return;
        }

        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escape && !ctx.wants_keyboard_input() {
            if self.pending_bulk_move.is_some() {
                self.cancel_pending_bulk_move();
                return;
            }
            if self.compose.is_some() {
                self.compose = None;
                return;
            }
            if self.list_multi_select {
                self.toggle_list_multi_select();
                return;
            }
            if self.show_options || self.show_calendar {
                self.close_options_panel();
                self.show_calendar = false;
                return;
            }
            if self.selected_person.is_some() {
                self.selected_person = None;
                self.chat_messages.clear();
                self.chat_reply_focused = false;
                return;
            }
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
            // Ctrl/Cmd+A — select all mid-list messages (not plain `a` = Todo).
            if i.key_pressed(egui::Key::A)
                && (i.modifiers.ctrl || i.modifiers.command)
                && !i.modifiers.alt
                && !i.modifiers.shift
            {
                self.select_all_visible_messages();
            }
            if i.key_pressed(egui::Key::D) && !i.modifiers.any() {
                self.open_desk();
            }
            if i.key_pressed(egui::Key::A) && !i.modifiers.any() {
                self.toggle_options();
            }
            if i.key_pressed(egui::Key::C) && !i.modifiers.any() {
                self.toggle_calendar();
            }
            if i.key_pressed(egui::Key::N) && !i.modifiers.any() {
                self.open_compose_new();
            }
            if i.key_pressed(egui::Key::R) && !i.modifiers.shift && !i.modifiers.ctrl {
                if self.selected_person.is_some() {
                    self.chat_reply_focused = true;
                } else {
                    self.open_reply(false);
                }
            }
            if i.key_pressed(egui::Key::R) && i.modifiers.shift {
                self.open_reply(true);
            }
            if i.key_pressed(egui::Key::E) && !i.modifiers.any() {
                if self.list_multi_select && !self.list_selected.is_empty() {
                    self.move_listed_messages(SpecialFolderKind::Archive);
                } else {
                    self.move_current_message(SpecialFolderKind::Archive);
                }
            }
            let trash = i.key_pressed(egui::Key::Delete)
                || i.events
                    .iter()
                    .any(|ev| matches!(ev, egui::Event::Text(t) if t == "#"));
            if trash {
                if self.list_multi_select && !self.list_selected.is_empty() {
                    self.move_listed_messages(SpecialFolderKind::Trash);
                } else {
                    self.move_current_message(SpecialFolderKind::Trash);
                }
            }
            if i.key_pressed(egui::Key::G) {
                self.start_sync(i.modifiers.shift);
            }
        });

        let (delta, extend) = ctx.input(|i| {
            let down = i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown);
            let up = i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp);
            if down {
                Some((1isize, i.modifiers.shift))
            } else if up {
                Some((-1isize, i.modifiers.shift))
            } else {
                None
            }
        })
        .map_or((None, false), |(d, e)| (Some(d), e));
        if let Some(delta) = delta {
            self.navigate_list(delta, extend);
        }
    }
}

fn load_outgoing_attachments(
    items: &[crate::compose::ComposeAttachment],
) -> Result<Vec<mailbox_core::OutgoingAttachment>, String> {
    let mut out = Vec::with_capacity(items.len());
    for att in items {
        let bytes = std::fs::read(&att.path)
            .map_err(|e| format!("Could not read attachment {}: {e}", att.name))?;
        if bytes.len() > 25 * 1024 * 1024 {
            return Err(format!("Attachment {} is larger than 25 MB", att.name));
        }
        out.push(mailbox_core::OutgoingAttachment {
            filename: att.name.clone(),
            mime: crate::compose::mime_for_path(&att.path),
            bytes,
        });
    }
    Ok(out)
}

impl App for MailboxApp {
    /// eframe defaults to a dark semi-transparent clear color; any hairline gap
    /// between SidePanel and CentralPanel would show as a muddy strip.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Mild zoom on large monitors (laptop stay ~1.0). Uniform PPP keeps layout ratios.
        theme::apply_screen_zoom(ctx);

        // Tear down WebKitGTK *before* winit destroys the host window — otherwise
        // wry’s Drop (destroy + X11 child teardown) can freeze the process on close.
        if self.exit_after_webview {
            self.exit_after_webview = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if ctx.input(|i| i.viewport().close_requested())
            && (self.mail_webview.is_alive() || self.compose_webview.is_alive())
        {
            self.mail_webview.destroy();
            self.compose_webview.destroy();
            self.exit_after_webview = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.request_repaint();
            return;
        }

        self.poll_bg();
        self.poll_avatars(ctx);
        self.logos.poll(ctx);
        self.mail_images.poll(ctx);
        crate::mail_webview::MailWebView::pump_platform();
        self.poll_todo_selection_insert();
        if self.pending_todo_selection {
            ctx.request_repaint();
        }
        self.sync_options_with_open_message();
        self.sync_compose_from_webview();
        if self.pending_compose_send {
            self.pending_compose_send_frames =
                self.pending_compose_send_frames.saturating_add(1);
            if let Some(c) = self.compose_webview.take_flush() {
                if let Some(draft) = self.compose.as_mut() {
                    let flush_empty = c.text.trim().is_empty()
                        && crate::compose::html_looks_empty(&c.html);
                    let draft_has_body = !draft.body_is_empty();
                    // Never wipe a good draft with an empty flush (WebKit race / reload).
                    if !flush_empty || !draft_has_body {
                        if !c.text.trim().is_empty() {
                            draft.body = c.text;
                        } else if draft.body.trim().is_empty() && !c.html.trim().is_empty() {
                            draft.body = mailbox_core::html_to_plain(&c.html);
                        }
                        if !crate::compose::html_looks_empty(&c.html) {
                            draft.body_html = c.html;
                        }
                    }
                }
                self.finish_submit_compose();
            } else if !self.compose_webview.is_alive()
                || self.compose_webview.flush_failed()
                || self.pending_compose_send_frames >= 45
            {
                // ~0.7s timeout or failed callback — send with last IPC-synced body.
                self.finish_submit_compose();
            } else {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
        theme::sync_system(ctx, self.config.ui.theme);
        self.handle_keys(ctx);
        self.tick_auto_sync(ctx);

        // Reader / composer set these while painting; clear first so a missing request hides.
        self.mail_webview_req = None;
        self.compose_webview_req = None;

        if self.show_help {
            self.help_window(ctx);
        }
        if FEEDBACK_UI_ENABLED && self.show_feedback {
            self.feedback_window(ctx);
        }
        if self.pending_bulk_move.is_some() {
            self.show_bulk_confirm_dialog(ctx);
        }

        self.show_shell(ctx);

        if self.config.ui.sound_clicks && ctx.input(|i| i.pointer.primary_clicked()) {
            crate::sounds::play_click(self.config.ui.sound_theme);
        }

        // Compose paints inside the mail body card (see mail_body_pane).

        // Compose editor webview (above egui). Hide mail reader while composing.
        let hide_mail = self.show_settings
            || self.show_help
            || self.show_feedback
            || self.compose.is_some()
            || self.pending_bulk_move.is_some();
        if hide_mail {
            self.mail_webview_miss_frames = 0;
            self.mail_webview.hide();
        } else if let Some(req) = self.mail_webview_req.take() {
            self.mail_webview_miss_frames = 0;
            self.mail_webview.show_html(
                frame,
                &req.msg_id,
                &req.body_html,
                req.load_images,
                req.dark,
                req.bounds,
                req.pixels_per_point,
            );
        } else {
            // One missing frame is common while panels settle; don't park/unmap
            // immediately (that path used to spam Gdk frame-clock asserts).
            self.mail_webview_miss_frames = self.mail_webview_miss_frames.saturating_add(1);
            if self.mail_webview_miss_frames >= 3 {
                self.mail_webview.hide();
            }
        }
        self.mail_webview.poll_links();

        if let Some(req) = self.compose_webview_req.take() {
            self.compose_webview.show(frame, &req);
            ctx.request_repaint_after(Duration::from_millis(50));
        } else {
            self.compose_webview.hide();
        }

        if self.busy.any()
            || self.calendar.loading
            || self.pending_compose_send
            || (!self.status.contains("Ready") && self.status.ends_with('…'))
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.compose_webview.destroy();
        self.mail_webview.destroy();
    }
}

/// Minimal application/x-www-form-urlencoded encoder for GitHub issue query params.
fn query_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0xf) as usize] as char);
            }
        }
    }
    out
}
