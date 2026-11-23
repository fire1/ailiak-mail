//! Full-page Settings (Appearance first — matches Ailiak.com Studio).

use egui::{Align, Color32, Layout, RichText, Sense, Ui, Vec2};
use egui_phosphor::regular as ph;

use mailbox_core::config::{MailAccountConfig, MailAuthMethod, MailProtocol, ThemeMode};
use mailbox_core::oauth::OAuthProvider;
use mailbox_core::providers::MailProvider;
use mailbox_core::secrets;

use crate::app::MailboxApp;
use crate::provider_logos;
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Appearance,
    Accounts,
    Folders,
    Ailiak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountWizardStep {
    /// Pick Gmail / Yahoo / … / IMAP / POP3.
    Provider,
    /// Name, email, password (+ hosts for custom).
    Credentials,
}

#[derive(Debug, Clone)]
pub struct AccountWizard {
    pub step: AccountWizardStep,
    pub provider: Option<MailProvider>,
}

impl Default for AccountWizard {
    fn default() -> Self {
        Self {
            step: AccountWizardStep::Provider,
            provider: None,
        }
    }
}

impl SettingsSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Accounts => "Accounts",
            Self::Folders => "Folders",
            Self::Ailiak => "Ailiak",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Self::Appearance => "Theme and accent",
            Self::Accounts => "Mail accounts and avatars",
            Self::Folders => "Show, hide, and sync",
            Self::Ailiak => "Desk token and hashtag",
        }
    }

    const ALL: [Self; 4] = [
        Self::Appearance,
        Self::Accounts,
        Self::Folders,
        Self::Ailiak,
    ];
}

