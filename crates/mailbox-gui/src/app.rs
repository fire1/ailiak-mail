use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::Result;
use eframe::App;
use egui::{Color32, RichText, TextureHandle};
use mailbox_ailiak::{
    build_payload, compose_hint, parse_compose_commands, short_task_number, todo_defaults,
    AiliakClient, DeskTask,
};
use mailbox_core::config::{AppConfig, MailAccountConfig, ThemeMode};
use mailbox_core::paths::AppPaths;
use mailbox_core::secrets;
use mailbox_core::seed::{demo_account_id, seed_demo_if_empty};
use mailbox_core::store::Store;
use mailbox_core::sync::{sync_account, test_connection, SyncOptions};
use mailbox_core::types::{Envelope, Folder, Message, MessageId};

use crate::brand;
use crate::calendar::CalendarState;
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
    pub(crate) status: String,
    pub(crate) show_settings: bool,
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
    pub(crate) imap_host_input: String,
    pub(crate) imap_user_input: String,
    pub(crate) imap_password_input: String,
    pub(crate) smtp_host_input: String,
    pub(crate) ailiak_mark: TextureHandle,
    bg_tx: Sender<BgMsg>,
    bg_rx: Receiver<BgMsg>,
}

impl MailboxApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
        // Theme styles installed after config load (below).

        let paths = AppPaths::resolve()?;
        paths.ensure()?;
        let config = AppConfig::load_or_init(&paths)?;
        theme::install(&cc.egui_ctx, config.ui.theme);
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
            "Ready — Sync to pull mail from IMAP".into()
        } else {
            "Ready — demo mail, or add Gmail in Settings".into()
        };

        let (bg_tx, bg_rx) = mpsc::channel();
        let ailiak_mark = brand::load_ailiak_mark(&cc.egui_ctx);

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
            status,
            show_settings: false,
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
            imap_host_input: String::new(),
            imap_user_input: String::new(),
            imap_password_input: String::new(),
            smtp_host_input: String::new(),
            ailiak_mark,
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

    fn apply_gmail_preset(&mut self) {
        let email = if self.account_email_input.trim().is_empty() {
            "you@gmail.com".into()
        } else {
            self.account_email_input.trim().to_string()
        };
        let g = MailAccountConfig::gmail(email);
        self.account_name_input = g.name;
        self.account_email_input = g.email.clone();
        self.imap_host_input = g.imap_host;
        self.imap_user_input = g.username;
        self.smtp_host_input = g.smtp_host;
        self.status = "Gmail preset filled — paste an App Password, Save, then Test / Sync".into();
    }

    fn current_account_draft(&self) -> Option<MailAccountConfig> {
        let email = self.account_email_input.trim();
        if email.is_empty() && self.imap_host_input.trim().is_empty() {
            return self.config.primary_account().cloned();
        }
        let id = self
            .config
            .primary_account()
            .map(|a| a.id.clone())
            .unwrap_or_else(|| {
                if self.imap_host_input.eq_ignore_ascii_case("imap.gmail.com") {
                    "gmail".into()
                } else {
                    "primary".into()
                }
            });
        Some(MailAccountConfig {
            id,
            name: if self.account_name_input.trim().is_empty() {
                email.to_string()
            } else {
                self.account_name_input.trim().to_string()
            },
            email: email.to_string(),
            imap_host: self.imap_host_input.trim().to_string(),
            imap_port: 993,
            imap_tls: true,
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

    fn resolve_mail_password(&self, account_id: &str) -> Result<String, String> {
        if !self.imap_password_input.is_empty() {
            return Ok(self.imap_password_input.clone());
        }
        match secrets::get_mail_password(account_id) {
            Ok(Some(p)) => Ok(p),
            Ok(None) => {
                Err("No password in keyring — paste a Gmail App Password in Settings".into())
            }
            Err(e) => Err(format!("Keyring: {e}")),
        }
    }

    fn test_mail_connection(&mut self) {
        let Some(account) = self.current_account_draft() else {
            self.status = "Enter your Gmail address first".into();
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
        let label = account.email.clone();
        self.status = format!("Testing IMAP login for {label}…");
        thread::spawn(move || match test_connection(&account, &password) {
            Ok(()) => {
                let _ = tx.send(BgMsg::TestOk(format!("IMAP OK for {label}")));
            }
            Err(e) => {
                let _ = tx.send(BgMsg::Error(e.to_string()));
            }
        });
    }

    pub(crate) fn start_sync(&mut self) {
        let Some(account) = self
            .config
            .primary_account()
            .cloned()
            .or_else(|| self.current_account_draft())
        else {
            self.status = "Add a Gmail account in Settings first".into();
            self.show_settings = true;
            return;
        };
        let password = match self.resolve_mail_password(&account.id) {
            Ok(p) => p,
            Err(e) => {
                self.status = e;
                self.show_settings = true;
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

    fn reload_settings_fields(&mut self) {
        if let Some(a) = self.config.primary_account() {
            self.account_name_input = a.name.clone();
            self.account_email_input = a.email.clone();
            self.imap_host_input = a.imap_host.clone();
            self.imap_user_input = a.username.clone();
            self.smtp_host_input = a.smtp_host.clone();
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

    pub(crate) fn open_todo(&mut self) {
        let Some(msg) = self.open_message.clone() else {
            self.status = "Open a message first".into();
            return;
        };
        if !self.config.ailiak.enabled {
            self.status = "Enable Ailiak (bottom-left) in Settings first".into();
            self.show_settings = true;
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

    fn submit_todo(&mut self) {
        if self.todo_message_id.is_empty() {
            self.status = "No message linked for Todo".into();
            return;
        }
        let token = match secrets::get_ailiak_token() {
            Ok(Some(t)) => t,
            Ok(None) => {
                self.status = "No Ailiak token — add one in Settings".into();
                self.show_settings = true;
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

    fn test_ailiak(&mut self) {
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

    fn save_settings(&mut self) -> Result<()> {
        if let Some(account) = self.current_account_draft() {
            if !account.email.is_empty() && !account.imap_host.is_empty() {
                let id = account.id.clone();
                if !self.imap_password_input.is_empty() {
                    secrets::set_mail_password(&id, &self.imap_password_input)?;
                    self.imap_password_input.clear();
                }
                self.account_id = id;
                self.config.accounts = vec![account];
                self.config.ui.use_demo_data = false;
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

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("Mail account").strong());
                ui.label(
                    RichText::new(
                        "Gmail: use an App Password (Google Account → Security → App passwords). \
                         Stored in the OS keyring, never in config.toml.",
                    )
                    .small()
                    .color(theme::mute(ui.ctx())),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Use Gmail defaults").clicked() {
                        self.apply_gmail_preset();
                    }
                });
                ui.add_space(6.0);
                egui::Grid::new("account_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut self.account_name_input);
                        ui.end_row();
                        ui.label("Email");
                        ui.text_edit_singleline(&mut self.account_email_input);
                        ui.end_row();
                        ui.label("IMAP host");
                        ui.text_edit_singleline(&mut self.imap_host_input);
                        ui.end_row();
                        ui.label("Username");
                        ui.text_edit_singleline(&mut self.imap_user_input);
                        ui.end_row();
                        ui.label("App password");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.imap_password_input)
                                .password(true)
                                .hint_text("16-char Gmail app password"),
                        );
                        ui.end_row();
                        ui.label("SMTP host");
                        ui.text_edit_singleline(&mut self.smtp_host_input);
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Test IMAP").clicked() {
                        self.test_mail_connection();
                    }
                    if ui.button("Save account").clicked() {
                        if let Err(e) = self.save_settings() {
                            self.status = e.to_string();
                        }
                    }
                    if ui.button("Sync now").clicked() {
                        if let Err(e) = self.save_settings() {
                            self.status = e.to_string();
                        } else {
                            self.start_sync();
                        }
                    }
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(RichText::new("Appearance").strong());
                ui.label(
                    RichText::new("Default follows the system light/dark setting.")
                        .small()
                        .color(theme::mute(ui.ctx())),
                );
                let mut mode = self.config.ui.theme;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut mode, ThemeMode::System, "System");
                    ui.selectable_value(&mut mode, ThemeMode::Light, "Light");
                    ui.selectable_value(&mut mode, ThemeMode::Dark, "Dark");
                });
                if mode != self.config.ui.theme {
                    self.config.ui.theme = mode;
                    theme::apply_preference(ui.ctx(), mode);
                    if let Err(e) = self.config.save(&self.paths.config_file()) {
                        self.status = e.to_string();
                    } else {
                        self.status = format!("Theme: {}", theme::resolved_label(mode));
                    }
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(RichText::new("Ailiak").strong());
                ui.checkbox(&mut self.config.ailiak.enabled, "Enable Send to Ailiak");
                egui::Grid::new("ailiak_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Base URL");
                        ui.text_edit_singleline(&mut self.config.ailiak.base_url);
                        ui.end_row();
                        ui.label("Agent token");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.ailiak_token_input)
                                .password(true)
                                .hint_text("ailiak_…"),
                        );
                        ui.end_row();
                        ui.label("Hashtag");
                        ui.text_edit_singleline(&mut self.config.ailiak.hashtag);
                        ui.end_row();
                    });
                ui.horizontal(|ui| {
                    if ui.button("Test whoami").clicked() {
                        self.test_ailiak();
                    }
                    if ui.button("Clear token").clicked() {
                        if let Err(e) = secrets::delete_ailiak_token() {
                            self.status = e.to_string();
                        } else {
                            self.config.ailiak.enabled = false;
                            self.status = "Ailiak token removed".into();
                        }
                    }
                });

                ui.add_space(12.0);
                ui.label(
                    RichText::new(format!(
                        "Config: {}\nData: {}",
                        self.paths.config_file().display(),
                        self.paths.db_file().display()
                    ))
                    .small()
                    .color(theme::mute(ui.ctx())),
                );

                ui.add_space(8.0);
                if ui.button("Save").clicked() {
                    if let Err(e) = self.save_settings() {
                        self.status = e.to_string();
                    }
                }
            });
        self.show_settings = open;
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
                ui.monospace("g         sync selected folder");
                ui.monospace("a         Todo (mail → Ailiak)");
                ui.monospace("c         calendar (Desk days)");
                ui.monospace(",         settings");
                ui.monospace("?         this help");
                ui.add_space(8.0);
                ui.label(RichText::new("Todo / slash").strong());
                ui.label(
                    "In the Todo strip: /from tomorrow, /todo, /estimate 1h, /spent 30m, /done, #mail",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Gmail").strong());
                ui.label(
                    "Settings → Use Gmail defaults → paste an App Password → Test IMAP → Sync.",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Theme").strong());
                ui.label("Settings → Appearance: System (default), Light, or Dark.");
                ui.add_space(8.0);
                ui.label(RichText::new("Principles").strong());
                ui.label("Mail stays in Mailbox. Ailiak is the desk for work that needs tracking.");
            });
        self.show_help = open;
    }

    fn todo_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_todo;
        egui::Window::new("Todo → Ailiak Desk")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Same slash language as Ailiak compose")
                        .small()
                        .color(theme::mute(ui.ctx())),
                );
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("Day");
                    let today = mailbox_ailiak::today_key();
                    let tomorrow = mailbox_ailiak::shift_day(&today, 1);
                    let sel = self.calendar.selected.clone();
                    if ui
                        .selectable_label(sel.as_deref() == Some(today.as_str()), "Today")
                        .clicked()
                    {
                        self.calendar.select_today();
                    }
                    if ui
                        .selectable_label(sel.as_deref() == Some(tomorrow.as_str()), "Tomorrow")
                        .clicked()
                    {
                        self.calendar.select_tomorrow();
                    }
                    if ui
                        .selectable_label(sel.as_deref() == Some(""), "None")
                        .clicked()
                    {
                        self.calendar.clear_day();
                    }
                    if let Some(d) = self.calendar.selected.as_deref() {
                        if !d.is_empty() {
                            ui.label(RichText::new(d).small().color(theme::accent(ui.ctx())));
                        }
                    }
                    if ui.small_button("Calendar").clicked() {
                        self.show_calendar = true;
                    }
                });

                ui.add_space(6.0);
                ui.label("Line (slash commands ok)");
                ui.add(
                    egui::TextEdit::multiline(&mut self.todo_line)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text("Reply: …  #mail   /from tomorrow /estimate 1h"),
                );
                let hint = compose_hint(&parse_compose_commands(&self.todo_line));
                ui.label(RichText::new(hint).small().color(theme::mute(ui.ctx())));

                ui.add_space(6.0);
                ui.label("Detail");
                ui.add(
                    egui::TextEdit::multiline(&mut self.todo_detail)
                        .desired_rows(8)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Create on Desk").color(Color32::WHITE),
                            )
                            .fill(theme::accent(ui.ctx())),
                        )
                        .clicked()
                    {
                        self.submit_todo();
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_todo = false;
                    }
                });
            });
        self.show_todo = open;
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.show_settings || self.show_help || self.show_todo {
            return;
        }
        ctx.input(|i| {
            if i.modifiers.shift && i.key_pressed(egui::Key::Slash) {
                self.show_help = true;
            }
            if i.key_pressed(egui::Key::Comma) {
                self.show_settings = true;
            }
            if i.key_pressed(egui::Key::A) && !i.modifiers.any() {
                self.open_todo();
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
        theme::sync_system(ctx, self.config.ui.theme);
        self.handle_keys(ctx);

        if self.show_settings {
            self.settings_window(ctx);
        }
        if self.show_help {
            self.help_window(ctx);
        }
        if self.show_todo {
            self.todo_window(ctx);
        }

        self.show_shell(ctx);

        if (!self.status.contains("Ready") && self.status.ends_with('…')) || self.calendar.loading
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}
