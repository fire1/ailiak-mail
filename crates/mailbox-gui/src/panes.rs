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
                // Keep shorter than the bar so Align::Center can sit it optically mid-row.
                let accent = theme::resolve_accent(&self.config.ui.accent).color;
                let dark = ui.visuals().dark_mode;
                self.ailiak_logo.sync(accent, dark);
                let h = 22.0_f32.min((row_h - 6.0).max(16.0));
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

        // Right: Archive / Trash / remote images / Sync
        let right = Rect::from_min_max(
            egui::pos2(bar.right() - (bar.width() * 0.28).max(248.0), bar.top()),
            bar.right_bottom(),
        );
        ui.allocate_new_ui(UiBuilder::new().max_rect(right), |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::toolbar_button(
                    ui,
                    icons::ToolbarIcon::Sync,
                    "Sync",
                    "List folders + download the selected folder (g)",
                    self.busy.sync,
                )
                .clicked()
                {
                    self.start_sync(false);
                }
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Compose,
                    "Compose (n)",
                    !self.busy.send,
                    self.busy.send,
                )
                .clicked()
                {
                    self.open_compose_new();
                }
                let has_msg = self.open_message.is_some();
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Reply,
                    "Reply (r)",
                    has_msg && !self.busy.send,
                    self.busy.send,
                )
                .clicked()
                {
                    if self.selected_person.is_some() {
                        self.chat_reply_focused = true;
                    } else {
                        self.open_reply(false);
                    }
                }
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::ReplyAll,
                    "Reply all (Shift+R)",
                    has_msg && !self.busy.send,
                    self.busy.send,
                )
                .clicked()
                {
                    self.open_reply(true);
                }
                let images_on = self.load_remote_images;
                let from = self
                    .open_message
                    .as_ref()
                    .map(|m| m.envelope.from_addr.clone())
                    .unwrap_or_default();
                let threat_n = self
                    .cached_mail_body()
                    .map(|b| b.image_threats.len())
                    .unwrap_or(0);
                let threat_note = if threat_n > 0 {
                    format!(
                        "\n{threat_n} tracking-like image(s) stay blocked for privacy"
                    )
                } else {
                    String::new()
                };
                let images_tip = if !has_msg {
                    "Open a message to allow or block remote images for its sender".to_string()
                } else if images_on {
                    format!(
                        "Remote images ON for {from}\nClick to block images from this sender (including later mail){threat_note}"
                    )
                } else {
                    format!(
                        "Remote images OFF for {from}\nClick to always load images from this sender (benign images only; tracking-like stay blocked){threat_note}"
                    )
                };
                if icons::toolbar_remote_images_button(ui, images_on, has_msg, &images_tip)
                    .clicked()
                {
                    if images_on {
                        self.block_remote_images_for_sender();
                    } else {
                        self.allow_remote_images_for_sender();
                    }
                }
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Trash,
                    "Trash current message (#)",
                    has_msg && !self.busy.moving(),
                    self.busy.trash,
                )
                .clicked()
                {
                    self.move_current_message(mailbox_core::SpecialFolderKind::Trash);
                }
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Archive,
                    "Archive current message (e)",
                    has_msg && !self.busy.moving(),
                    self.busy.archive,
                )
                .clicked()
                {
                    self.move_current_message(mailbox_core::SpecialFolderKind::Archive);
                }
                let snooze_tip = if mailbox_core::is_snoozed_folder(&self.selected_folder) {
                    "Unsnooze — put back in its folder (stays as-is)"
                } else {
                    "Snooze — hide for later without marking read"
                };
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Snooze,
                    snooze_tip,
                    has_msg && !self.busy.snooze,
                    self.busy.snooze,
                )
                .clicked()
                {
                    self.snooze_current_message();
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
        let mut clicked_person: Option<String> = None;
        let mut open_folders_settings = false;
        let mut sync_folders: Option<bool> = None; // Some(true) = all IMAP folders
        let mut open_desk = false;
        let mut open_todo = false;
        let mut open_calendar = false;
        let mut open_settings = false;
        let mut open_help = false;
        let mut toggle_collapse = false;
        let mut toggle_label_path: Option<String> = None;

        ui.set_width(ui.available_width());
        // Tighter vertical rhythm than the global style (folders felt too airy).
        ui.spacing_mut().item_spacing.y = 2.0;

        // Top-down: scrollable accounts/folders, then a fixed tools strip (paper fill).
        // 5 rows × 28 + padding (Desk, Todo, Calendar, Settings, Help).
        let row_h = 28.0;
        let footer_h = row_h * 5.0 + 12.0 + 16.0;
        let body_h = (ui.available_height() - footer_h).max(48.0);

        let paper = theme::paper(ui.ctx());
        // Match SidePanel left margin; right gutter lets paper bands show rounded NE/SE.
        let side_inset = 8.0;
        let paper_right_gutter = 8.0;

        ScrollArea::vertical()
            .id_salt("nav_scroll_v5")
            .max_height(body_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;

                // Accounts paper band — flush left, rounded on the right.
                layout::nav_paper_band(
                    paper,
                    egui::Margin {
                        left: side_inset,
                        right: side_inset,
                        top: 8.0,
                        bottom: 8.0,
                    },
                    egui::Margin {
                        left: -side_inset, // cancel SidePanel left margin → flush to window
                        right: paper_right_gutter,
                        top: -8.0, // cancel SidePanel top margin
                        bottom: 0.0,
                    },
                )
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
                                // Leave 8px so the collapse control isn't flush to the edge.
                                ui.add_space((ui.available_width() - 28.0 - 8.0).max(0.0));
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

                // People / Folders / Labels. Folders+Labels can both stay open;
                // opening People closes them (and vice versa).
                let show_all_mail = self.config.ui.show_all_mail;
                let system_folders: Vec<_> = folders
                    .iter()
                    .filter(|f| {
                        let role = mailbox_core::classify_mailbox(&f.path, &f.name);
                        role.is_system()
                            && (show_all_mail
                                || !mailbox_core::is_all_mail(&f.path, &f.name))
                    })
                    .cloned()
                    .collect();
                let user_folders: Vec<_> = folders
                    .iter()
                    .filter(|f| {
                        mailbox_core::classify_mailbox(&f.path, &f.name)
                            == mailbox_core::MailboxRole::User
                    })
                    .cloned()
                    .collect();
                let label_tree = mailbox_core::build_label_tree(&user_folders);
                let folders_unread: u32 = system_folders.iter().map(|f| f.unread).sum();
                let labels_unread: u32 = label_tree.iter().map(|n| n.unread_sum).sum();
                let has_people = !self.people_cache.is_empty();
                let has_labels = !label_tree.is_empty();
                let selected_person = self.selected_person.clone();

                if has_people {
                    let open = self.people_section_open;
                    let people_unread: u32 = self.people_cache.iter().map(|p| p.unread).sum();
                    let header = nav_accordion_header(
                        ui,
                        "People",
                        open,
                        people_unread,
                        mute,
                        collapsed,
                        None,
                    );
                    if header.toggle {
                        if open {
                            self.people_section_open = false;
                        } else {
                            self.people_section_open = true;
                            self.folders_section_open = false;
                            self.labels_section_open = false;
                            self.staks_section_open = false;
                        }
                    }
                    ui.add_space(2.0);
                    if self.people_section_open {
                        for person in &self.people_cache {
                            let selected =
                                selected_person.as_deref() == Some(person.addr.as_str());
                            let label = crate::people::display_name(person).to_string();
                            if icons::stak_nav_button(
                                ui,
                                &label,
                                person.unread,
                                selected,
                                collapsed,
                                None,
                            )
                            .on_hover_text(&person.addr)
                            .clicked()
                            {
                                clicked_person = Some(person.addr.clone());
                            }
                        }
                        ui.add_space(8.0);
                    } else {
                        ui.add_space(4.0);
                    }
                }

                let (is_pop3, is_abv) = self
                    .selected_account()
                    .map(|a| (a.is_pop3(), a.looks_like_abv()))
                    .unwrap_or((false, false));
                let sync_tip = mailbox_core::nav_sync_tip(is_pop3, is_abv);

                {
                    let open = self.folders_section_open;
                    let header = nav_accordion_header(
                        ui,
                        "Folders",
                        open,
                        folders_unread,
                        mute,
                        collapsed,
                        if collapsed {
                            None
                        } else {
                            Some(FolderHeaderActions {
                                sync_tip,
                                sync_busy: self.busy.sync,
                                manage_tip: "Manage folders & labels",
                            })
                        },
                    );
                    if header.toggle {
                        if open {
                            self.folders_section_open = false;
                        } else {
                            self.folders_section_open = true;
                            self.staks_section_open = false;
                            self.people_section_open = false;
                        }
                    }
                    if header.manage {
                        open_folders_settings = true;
                    }
                    if header.sync {
                        sync_folders = Some(!is_pop3);
                    }
                    ui.add_space(2.0);
                }

                if self.folders_section_open {
                    for folder in &system_folders {
                        let selected = folder.path == selected_folder;
                        let kind = FolderIcon::from_folder(&folder.path, &folder.name);
                        if icons::folder_nav_button(
                            ui,
                            kind,
                            &folder.name,
                            folder.unread,
                            selected,
                            collapsed,
                        )
                        .clicked()
                        {
                            clicked_folder = Some(folder.path.clone());
                        }
                    }
                    ui.add_space(4.0);
                }

                if has_labels {
                    let open = self.labels_section_open;
                    let header = nav_accordion_header(
                        ui,
                        "Labels",
                        open,
                        labels_unread,
                        mute,
                        collapsed,
                        if collapsed {
                            None
                        } else {
                            Some(FolderHeaderActions {
                                sync_tip,
                                sync_busy: self.busy.sync,
                                manage_tip: "Manage folders & labels",
                            })
                        },
                    );
                    if header.toggle {
                        if open {
                            self.labels_section_open = false;
                        } else {
                            self.labels_section_open = true;
                            self.staks_section_open = false;
                        }
                    }
                    if header.manage {
                        open_folders_settings = true;
                    }
                    if header.sync {
                        sync_folders = Some(!is_pop3);
                    }
                    ui.add_space(2.0);
                    if self.labels_section_open {
                        paint_label_tree(
                            ui,
                            &label_tree,
                            &selected_folder,
                            collapsed,
                            &self.label_expanded,
                            0,
                            "",
                            &mut clicked_folder,
                            &mut toggle_label_path,
                        );
                    }
                }
            });

        // Tools strip — same paper as Accounts; flush left, rounded on the right.
        layout::nav_paper_band(
            paper,
            egui::Margin {
                left: side_inset,
                right: side_inset,
                top: 8.0,
                bottom: 16.0,
            },
            egui::Margin {
                left: -side_inset,
                right: paper_right_gutter,
                top: 0.0,
                bottom: 8.0,
            },
        )
        .show(ui, |ui| {
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
        });

        if let Some(path) = toggle_label_path {
            if self.label_expanded.contains(&path) {
                self.label_expanded.remove(&path);
            } else {
                self.label_expanded.insert(path);
            }
        }
        if toggle_collapse {
            self.folders_collapsed = !self.folders_collapsed;
        }
        if let Some(id) = switch_to {
            self.on_account_avatar_click(&id);
        }
        if add_account {
            self.clear_list_multi_select();
            self.start_new_account();
        }
        if let Some(path) = clicked_folder {
            self.clear_list_multi_select();
            self.selected_folder = path;
            self.selected_stak = None;
            self.selected_person = None;
            self.chat_messages.clear();
            self.selected_message = None;
            self.open_message = None;
            if let Err(e) = self.refresh_envelopes() {
                self.status = e.to_string();
            } else if let Some(first) = self.envelopes.first().map(|e| e.id.clone()) {
                let _ = self.open_id(&first);
            }
        }
        if let Some(person) = clicked_person {
            self.clear_list_multi_select();
            self.open_person_chat(&person);
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

    /// Folder title + archive/trash — sits above the mid list card.
    pub(crate) fn list_pane_header(&mut self, ui: &mut Ui) {
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
        let multi = self.list_multi_select;
        let selected_n = if multi {
            self.list_selected.len()
        } else {
            0
        };
        ui.horizontal(|ui| {
            let title = if multi {
                if selected_n == 0 {
                    format!("{list_title} · select")
                } else {
                    format!("{list_title} · {selected_n} selected")
                }
            } else {
                list_title
            };
            ui.label(
                RichText::new(title)
                    .strong()
                    .size(14.0)
                    .family(theme::nav_family())
                    .color(theme::ink(ui.ctx())),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(10.0);
                let enabled = if multi {
                    selected_n > 0 && !self.busy.moving()
                } else {
                    list_count > 0 && !self.busy.moving()
                };
                let trash_tip = if multi {
                    "Trash selected messages"
                } else {
                    "Trash all messages in this list"
                };
                let archive_tip = if multi {
                    "Archive selected messages"
                } else {
                    "Archive all messages in this list"
                };
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Trash,
                    trash_tip,
                    enabled,
                    self.busy.trash,
                )
                .clicked()
                {
                    self.move_listed_messages(mailbox_core::SpecialFolderKind::Trash);
                }
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Archive,
                    archive_tip,
                    enabled,
                    self.busy.archive,
                )
                .clicked()
                {
                    self.move_listed_messages(mailbox_core::SpecialFolderKind::Archive);
                }
            });
        });
    }

    /// Message list body (inside the mid card).
    pub(crate) fn list_pane(&mut self, ui: &mut Ui) {
        let col_w = ui.available_width();
        // Cap content to the panel — never set_min_width larger than the
        // resized panel or egui will persist a wider PanelState and snap back.
        ui.set_max_width(col_w);

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
        let multi = self.list_multi_select;
        let mut open: Option<MessageId> = None;
        let mut toggle_sel: Option<MessageId> = None;
        // Avatar click while not in multi-select → enter mode and check this id.
        let mut start_multi: Option<MessageId> = None;

        // Thin floating bar (widens on hover); thumb stays visible for position.
        ui.spacing_mut().scroll = theme::list_scroll_style();

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
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
                    if icons::busy_text_button(ui, "Sync now", self.busy.sync)
                        .on_hover_text("Refresh this folder (g)")
                        .clicked()
                    {
                        self.start_sync(false);
                    }
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
                    let is_checked = multi && self.list_selected.contains(&env_id);
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

                    let row_fill = if multi && is_checked {
                        theme::select(ui.ctx())
                    } else if is_sel && !multi {
                        theme::select(ui.ctx())
                    } else {
                        Color32::TRANSPARENT
                    };

                    let mut avatar_clicked = false;
                    let mut av_rect = egui::Rect::NOTHING;
                    let row = egui::Frame::none()
                        .fill(row_fill)
                        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                        .rounding(8.0)
                        .show(ui, |ui| {
                            // available_width is already inside this Frame's margins —
                            // do not use outer `col_w` or rows overflow and egui shifts
                            // the floating scrollbar inward (gap before the panel edge).
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let size = brand_logos::LIST_SIZE;
                                // Own click target — must not share Sense with the row body,
                                // or Frame.interact steals the avatar click.
                                let (rect, av_resp) =
                                    ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
                                av_rect = rect;
                                // Only checked rows become checkboxes; others keep sender pictures.
                                if is_checked {
                                    icons::paint_selection_checkbox(ui, av_rect, true);
                                } else {
                                    match &avatar {
                                        crate::avatars::AvatarView::Image(tex) => {
                                            egui::Image::new((tex.id(), egui::vec2(size, size)))
                                                .rounding(egui::Rounding::same(
                                                    (size * 0.22).clamp(4.0, 8.0),
                                                ))
                                                .paint_at(ui, av_rect);
                                        }
                                        crate::avatars::AvatarView::Initials { letters, fill } => {
                                            ui.painter().rect_filled(
                                                av_rect,
                                                egui::Rounding::same(
                                                    (size * 0.22).clamp(4.0, 8.0),
                                                ),
                                                *fill,
                                            );
                                            ui.painter().text(
                                                av_rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                letters,
                                                egui::FontId::proportional(
                                                    (size * 0.4).clamp(8.0, 12.0),
                                                ),
                                                if fill.r() as u16
                                                    + fill.g() as u16
                                                    + fill.b() as u16
                                                    > 400
                                                {
                                                    Color32::from_rgb(0x2a, 0x35, 0x31)
                                                } else {
                                                    Color32::from_rgb(0xf4, 0xf8, 0xf6)
                                                },
                                            );
                                        }
                                    }
                                }
                                let tip = if is_checked {
                                    "Uncheck message"
                                } else {
                                    "Click picture to multi-select messages"
                                };
                                if av_resp.on_hover_text(tip).clicked() {
                                    avatar_clicked = true;
                                }

                                ui.add_space(6.0);
                                // Labels must be hover-only — default Label sense steals
                                // clicks so row open only fired on padding gaps.
                                let label_sense = Sense::hover();
                                ui.vertical(|ui| {
                                    ui.set_min_width(ui.available_width());
                                    // Date first (right), then name fills leftover and truncates —
                                    // avoids the date overflowing into the scrollbar gutter.
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&date)
                                                        .small()
                                                        .color(theme::mute(ui.ctx())),
                                                )
                                                .sense(label_sense),
                                            );
                                            ui.add(
                                                egui::Label::new(name_style)
                                                    .truncate()
                                                    .sense(label_sense),
                                            );
                                        },
                                    );
                                    ui.add(
                                        egui::Label::new(subject)
                                            .truncate()
                                            .sense(label_sense),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&snippet)
                                                .small()
                                                .color(theme::mute(ui.ctx())),
                                        )
                                        .truncate()
                                        .sense(label_sense),
                                    );
                                });
                            });
                        });

                    // Whole-row click (minus avatar) opens the message. Using Frame
                    // interact avoids egui 0.29 missing clicks on vertical().response.
                    let row_click = row.response.interact(Sense::click());
                    let body_clicked = !avatar_clicked
                        && row_click.clicked()
                        && row_click.interact_pointer_pos().is_some_and(|p| !av_rect.contains(p));

                    // Avatar = multi-select; row body always opens/focuses the message.
                    if avatar_clicked {
                        if multi {
                            toggle_sel = Some(env_id);
                        } else {
                            start_multi = Some(env_id);
                        }
                    } else if body_clicked {
                        open = Some(env_id);
                    }
                    ui.add_space(2.0);
                }
            });

        if let Some(id) = start_multi {
            if !self.list_multi_select {
                self.toggle_list_multi_select();
            }
            self.toggle_list_message_selected(&id);
        } else if let Some(id) = toggle_sel {
            self.toggle_list_message_selected(&id);
        }
        if let Some(id) = open {
            if let Err(e) = self.open_id(&id) {
                self.status = e.to_string();
            }
        }
    }

    /// Classic-mail reply: original message on top, Talanoa dock below — hard clipped.
    fn paint_inline_reply_reader(
        &mut self,
        ui: &mut Ui,
        msg: &mailbox_core::types::Message,
    ) {
        let load_images = self.load_remote_images;
        let body = self.cached_mail_body().cloned();
        let window_h = ui.ctx().screen_rect().height();
        let focused = self.compose.as_ref().is_some_and(|c| c.dock_focused);
        let paper = theme::reader_card(ui.ctx());

        // Heights must sum exactly to budget — never .max() a slice back up or
        // the original-mail strip and the dock share the same pixels (overlapping
        // “Robin” / from-line when switching messages).
        let bottom_clearance = 24.0;
        let gap = 8.0;
        let min_top = 72.0;
        let budget = ui.available_height().min(ui.clip_rect().height()).max(0.0);
        let compose_cap = (budget - bottom_clearance - gap - min_top).max(140.0);
        let compose_h = if focused {
            (budget * 0.32).clamp(180.0, 260.0)
        } else {
            176.0
        }
        .min(compose_cap);
        let top_h = (budget - compose_h - gap - bottom_clearance).max(0.0);
        let width = ui.available_width();

        // Top: original mail (clipped).
        let (top_rect, _) =
            ui.allocate_exact_size(egui::vec2(width, top_h), Sense::hover());
        ui.allocate_new_ui(UiBuilder::new().max_rect(top_rect), |ui| {
            ui.set_clip_rect(top_rect);
            ui.painter().rect_filled(top_rect, 0.0, paper);
            ui.label(
                RichText::new(&msg.envelope.subject)
                    .size(15.0)
                    .strong()
                    .color(theme::ink(ui.ctx())),
            );
            ui.label(
                RichText::new(msg.display_from())
                    .small()
                    .color(theme::mute(ui.ctx())),
            );
            ui.add_space(6.0);
            ScrollArea::vertical()
                .id_salt("inline_reply_orig")
                .auto_shrink([false, false])
                .max_height((top_rect.height() - 48.0).max(24.0))
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width());
                    if let Some(body) = body.as_ref() {
                        crate::mail_reader::show_body(
                            ui,
                            body,
                            load_images,
                            &mut self.mail_images,
                        );
                    } else {
                        ui.label(&msg.body_text);
                    }
                });
        });

        ui.add_space(gap);

        // Bottom: opaque dock (covers any bleed from above).
        let (bot_rect, _) =
            ui.allocate_exact_size(egui::vec2(width, compose_h), Sense::hover());
        ui.allocate_new_ui(UiBuilder::new().max_rect(bot_rect), |ui| {
            ui.set_clip_rect(bot_rect);
            ui.painter().rect_filled(bot_rect, 0.0, paper);
            if let Some(mut draft) = self.compose.take() {
                let res = crate::compose::show_inline_compose(
                    ui,
                    &mut draft,
                    self.busy.send,
                    window_h,
                );
                if res.send {
                    self.compose = Some(draft);
                    self.submit_compose();
                } else if res.open_full {
                    draft.inline = false;
                    self.compose = Some(draft);
                } else if !res.close {
                    self.compose = Some(draft);
                }
            }
        });

        // Guaranteed empty strip under the dock (inside the reader card).
        ui.add_space(bottom_clearance);
    }

    /// People conversation — chat bubbles + focus-expanding reply dock.
    pub(crate) fn chat_reader_pane(&mut self, ui: &mut Ui) {
        let peer = self.selected_person.clone().unwrap_or_default();
        let me = self.account_email().to_lowercase();
        let title = self
            .people_cache
            .iter()
            .find(|p| p.addr == peer)
            .map(|p| crate::people::display_name(p).to_string())
            .unwrap_or_else(|| peer.clone());

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&title)
                    .size(18.0)
                    .strong()
                    .color(theme::ink(ui.ctx())),
            );
            ui.label(RichText::new(&peer).small().color(theme::mute(ui.ctx())));
        });
        ui.add_space(8.0);
        ui.separator();

        let window_h = ui.ctx().screen_rect().height();
        // Exact split so the title row and the reply dock never share pixels.
        let dock_h = if self.chat_reply_focused {
            (window_h * 0.30).clamp(188.0, 280.0)
        } else {
            148.0
        };
        let gap = 8.0;
        let budget = ui.available_height().max(0.0);
        let dock_h = dock_h.min((budget - gap - 48.0).max(120.0));
        let scroll_h = (budget - dock_h - gap).max(40.0);
        let width = ui.available_width();

        let (scroll_rect, _) =
            ui.allocate_exact_size(egui::vec2(width, scroll_h), Sense::hover());
        ui.allocate_new_ui(UiBuilder::new().max_rect(scroll_rect), |ui| {
            ui.set_clip_rect(scroll_rect);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .max_height(scroll_rect.height())
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if self.chat_messages.is_empty() {
                        ui.label(
                            RichText::new(
                                "No messages with this person in the local cache yet.",
                            )
                            .color(theme::mute(ui.ctx())),
                        );
                        return;
                    }
                    for msg in &self.chat_messages {
                        let mine = msg.envelope.from_addr.trim().eq_ignore_ascii_case(&me);
                        let bubble_fill = if mine {
                            theme::select(ui.ctx())
                        } else {
                            theme::panel(ui.ctx())
                        };
                        let align = if mine {
                            egui::Align::Max
                        } else {
                            egui::Align::Min
                        };
                        ui.with_layout(egui::Layout::top_down(align), |ui| {
                            let max_w = ui.available_width() * 0.78;
                            egui::Frame::none()
                                .fill(bubble_fill)
                                .rounding(12.0)
                                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                .show(ui, |ui| {
                                    ui.set_max_width(max_w);
                                    let who = if mine {
                                        "You".to_string()
                                    } else if msg.envelope.from_name.is_empty() {
                                        msg.envelope.from_addr.clone()
                                    } else {
                                        msg.envelope.from_name.clone()
                                    };
                                    ui.label(
                                        RichText::new(format!(
                                            "{who} · {}",
                                            msg.envelope.date.format("%b %d %H:%M")
                                        ))
                                        .small()
                                        .color(theme::mute(ui.ctx())),
                                    );
                                    if !msg.envelope.subject.is_empty() {
                                        ui.label(
                                            RichText::new(&msg.envelope.subject)
                                                .small()
                                                .strong(),
                                        );
                                    }
                                    ui.label(RichText::new(msg.body_text.trim()).size(13.0));
                                });
                        });
                        ui.add_space(8.0);
                    }
                });
        });

        ui.add_space(gap);
        let (dock_rect, _) =
            ui.allocate_exact_size(egui::vec2(width, dock_h), Sense::hover());
        let mut body = std::mem::take(&mut self.chat_reply);
        let mut focused = self.chat_reply_focused;
        let mut attachments = std::mem::take(&mut self.chat_attachments);
        let reply_name = self
            .people_cache
            .iter()
            .find(|p| p.addr == peer)
            .map(|p| crate::people::display_name(p).to_string())
            .unwrap_or_else(|| peer.clone());
        let from_email = self.account_email();
        let dock = ui
            .allocate_new_ui(UiBuilder::new().max_rect(dock_rect), |ui| {
                ui.set_clip_rect(dock_rect);
                crate::compose::show_chat_reply_dock(
                    ui,
                    &mut body,
                    &mut focused,
                    &mut attachments,
                    window_h,
                    self.busy.send,
                    &reply_name,
                    &from_email,
                )
            })
            .inner;
        self.chat_reply = body;
        self.chat_reply_focused = focused;
        self.chat_attachments = attachments;
        if dock.send {
            self.submit_chat_reply();
        }
        if dock.open_full {
            if let Some(msg) = self.chat_messages.last().cloned() {
                self.open_message = Some(msg);
                self.open_reply(false);
                if let Some(draft) = self.compose.as_mut() {
                    draft.inline = false;
                    draft.body = std::mem::take(&mut self.chat_reply);
                    draft.attachments = std::mem::take(&mut self.chat_attachments);
                }
            } else {
                let mut draft = crate::compose::ComposeDraft::new_blank();
                draft.to = peer;
                draft.from_email = from_email;
                draft.body = std::mem::take(&mut self.chat_reply);
                draft.attachments = std::mem::take(&mut self.chat_attachments);
                self.compose = Some(draft);
            }
        }
    }

    pub(crate) fn reader_pane(&mut self, ui: &mut Ui) {
        if self.selected_person.is_some() {
            self.chat_reader_pane(ui);
            return;
        }

        let Some(msg) = self.open_message.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Select a message").color(theme::mute(ui.ctx())));
            });
            return;
        };

        // Reply / reply-all: split reader into clipped original + opaque dock
        // so mail body can never paint through the composer. Only for the
        // message that opened the reply — switching mails clears compose.
        let inline_reply = self.compose.as_ref().is_some_and(|c| {
            c.inline
                && c.is_reply()
                && c.reply_source_id.as_ref() == Some(&msg.envelope.id)
        });
        if inline_reply {
            self.paint_inline_reply_reader(ui, &msg);
            return;
        }

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
        let load_images = self.load_remote_images;
        let threat_n = self
            .cached_mail_body()
            .map(|b| b.image_threats.len())
            .unwrap_or(0);
        if threat_n > 0 {
            ui.label(
                RichText::new(format!(
                    "{threat_n} tracking-like image(s) blocked for privacy"
                ))
                .small()
                .color(theme::mute(ui.ctx())),
            );
            ui.add_space(6.0);
        } else if use_webview && !load_images {
            let has_remote = self
                .cached_mail_body()
                .is_some_and(|b| b.has_remote_images);
            if has_remote {
                ui.label(
                    RichText::new(
                        "Remote images blocked until you allow this sender (toolbar image icon).",
                    )
                    .small()
                    .color(theme::mute(ui.ctx())),
                );
                ui.add_space(6.0);
            }
        }
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
                // Use the full remaining reader rect (not a one-shot available_size that
                // can be tiny before SidePanels finish settling).
                let rect = ui.available_rect_before_wrap();
                ui.allocate_rect(rect, Sense::hover());
                // Match reader card so recreate/load doesn't flash chrome through.
                let dark = ui.visuals().dark_mode;
                let underlay = theme::reader_card(ui.ctx());
                ui.painter().rect_filled(rect, 0.0, underlay);
                let loading = self.mail_webview.is_loading(&msg.envelope.id);
                if loading {
                    ui.put(
                        rect,
                        egui::Spinner::new().size(22.0).color(theme::mute(ui.ctx())),
                    );
                    ui.ctx().request_repaint();
                }
                // Root viewport points → wry child bounds (logical pixels).
                self.mail_webview_req = Some(crate::mail_webview::MailWebViewRequest {
                    bounds: rect,
                    msg_id: msg.envelope.id.clone(),
                    body_html: msg.body_html.clone(),
                    load_images,
                    dark,
                });
                // Keep painting until the webview has a stable non-tiny rect.
                if rect.width() < 80.0 || rect.height() < 80.0 {
                    ui.ctx().request_repaint();
                }
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
                if self.busy.ailiak || self.calendar.loading {
                    let size = egui::vec2(18.0, 18.0);
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
                    ui.put(rect, egui::Spinner::new().size(12.0));
                    let _ = resp.on_hover_text("Refreshing Desk…");
                } else if ui
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
            if self.busy.ailiak {
                ui.add_enabled_ui(false, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.label(
                            RichText::new("Create on Desk").color(theme::mute(ui.ctx())),
                        );
                    });
                });
            } else if ui
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
            // Right margin 0 so the ScrollArea scrollbar sits on the panel edge
            // (content keeps its own side_inset padding inside the scroll).
            let mut side = egui::SidePanel::left("nav_v4")
                .resizable(!collapsed)
                .show_separator_line(false)
                .frame(layout::panel_frame(
                    egui::Margin {
                        left: 8.0,
                        right: 0.0,
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

        // Tight gap between mid ↔ reader; keep outer chrome gutters.
        // Extra bottom when the inline reply dock is open so Send stays above the status bar.
        let inline_reply_open = self
            .compose
            .as_ref()
            .is_some_and(|c| c.inline && c.is_reply());
        let card_gutter = egui::Margin {
            left: 4.0,
            right: 10.0,
            top: 10.0,
            bottom: if inline_reply_open { 28.0 } else { 10.0 },
        };
        // Reader keeps top gutter; mid puts the folder title above the card.
        let list_card_gutter = egui::Margin {
            left: 10.0,
            right: 4.0,
            top: 4.0,
            bottom: 10.0,
        };
        let list_pad = egui::Margin {
            left: 10.0,
            right: 10.0,
            top: 8.0,
            bottom: 8.0,
        };
        let list_card = theme::list_card(ctx);
        let reader_card = theme::reader_card(ctx);
        let card_edge = egui::Stroke::new(
            1.0,
            theme::mute(ctx).linear_multiply(0.28),
        );

        let list_response = egui::SidePanel::left(Id::new("list_v4"))
            .resizable(false)
            .exact_width(self.list_width)
            .show_separator_line(false)
            .frame(layout::panel_frame(egui::Margin::ZERO, panel))
            .show(ctx, |ui| {
                layout::begin_panel(ui, panel);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(list_card_gutter.left);
                    ui.allocate_ui_with_layout(
                        egui::vec2(
                            (ui.available_width() - list_card_gutter.right).max(0.0),
                            28.0,
                        ),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            self.list_pane_header(ui);
                        },
                    );
                });
                ui.add_space(4.0);
                // Accent-tinted rounded card (title stays above on chrome).
                layout::content_card_stroke(list_card, list_card_gutter, list_pad, card_edge)
                    .show(ui, |ui| {
                        ui.set_min_height(ui.available_height());
                        self.list_pane(ui);
                    });
                layout::end_panel(ui);
            });

        self.list_panel_left = list_response.response.rect.left();
        let list_rect = list_response.response.rect;
        // Seam from exact_width — not response.rect.right(), which can grow if
        // content overflows and would leave a fake gap past the scrollbar.
        let seam_x = self.list_panel_left + self.list_width;
        let grab = ctx.style().interaction.resize_grab_radius_side.max(6.0);
        let resize_rect = Rect::from_x_y_ranges(
            (seam_x - grab)..=(seam_x + grab),
            list_rect.y_range(),
        );

        // Drag hit-target only — card gutters replace the old hairline seam.
        let resize = egui::Area::new(Id::new("list_v4_resize"))
            .order(Order::Foreground)
            .fixed_pos(resize_rect.min)
            .movable(false)
            .show(ctx, |ui| {
                let (_, response) =
                    ui.allocate_exact_size(resize_rect.size(), Sense::click_and_drag());
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
            .frame(layout::panel_frame(egui::Margin::ZERO, panel))
            .show(ctx, |ui| {
                layout::begin_panel(ui, panel);
                let reader_pad = if inline_reply_open {
                    egui::Margin {
                        left: 16.0,
                        right: 16.0,
                        top: 16.0,
                        // Inner bottom pad is small — paint_inline_reply_reader
                        // reserves its own bottom_clearance inside the card.
                        bottom: 4.0,
                    }
                } else {
                    egui::Margin::same(16.0)
                };
                layout::content_card(reader_card, card_gutter, reader_pad).show(ui, |ui| {
                    let h = ui.available_height();
                    if inline_reply_open {
                        // Cap height so the dock cannot push the card past the status bar.
                        ui.set_max_height(h);
                    } else {
                        ui.set_min_height(h);
                    }
                    self.reader_pane(ui);
                });
                layout::end_panel(ui);
            });
    }
}

