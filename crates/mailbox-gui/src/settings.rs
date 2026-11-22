//! Full-page Settings (Appearance first — matches Ailiak.com Studio).

use egui::{Align, Color32, Layout, RichText, Sense, Ui, Vec2};
use egui_phosphor::regular as ph;

use mailbox_core::config::{MailAccountConfig, MailProtocol, ThemeMode};
use mailbox_core::secrets;

use crate::app::MailboxApp;
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Appearance,
    Accounts,
    Ailiak,
}

impl SettingsSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Accounts => "Accounts",
            Self::Ailiak => "Ailiak",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Self::Appearance => "Theme and accent",
            Self::Accounts => "Mail accounts and avatars",
            Self::Ailiak => "Desk token and hashtag",
        }
    }

    const ALL: [Self; 3] = [Self::Appearance, Self::Accounts, Self::Ailiak];
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
        ui.label(
            RichText::new("Mail account")
                .strong()
                .size(16.0)
                .family(theme::nav_family()),
        );
        ui.label(
            RichText::new(
                "Gmail: App Password. POP3: older servers (USER/PASS). \
                 Passwords stay in the OS keyring, never in config.toml.",
            )
            .small()
            .color(theme::mute(ui.ctx())),
        );
        ui.add_space(10.0);

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
                ui.horizontal(|ui| {
                    if ui.button("Choose photo…").clicked() {
                        self.choose_account_avatar();
                    }
                    if ui.button("Clear photo").clicked() {
                        self.clear_account_avatar();
                    }
                });
                ui.label(
                    RichText::new("Local override, else Gravatar, else initials.")
                        .small()
                        .color(theme::mute(ui.ctx())),
                );
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Use Gmail defaults").clicked() {
                self.apply_gmail_preset();
            }
            if ui.button("Use POP3 defaults").clicked() {
                self.apply_pop3_preset();
            }
            if ui.button("New account").clicked() {
                self.start_new_account();
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Protocol");
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
                };
                tmp.apply_protocol_defaults();
                self.mail_port_input = tmp.imap_port.to_string();
            }
        });
        ui.add_space(8.0);

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
                ui.label(host_label);
                ui.text_edit_singleline(&mut self.imap_host_input);
                ui.end_row();
                ui.label("Port");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.mail_port_input).desired_width(72.0),
                    );
                    if ui
                        .checkbox(&mut self.mail_tls, "TLS")
                        .on_hover_text(
                            "POP3S :995 / IMAPS :993. Uncheck only for plain :110 / :143 \
                             (password sent in clear text).",
                        )
                        .changed()
                    {
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
                ui.add(
                    egui::TextEdit::singleline(&mut self.imap_password_input)
                        .password(true)
                        .hint_text(pass_hint),
                );
                ui.end_row();
                ui.label("SMTP host");
                ui.text_edit_singleline(&mut self.smtp_host_input);
                ui.end_row();
            });

        if self.mail_protocol == MailProtocol::Pop3 {
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "POP3 maps to a single Inbox. Mail is left on the server (no delete).",
                )
                .small()
                .color(theme::mute(ui.ctx())),
            );
            if !self.mail_tls {
                ui.label(
                    RichText::new("Warning: plain POP3 sends your password unencrypted.")
                        .small()
                        .color(Color32::from_rgb(180, 90, 40)),
                );
            }
        }

        ui.add_space(12.0);
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
                    self.start_sync();
                }
            }
        });

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
