//! Main chrome panes: top bar, folders, list, reader, status, shell layout.

use egui::{
    Align, Color32, CursorIcon, Id, Layout, Order, Rect, RichText, ScrollArea, Sense, Ui,
    UiBuilder,
};

use crate::app::MailboxApp;
use crate::avatars;
use crate::brand_logos;
use crate::calendar;
use crate::icons::{self, FolderIcon};
use crate::layout;
use crate::staks;
use crate::theme;
use mailbox_core::types::{date_group_label, MessageId};

impl MailboxApp {
    pub(crate) fn top_bar(&mut self, ui: &mut Ui) {
        let bar = ui.max_rect();
        let row_h = ui.available_height().max(28.0);

        // Left: full website logo → Ailiak Desk
        let left = Rect::from_min_size(
            bar.left_top(),
            egui::vec2((bar.width() * 0.28).max(160.0), row_h),
        );
        ui.allocate_new_ui(UiBuilder::new().max_rect(left), |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // SVG wordmark — mark box follows accent (same as ailiak.com).
                let accent = theme::resolve_accent(&self.config.ui.accent).color;
                let dark = ui.visuals().dark_mode;
                self.ailiak_logo.sync(accent, dark);
                let h = 28.0_f32.min(row_h);
                let w = h * (292.0 / 72.0);
                let resp = ui
                    .add(
                        egui::Image::new((self.ailiak_logo.texture().id(), egui::vec2(w, h)))
                            .sense(Sense::click()),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Open Ailiak Desk");
                if resp.clicked() {
                    self.open_desk();
                }
            });
        });

        // Right: Archive / Trash (current message) + Sync
        let right = Rect::from_min_max(
            egui::pos2(bar.right() - (bar.width() * 0.28).max(200.0), bar.top()),
            bar.right_bottom(),
        );
        ui.allocate_new_ui(UiBuilder::new().max_rect(right), |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::toolbar_button(
                    ui,
                    icons::ToolbarIcon::Sync,
                    "Sync",
                    "List folders + download the selected folder (g)",
                )
                .clicked()
                {
                    self.start_sync(false);
                }
                let has_msg = self.open_message.is_some();
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Trash,
                    "Trash current message (#)",
                    has_msg,
                )
                .clicked()
                {
                    self.move_current_message(mailbox_core::SpecialFolderKind::Trash);
                }
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Archive,
                    "Archive current message (e)",
                    has_msg,
                )
                .clicked()
                {
                    self.move_current_message(mailbox_core::SpecialFolderKind::Archive);
                }
            });
        });

        // Center: search
        let search_w = (bar.width() * 0.36).clamp(200.0, 360.0);
        let search_rect = Rect::from_center_size(
            egui::pos2(bar.center().x, bar.center().y),
            egui::vec2(search_w, 28.0),
        );
        ui.allocate_new_ui(UiBuilder::new().max_rect(search_rect), |ui| {
            self.search_field(ui);
        });
    }

    /// Search input with leading icon (focused by `/`).
    pub(crate) fn search_field(&mut self, ui: &mut Ui) {
        let mute = theme::mute(ui.ctx());
        let stroke = ui.visuals().widgets.inactive.bg_stroke;
        let fill = if ui.visuals().dark_mode {
            theme::paper(ui.ctx())
        } else {
            Color32::from_rgb(255, 255, 255)
        };

        egui::Frame::none()
            .fill(fill)
            .stroke(stroke)
            .rounding(6.0)
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(18.0);
                    // Match Body text size used by the search field.
                    let icon_size = 15.0;
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(icon_size, icon_size), Sense::hover());
                    icons::paint_search(ui, icon_rect, mute);
                    ui.add_space(6.0);

                    let width = (ui.available_width() - 4.0).max(80.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .id(Id::new("mail_search"))
                            .frame(false)
                            .hint_text("Search mail…  (/)")
                            .desired_width(width)
                            .clip_text(true),
                    );
                    if self.focus_search {
                        resp.request_focus();
                        self.focus_search = false;
                    }
                    if resp.changed() {
                        if let Err(e) = self.refresh_envelopes() {
                            self.status = e.to_string();
                        }
                    }
                });
            });
    }

    /// Combined account strip + folders (collapses to icons like before).
    pub(crate) fn nav_pane(&mut self, ui: &mut Ui) {
        let collapsed = self.folders_collapsed;
        let mute = theme::mute(ui.ctx());
        let avatar_size = if collapsed { 28.0 } else { 32.0 };
        let todo_enabled = self.open_message.is_some();

        let accounts: Vec<(String, String, String)> = self
            .config
            .accounts
            .iter()
            .map(|a| (a.id.clone(), a.name.clone(), a.email.clone()))
            .collect();
        let selected_id = self.account_id.clone();
        let folders = self.folders.clone();
        let selected_folder = self.selected_folder.clone();

        let mut switch_to: Option<String> = None;
        let mut add_account = false;
        let mut clicked_folder: Option<String> = None;
        let mut clicked_stak: Option<String> = None;
        let mut open_folders_settings = false;
        let mut sync_folders: Option<bool> = None; // Some(true) = all IMAP folders
        let mut open_desk = false;
        let mut open_todo = false;
        let mut open_calendar = false;
        let mut open_settings = false;
        let mut open_help = false;
        let mut toggle_collapse = false;

        ui.set_width(ui.available_width());
        // Tighter vertical rhythm than the global style (folders felt too airy).
        ui.spacing_mut().item_spacing.y = 2.0;

        // Top-down: scrollable accounts/folders, then a fixed tools strip (paper fill).
        // 5 rows × 28 + padding (Desk, Todo, Calendar, Settings, Help).
        let row_h = 28.0;
        let footer_h = row_h * 5.0 + 12.0 + 16.0;
        let body_h = (ui.available_height() - footer_h).max(48.0);

        let paper = theme::paper(ui.ctx());
        let side_inset = 8.0; // match SidePanel left/right margin + footer bleed

        ScrollArea::vertical()
            .id_salt("nav_scroll_v5")
            .max_height(body_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;

                // Same paper band as the bottom tools strip.
                egui::Frame::none()
                    .fill(paper)
                    .inner_margin(egui::Margin {
                        left: side_inset,
                        right: side_inset,
                        top: 8.0,
                        bottom: 8.0,
                    })
                    .outer_margin(egui::Margin {
                        left: -side_inset,
                        right: -side_inset,
                        top: -8.0, // cancel SidePanel top margin so paper hits the edge
                        bottom: 0.0,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if collapsed {
                            // Same centered column as folder / footer icons.
                            ui.horizontal(|ui| {
                                let btn = 28.0;
                                ui.add_space((ui.available_width() - btn).max(0.0) * 0.5);
                                if icons::collapse_button(ui, true, mute).clicked() {
                                    toggle_collapse = true;
                                }
                            });
                        } else {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Accounts")
                                        .small()
                                        .family(theme::nav_family())
                                        .color(mute),
                                );
                                ui.add_space(ui.available_width() - 28.0);
                                if icons::collapse_button(ui, false, mute).clicked() {
                                    toggle_collapse = true;
                                }
                            });
                        }
                        ui.add_space(4.0);

                        if collapsed {
                            // Narrow rail: stack avatars in a centered column.
                            for (id, name, email) in &accounts {
                                let selected = id == &selected_id;
                                let tip = if name.is_empty() {
                                    email.clone()
                                } else {
                                    format!("{name}\n{email}")
                                };
                                let view = self.resolve_avatar(ui.ctx(), email, name);
                                ui.horizontal(|ui| {
                                    ui.add_space(
                                        (ui.available_width() - avatar_size).max(0.0) * 0.5,
                                    );
                                    if avatars::avatar_button(
                                        ui,
                                        &view,
                                        avatar_size,
                                        selected,
                                        &tip,
                                    )
                                    .clicked()
                                    {
                                        switch_to = Some(id.clone());
                                    }
                                });
                                ui.add_space(4.0);
                            }
                            ui.horizontal(|ui| {
                                ui.add_space(
                                    (ui.available_width() - avatar_size).max(0.0) * 0.5,
                                );
                                if icons::plus_account_button(ui, avatar_size).clicked() {
                                    add_account = true;
                                }
                            });
                        } else {
                            // Wide panel: avatars + add on one row.
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for (id, name, email) in &accounts {
                                    let selected = id == &selected_id;
                                    let tip = if name.is_empty() {
                                        email.clone()
                                    } else {
                                        format!("{name}\n{email}")
                                    };
                                    let view = self.resolve_avatar(ui.ctx(), email, name);
                                    if avatars::avatar_button(
                                        ui,
                                        &view,
                                        avatar_size,
                                        selected,
                                        &tip,
                                    )
                                    .clicked()
                                    {
                                        switch_to = Some(id.clone());
                                    }
                                }
                                if icons::plus_account_button(ui, avatar_size).clicked() {
                                    add_account = true;
                                }
                            });
                        }
                    });

                ui.add_space(8.0);

                // Staks — sender/domain stacks from the current folder cache.
                let selected_stak = self.selected_stak.clone();
                if !self.staks_cache.is_empty() {
                    if !collapsed {
                        ui.label(
                            RichText::new("Staks")
                                .small()
                                .family(theme::nav_family())
                                .color(mute),
                        );
                        ui.add_space(4.0);
                    }
                    for stak in &self.staks_cache {
                        let selected = selected_stak.as_deref() == Some(stak.id.as_str());
                        let label = if stak.unread > 0 {
                            format!("{} {}", stak.label, stak.unread)
                        } else {
                            stak.label.clone()
                        };
                        let brand = brand_logos::texture(ui.ctx(), &stak.id);
                        if icons::stak_nav_button(
                            ui,
                            &label,
                            selected,
                            collapsed,
                            brand.as_ref(),
                        )
                        .clicked()
                        {
                            clicked_stak = Some(stak.id.clone());
                        }
                    }
                    ui.add_space(8.0);
                }

                if !collapsed {
                    let (is_pop3, is_abv) = self
                        .selected_account()
                        .map(|a| (a.is_pop3(), a.looks_like_abv()))
                        .unwrap_or((false, false));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Folders")
                                .small()
                                .family(theme::nav_family())
                                .color(mute),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let manage = ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(egui_phosphor::regular::GEAR)
                                            .family(theme::icon_family())
                                            .size(12.0)
                                            .color(mute),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text("Manage folders");
                            if manage.clicked() {
                                open_folders_settings = true;
                            }
                            let sync_tip = mailbox_core::nav_sync_tip(is_pop3, is_abv);
                            let sync = ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(egui_phosphor::regular::ARROWS_CLOCKWISE)
                                            .family(theme::icon_family())
                                            .size(12.0)
                                            .color(mute),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text(sync_tip);
                            if sync.clicked() {
                                sync_folders = Some(!is_pop3);
                            }
                        });
                    });
                    ui.add_space(4.0);
                }

                for folder in &folders {
                    let selected = folder.path == selected_folder;
                    let label = if folder.unread > 0 {
                        format!("{} ({})", folder.name, folder.unread)
                    } else {
                        folder.name.clone()
                    };
                    let kind = FolderIcon::from_folder(&folder.path, &folder.name);
                    if icons::folder_nav_button(ui, kind, &label, selected, collapsed).clicked() {
                        clicked_folder = Some(folder.path.clone());
                    }
                }
            });

        // Tools strip — same paper as the accounts band / mail reader.
        let max = ui.max_rect();
        let top = ui.cursor().top();
        let footer_bg = Rect::from_min_max(
            egui::pos2(max.left() - side_inset, top),
            egui::pos2(max.right() + side_inset, max.bottom()),
        );
        ui.painter().rect_filled(footer_bg, 0.0, paper);

        ui.add_space(8.0);
        if icons::desk_nav_button(ui, collapsed).clicked() {
            open_desk = true;
        }
        if icons::todo_nav_button(ui, todo_enabled, collapsed).clicked() {
            open_todo = true;
        }
        if icons::calendar_nav_button(ui, collapsed).clicked() {
            open_calendar = true;
        }
        if icons::settings_nav_button(ui, collapsed).clicked() {
            open_settings = true;
        }
        if icons::help_nav_button(ui, collapsed).clicked() {
            open_help = true;
        }
        ui.add_space(16.0);
        // Keep paper continuous through leftover space under the tools.
        let leftover = ui.available_height();
        if leftover > 0.0 {
            ui.allocate_exact_size(egui::vec2(ui.available_width(), leftover), Sense::hover());
        }

        if toggle_collapse {
            self.folders_collapsed = !self.folders_collapsed;
        }
        if let Some(id) = switch_to {
            self.switch_account(&id);
        }
        if add_account {
            self.start_new_account();
        }
        if let Some(path) = clicked_folder {
            self.selected_folder = path;
            self.selected_stak = None;
            self.selected_message = None;
            self.open_message = None;
            if let Err(e) = self.refresh_envelopes() {
                self.status = e.to_string();
            } else if let Some(first) = self.envelopes.first().map(|e| e.id.clone()) {
                let _ = self.open_id(&first);
            }
        }
        if let Some(stak_id) = clicked_stak {
            if self.selected_stak.as_deref() == Some(stak_id.as_str()) {
                self.selected_stak = None;
            } else {
                self.selected_stak = Some(stak_id);
            }
            self.selected_message = None;
            self.open_message = None;
            if let Some(first) = self
                .envelopes
                .iter()
                .find(|e| {
                    self.selected_stak
                        .as_ref()
                        .is_none_or(|id| staks::envelope_in_stak(e, id))
                })
                .map(|e| e.id.clone())
            {
                let _ = self.open_id(&first);
            }
        }
        if open_desk {
            self.open_desk();
        }
        if open_todo {
            self.toggle_todo();
        }
        if open_calendar {
            self.toggle_calendar();
        }
        if let Some(all) = sync_folders {
            self.start_sync(all);
        }
        if open_folders_settings {
            self.open_settings(crate::settings::SettingsSection::Folders);
        }
        if open_settings {
            self.editing_account_id = self.account_id.clone();
            self.reload_settings_fields();
            self.open_settings(crate::settings::SettingsSection::Appearance);
        }
        if open_help {
            self.show_help = !self.show_help;
        }
    }

    pub(crate) fn list_pane(&mut self, ui: &mut Ui) {
        let col_w = ui.available_width();
        // Cap content to the panel — never set_min_width larger than the
        // resized panel or egui will persist a wider PanelState and snap back.
        ui.set_max_width(col_w);

        let folder_title = self
            .folders
            .iter()
            .find(|f| f.path == self.selected_folder)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| self.selected_folder.clone());
        let list_title = if let Some(stak_id) = self.selected_stak.as_deref() {
            let label = self
                .staks_cache
                .iter()
                .find(|s| s.id == stak_id)
                .map(|s| s.label.clone())
                .unwrap_or_else(|| stak_id.to_string());
            format!("{label} · {folder_title}")
        } else {
            folder_title
        };
        let list_count = if let Some(stak_id) = self.selected_stak.as_deref() {
            self.envelopes
                .iter()
                .filter(|e| staks::envelope_in_stak(e, stak_id))
                .count()
        } else {
            self.envelopes.len()
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&list_title)
                    .strong()
                    .size(14.0)
                    .family(theme::nav_family())
                    .color(theme::ink(ui.ctx())),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let enabled = list_count > 0;
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Trash,
                    "Trash all messages in this list",
                    enabled,
                )
                .clicked()
                {
                    self.move_listed_messages(mailbox_core::SpecialFolderKind::Trash);
                }
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Archive,
                    "Archive all messages in this list",
                    enabled,
                )
                .clicked()
                {
                    self.move_listed_messages(mailbox_core::SpecialFolderKind::Archive);
                }
            });
        });
        ui.add_space(8.0);

        let env_indices: Vec<usize> = if let Some(stak_id) = self.selected_stak.as_deref() {
            self.envelopes
                .iter()
                .enumerate()
                .filter(|(_, e)| staks::envelope_in_stak(e, stak_id))
                .map(|(i, _)| i)
                .collect()
        } else {
            (0..self.envelopes.len()).collect()
        };
        let selected = self.selected_message.clone();
        let mut open: Option<MessageId> = None;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(col_w);
                if env_indices.is_empty() {
                    ui.label(RichText::new("No messages").color(theme::mute(ui.ctx())));
                    ui.add_space(6.0);
                    let hint = if !self.list_empty_hint.is_empty() {
                        self.list_empty_hint.clone()
                    } else if self.folders.is_empty() {
                        "Nothing downloaded yet. Press Sync (g) — or open Settings → Test connection.".into()
                    } else {
                        "This folder is empty in the local cache. Press Sync (g) to refresh.".into()
                    };
                    ui.label(
                        RichText::new(hint)
                            .small()
                            .color(theme::mute(ui.ctx())),
                    );
                    ui.add_space(8.0);
                    let is_pop3 = self
                        .selected_account()
                        .map(|a| a.protocol == mailbox_core::config::MailProtocol::Pop3)
                        .unwrap_or(false);
                    ui.horizontal(|ui| {
                        if ui.button("Sync now").clicked() {
                            self.start_sync(false);
                        }
                        if !is_pop3
                            && ui
                                .button("Sync all folders")
                                .on_hover_text("Download every IMAP folder")
                                .clicked()
                        {
                            self.start_sync(true);
                        }
                    });
                    return;
                }

                let mut last_group: Option<String> = None;
                for &idx in &env_indices {
                    // Copy row fields so we can call resolve_avatar (&mut self) without
                    // holding a borrow on envelopes.
                    let (env_id, from_name, from_addr, subject_s, snippet, seen, date_ts) = {
                        let env = &self.envelopes[idx];
                        (
                            env.id.clone(),
                            env.from_name.clone(),
                            env.from_addr.clone(),
                            env.subject.clone(),
                            env.snippet.clone(),
                            env.seen,
                            env.date,
                        )
                    };
                    let group = date_group_label(date_ts);
                    if last_group.as_ref() != Some(&group) {
                        if last_group.is_some() {
                            ui.add_space(8.0);
                        }
                        ui.label(
                            RichText::new(&group)
                                .small()
                                .color(theme::mute(ui.ctx())),
                        );
                        ui.add_space(4.0);
                        last_group = Some(group);
                    }

                    let is_sel = selected.as_ref() == Some(&env_id);
                    let from = if from_name.is_empty() {
                        from_addr.as_str()
                    } else {
                        from_name.as_str()
                    };
                    let date = date_ts.format("%b %d").to_string();
                    let name_style = if seen {
                        RichText::new(from).size(12.0)
                    } else {
                        RichText::new(from)
                            .strong()
                            .size(12.0)
                            .color(theme::ink(ui.ctx()))
                    };
                    let subject = if seen {
                        RichText::new(&subject_s)
                            .small()
                            .color(theme::mute(ui.ctx()))
                    } else {
                        RichText::new(&subject_s)
                            .small()
                            .color(theme::unread(ui.ctx()))
                    };

                    let avatar = if let Some(brand_id) = staks::brand_id_for_addr(&from_addr) {
                        brand_logos::texture(ui.ctx(), brand_id)
                            .map(crate::avatars::AvatarView::Image)
                            .unwrap_or_else(|| {
                                self.resolve_avatar(ui.ctx(), &from_addr, &from_name)
                            })
                    } else {
                        self.resolve_avatar(ui.ctx(), &from_addr, &from_name)
                    };

                    let row = egui::Frame::none()
                        .fill(if is_sel {
                            theme::select(ui.ctx())
                        } else {
                            Color32::TRANSPARENT
                        })
                        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                        .rounding(8.0)
                        .show(ui, |ui| {
                            ui.set_width(col_w - 4.0);
                            ui.horizontal(|ui| {
                                avatars::avatar_widget(ui, &avatar, brand_logos::LIST_SIZE);
                                ui.add_space(6.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.add(egui::Label::new(name_style).truncate());
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(&date)
                                                        .small()
                                                        .color(theme::mute(ui.ctx())),
                                                );
                                            },
                                        );
                                    });
                                    ui.add(egui::Label::new(subject).truncate());
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&snippet)
                                                .small()
                                                .color(theme::mute(ui.ctx())),
                                        )
                                        .truncate(),
                                    );
                                });
                            });
                        });

                    if row.response.interact(Sense::click()).clicked() {
                        open = Some(env_id);
                    }
                    ui.add_space(2.0);
                }
            });

        if let Some(id) = open {
            if let Err(e) = self.open_id(&id) {
                self.status = e.to_string();
            }
        }
    }

    pub(crate) fn reader_pane(&mut self, ui: &mut Ui) {
        let Some(msg) = self.open_message.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Select a message").color(theme::mute(ui.ctx())));
            });
            return;
        };

        ui.label(
            RichText::new(&msg.envelope.subject)
                .size(18.0)
                .strong()
                .color(theme::ink(ui.ctx())),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("From  {}", msg.display_from()))
                .size(13.0)
                .color(theme::mute(ui.ctx())),
        );
        if !msg.to.is_empty() {
            ui.label(
                RichText::new(format!("To    {}", msg.to.join(", ")))
                    .size(13.0)
                    .color(theme::mute(ui.ctx())),
            );
        }
        ui.label(
            RichText::new(format!(
                "Date  {}",
                msg.envelope.date.format("%Y-%m-%d %H:%M UTC")
            ))
            .size(13.0)
            .color(theme::mute(ui.ctx())),
        );
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        let html = msg.body_html.trim();
        let use_webview = !html.is_empty();
        let has_remote = if use_webview {
            crate::mail_webview::MailWebView::has_remote_images(html)
        } else {
            self.cached_mail_body()
                .map(|b| b.has_remote_images)
                .unwrap_or(false)
        };
        if has_remote {
            let from = msg.envelope.from_addr.clone();
            ui.horizontal(|ui| {
                if self.load_remote_images {
                    if ui
                        .button("Block images from sender")
                        .on_hover_text(format!(
                            "Stop loading remote images from {from} (including later mail)"
                        ))
                        .clicked()
                    {
                        self.block_remote_images_for_sender();
                    }
                    ui.label(
                        RichText::new(format!("Images allowed for {from}"))
                            .small()
                            .color(theme::mute(ui.ctx())),
                    );
                } else if ui
                    .button("Always load images from sender…")
                    .on_hover_text(format!(
                        "Allow remote images for {from} on this and future messages (may include tracking pixels)"
                    ))
                    .clicked()
                {
                    self.allow_remote_images_for_sender();
                }
            });
            ui.add_space(8.0);
        }

        let load_images = self.load_remote_images;
        if use_webview {
            if let Some(err) = self.mail_webview.last_error.as_ref() {
                ui.label(
                    RichText::new(format!(
                        "HTML webview unavailable ({err}). Falling back to plain text."
                    ))
                    .small()
                    .color(theme::mute(ui.ctx())),
                );
                ui.add_space(6.0);
                let body = self.cached_mail_body().cloned();
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width());
                        if let Some(body) = body.as_ref() {
                            crate::mail_reader::show_body(
                                ui,
                                body,
                                load_images,
                                &mut self.mail_images,
                            );
                        }
                    });
            } else {
                let (rect, _) =
                    ui.allocate_exact_size(ui.available_size(), Sense::hover());
                // Root viewport points → wry child bounds (logical pixels).
                self.mail_webview_req = Some(crate::mail_webview::MailWebViewRequest {
                    bounds: rect,
                    msg_id: msg.envelope.id.clone(),
                    body_html: msg.body_html.clone(),
                    load_images,
                });
            }
        } else {
            let body = self.cached_mail_body().cloned();
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width());
                    if let Some(body) = body.as_ref() {
                        crate::mail_reader::show_body(
                            ui,
                            body,
                            load_images,
                            &mut self.mail_images,
                        );
                    }
                });
        }
    }

    /// Desk calendar (right side panel — mutually exclusive with Todo).
    pub(crate) fn calendar_pane(&mut self, ui: &mut Ui) {
        use egui_phosphor::regular as ph;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Calendar")
                    .strong()
                    .size(15.0)
                    .family(theme::nav_family())
                    .color(theme::accent(ui.ctx())),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let close = ui
                    .add(
                        egui::Button::new(
                            RichText::new(ph::X)
                                .family(theme::icon_family())
                                .size(14.0)
                                .color(theme::mute(ui.ctx())),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Close (Esc)");
                if close.clicked() {
                    self.show_calendar = false;
                }
                if ui
                    .small_button("↻")
                    .on_hover_text("Refresh Desk")
                    .clicked()
                {
                    self.refresh_desk_tasks();
                }
            });
        });
        ui.label(
            RichText::new("Ailiak Desk")
                .small()
                .color(theme::mute(ui.ctx())),
        );
        ui.add_space(8.0);
        let _ = calendar::show_month(ui, &mut self.calendar);
    }

    /// Todo → Desk compose (right side panel — mutually exclusive with Calendar).
    pub(crate) fn todo_pane(&mut self, ui: &mut Ui) {
        use egui_phosphor::regular as ph;
        use mailbox_ailiak::{compose_hint, parse_compose_commands};

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Todo")
                    .strong()
                    .size(15.0)
                    .family(theme::nav_family())
                    .color(theme::accent(ui.ctx())),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let close = ui
                    .add(
                        egui::Button::new(
                            RichText::new(ph::X)
                                .family(theme::icon_family())
                                .size(14.0)
                                .color(theme::mute(ui.ctx())),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Close (Esc)");
                if close.clicked() {
                    self.show_todo = false;
                }
            });
        });
        ui.label(
            RichText::new("Mail → Ailiak Desk")
                .small()
                .color(theme::mute(ui.ctx())),
        );
        ui.add_space(10.0);

        ui.label(RichText::new("Day").small().strong());
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
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
                self.open_calendar_panel();
            }
        });

        ui.add_space(12.0);
        ui.label(RichText::new("Line").small().strong());
        ui.label(
            RichText::new("Slash commands ok — same as Ailiak compose")
                .small()
                .color(theme::mute(ui.ctx())),
        );
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.todo_line)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Reply: …  #mail   /from tomorrow /estimate 1h"),
        );
        let hint = compose_hint(&parse_compose_commands(&self.todo_line));
        ui.label(RichText::new(hint).small().color(theme::mute(ui.ctx())));

        ui.add_space(12.0);
        ui.label(RichText::new("Detail").small().strong());
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.todo_detail)
                        .desired_rows(10)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
            });

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("Create on Desk").color(Color32::WHITE))
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
    }

    pub(crate) fn status_bar(&mut self, ui: &mut Ui) {
        use egui_phosphor::regular as ph;

        ui.horizontal(|ui| {
            let ailiak_label = if self.config.ailiak.enabled {
                "Ailiak"
            } else {
                "Ailiak (off)"
            };
            let ailiak = ui
                .add(
                    egui::Button::new(RichText::new(ailiak_label).small().color(
                        if self.config.ailiak.enabled {
                            theme::accent(ui.ctx())
                        } else {
                            theme::mute(ui.ctx())
                        },
                    ))
                    .frame(false),
                )
                .on_hover_text("Ailiak bridge — open Settings");
            if ailiak.clicked() {
                self.open_settings(crate::settings::SettingsSection::Ailiak);
            }

            ui.separator();
            let account_label = self
                .selected_account()
                .map(|a| {
                    if !a.name.is_empty() {
                        a.name.clone()
                    } else if !a.email.is_empty() {
                        a.email.clone()
                    } else {
                        a.id.clone()
                    }
                })
                .unwrap_or_else(|| self.account_id.clone());
            let status_text = self.status.clone();
            ui.label(
                RichText::new(format!(
                    "{} · {} · {}",
                    account_label, self.selected_folder, status_text
                ))
                .small()
                .color(theme::mute(ui.ctx())),
            );
            if !status_text.is_empty()
                && ui
                    .add(
                        egui::Button::new(
                            RichText::new(ph::COPY)
                                .family(theme::icon_family())
                                .size(12.0)
                                .color(theme::mute(ui.ctx())),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Copy status message")
                    .clicked()
            {
                ui.ctx().copy_text(status_text);
            }
        });
    }

    /// Shell: combined nav (accounts + folders) · list · reader [+ calendar].
    pub(crate) fn show_shell(&mut self, ctx: &egui::Context) {
        let panel = theme::panel(ctx);
        let paper = theme::paper(ctx);

        egui::TopBottomPanel::top("top")
            .exact_height(44.0)
            .frame(layout::panel_frame(egui::Margin::symmetric(10.0, 0.0), panel))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.add_space(7.0);
                self.top_bar(ui);
            });

        egui::TopBottomPanel::bottom("status")
            .exact_height(28.0)
            .frame(layout::panel_frame(egui::Margin::symmetric(10.0, 0.0), panel))
            .show_separator_line(false)
            .show(ctx, |ui| {
                self.status_bar(ui);
            });

        {
            let collapsed = self.folders_collapsed;
            // Bottom margin is 0 so the tools strip (Desk/Todo/…) can paint paper to the edge.
            let mut side = egui::SidePanel::left("nav_v4")
                .resizable(!collapsed)
                .show_separator_line(false)
                .frame(layout::panel_frame(
                    egui::Margin {
                        left: 8.0,
                        right: 8.0,
                        top: 8.0,
                        bottom: 0.0,
                    },
                    panel,
                ));
            side = if collapsed {
                side.exact_width(56.0)
            } else {
                side.default_width(176.0).width_range(148.0..=240.0)
            };
            side.show(ctx, |ui| {
                layout::begin_panel(ui, panel);
                self.nav_pane(ui);
                layout::end_panel(ui);
            });
        }

        if self.show_settings {
            egui::CentralPanel::default()
                .frame(layout::panel_frame(egui::Margin::same(20.0), paper))
                .show(ctx, |ui| {
                    self.settings_page(ui);
                });
            return;
        }

        // Mid column: exact_width owned by the app. egui's built-in SidePanel
        // resize persists content-driven widths and snaps back on mouse move.
        let list_range = 280.0_f32..=520.0;
        self.list_width = self.list_width.clamp(*list_range.start(), *list_range.end());

        let list_response = egui::SidePanel::left(Id::new("list_v4"))
            .resizable(false)
            .exact_width(self.list_width)
            .show_separator_line(false)
            .frame(layout::panel_frame(egui::Margin::symmetric(10.0, 8.0), panel))
            .show(ctx, |ui| {
                layout::begin_panel(ui, panel);
                self.list_pane(ui);
                layout::end_panel(ui);
            });

        self.list_panel_left = list_response.response.rect.left();
        let list_rect = list_response.response.rect;
        let grab = ctx.style().interaction.resize_grab_radius_side.max(6.0);
        let resize_rect = Rect::from_x_y_ranges(
            (list_rect.right() - grab)..=(list_rect.right() + grab),
            list_rect.y_range(),
        );

        let resize = egui::Area::new(Id::new("list_v4_resize"))
            .order(Order::Foreground)
            .fixed_pos(resize_rect.min)
            .movable(false)
            .show(ctx, |ui| {
                let (_, response) =
                    ui.allocate_exact_size(resize_rect.size(), Sense::click_and_drag());
                // Hairline affordance on the seam.
                let x = ui.painter().round_to_pixel_center(list_rect.right() - 0.5);
                ui.painter().vline(
                    x,
                    list_rect.y_range(),
                    egui::Stroke::new(1.0_f32, theme::mute(ui.ctx()).linear_multiply(0.45)),
                );
                response
            })
            .inner;

        if resize.dragged() {
            if let Some(pointer) = resize.interact_pointer_pos() {
                self.list_width = (pointer.x - self.list_panel_left)
                    .clamp(*list_range.start(), *list_range.end());
                ctx.request_repaint();
            }
            ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
        } else if resize.hovered() {
            ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
        }

        // One right side panel: Todo and Calendar replace each other.
        if self.show_todo || self.show_calendar {
            let (default_w, min_w, max_w) = if self.show_todo {
                (380.0, 320.0, 520.0)
            } else {
                (280.0, 220.0, 400.0)
            };
            egui::SidePanel::right("desk_side_v1")
                .resizable(true)
                .default_width(default_w)
                .width_range(min_w..=max_w)
                .show_separator_line(false)
                .frame(layout::panel_frame(
                    egui::Margin::symmetric(if self.show_todo { 14.0 } else { 12.0 }, 12.0),
                    panel,
                ))
                .show(ctx, |ui| {
                    layout::begin_panel(ui, panel);
                    if self.show_todo {
                        self.todo_pane(ui);
                    } else {
                        self.calendar_pane(ui);
                    }
                    layout::end_panel(ui);
                });
        }

        egui::CentralPanel::default()
            .frame(layout::panel_frame(egui::Margin::same(16.0), paper))
            .show(ctx, |ui| {
                self.reader_pane(ui);
            });
    }
}
