use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::Result;
use eframe::App;
use egui::RichText;
use mailbox_ailiak::{
    build_payload, short_task_number, todo_defaults, AiliakClient, DeskTask,
};
use mailbox_core::config::{AppConfig, MailAccountConfig, MailProtocol};
use mailbox_core::paths::AppPaths;
use mailbox_core::secrets;
use mailbox_core::seed::{demo_account_id, seed_demo_if_empty};
use mailbox_core::store::Store;
use mailbox_core::sync::{sync_account, test_connection, SyncOptions};
use mailbox_core::types::{Envelope, Folder, Message, MessageId};

use crate::avatars::{AvatarCache, AvatarMsg};
use crate::brand::AiliakLogo;
use crate::calendar::CalendarState;
use crate::settings::SettingsSection;
use crate::theme;

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
    },
    TestOk(String),
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
    pub(crate) selected_message: Option<MessageId>,
    pub(crate) open_message: Option<Message>,
    pub(crate) search: String,
    /// Request keyboard focus on the top-bar search field (set by `/`).
    pub(crate) focus_search: bool,
    pub(crate) status: String,
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
    pub(crate) smtp_host_input: String,
    /// Account id currently edited in Settings (may be a new draft).
    pub(crate) editing_account_id: String,
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
            "Ready — demo mail, or add Gmail / POP3 in Settings".into()
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
            selected_message: None,
            open_message: None,
            search: String::new(),
            focus_search: false,
            status,
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
            smtp_host_input: String::new(),
            editing_account_id,
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

    pub(crate) fn apply_gmail_preset(&mut self) {
        let email = if self.account_email_input.trim().is_empty() {
            "you@gmail.com".into()
        } else {
            self.account_email_input.trim().to_string()
        };
        let g = MailAccountConfig::gmail(email);
        self.mail_protocol = MailProtocol::Imap;
        self.account_name_input = g.name;
        self.account_email_input = g.email.clone();
        self.imap_host_input = g.imap_host;
        self.mail_port_input = g.imap_port.to_string();
        self.mail_tls = g.imap_tls;
        self.imap_user_input = g.username;
        self.smtp_host_input = g.smtp_host;
        self.status = "Gmail preset filled — paste an App Password, Save, then Test / Sync".into();
    }

    pub(crate) fn apply_pop3_preset(&mut self) {
        let email = self.account_email_input.trim().to_string();
        let p = MailAccountConfig::pop3_template(if email.is_empty() {
            "you@example.com".into()
        } else {
            email
        });
        self.mail_protocol = MailProtocol::Pop3;
        if self.account_name_input.trim().is_empty() {
            self.account_name_input = p.name;
        }
        self.mail_port_input = p.imap_port.to_string();
        self.mail_tls = p.imap_tls;
        if self.imap_user_input.trim().is_empty() && !self.account_email_input.trim().is_empty() {
            self.imap_user_input = self.account_email_input.trim().to_string();
        }
        self.status =
            "POP3 defaults (port 995, TLS) — set host, password, Save, then Test / Sync".into();
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
            smtp_port: 587,
            smtp_tls: true,
            username: if self.imap_user_input.trim().is_empty() {
                email.to_string()
            } else {
                self.imap_user_input.trim().to_string()
            },
        })
    }

    pub(crate) fn switch_account(&mut self, id: &str) {
        if self.account_id == id {
            self.editing_account_id = id.to_string();
            self.reload_settings_fields();
            return;
        }
        self.account_id = id.to_string();
        self.editing_account_id = id.to_string();
        self.selected_message = None;
        self.open_message = None;
        self.search.clear();
        self.reload_settings_fields();
        if let Err(e) = self.refresh_folders() {
            self.status = e.to_string();
        } else if let Err(e) = self.refresh_envelopes() {
            self.status = e.to_string();
        } else if let Some(first) = self.envelopes.first().map(|e| e.id.clone()) {
            let _ = self.open_id(&first);
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
        self.smtp_host_input.clear();
        self.open_settings(SettingsSection::Accounts);
        self.status = "New account — fill details and Save".into();
    }

    fn resolve_mail_password(&self, account_id: &str) -> Result<String, String> {
        if !self.imap_password_input.is_empty() {
            return Ok(self.imap_password_input.clone());
        }
        match secrets::get_mail_password(account_id) {
            Ok(Some(p)) => Ok(p),
            Ok(None) => Err(
                "No password in keyring — paste your mail password in Settings".into(),
            ),
            Err(e) => Err(format!("Keyring: {e}")),
        }
    }

    pub(crate) fn test_mail_connection(&mut self) {
        let Some(account) = self.current_account_draft() else {
            self.status = "Enter email and server host first".into();
            return;
        };
        let password = match self.resolve_mail_password(&account.id) {
            Ok(p) => p,
            Err(e) => {
                self.status = e;
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
        self.status = format!("Testing {proto} login for {label}…");
        thread::spawn(move || match test_connection(&account, &password) {
            Ok(()) => {
                let _ = tx.send(BgMsg::TestOk(format!("{proto} OK for {label}")));
            }
            Err(e) => {
                let _ = tx.send(BgMsg::Error(e.to_string()));
            }
        });
    }

    pub(crate) fn start_sync(&mut self) {
        let Some(account) = self
            .selected_account()
            .cloned()
            .or_else(|| self.current_account_draft())
        else {
            self.status = "Add a mail account in Settings first".into();
            self.open_settings(SettingsSection::Accounts);
            return;
        };
        let password = match self.resolve_mail_password(&account.id) {
            Ok(p) => p,
            Err(e) => {
                self.status = e;
                self.open_settings(SettingsSection::Accounts);
                return;
            }
        };

        // Persist password if typed this session
        if !self.imap_password_input.is_empty() {
            if let Err(e) = secrets::set_mail_password(&account.id, &password) {
                self.status = format!("Keyring: {e}");
                return;
            }
            self.imap_password_input.clear();
        }

        let db_path = self.paths.db_file();
        let folder = self.selected_folder.clone();
        let tx = self.bg_tx.clone();
        self.status = format!("Syncing {}…", folder);
        thread::spawn(move || {
            let result = (|| -> Result<_> {
                let store = Store::open(&db_path)?;
                let report = sync_account(
                    &store,
                    &account,
                    &password,
                    SyncOptions {
                        max_messages: 40,
                        folder_path: folder,
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
                    });
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::Error(e.to_string()));
                }
            }
        });
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
        if let Some(msg) = self.store.get_message(id)? {
            if !msg.envelope.seen {
                self.store.mark_seen(id, true)?;
            }
            self.selected_message = Some(id.clone());
            self.open_message = Some(msg);
            self.refresh_envelopes()?;
        }
        Ok(())
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
                BgMsg::TestOk(s) => {
                    self.status = s;
                }
                BgMsg::SyncOk {
                    account_id,
                    folders,
                    messages,
                    folder_path,
                } => {
                    self.account_id = account_id;
                    self.selected_folder = folder_path.clone();
                    self.status = format!(
                        "Synced {messages} messages from {folder_path} ({folders} folders)"
                    );
                    if let Err(e) = self.refresh_folders() {
                        self.status = e.to_string();
                    } else if let Err(e) = self.refresh_envelopes() {
                        self.status = e.to_string();
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
                    self.status = format!("Error: {e}");
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
        self.avatars.resolve(
            ctx,
            &self.paths.avatars_dir(),
            email,
            name,
            &self.avatar_tx,
        )
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

    /// Toggle the Todo side panel (same idea as Calendar).
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
        self.show_todo = true;
        self.show_calendar = true;
        self.refresh_desk_tasks();
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
        if let Some(mut account) = self.current_account_draft() {
            if !account.imap_host.is_empty()
                && (!account.email.is_empty() || !account.username.is_empty())
            {
                if account.id.is_empty() {
                    account.id = Self::fresh_account_id(&account.email);
                }
                let id = account.id.clone();
                if !self.imap_password_input.is_empty() {
                    secrets::set_mail_password(&id, &self.imap_password_input)?;
                    self.imap_password_input.clear();
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
        self.status = format!("Saved {}", self.paths.config_file().display());
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
                    "Gmail: Settings → Use Gmail defaults → App Password → Test IMAP → Sync.",
                );
                ui.label(
                    "Older servers: Settings → POP3 → host/port (995 TLS or 110 plain) → Test → Sync.",
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
        if escape && !ctx.wants_keyboard_input() && self.show_todo {
            self.show_todo = false;
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
                self.show_calendar = !self.show_calendar;
                if self.show_calendar {
                    self.refresh_desk_tasks();
                }
            }
            if i.key_pressed(egui::Key::G) && !i.modifiers.any() {
                self.start_sync();
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

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_bg();
        self.poll_avatars(ctx);
        theme::sync_system(ctx, self.config.ui.theme);
        self.handle_keys(ctx);

        if self.show_help {
            self.help_window(ctx);
        }

        self.show_shell(ctx);

        if (!self.status.contains("Ready") && self.status.ends_with('…')) || self.calendar.loading
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}