impl MailboxApp {
    pub(crate) fn settings_page(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let back = ui
                .add(
                    egui::Button::new(
                        RichText::new(ph::CARET_LEFT)
                            .family(theme::icon_family())
                            .size(18.0)
                            .color(theme::ink(ui.ctx())),
                    )
                    .frame(false),
                )
                .on_hover_text("Back to mail (Esc)");
            if back.clicked() {
                self.show_settings = false;
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new("Settings")
                    .strong()
                    .size(16.0)
                    .family(theme::nav_family())
                    .color(theme::ink(ui.ctx())),
            );
        });
        ui.add_space(12.0);

        let body_h = ui.available_height();
        ui.horizontal(|ui| {
            ui.set_min_height(body_h);

            // Section list (own vertical column)
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.set_width(200.0);
                ui.set_min_height(body_h);
                for section in SettingsSection::ALL {
                    let selected = self.settings_section == section;
                    let fill = if selected {
                        theme::select(ui.ctx())
                    } else {
                        Color32::TRANSPARENT
                    };
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());
                    if resp.hovered() && !selected {
                        ui.painter()
                            .rect_filled(rect, 8.0, theme::chip(ui.ctx()));
                    } else {
                        ui.painter().rect_filled(rect, 8.0, fill);
                    }
                    let title_color = if selected {
                        theme::accent(ui.ctx())
                    } else {
                        theme::ink(ui.ctx())
                    };
                    ui.painter().text(
                        egui::pos2(rect.left() + 12.0, rect.top() + 14.0),
                        egui::Align2::LEFT_CENTER,
                        section.label(),
                        theme::nav_font(13.0),
                        title_color,
                    );
                    ui.painter().text(
                        egui::pos2(rect.left() + 12.0, rect.top() + 34.0),
                        egui::Align2::LEFT_CENTER,
                        section.blurb(),
                        egui::FontId::proportional(11.0),
                        theme::mute(ui.ctx()),
                    );
                    if resp.clicked() {
                        self.settings_section = section;
                    }
                    ui.add_space(4.0);
                }
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(16.0);

            // Content must be top-down — horizontal parent would inline every widget.
            let content_w = (ui.available_width() - 8.0).max(320.0);
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.set_width(content_w);
                ui.set_min_height(body_h);
                egui::ScrollArea::vertical()
                    .id_salt("settings_content")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(content_w);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.set_width(content_w.max(0.0) - 4.0);
                            match self.settings_section {
                                SettingsSection::Appearance => self.appearance_panel(ui),
                                SettingsSection::Accounts => self.accounts_panel(ui),
                                SettingsSection::Folders => self.folders_panel(ui),
                                SettingsSection::Ailiak => self.ailiak_panel(ui),
                            }
                        });
                    });
            });
        });
    }

    fn appearance_panel(&mut self, ui: &mut Ui) {
        ui.label(
            RichText::new("Appearance")
                .strong()
                .size(16.0)
                .family(theme::nav_family()),
        );
        ui.label(
            RichText::new("Same theme and accent choices as Ailiak.com Desk.")
                .small()
                .color(theme::mute(ui.ctx())),
        );
        ui.add_space(16.0);

        ui.label(RichText::new("Theme").strong());
        ui.add_space(6.0);
        let mut mode = self.config.ui.theme;
        ui.horizontal(|ui| {
            theme_chip(ui, &mut mode, ThemeMode::System, "System");
            theme_chip(ui, &mut mode, ThemeMode::Light, "Light");
            theme_chip(ui, &mut mode, ThemeMode::Dark, "Dark");
        });
        if mode != self.config.ui.theme {
            self.config.ui.theme = mode;
            theme::apply_appearance(ui.ctx(), mode, &self.config.ui.accent);
            self.persist_ui_prefs(ui.ctx());
        }

        ui.add_space(20.0);
        ui.label(RichText::new("Accent").strong());
        ui.label(
            RichText::new("Highlights, selection, and links.")
                .small()
                .color(theme::mute(ui.ctx())),
        );
        ui.add_space(8.0);

        let current = self.config.ui.accent.clone();
        let mut picked: Option<&'static str> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
            for opt in theme::ACCENT_OPTIONS {
                let selected = current == opt.id;
                ui.vertical(|ui| {
                    ui.set_width(56.0);
                    ui.with_layout(Layout::top_down(Align::Center), |ui| {
                        let size = Vec2::splat(28.0);
                        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                        let center = rect.center();
                        ui.painter().circle_filled(center, 12.0, opt.color);
                        if selected {
                            ui.painter().circle_stroke(
                                center,
                                13.5,
                                egui::Stroke::new(2.0, theme::ink(ui.ctx())),
                            );
                        } else if resp.hovered() {
                            ui.painter().circle_stroke(
                                center,
                                13.5,
                                egui::Stroke::new(1.5, theme::mute(ui.ctx())),
                            );
                        }
                        if resp.clicked() {
                            picked = Some(opt.id);
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(opt.label)
                                .size(11.0)
                                .color(if selected {
                                    theme::accent(ui.ctx())
                                } else {
                                    theme::mute(ui.ctx())
                                }),
                        );
                    });
                });
            }
        });
        if let Some(id) = picked {
            if id != self.config.ui.accent {
                self.config.ui.accent = id.to_string();
                theme::apply_appearance(ui.ctx(), self.config.ui.theme, id);
                self.persist_ui_prefs(ui.ctx());
            }
        }

        ui.add_space(20.0);
        if ui.button("Reset appearance defaults").clicked() {
            self.config.ui.theme = ThemeMode::System;
            self.config.ui.accent = theme::DEFAULT_ACCENT_ID.into();
            theme::apply_appearance(ui.ctx(), self.config.ui.theme, &self.config.ui.accent);
            self.persist_ui_prefs(ui.ctx());
        }
    }

    fn persist_ui_prefs(&mut self, _ctx: &egui::Context) {
        if let Err(e) = self.config.save(&self.paths.config_file()) {
            self.status = e.to_string();
        } else {
            self.status = format!(
                "Appearance: {} · {}",
                theme::resolved_label(self.config.ui.theme),
                theme::resolve_accent(&self.config.ui.accent).label
            );
        }
    }

    fn accounts_panel(&mut self, ui: &mut Ui) {
        if self.account_wizard.is_some() {
            self.account_wizard_panel(ui);
            return;
        }

        ui.label(
            RichText::new("Accounts")
                .strong()
                .size(16.0)
                .family(theme::nav_family()),
        );
        ui.label(
            RichText::new("Passwords stay in the OS keyring, never in config.toml.")
                .small()
                .color(theme::mute(ui.ctx())),
        );
        ui.add_space(12.0);

        if ui
            .add(
                egui::Button::new(RichText::new("Add account").color(Color32::WHITE))
                    .fill(theme::accent(ui.ctx())),
            )
            .clicked()
        {
            self.start_new_account();
            return;
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        if self.config.accounts.is_empty() {
            ui.label(
                RichText::new("No accounts yet — add Gmail, Outlook, or any IMAP/POP3 mailbox.")
                    .color(theme::mute(ui.ctx())),
            );
            return;
        }

        ui.label(RichText::new("Saved accounts").small().strong());
        ui.label(
            RichText::new("The default account opens automatically when AiliakMail starts.")
                .small()
                .color(theme::mute(ui.ctx())),
        );
        ui.add_space(6.0);
        let accounts: Vec<(String, String, String)> = self
            .config
            .accounts
            .iter()
            .map(|a| (a.id.clone(), a.name.clone(), a.email.clone()))
            .collect();
        let editing = self.editing_account_id.clone();
        let default_id = self
            .config
            .primary_account()
            .map(|a| a.id.clone())
            .unwrap_or_default();
        let mut pick: Option<String> = None;
        for (id, name, email) in &accounts {
            let selected = id == &editing;
            let is_default = id == &default_id;
            let base = if name.is_empty() {
                email.clone()
            } else {
                format!("{name}  ·  {email}")
            };
            let label = if is_default {
                format!("{base}  ·  default")
            } else {
                base
            };
            if ui.selectable_label(selected, label).clicked() {
                pick = Some(id.clone());
            }
        }
        if let Some(id) = pick {
            self.editing_account_id = id.clone();
            self.account_id = id;
            self.reload_settings_fields();
        }

        ui.add_space(16.0);
        self.account_edit_form(ui);
    }

    fn account_wizard_panel(&mut self, ui: &mut Ui) {
        let step = self
            .account_wizard
            .as_ref()
            .map(|w| w.step)
            .unwrap_or(AccountWizardStep::Provider);
        let provider = self.account_wizard.as_ref().and_then(|w| w.provider);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Add account")
                    .strong()
                    .size(16.0)
                    .family(theme::nav_family()),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    self.cancel_account_wizard();
                }
            });
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(match step {
                AccountWizardStep::Provider => "Step 1 of 2 — choose your provider",
                AccountWizardStep::Credentials => "Step 2 of 2 — name, email, password",
            })
            .small()
            .color(theme::mute(ui.ctx())),
        );
        ui.add_space(14.0);

        match step {
            AccountWizardStep::Provider => {
                ui.label(RichText::new("Popular").small().strong());
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    for p in [
                        MailProvider::Gmail,
                        MailProvider::Yahoo,
                        MailProvider::Office365,
                        MailProvider::Outlook,
                        MailProvider::Icloud,
                        MailProvider::Yandex,
                        MailProvider::Gmx,
                        MailProvider::Abv,
                    ] {
                        if provider_card(ui, p).clicked() {
                            self.wizard_select_provider(p);
                        }
                    }
                });
                ui.add_space(16.0);
                ui.label(RichText::new("Other").small().strong());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for p in [MailProvider::Imap, MailProvider::Pop3] {
                        if provider_card(ui, p).clicked() {
                            self.wizard_select_provider(p);
                        }
                    }
                });
            }
            AccountWizardStep::Credentials => {
                let preset = provider.map(|p| p.preset());
                if let (Some(prov), Some(p)) = (provider, preset) {
                    ui.horizontal(|ui| {
                        let tex = provider_logos::texture(ui.ctx(), prov);
                        let s = provider_logos::DISPLAY_SIZE;
                        ui.add(egui::Image::new((tex.id(), egui::vec2(s, s))));
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(p.label)
                                    .strong()
                                    .color(theme::accent(ui.ctx())),
                            );
                            ui.label(
                                RichText::new(p.blurb)
                                    .small()
                                    .color(theme::mute(ui.ctx())),
                            );
                        });
                    });
                    ui.add_space(8.0);
                }

                if ui
                    .add(egui::Button::new("← Change provider").frame(false))
                    .clicked()
                {
                    if let Some(wiz) = self.account_wizard.as_mut() {
                        wiz.step = AccountWizardStep::Provider;
                        wiz.provider = None;
                    }
                }
                ui.add_space(10.0);

                let oauth_mail = provider.filter(|p| p.supports_oauth());
                let oauth_kind = oauth_mail.and_then(OAuthProvider::from_mail_provider);
                if let (Some(mail_p), Some(oauth), Some(p)) = (oauth_mail, oauth_kind, preset) {
                    ui.horizontal(|ui| {
                        ui.label("Display name");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.account_name_input)
                                .desired_width(240.0)
                                .hint_text("Optional — defaults from your account"),
                        );
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!(
                            "Authorize AiliakMail with {} in your browser (2FA stays with them).",
                            oauth.label()
                        ))
                        .small()
                        .color(theme::mute(ui.ctx())),
                    );
                    ui.add_space(8.0);
                    if !self.oauth_app_embedded(oauth) && !self.oauth_ready(oauth) {
                        ui.label(
                            RichText::new(
                                "This build has no embedded OAuth app yet — use an app password below, or paste a developer Client ID.",
                            )
                            .small()
                            .color(Color32::from_rgb(196, 80, 72)),
                        );
                        egui::CollapsingHeader::new("Developer: OAuth client")
                            .default_open(false)
                            .show(ui, |ui| {
                                oauth_client_fields(ui, &mut self.config.oauth, oauth);
                            });
                        ui.add_space(8.0);
                    }
                    ui.horizontal(|ui| {
                        let sign_in = ui.add_enabled(
                            self.oauth_ready(oauth),
                            egui::Button::new(
                                RichText::new(p.oauth_button).color(Color32::WHITE),
                            )
                            .fill(theme::accent(ui.ctx())),
                        );
                        if sign_in.clicked() {
                            let _ = self.config.save(&self.paths.config_file());
                            self.start_oauth_sign_in(mail_p);
                        }
                    });
                    ui.add_space(14.0);
                    egui::CollapsingHeader::new("Use password / app password instead")
                        .default_open(!self.oauth_ready(oauth))
                        .show(ui, |ui| {
                            wizard_password_fields(ui, self, preset);
                        });
                } else {
                    wizard_password_fields(ui, self, preset);
                }

                let custom = provider.is_some_and(|p| p.needs_custom_hosts());
                if custom {
                    ui.add_space(14.0);
                    ui.label(RichText::new("Servers").small().strong());
                    ui.add_space(6.0);
                    let host_label = match self.mail_protocol {
                        MailProtocol::Imap => "IMAP host",
                        MailProtocol::Pop3 => "POP3 host",
                    };
                    egui::Grid::new("wizard_hosts")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(host_label);
                            ui.add(
                                egui::TextEdit::singleline(&mut self.imap_host_input)
                                    .desired_width(280.0)
                                    .hint_text("mail.example.com"),
                            );
                            ui.end_row();
                            ui.label("Port");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.mail_port_input)
                                        .desired_width(72.0),
                                );
                                if ui.checkbox(&mut self.mail_tls, "TLS").changed() {
                                    let mut tmp = MailAccountConfig {
                                        id: String::new(),
                                        name: String::new(),
                                        email: String::new(),
                                        protocol: self.mail_protocol,
                                        imap_host: String::new(),
                                        imap_port: 0,
                                        imap_tls: self.mail_tls,
                                        smtp_host: String::new(),
                                        smtp_port: 587,
                                        smtp_tls: true,
                                        username: String::new(),
                                        auth: MailAuthMethod::Password,
                                    };
                                    tmp.apply_protocol_defaults();
                                    self.mail_port_input = tmp.imap_port.to_string();
                                }
                            });
                            ui.end_row();
                            ui.label("SMTP host");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.smtp_host_input)
                                    .desired_width(280.0)
                                    .hint_text("smtp.example.com"),
                            );
                            ui.end_row();
                        });
                } else if let Some(p) = preset {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(format!(
                            "Incoming  {}:{}  ·  Outgoing  {}:{}",
                            p.imap_host, p.imap_port, p.smtp_host, p.smtp_port
                        ))
                        .small()
                        .color(theme::mute(ui.ctx())),
                    );
                }

                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    let can_save = !self.account_email_input.trim().is_empty()
                        && !self.imap_password_input.is_empty()
                        && (!custom || !self.imap_host_input.trim().is_empty());
                    let save = ui.add_enabled(
                        can_save,
                        egui::Button::new(RichText::new("Save & continue").color(Color32::WHITE))
                            .fill(theme::accent(ui.ctx())),
                    );
                    if save.clicked() {
                        if self.imap_user_input.trim().is_empty() {
                            self.imap_user_input = self.account_email_input.trim().to_string();
                        }
                        match self.save_settings() {
                            Ok(()) => {
                                self.account_wizard = None;
                                self.status = "Account saved — Sync to pull mail".into();
                            }
                            Err(e) => self.status = e.to_string(),
                        }
                    }
                    if ui.button("Test connection").clicked() {
                        if self.imap_user_input.trim().is_empty() {
                            self.imap_user_input = self.account_email_input.trim().to_string();
                        }
                        self.test_mail_connection();
                    }
                });
                mail_test_feedback_row(ui, &self.mail_test_feedback);
            }
        }
    }

    fn account_edit_form(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Edit selected").small().strong());
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let email = self.account_email_input.clone();
            let name = self.account_name_input.clone();
            let view = self.resolve_avatar(ui.ctx(), &email, &name);
            crate::avatars::avatar_widget(ui, &view, 40.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Profile picture")
                        .small()
                        .color(theme::mute(ui.ctx())),
                );
                ui.label(
                    RichText::new(
                        "Google accounts use your Google photo automatically; otherwise Gravatar or Choose photo…",
                    )
                    .small()
                    .color(theme::mute(ui.ctx())),
                );
                ui.horizontal(|ui| {
                    if ui.button("Choose photo…").clicked() {
                        self.choose_account_avatar();
                    }
                    if ui.button("Clear photo").clicked() {
                        self.clear_account_avatar();
                    }
                });
            });
        });
        ui.add_space(10.0);

        let host_label = match self.mail_protocol {
            MailProtocol::Imap => "IMAP host",
            MailProtocol::Pop3 => "POP3 host",
        };
        let pass_hint = match self.mail_protocol {
            MailProtocol::Imap => "App password or mail password",
            MailProtocol::Pop3 => "POP3 password",
        };
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
                ui.label("Protocol");
                ui.horizontal(|ui| {
                    let prev = self.mail_protocol;
                    ui.selectable_value(&mut self.mail_protocol, MailProtocol::Imap, "IMAP");
                    ui.selectable_value(&mut self.mail_protocol, MailProtocol::Pop3, "POP3");
                    if self.mail_protocol != prev {
                        let mut tmp = MailAccountConfig {
                            id: String::new(),
                            name: String::new(),
                            email: String::new(),
                            protocol: self.mail_protocol,
                            imap_host: String::new(),
                            imap_port: 0,
                            imap_tls: self.mail_tls,
                            smtp_host: String::new(),
                            smtp_port: 587,
                            smtp_tls: true,
                            username: String::new(),
                            auth: MailAuthMethod::Password,
                        };
                        tmp.apply_protocol_defaults();
                        self.mail_port_input = tmp.imap_port.to_string();
                    }
                });
                ui.end_row();
                ui.label(host_label);
                ui.text_edit_singleline(&mut self.imap_host_input);
                ui.end_row();
                ui.label("Port");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.mail_port_input).desired_width(72.0),
                    );
                    if ui.checkbox(&mut self.mail_tls, "TLS").changed() {
                        let mut tmp = MailAccountConfig {
                            id: String::new(),
                            name: String::new(),
                            email: String::new(),
                            protocol: self.mail_protocol,
                            imap_host: String::new(),
                            imap_port: 0,
                            imap_tls: self.mail_tls,
                            smtp_host: String::new(),
                            smtp_port: 587,
                            smtp_tls: true,
                            username: String::new(),
                            auth: MailAuthMethod::Password,
                        };
                        tmp.apply_protocol_defaults();
                        self.mail_port_input = tmp.imap_port.to_string();
                    }
                });
                ui.end_row();
                ui.label("Username");
                ui.text_edit_singleline(&mut self.imap_user_input);
                ui.end_row();
                ui.label("Password");
                let saved = secrets::has_mail_password(&self.editing_account_id);
                let hint = if saved && self.imap_password_input.is_empty() {
                    "Saved — leave blank to keep, or type a new password"
                } else {
                    pass_hint
                };
                password_edit(
                    ui,
                    &mut self.imap_password_input,
                    &mut self.show_mail_password,
                    hint,
                    220.0,
                );
                ui.end_row();
                ui.label("SMTP host");
                ui.text_edit_singleline(&mut self.smtp_host_input);
                ui.end_row();
            });

        if self.mail_protocol == MailProtocol::Pop3 {
            ui.add_space(4.0);
            let pop3_note = if self.imap_host_input.to_lowercase().contains("abv.bg")
                || self.account_email_input.to_lowercase().ends_with("@abv.bg")
            {
                "ABV supports POP3 only (no IMAP). Sync downloads Inbox; Sent, Spam, Drafts, and custom folders stay in ABV webmail. Mail is left on the server."
            } else {
                "POP3 maps to a single Inbox — server folders cannot be listed. Mail is left on the server (no delete)."
            };
            ui.label(
                RichText::new(pop3_note)
                    .small()
                    .color(theme::mute(ui.ctx())),
            );
        }

        let editing = self.config.account_by_id(&self.editing_account_id);
        let edit_oauth = editing.and_then(|a| OAuthProvider::from_auth_method(a.auth));
        let inferred_mail = infer_mail_provider(
            &self.imap_host_input,
            &self.account_email_input,
        );
        let sign_in_mail = edit_oauth
            .and_then(|o| match o {
                OAuthProvider::Google => Some(MailProvider::Gmail),
                OAuthProvider::Microsoft => Some(MailProvider::Outlook),
                OAuthProvider::Yahoo => Some(MailProvider::Yahoo),
                OAuthProvider::Yandex => Some(MailProvider::Yandex),
            })
            .or(inferred_mail.filter(|p| p.supports_oauth()));

        if let Some(mail_p) = sign_in_mail {
            if let Some(oauth) = OAuthProvider::from_mail_provider(mail_p) {
                let preset = mail_p.preset();
                ui.add_space(8.0);
                if !self.oauth_app_embedded(oauth) && !self.oauth_ready(oauth) {
                    egui::CollapsingHeader::new("Developer: OAuth client")
                        .default_open(false)
                        .show(ui, |ui| {
                            oauth_client_fields(ui, &mut self.config.oauth, oauth);
                        });
                }
                let label = if edit_oauth == Some(oauth) {
                    format!("{} again", preset.oauth_button)
                } else {
                    preset.oauth_button.to_string()
                };
                if ui
                    .add_enabled(self.oauth_ready(oauth), egui::Button::new(label))
                    .clicked()
                {
                    let _ = self.config.save(&self.paths.config_file());
                    self.start_oauth_sign_in(mail_p);
                }
                if edit_oauth == Some(oauth) {
                    ui.label(
                        RichText::new(format!(
                            "This account authorized AiliakMail via {}.",
                            oauth.label()
                        ))
                        .small()
                        .color(theme::mute(ui.ctx())),
                    );
                }
            }
        }

        ui.add_space(12.0);
        let editing_id = self.editing_account_id.clone();
        let is_default = !editing_id.is_empty() && self.config.is_default_account(&editing_id);
        ui.horizontal(|ui| {
            if is_default {
                ui.label(
                    RichText::new("Default at startup")
                        .small()
                        .strong()
                        .color(theme::accent(ui.ctx())),
                );
            } else if ui
                .button("Use as default")
                .on_hover_text("Open this account automatically when the app starts")
                .clicked()
            {
                self.config.set_default_account_id(editing_id.clone());
                if let Err(e) = self.config.save(&self.paths.config_file()) {
                    self.status = e.to_string();
                } else {
                    self.status = "Default account saved — used on next launch".into();
                }
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let test_label = match self.mail_protocol {
                MailProtocol::Imap => "Test IMAP",
                MailProtocol::Pop3 => "Test POP3",
            };
            if ui.button(test_label).clicked() {
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
                    self.start_sync(false);
                }
            }
        });
        mail_test_feedback_row(ui, &self.mail_test_feedback);

        ui.add_space(16.0);
        ui.label(
            RichText::new(format!(
                "Config: {}\nData: {}",
                self.paths.config_file().display(),
                self.paths.db_file().display()
            ))
            .small()
            .color(theme::mute(ui.ctx())),
        );
    }

    fn folders_panel(&mut self, ui: &mut Ui) {
        let is_pop3 = self
            .selected_account()
            .map(|a| a.protocol == MailProtocol::Pop3)
            .unwrap_or(self.mail_protocol == MailProtocol::Pop3);
        let is_abv = self.selected_account().is_some_and(|a| {
            a.imap_host.to_lowercase().contains("abv.bg")
                || a.email.to_lowercase().ends_with("@abv.bg")
        });

        ui.label(
            RichText::new("Folders")
                .strong()
                .size(16.0)
                .family(theme::nav_family()),
        );
        let folder_help = if is_abv {
            "ABV has no IMAP — Sync only downloads Inbox. Sent, Spam, Drafts, and custom folders stay in ABV webmail."
        } else if is_pop3 {
            "POP3 cannot list server folders. Sync downloads Inbox only; hide/show applies to local folders."
        } else {
            "IMAP lists every mailbox on Sync. Hide folders you don’t want in the left rail."
        };
        ui.label(
            RichText::new(folder_help)
                .small()
                .color(theme::mute(ui.ctx())),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            if ui
                .button(if is_pop3 { "Sync Inbox" } else { "Sync this folder" })
                .on_hover_text(if is_pop3 {
                    "Download Inbox via POP3 (g)"
                } else {
                    "Refresh folder list + download the selected folder (g)"
                })
                .clicked()
            {
                self.start_sync(false);
            }
            if !is_pop3
                && ui
                    .button("Sync all folders")
                    .on_hover_text("Download mail for every IMAP folder (Shift+G)")
                    .clicked()
            {
                self.start_sync(true);
            }
        });
        ui.add_space(16.0);

        let all = match self.store.list_all_folders(&self.account_id) {
            Ok(f) => f,
            Err(e) => {
                ui.label(
                    RichText::new(format!("Could not load folders: {e}"))
                        .color(theme::mute(ui.ctx())),
                );
                return;
            }
        };

        if all.is_empty() {
            ui.label(
                RichText::new("No folders yet — Sync once to list them from the server.")
                    .color(theme::mute(ui.ctx())),
            );
            return;
        }

        ui.label(
            RichText::new("Visibility")
                .strong()
                .size(13.0)
                .family(theme::nav_family()),
        );
        ui.add_space(6.0);

        let mut toggle: Option<(String, bool)> = None;
        for folder in &all {
            ui.horizontal(|ui| {
                let mut visible = !folder.hidden;
                if ui
                    .checkbox(&mut visible, &folder.name)
                    .on_hover_text(&folder.path)
                    .changed()
                {
                    toggle = Some((folder.path.clone(), !visible));
                }
                if folder.unread > 0 {
                    ui.label(
                        RichText::new(format!("({})", folder.unread))
                            .small()
                            .color(theme::mute(ui.ctx())),
                    );
                }
                if folder.hidden {
                    ui.label(
                        RichText::new("hidden")
                            .small()
                            .color(theme::mute(ui.ctx())),
                    );
                }
            });
        }
        if let Some((path, hidden)) = toggle {
            self.set_folder_hidden(&path, hidden);
        }
    }

    fn ailiak_panel(&mut self, ui: &mut Ui) {
        ui.label(
            RichText::new("Ailiak")
                .strong()
                .size(16.0)
                .family(theme::nav_family()),
        );
        ui.label(
            RichText::new("Connect AiliakMail to your Desk for Todo.")
                .small()
                .color(theme::mute(ui.ctx())),
        );
        ui.add_space(10.0);
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
        ui.add_space(8.0);
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
            if ui.button("Save").clicked() {
                if let Err(e) = self.save_settings() {
                    self.status = e.to_string();
                }
            }
        });
    }
}