struct FolderHeaderActions {
    sync_tip: &'static str,
    sync_busy: bool,
    manage_tip: &'static str,
}

struct NavAccordionResult {
    toggle: bool,
    manage: bool,
    sync: bool,
}

/// Collapsible section title with optional unread badge and folder action icons.
/// In the icon rail (`collapsed`), only a centered caret (+ badge) is shown.
fn nav_accordion_header(
    ui: &mut Ui,
    title: &str,
    open: bool,
    unread: u32,
    mute: Color32,
    collapsed: bool,
    folder_actions: Option<FolderHeaderActions>,
) -> NavAccordionResult {
    let mut result = NavAccordionResult {
        toggle: false,
        manage: false,
        sync: false,
    };

    let caret = if open {
        egui_phosphor::regular::CARET_DOWN
    } else {
        egui_phosphor::regular::CARET_RIGHT
    };
    let tip = if open {
        format!("Collapse {title}")
    } else {
        format!("Expand {title}")
    };

    if collapsed {
        ui.horizontal(|ui| {
            let btn = 28.0;
            ui.add_space((ui.available_width() - btn).max(0.0) * 0.5);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(btn, btn), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, 6.0, theme::chip(ui.ctx()));
            }
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                caret,
                theme::icon_font(14.0),
                mute,
            );
            if unread > 0 {
                icons::paint_count_badge(ui, rect, unread, true);
            }
            if resp.on_hover_text(tip).clicked() {
                result.toggle = true;
            }
        });
        return result;
    }

    // Peek hover on the upcoming header row so gear/sync stay hidden until needed.
    let row_h = 28.0;
    let peek = ui.available_rect_before_wrap();
    let header_rect =
        egui::Rect::from_min_size(peek.min, egui::vec2(peek.width(), row_h));
    let row_hovered = ui.rect_contains_pointer(header_rect);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // Phosphor caret must use icon_family — nav_family (Roboto Mono) shows tofu.
        let caret_resp = ui
            .add(
                egui::Button::new(
                    RichText::new(caret)
                        .family(theme::icon_family())
                        .size(11.0)
                        .color(mute),
                )
                .frame(false),
            )
            .on_hover_text(&tip);
        let title_resp = ui
            .add(
                egui::Button::new(
                    RichText::new(title)
                        .small()
                        .family(theme::nav_family())
                        .color(mute),
                )
                .frame(false),
            )
            .on_hover_text(&tip);
        if caret_resp.clicked() || title_resp.clicked() {
            result.toggle = true;
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Nav panel right margin is 0 (scrollbar flush); keep action icons inset.
            ui.add_space(8.0);
            if let Some(actions) = folder_actions {
                let show_actions = row_hovered || actions.sync_busy;
                if show_actions {
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
                        .on_hover_text(actions.manage_tip);
                    if manage.clicked() {
                        result.manage = true;
                    }
                    if actions.sync_busy {
                        let size = egui::vec2(16.0, 16.0);
                        let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
                        ui.put(rect, egui::Spinner::new().size(12.0));
                        let _ = resp.on_hover_text(format!("{} (working…)", actions.sync_tip));
                    } else {
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
                            .on_hover_text(actions.sync_tip);
                        if sync.clicked() {
                            result.sync = true;
                        }
                    }
                }
            }

            if unread > 0 {
                let size = icons::count_badge_size(ui, unread);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                icons::paint_count_badge_at(ui, rect, unread);
            }
        });
    });

    result
}

