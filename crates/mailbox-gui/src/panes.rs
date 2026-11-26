//! Main chrome panes: top bar, folders, list, reader, status, shell layout.

use egui::{
    Align, Color32, CursorIcon, Id, Layout, Order, Rect, RichText, ScrollArea, Sense, Ui,
    UiBuilder,
};

use crate::app::MailboxApp;
use crate::avatars;
use crate::brand;
use crate::brand_logos;
use crate::calendar;
use crate::icons::{self, FolderIcon};
use crate::layout;
use crate::staks;
use crate::theme;
use mailbox_core::types::{date_group_label, list_date_label, Message, MessageId};

/// Plain body for list/fallback UI — never confuse empty HTML with the old stub.
fn readable_message_plain(msg: &Message) -> String {
    let plain = msg.body_text.trim();
    let stub = mailbox_core::missing_plain_placeholder();
    if !plain.is_empty() && plain != stub {
        return msg.body_text.clone();
    }
    if !msg.body_html.trim().is_empty() {
        let derived = mailbox_core::html_to_plain(&msg.body_html);
        if !derived.is_empty() {
            return derived;
        }
        return stub.to_string();
    }
    if plain.is_empty() {
        "(no plain-text body)".into()
    } else {
        msg.body_text.clone()
    }
}

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
                // Accent mark (SVG) + crisp PNG wordmark — sharper than full-SVG text.
                let accent = theme::resolve_accent(&self.config.ui.accent).color;
                let dark = ui.visuals().dark_mode;
                self.ailiak_logo.sync(accent, dark);
                let h = brand::DISPLAY_HEIGHT.min((row_h - 4.0).max(22.0));
                let mark = h;
                let wm_h = h * 0.78;
                let wm_w = wm_h * self.ailiak_logo.wordmark_aspect();
                ui.spacing_mut().item_spacing.x = 7.0;
                let mark_resp = ui
                    .add(
                        egui::Image::new((self.ailiak_logo.mark().id(), egui::vec2(mark, mark)))
                            .sense(Sense::click()),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Open Ailiak Desk");
                let wm_resp = ui
                    .add(
                        egui::Image::new((
                            self.ailiak_logo.wordmark().id(),
                            egui::vec2(wm_w, wm_h),
                        ))
                        .sense(Sense::click()),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Open Ailiak Desk");
                if mark_resp.clicked() || wm_resp.clicked() {
                    self.open_desk();
                }
            });
        });

        // Right (L→R): Done, Trash, Reply, Reply all, Load, Sync
        // Painted right-to-left so Sync stays on the far right.
        // Archive icon omitted — Done (✓) archives (same action).
        let right = Rect::from_min_max(
            egui::pos2(bar.right() - (bar.width() * 0.34).max(320.0), bar.top()),
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
                let has_msg = self.open_message.is_some();
                let images_on = self.load_remote_images;
                let has_remote = self
                    .cached_mail_body()
                    .is_some_and(|b| b.has_remote_images);
                // One-shot Load: only while blocked remote images exist; gone once loaded.
                if has_msg && !images_on && has_remote {
                    let from = self
                        .open_message
                        .as_ref()
                        .map(|m| m.envelope.from_addr.clone())
                        .unwrap_or_default();
                    let domain = mailbox_core::remote_image_domain(&from);
                    let scope = if domain.is_empty() {
                        from.clone()
                    } else {
                        format!("*.{domain}")
                    };
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
                    let images_tip = format!(
                        "Load remote images for {scope}\nAllowed for this Stak domain; tracking-like stay blocked{threat_note}"
                    );
                    if icons::toolbar_load_images_button(ui, true, &images_tip).clicked() {
                        self.allow_remote_images_for_sender();
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
                // Snooze / Archive toolbar icons hidden — Done (✓) archives.
                let trash_tip = if self.selected_person.is_some() {
                    "Trash this People conversation (#)"
                } else {
                    "Trash current message (#)"
                };
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Trash,
                    trash_tip,
                    has_msg && !self.busy.moving(),
                    self.busy.trash,
                )
                .clicked()
                {
                    self.move_current_message(mailbox_core::SpecialFolderKind::Trash);
                }
                let done_tip = if self.selected_person.is_some() {
                    "Done — archive this People conversation (e)"
                } else {
                    "Done — archive current message (e)"
                };
                if icons::toolbar_icon_button(
                    ui,
                    icons::ToolbarIcon::Done,
                    done_tip,
                    has_msg && !self.busy.moving(),
                    self.busy.archive,
                )
                .clicked()
                {
                    // Same as rusty-inbox Done: leave the inbox via Archive.
                    self.move_current_message(mailbox_core::SpecialFolderKind::Archive);
                }
            });
        });

        // Center: compose + search (same row height).
        let compose_gap = 8.0;
        let row_h = 28.0;
        // Wider pad on Compose ≈ 110px; keep room next to search.
        let compose_w = 112.0;
        let search_w = (bar.width() * 0.36).clamp(200.0, 360.0);
        let center_w = compose_w + compose_gap + search_w;
        let center_rect = Rect::from_center_size(
            egui::pos2(bar.center().x, bar.center().y),
            egui::vec2(center_w, row_h),
        );
        ui.allocate_new_ui(UiBuilder::new().max_rect(center_rect), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = compose_gap;
                ui.set_min_height(row_h);
                if icons::toolbar_button_emphasized(
                    ui,
                    icons::ToolbarIcon::Compose,
                    "Compose",
                    "Compose new message",
                    self.busy.send,
                    row_h,
                )
                .clicked()
                {
                    self.open_compose_new();
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(search_w, row_h),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_width(search_w);
                        self.search_field(ui);
                    },
                );
            });
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
        let mut clicked_person: Option<String> = None;
        let mut open_folders_settings = false;
        let mut sync_folders: Option<bool> = None; // Some(true) = all IMAP folders
        let mut open_todo = false;
        let mut open_calendar = false;
        let mut open_settings = false;
        let mut open_feedback = false;
        let mut open_help = false;
        let mut toggle_collapse = false;
        let mut toggle_label_path: Option<String> = None;

        ui.set_width(ui.available_width());
        // Tighter vertical rhythm than the global style (folders felt too airy).
        ui.spacing_mut().item_spacing.y = 2.0;

        // Top-down: scrollable accounts/folders, then a fixed tools strip (paper fill).
        // Rows: Todo, Calendar, Settings, [Feedback], Help.
        // Height = rows + item gaps + band pad (8+16) + outer bottom (8).
        let row_h = 28.0;
        let tool_rows = if crate::app::FEEDBACK_UI_ENABLED {
            5.0
        } else {
            4.0
        };
        let footer_h = row_h * tool_rows + (tool_rows - 1.0) * 2.0 + 8.0 + 16.0 + 8.0;
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
                // “+” only while this band is hovered (one-frame lag via temp memory).
                let accounts_hover_id = egui::Id::new("nav_accounts_band_hover");
                let accounts_hovered = ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(accounts_hover_id))
                    .unwrap_or(false);
                let accounts_band = layout::nav_paper_band(
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

                        let gap = 6.0;
                        let show_plus = accounts_hovered;
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
                            if show_plus {
                                ui.horizontal(|ui| {
                                    ui.add_space(
                                        (ui.available_width() - avatar_size).max(0.0) * 0.5,
                                    );
                                    if icons::plus_account_button(ui, avatar_size).clicked() {
                                        add_account = true;
                                    }
                                });
                            }
                        } else {
                            // Wide panel: centered avatar row; “+” only while hovered.
                            let n = accounts.len() + usize::from(show_plus);
                            let row_w = if n == 0 {
                                0.0
                            } else {
                                n as f32 * avatar_size
                                    + (n.saturating_sub(1) as f32) * gap
                            };
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                ui.add_space((ui.available_width() - row_w).max(0.0) * 0.5);
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
                                if show_plus
                                    && icons::plus_account_button(ui, avatar_size).clicked()
                                {
                                    add_account = true;
                                }
                            });
                        }
                    });
                ui.ctx().data_mut(|d| {
                    d.insert_temp(accounts_hover_id, accounts_band.response.hovered());
                });

                ui.add_space(8.0);

                // Folders → Staks → People → Labels.
                // Staks + People can both stay open; opening either closes
                // Folders/Labels (and vice versa).
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
                let has_staks = !self.staks_cache.is_empty();
                let has_people = !self.people_cache.is_empty();
                let has_labels = !label_tree.is_empty();
                let selected_stak = self.selected_stak.clone();
                let selected_person = self.selected_person.clone();

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
                        egui_phosphor::regular::FOLDER,
                        open,
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
                    ui.add_space(8.0);
                } else {
                    ui.add_space(4.0);
                }

                if has_staks {
                    let open = self.staks_section_open;
                    let header = nav_accordion_header(
                        ui,
                        "Staks",
                        egui_phosphor::regular::STACK,
                        open,
                        mute,
                        collapsed,
                        None,
                    );
                    if header.toggle {
                        if open {
                            self.staks_section_open = false;
                        } else {
                            self.staks_section_open = true;
                            self.folders_section_open = false;
                            self.labels_section_open = false;
                        }
                    }
                    ui.add_space(2.0);
                    if self.staks_section_open {
                        let staks: Vec<_> = self.staks_cache.clone();
                        for stak in &staks {
                            let selected = selected_stak.as_deref() == Some(stak.id.as_str());
                            let brand = self.brand_texture(ui.ctx(), &stak.id);
                            if icons::stak_nav_button(
                                ui,
                                &stak.label,
                                0,
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
                    } else {
                        ui.add_space(4.0);
                    }
                }

                if has_people {
                    let open = self.people_section_open;
                    let header = nav_accordion_header(
                        ui,
                        "People",
                        egui_phosphor::regular::USER,
                        open,
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

                if has_labels {
                    let open = self.labels_section_open;
                    let header = nav_accordion_header(
                        ui,
                        "Labels",
                        egui_phosphor::regular::TAG,
                        open,
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
            if icons::todo_nav_button(ui, todo_enabled, collapsed).clicked() {
                open_todo = true;
            }
            if icons::calendar_nav_button(ui, collapsed).clicked() {
                open_calendar = true;
            }
            if icons::settings_nav_button(ui, collapsed).clicked() {
                open_settings = true;
            }
            if crate::app::FEEDBACK_UI_ENABLED
                && icons::feedback_nav_button(ui, collapsed).clicked()
            {
                open_feedback = true;
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
        if let Some(stak_id) = clicked_stak {
            self.clear_list_multi_select();
            self.selected_person = None;
            self.chat_messages.clear();
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
        if let Some(person) = clicked_person {
            self.clear_list_multi_select();
            self.selected_stak = None;
            self.open_person_chat(&person);
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
        if open_feedback {
            self.open_compose_feedback();
        }
        if open_help {
            self.show_help = !self.show_help;
            if self.show_help {
                self.show_feedback = false;
            }
        }
    }

    /// Folder title + mark-read / archive / trash — sits above the mid list card.
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
        let title = if multi {
            if selected_n == 0 {
                format!("{list_title} · select")
            } else {
                format!("{list_title} · {selected_n} selected")
            }
        } else {
            list_title
        };
        let move_enabled = if multi {
            selected_n > 0 && !self.busy.moving()
        } else {
            list_count > 0 && !self.busy.moving()
        };
        let read_enabled = if multi {
            selected_n > 0
        } else {
            list_count > 0
        };
        let trash_tip = if multi {
            if selected_n > 1 {
                "Trash selected messages (asks first)"
            } else {
                "Trash selected message"
            }
        } else {
            "Trash all messages in this folder/label (asks first)"
        };
        let read_tip = if multi {
            if selected_n > 1 {
                "Mark selected messages as read (asks first)"
            } else {
                "Mark selected message as read"
            }
        } else {
            "Mark all unread in this folder/label as read (asks first)"
        };
        // Title left, actions flush right — allocate remaining width so RTL
        // doesn't pack the buttons beside the title.
        // Visual L→R: Mark read, Trash (Archive removed — use top-bar Done / `e`).
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(title)
                    .strong()
                    .size(14.0)
                    .family(theme::nav_family())
                    .color(theme::ink(ui.ctx())),
            );
            ui.allocate_ui_with_layout(
                ui.available_size(),
                Layout::right_to_left(Align::Center),
                |ui| {
                    if icons::toolbar_icon_button(
                        ui,
                        icons::ToolbarIcon::Trash,
                        trash_tip,
                        move_enabled,
                        self.busy.trash,
                    )
                    .clicked()
                    {
                        self.move_listed_messages(mailbox_core::SpecialFolderKind::Trash);
                    }
                    if icons::toolbar_icon_button(
                        ui,
                        icons::ToolbarIcon::MarkRead,
                        read_tip,
                        read_enabled,
                        false,
                    )
                    .clicked()
                    {
                        self.mark_listed_messages_read();
                    }
                },
            );
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
        let mut toggle_star: Option<MessageId> = None;

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
                    let (
                        env_id,
                        from_name,
                        from_addr,
                        subject_s,
                        snippet,
                        seen,
                        flagged,
                        labels,
                        date_ts,
                    ) = {
                        let env = &self.envelopes[idx];
                        (
                            env.id.clone(),
                            env.from_name.clone(),
                            env.from_addr.clone(),
                            env.subject.clone(),
                            env.snippet.clone(),
                            env.seen,
                            env.flagged,
                            env.labels.clone(),
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
                    let date = list_date_label(date_ts);
                    // Unread rows use a white fill — force dark text so dark theme stays readable.
                    let name_style = if seen {
                        RichText::new(from).size(12.0)
                    } else {
                        RichText::new(from)
                            .strong()
                            .size(12.0)
                            .color(theme::unread_row_ink())
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
                    let date_color = if seen {
                        theme::mute(ui.ctx())
                    } else {
                        theme::unread_row_mute()
                    };

                    // Personal hosts (gmail.com, outlook.com, …) → profile photo / initials.
                    // Brand marks are for company senders (stripe.com, noreply@google.com).
                    let host = from_addr
                        .split('@')
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_lowercase();
                    let avatar = if mailbox_core::is_personal_mailbox_domain(&host) {
                        self.resolve_avatar(ui.ctx(), &from_addr, &from_name)
                    } else if let Some(brand_id) = staks::brand_id_for_addr(&from_addr) {
                        self.brand_texture(ui.ctx(), brand_id)
                            .map(crate::avatars::AvatarView::Image)
                            .unwrap_or_else(|| {
                                self.resolve_avatar(ui.ctx(), &from_addr, &from_name)
                            })
                    } else {
                        // Unknown sender domain — still try Logo.dev on the host.
                        let domain = mailbox_core::remote_image_domain(&from_addr);
                        let logo_key = if !domain.is_empty() {
                            domain
                        } else {
                            host
                        };
                        if !logo_key.is_empty() {
                            if let Some(tex) = self.brand_texture(ui.ctx(), &logo_key) {
                                crate::avatars::AvatarView::Image(tex)
                            } else {
                                self.resolve_avatar(ui.ctx(), &from_addr, &from_name)
                            }
                        } else {
                            self.resolve_avatar(ui.ctx(), &from_addr, &from_name)
                        }
                    };

                    let row_fill = if multi && is_checked {
                        theme::select(ui.ctx())
                    } else if is_sel && !multi {
                        theme::select(ui.ctx())
                    } else if !seen {
                        theme::unread_row(ui.ctx())
                    } else {
                        Color32::TRANSPARENT
                    };

                    let mut avatar_clicked = false;
                    let mut star_clicked = false;
                    let mut av_rect = egui::Rect::NOTHING;
                    let mut star_rect = egui::Rect::NOTHING;
                    let row = egui::Frame::none()
                        .fill(row_fill)
                        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                        .rounding(8.0)
                        .show(ui, |ui| {
                            // available_width is already inside this Frame's margins —
                            // do not use outer `col_w` or rows overflow and egui shifts
                            // the floating scrollbar inward (gap before the panel edge).
                            ui.set_width(ui.available_width());
                            // Top-align avatar with the sender title (not centered on the
                            // whole name+subject+snippet block).
                            ui.horizontal_top(|ui| {
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
                                            let r = (size * 0.22).clamp(4.0, 8.0);
                                            let plate = if ui.visuals().dark_mode {
                                                Color32::from_rgba_unmultiplied(255, 255, 255, 18)
                                            } else {
                                                Color32::from_rgba_unmultiplied(255, 255, 255, 230)
                                            };
                                            ui.painter().rect_filled(av_rect, r, plate);
                                            egui::Image::new((tex.id(), egui::vec2(size, size)))
                                                .maintain_aspect_ratio(true)
                                                .fit_to_exact_size(egui::vec2(size, size))
                                                .rounding(egui::Rounding::same(r))
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
                                let text_w = ui.available_width();
                                ui.allocate_ui_with_layout(
                                    egui::vec2(text_w, 0.0),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.set_width(text_w);
                                        // Name left (truncate), date natural width on the right.
                                        // Avoid RTL+truncate — it right-aligns the name text.
                                        ui.horizontal(|ui| {
                                            let star_w = if flagged { 14.0 } else { 0.0 };
                                            let date_w = 52.0_f32 + star_w;
                                            let name_w = (ui.available_width() - date_w).max(40.0);
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(name_w, ui.spacing().interact_size.y),
                                                Layout::left_to_right(Align::Center),
                                                |ui| {
                                                    ui.add(
                                                        egui::Label::new(name_style)
                                                            .truncate()
                                                            .sense(label_sense),
                                                    );
                                                },
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(&date)
                                                                .small()
                                                                .color(date_color),
                                                        )
                                                        .sense(label_sense),
                                                    );
                                                    if flagged {
                                                        ui.add_space(2.0);
                                                        let (rect, star_resp) = ui
                                                            .allocate_exact_size(
                                                                egui::vec2(14.0, 14.0),
                                                                Sense::click(),
                                                            );
                                                        star_rect = rect;
                                                        icons::paint_star(
                                                            ui,
                                                            star_rect,
                                                            theme::accent(ui.ctx()),
                                                        );
                                                        if star_resp
                                                            .on_hover_text("Unstar")
                                                            .clicked()
                                                        {
                                                            star_clicked = true;
                                                        }
                                                    }
                                                },
                                            );
                                        });
                                        ui.add(
                                            egui::Label::new(subject)
                                                .truncate()
                                                .sense(label_sense),
                                        );
                                        if !labels.is_empty() {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                let chip_fg = if seen {
                                                    theme::mute(ui.ctx())
                                                } else {
                                                    theme::unread_row_mute()
                                                };
                                                let chip_bg = theme::chip(ui.ctx());
                                                for lab in labels.iter().take(4) {
                                                    egui::Frame::none()
                                                        .fill(chip_bg)
                                                        .rounding(4.0)
                                                        .inner_margin(egui::Margin::symmetric(
                                                            5.0, 1.0,
                                                        ))
                                                        .show(ui, |ui| {
                                                            ui.add(
                                                                egui::Label::new(
                                                                    RichText::new(lab)
                                                                        .size(10.0)
                                                                        .color(chip_fg),
                                                                )
                                                                .sense(label_sense),
                                                            );
                                                        });
                                                }
                                                if labels.len() > 4 {
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(format!(
                                                                "+{}",
                                                                labels.len() - 4
                                                            ))
                                                            .size(10.0)
                                                            .color(chip_fg),
                                                        )
                                                        .sense(label_sense),
                                                    );
                                                }
                                            });
                                        }
                                        let snippet_color = if seen {
                                            theme::mute(ui.ctx())
                                        } else {
                                            theme::unread_row_mute()
                                        };
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&snippet)
                                                    .small()
                                                    .color(snippet_color),
                                            )
                                            .truncate()
                                            .sense(label_sense),
                                        );
                                    },
                                );
                            });
                        });

                    // Whole-row click (minus avatar / star) opens the message. Using Frame
                    // interact avoids egui 0.29 missing clicks on vertical().response.
                    let row_click = row.response.interact(Sense::click());
                    let body_clicked = !avatar_clicked
                        && !star_clicked
                        && row_click.clicked()
                        && row_click.interact_pointer_pos().is_some_and(|p| {
                            !av_rect.contains(p) && !star_rect.contains(p)
                        });

                    // Avatar = multi-select; star = unstar; row body opens the message.
                    if star_clicked {
                        toggle_star = Some(env_id);
                    } else if avatar_clicked {
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
        if let Some(id) = toggle_star {
            self.toggle_starred(&id);
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
            self.paint_reader_subject(ui, &msg.envelope.subject, 15.0);
            ui.label(
                RichText::new(msg.display_from())
                    .small()
                    .color(theme::reader_mute()),
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
                        ui.label(readable_message_plain(msg));
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
                self.apply_compose_webview_result(&draft, &res, ui.ctx());
                let hotkey_send = self.compose_webview.take_send_hotkey();
                if res.send || hotkey_send {
                    self.compose = Some(draft);
                    self.submit_compose();
                } else if res.open_full {
                    draft.inline = false;
                    self.compose = Some(draft);
                } else if res.close {
                    self.compose_webview.hide();
                } else {
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
                    .color(theme::reader_ink()),
            );
            ui.label(RichText::new(&peer).small().color(theme::reader_mute()));
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
                            .color(theme::reader_mute()),
                        );
                        return;
                    }
                    for msg in &self.chat_messages {
                        let mine = msg.envelope.from_addr.trim().eq_ignore_ascii_case(&me);
                        // Light-surface bubbles on the white reader card.
                        let bubble_fill = if mine {
                            theme::reader_chip()
                        } else {
                            Color32::from_rgb(0xf0, 0xf1, 0xf2)
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
                                        .color(theme::reader_mute()),
                                    );
                                    if !msg.envelope.subject.is_empty() {
                                        ui.label(
                                            RichText::new(&msg.envelope.subject)
                                                .small()
                                                .strong(),
                                        );
                                    }
                                    ui.label(
                                        RichText::new(readable_message_plain(msg).trim())
                                            .size(13.0),
                                    );
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
            let plain = std::mem::take(&mut self.chat_reply);
            let atts = std::mem::take(&mut self.chat_attachments);
            if let Some(msg) = self.chat_messages.last().cloned() {
                self.open_message = Some(msg);
                self.open_reply(false);
                if let Some(draft) = self.compose.as_mut() {
                    draft.inline = false;
                    draft.set_plain_body(plain);
                    draft.attachments = atts;
                }
            } else {
                let mut draft = crate::compose::ComposeDraft::new_blank();
                draft.to = vec![peer];
                draft.from_email = from_email;
                draft.set_plain_body(plain);
                draft.attachments = atts;
                self.compose = Some(draft);
            }
        }
    }

    /// Subject line: collapse long titles; click to expand / collapse.
    fn paint_reader_subject(&mut self, ui: &mut Ui, subject: &str, size: f32) {
        let subject = subject.trim();
        if subject.is_empty() {
            ui.label(
                RichText::new("(no subject)")
                    .size(size)
                    .italics()
                    .color(theme::reader_mute()),
            );
            return;
        }

        const COLLAPSE_CHARS: usize = 64;
        let char_n = subject.chars().count();
        let can_collapse = char_n > COLLAPSE_CHARS;
        let expanded = self.reader_subject_expanded || !can_collapse;

        let (shown, tip) = if expanded {
            (
                subject.to_string(),
                if can_collapse {
                    "Click to collapse subject"
                } else {
                    ""
                },
            )
        } else {
            let head: String = subject
                .chars()
                .take(COLLAPSE_CHARS.saturating_sub(1))
                .collect();
            (format!("{head}…"), "Click to show full subject")
        };

        let label = RichText::new(&shown)
            .size(size)
            .strong()
            .color(theme::reader_ink());
        let resp = if expanded {
            ui.add(egui::Label::new(label).wrap().sense(Sense::click()))
        } else {
            ui.add(
                egui::Label::new(label)
                    .truncate()
                    .sense(Sense::click()),
            )
        };
        let resp = if tip.is_empty() {
            resp
        } else {
            resp.on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(tip)
        };
        if can_collapse && resp.clicked() {
            self.reader_subject_expanded = !self.reader_subject_expanded;
        }
    }

    /// Compact From line; expand for To / Cc / Date.
    /// Privacy / tracking note sits on the right of the collapsed From row.
    fn paint_reader_from_header(
        &mut self,
        ui: &mut Ui,
        msg: &mailbox_core::types::Message,
    ) {
        use egui_phosphor::regular as ph;

        let mute = theme::reader_mute();
        let expanded = self.reader_headers_expanded;
        let caret = if expanded {
            ph::CARET_DOWN
        } else {
            ph::CARET_RIGHT
        };
        let tip = if expanded {
            "Hide To, Cc, Date"
        } else {
            "Show To, Cc, Date"
        };
        let threat_n = self
            .cached_mail_body()
            .map(|b| b.image_threats.len())
            .unwrap_or(0);
        let privacy_note = if threat_n > 0 {
            Some(format!(
                "{threat_n} tracking-like image(s) blocked for privacy"
            ))
        } else {
            None
        };

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let caret_resp = ui
                .add(
                    egui::Button::new(
                        RichText::new(caret)
                            .family(theme::icon_family())
                            .size(12.0)
                            .color(mute),
                    )
                    .frame(false),
                )
                .on_hover_text(tip);
            let from_resp = ui
                .add(
                    egui::Button::new(
                        RichText::new(msg.display_from())
                            .size(13.0)
                            .color(mute),
                    )
                    .frame(false),
                )
                .on_hover_text(tip);
            if caret_resp.clicked() || from_resp.clicked() {
                self.reader_headers_expanded = !expanded;
            }

            if !expanded {
                if let Some(note) = privacy_note.as_ref() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(note).small().color(mute));
                    });
                }
            }
        });

        if self.reader_headers_expanded {
            ui.add_space(4.0);
            let meta = |ui: &mut Ui, label: &str, value: &str| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(label)
                            .size(12.0)
                            .color(mute.linear_multiply(0.85)),
                    );
                    ui.label(RichText::new(value).size(12.0).color(mute));
                });
            };
            meta(ui, "From", &msg.display_from());
            if !msg.to.is_empty() {
                meta(ui, "To", &msg.to.join(", "));
            }
            if !msg.cc.is_empty() {
                meta(ui, "Cc", &msg.cc.join(", "));
            }
            meta(
                ui,
                "Date",
                &msg.envelope.date.format("%Y-%m-%d %H:%M UTC").to_string(),
            );
            if let Some(note) = privacy_note.as_ref() {
                ui.add_space(2.0);
                ui.label(RichText::new(note).small().color(mute));
            }
        }
    }

    /// New message / expanded reply — fills the mail body card.
    fn paint_compose_reader(&mut self, ui: &mut Ui) {
        if let Some(mut draft) = self.compose.take() {
            let hints = self.compose_hints.clone();
            let res = crate::compose::show_reader_compose(
                ui,
                &mut draft,
                self.busy.send,
                &hints,
                &mut self.compose_autofocus_to,
                &mut self.compose_autofocus_subject,
            );
            self.apply_compose_webview_result(&draft, &res, ui.ctx());
            let hotkey_send = self.compose_webview.take_send_hotkey();
            if res.send || hotkey_send {
                self.compose = Some(draft);
                self.submit_compose();
            } else if res.close {
                self.compose_webview.hide();
            } else {
                self.compose = Some(draft);
            }
        }
    }

    pub(crate) fn reader_pane(&mut self, ui: &mut Ui) {
        // Post-send confirmation replaces the compose form until dismissed.
        if let Some(notice) = self.compose_sent.clone() {
            match crate::compose::show_compose_sent(ui, &notice) {
                crate::compose::ComposeSentAction::WriteAnother => {
                    self.open_compose_new();
                }
                crate::compose::ComposeSentAction::Done => {
                    self.dismiss_compose_sent();
                }
                crate::compose::ComposeSentAction::None => {}
            }
            return;
        }

        // Full-card composer (new mail, or reply expanded from the dock).
        if self.compose.as_ref().is_some_and(|c| !c.inline) {
            self.paint_compose_reader(ui);
            return;
        }

        if self.selected_person.is_some() {
            self.chat_reader_pane(ui);
            return;
        }

        let Some(msg) = self.open_message.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Select a message").color(theme::reader_mute()));
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

        self.paint_reader_subject(ui, &msg.envelope.subject, 18.0);
        ui.add_space(6.0);
        self.paint_reader_from_header(ui, &msg);
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        let html = msg.body_html.trim();
        let use_webview = !html.is_empty();
        let load_images = self.load_remote_images;
        // Tracking note lives on the From row; only show the remote-images hint here.
        if use_webview && !load_images {
            let has_remote = self
                .cached_mail_body()
                .is_some_and(|b| b.has_remote_images);
            let has_threats = self
                .cached_mail_body()
                .is_some_and(|b| b.has_image_threats());
            if has_remote && !has_threats {
                ui.label(
                    RichText::new(
                        "Remote images blocked until you allow this domain (toolbar image icon).",
                    )
                    .small()
                    .color(theme::reader_mute()),
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
                    .color(theme::reader_mute()),
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
                // Theme-matched underlay (not pure black) while WebKit swaps messages.
                // Mail HTML itself still uses a light reading canvas inside the document.
                let dark = ui.visuals().dark_mode;
                ui.painter()
                    .rect_filled(rect, 0.0, theme::mail_webview_backdrop(dark));
                let loading = self.mail_webview.is_loading(&msg.envelope.id);
                if loading {
                    ui.put(
                        rect,
                        egui::Spinner::new().size(22.0).color(theme::reader_mute()),
                    );
                    ui.ctx().request_repaint();
                }
                // Root viewport points → wry child bounds (physical on Linux via ppp).
                self.mail_webview_req = Some(crate::mail_webview::MailWebViewRequest {
                    bounds: rect,
                    pixels_per_point: ui.ctx().pixels_per_point(),
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
            // Left rail stays the smallest column (icons / compact accordion).
            // Collapsed needs room for 28px icons + unread badge overlays + side inset.
            side = if collapsed {
                side.exact_width(72.0)
            } else {
                side.default_width(168.0).width_range(140.0..=200.0)
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

        // Mid column ~30% of window; reader (central) keeps ≥50%. Nav is smallest.
        // exact_width is owned by us — egui's SidePanel resize snaps content-driven widths.
        let win_w = ctx.screen_rect().width();
        let nav_w = if self.folders_collapsed { 72.0 } else { 176.0 };
        let desk_w = if self.show_todo {
            380.0
        } else if self.show_calendar {
            280.0
        } else {
            0.0
        };
        let reader_floor = (win_w * 0.50).max(320.0);
        let list_max = (win_w - nav_w - desk_w - reader_floor)
            .clamp(240.0, 520.0);
        let list_min = 240.0_f32.min(list_max);
        let list_range = list_min..=list_max;
        let target_list = (win_w * 0.30).clamp(*list_range.start(), *list_range.end());
        if self.last_shell_width <= 0.0 {
            self.list_width = target_list;
            self.last_shell_width = win_w;
        } else if (win_w - self.last_shell_width).abs() > 12.0 {
            // Keep the user's relative split when the window is resized.
            let scale = win_w / self.last_shell_width.max(1.0);
            self.list_width = (self.list_width * scale).clamp(*list_range.start(), *list_range.end());
            self.last_shell_width = win_w;
        }
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
            1.0_f32,
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
                    // Reader card is always white paper; force light widgets so
                    // TextEdit / separators stay dark-on-light in a dark shell.
                    if ui.visuals().dark_mode {
                        ui.set_style((*ui.ctx().style_of(egui::Theme::Light)).clone());
                    }
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

/// Collapsible section title (no unread badge — badges stay on inner rows only).
/// Idle: group icon. Hover: collapse/expand caret. Collapsed rail: icon-only.
fn nav_accordion_header(
    ui: &mut Ui,
    title: &str,
    group_icon: &'static str,
    open: bool,
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
            // Group icon at rest; caret reveals collapse state on hover.
            let glyph = if resp.hovered() { caret } else { group_icon };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                theme::icon_font(14.0),
                mute,
            );
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
    let leading = if row_hovered { caret } else { group_icon };

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // Fixed slot so swapping group icon ↔ caret does not shift the title.
        let icon_slot = egui::vec2(16.0, 16.0);
        let (icon_rect, icon_resp) = ui.allocate_exact_size(icon_slot, Sense::click());
        ui.painter().text(
            icon_rect.center(),
            egui::Align2::CENTER_CENTER,
            leading,
            theme::icon_font(13.0),
            mute,
        );
        let icon_resp = icon_resp.on_hover_text(&tip);
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
        if icon_resp.clicked() || title_resp.clicked() {
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
                                    RichText::new(egui_phosphor::regular::LIGHTNING)
                                        .family(theme::icon_family())
                                        .size(12.0)
                                        .color(theme::accent(ui.ctx())),
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