fn wizard_password_fields(
    ui: &mut Ui,
    app: &mut MailboxApp,
    preset: Option<mailbox_core::ProviderPreset>,
) {
    egui::Grid::new("wizard_creds")
        .num_columns(2)
        .spacing([12.0, 10.0])
        .show(ui, |ui| {
            ui.label("Display name");
            ui.add(
                egui::TextEdit::singleline(&mut app.account_name_input)
                    .desired_width(280.0)
                    .hint_text("How it appears in the sidebar"),
            );
            ui.end_row();
            ui.label("Email");
            let email_resp = ui.add(
                egui::TextEdit::singleline(&mut app.account_email_input)
                    .desired_width(280.0)
                    .hint_text("you@example.com"),
            );
            ui.end_row();
            if email_resp.changed() {
                let email = app.account_email_input.trim().to_string();
                app.imap_user_input = email;
            }
            ui.label("Password");
            let hint = preset
                .map(|p| p.password_hint)
                .unwrap_or("Mail password");
            password_edit(
                ui,
                &mut app.imap_password_input,
                &mut app.show_mail_password,
                hint,
                280.0,
            );
            ui.end_row();
        });
    if let Some(p) = preset {
        ui.add_space(6.0);
        ui.label(
            RichText::new(p.password_hint)
                .small()
                .color(theme::mute(ui.ctx())),
        );
    }
}