/// Recursively paint the Labels tree (indent + expand carets).
fn paint_label_tree(
    ui: &mut Ui,
    nodes: &[mailbox_core::LabelNode],
    selected_folder: &str,
    collapsed: bool,
    label_expanded: &std::collections::HashSet<String>,
    indent_level: u8,
    parent_key: &str,
    clicked_folder: &mut Option<String>,
    toggle_label_path: &mut Option<String>,
) {
    for node in nodes {
        let node_key = if parent_key.is_empty() {
            node.segment.clone()
        } else {
            format!("{parent_key}/{}", node.segment)
        };
        let has_kids = !node.children.is_empty();
        let expanded = !has_kids || label_expanded.contains(&node_key);

        if collapsed {
            if let Some(path) = node.full_path.as_ref() {
                let selected = path == selected_folder;
                if icons::folder_nav_button(
                    ui,
                    FolderIcon::Folder,
                    &node.display_name,
                    node.unread_sum,
                    selected,
                    true,
                )
                .clicked()
                {
                    *clicked_folder = Some(path.clone());
                }
            }
            if has_kids {
                paint_label_tree(
                    ui,
                    &node.children,
                    selected_folder,
                    collapsed,
                    label_expanded,
                    indent_level.saturating_add(1),
                    &node_key,
                    clicked_folder,
                    toggle_label_path,
                );
            }
            continue;
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let caret_w = 16.0;
            if has_kids {
                let caret = if expanded {
                    egui_phosphor::regular::CARET_DOWN
                } else {
                    egui_phosphor::regular::CARET_RIGHT
                };
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(caret_w, 28.0), Sense::click());
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    caret,
                    theme::icon_font(11.0),
                    theme::mute(ui.ctx()),
                );
                if resp.clicked() {
                    *toggle_label_path = Some(node_key.clone());
                }
            } else {
                ui.add_space(caret_w);
            }

            let selected = node
                .full_path
                .as_deref()
                .is_some_and(|p| p == selected_folder);
            let badge = if has_kids {
                node.unread_sum
            } else {
                node.unread
            };
            let resp = icons::folder_nav_button_indent(
                ui,
                FolderIcon::Folder,
                &node.display_name,
                badge,
                selected,
                false,
                indent_level,
            );
            if resp.clicked() {
                if let Some(path) = node.full_path.as_ref() {
                    *clicked_folder = Some(path.clone());
                } else if has_kids {
                    *toggle_label_path = Some(node_key.clone());
                }
            }
        });

        if has_kids && expanded {
            paint_label_tree(
                ui,
                &node.children,
                selected_folder,
                collapsed,
                label_expanded,
                indent_level.saturating_add(1),
                &node_key,
                clicked_folder,
                toggle_label_path,
            );
        }
    }
}