fn infer_mail_provider(imap_host: &str, email: &str) -> Option<MailProvider> {
    let host = imap_host.trim().to_ascii_lowercase();
    let email = email.trim().to_ascii_lowercase();
    if host.contains("gmail.com")
        || email.ends_with("@gmail.com")
        || email.ends_with("@googlemail.com")
    {
        return Some(MailProvider::Gmail);
    }
    if host.contains("mail.yahoo.") || email.contains("@yahoo.") || email.ends_with("@ymail.com")
    {
        return Some(MailProvider::Yahoo);
    }
    if host.contains("office365")
        || host.contains("outlook.")
        || email.ends_with("@outlook.com")
        || email.ends_with("@hotmail.com")
        || email.ends_with("@live.com")
    {
        return Some(MailProvider::Outlook);
    }
    if host.contains("yandex.") || email.contains("@yandex.") {
        return Some(MailProvider::Yandex);
    }
    if host.contains("abv.bg") || email.ends_with("@abv.bg") {
        return Some(MailProvider::Abv);
    }
    None
}

fn oauth_client_fields(ui: &mut Ui, oauth: &mut mailbox_core::OAuthConfig, provider: OAuthProvider) {
    match provider {
        OAuthProvider::Google => {
            ui.label(
                RichText::new("One-time setup — Google OAuth Client ID")
                    .small()
                    .strong(),
            );
            ui.label(
                RichText::new(
                    "Google Cloud Console → Credentials → Create OAuth client → Desktop app.",
                )
                .small()
                .color(theme::mute(ui.ctx())),
            );
            ui.add(
                egui::TextEdit::singleline(&mut oauth.google_client_id)
                    .desired_width(360.0)
                    .hint_text("….apps.googleusercontent.com"),
            );
        }
        OAuthProvider::Microsoft => {
            ui.label(
                RichText::new("One-time setup — Microsoft Application (client) ID")
                    .small()
                    .strong(),
            );
            ui.label(
                RichText::new(
                    "Azure Portal → App registrations → New → Public client/native. Add redirect http://127.0.0.1.",
                )
                .small()
                .color(theme::mute(ui.ctx())),
            );
            ui.add(
                egui::TextEdit::singleline(&mut oauth.microsoft_client_id)
                    .desired_width(360.0)
                    .hint_text("xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"),
            );
        }
        OAuthProvider::Yahoo => {
            ui.label(
                RichText::new("One-time setup — Yahoo Client ID + Secret")
                    .small()
                    .strong(),
            );
            ui.label(
                RichText::new("developer.yahoo.com → Create app → redirect http://127.0.0.1")
                    .small()
                    .color(theme::mute(ui.ctx())),
            );
            ui.add(
                egui::TextEdit::singleline(&mut oauth.yahoo_client_id)
                    .desired_width(360.0)
                    .hint_text("Client ID"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut oauth.yahoo_client_secret)
                    .desired_width(360.0)
                    .password(true)
                    .hint_text("Client Secret"),
            );
        }
        OAuthProvider::Yandex => {
            ui.label(
                RichText::new("One-time setup — Yandex Client ID + Secret")
                    .small()
                    .strong(),
            );
            ui.label(
                RichText::new("oauth.yandex.com → Create app → redirect http://127.0.0.1")
                    .small()
                    .color(theme::mute(ui.ctx())),
            );
            ui.add(
                egui::TextEdit::singleline(&mut oauth.yandex_client_id)
                    .desired_width(360.0)
                    .hint_text("Client ID"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut oauth.yandex_client_secret)
                    .desired_width(360.0)
                    .password(true)
                    .hint_text("Client Secret"),
            );
        }
    }
}

fn mail_test_feedback_row(ui: &mut Ui, feedback: &str) {
    if feedback.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let is_error = feedback.starts_with("Error:")
            || feedback.contains("failed")
            || feedback.contains("needs a Google App Password")
            || feedback.contains("app-specific password");
        let color = if is_error {
            Color32::from_rgb(196, 80, 72)
        } else if feedback.ends_with('…') {
            theme::mute(ui.ctx())
        } else {
            theme::accent(ui.ctx())
        };
        ui.add(
            egui::Label::new(RichText::new(feedback).small().color(color)).wrap(),
        );
        if ui
            .add(
                egui::Button::new(
                    RichText::new(ph::COPY)
                        .family(theme::icon_family())
                        .size(12.0)
                        .color(theme::mute(ui.ctx())),
                )
                .frame(false),
            )
            .on_hover_text("Copy message")
            .clicked()
        {
            ui.ctx().copy_text(feedback.to_string());
        }
    });
}

fn password_edit(
    ui: &mut Ui,
    password: &mut String,
    show: &mut bool,
    hint: &str,
    width: f32,
) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(password)
                .desired_width(width)
                .password(!*show)
                .hint_text(hint),
        );
        let glyph = if *show { ph::EYE_SLASH } else { ph::EYE };
        let tip = if *show { "Hide password" } else { "Show password" };
        if ui
            .add(
                egui::Button::new(
                    RichText::new(glyph)
                        .family(theme::icon_family())
                        .size(16.0)
                        .color(theme::mute(ui.ctx())),
                )
                .frame(false),
            )
            .on_hover_text(tip)
            .clicked()
        {
            *show = !*show;
        }
    });
}

fn provider_card(ui: &mut Ui, provider: MailProvider) -> egui::Response {
    let preset = provider.preset();
    let logo = provider_logos::DISPLAY_SIZE;
    let size = egui::vec2(168.0, 64.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        theme::select(ui.ctx())
    } else if response.hovered() {
        theme::chip(ui.ctx())
    } else {
        theme::paper(ui.ctx())
    };
    ui.painter().rect(
        rect,
        8.0,
        fill,
        egui::Stroke::new(1.0, theme::mute(ui.ctx()).linear_multiply(0.35)),
    );

    let tex = provider_logos::texture(ui.ctx(), provider);
    let logo_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 14.0 + logo * 0.5, rect.center().y),
        egui::vec2(logo, logo),
    );
    egui::Image::new((tex.id(), logo_rect.size())).paint_at(ui, logo_rect);

    let text_x = rect.left() + 14.0 + logo + 10.0;
    ui.painter().text(
        egui::pos2(text_x, rect.center().y - 8.0),
        egui::Align2::LEFT_CENTER,
        preset.label,
        theme::nav_font(13.0),
        theme::ink(ui.ctx()),
    );
    ui.painter().text(
        egui::pos2(text_x, rect.center().y + 10.0),
        egui::Align2::LEFT_CENTER,
        preset.blurb,
        egui::FontId::proportional(11.0),
        theme::mute(ui.ctx()),
    );
    response.on_hover_text(preset.password_hint)
}

fn theme_chip(ui: &mut Ui, mode: &mut ThemeMode, value: ThemeMode, label: &str) {
    let selected = *mode == value;
    let text = if selected {
        RichText::new(label)
            .size(13.0)
            .strong()
            .color(Color32::WHITE)
    } else {
        RichText::new(label).size(13.0)
    };
    let mut btn = egui::Button::new(text);
    if selected {
        btn = btn.fill(theme::accent(ui.ctx()));
    }
    if ui.add(btn).clicked() {
        *mode = value;
    }
}
