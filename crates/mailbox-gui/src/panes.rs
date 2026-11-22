//! Main chrome panes: top bar, folders, list, reader, status, shell layout.

use egui::{
    Align, Color32, CursorIcon, Id, Layout, Order, Rect, RichText, ScrollArea, Sense, Ui,
    UiBuilder,
};

use crate::app::MailboxApp;
use crate::avatars;
use crate::calendar;
use crate::icons::{self, FolderIcon};
use crate::layout;
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
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab("https://ailiak.com/desk"));
                }
            });
        });

        // Right: Sync / Calendar
        let right = Rect::from_min_max(
            egui::pos2(bar.right() - (bar.width() * 0.28).max(140.0), bar.top()),
            bar.right_bottom(),
        );
        ui.allocate_new_ui(UiBuilder::new().max_rect(right), |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::toolbar_button(
                    ui,
                    icons::ToolbarIcon::Calendar,
                    "Calendar",
                    "Ailiak Desk calendar (c)",
                )
                .clicked()
                {
                    self.show_calendar = !self.show_calendar;
                    if self.show_calendar {
                        self.refresh_desk_tasks();
                    }
                }
                if icons::toolbar_button(
                    ui,
                    icons::ToolbarIcon::Sync,
                    "Sync",
                    "Fetch folders + messages for the selected folder (g)",
                )
                .clicked()
                {
                    self.start_sync();
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
        let mut open_todo = false;
        let mut open_settings = false;
        let mut open_help = false;
        let mut toggle_collapse = false;

        ui.set_width(ui.available_width());
        // Tighter vertical rhythm than the global style (folders felt too airy).
        ui.spacing_mut().item_spacing.y = 2.0;

        // Top-down: scrollable accounts/folders, then a fixed footer.
        // 3 rows × 28 + separator + bottom breathing room.
        let row_h = 28.0;
        let footer_h = row_h * 3.0 + 8.0 + 8.0 + 18.0;
        let body_h = (ui.available_height() - footer_h).max(48.0);

        ScrollArea::vertical()
            .id_salt("nav_scroll_v5")
            .max_height(body_h)
            .auto_shrink([false, false])
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

                for (id, name, email) in &accounts {
                    let selected = id == &selected_id;
                    let tip = if name.is_empty() {
                        email.clone()
                    } else {
                        format!("{name}\n{email}")
                    };
                    let view = self.resolve_avatar(ui.ctx(), email, name);
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - avatar_size).max(0.0) * 0.5);
                        if avatars::avatar_button(ui, &view, avatar_size, selected, &tip).clicked()
                        {
                            switch_to = Some(id.clone());
                        }
                    });
                    ui.add_space(4.0);
                }
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - avatar_size).max(0.0) * 0.5);
                    if icons::plus_account_button(ui, avatar_size).clicked() {
                        add_account = true;
                    }
                });

                ui.add_space(8.0);
                if !collapsed {
                    ui.label(
                        RichText::new("Folders")
                            .small()
                            .family(theme::nav_family())
                            .color(mute),
                    );
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

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        if icons::todo_nav_button(ui, todo_enabled, collapsed).clicked() {
            open_todo = true;
        }
        if icons::settings_nav_button(ui, collapsed).clicked() {
            open_settings = true;
        }
        if icons::help_nav_button(ui, collapsed).clicked() {
            open_help = true;
        }
        ui.add_space(16.0);

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
            self.selected_message = None;
            self.open_message = None;
            if let Err(e) = self.refresh_envelopes() {
                self.status = e.to_string();
            } else if let Some(first) = self.envelopes.first().map(|e| e.id.clone()) {
                let _ = self.open_id(&first);
            }
        }
        if open_todo {
            self.toggle_todo();
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
        ui.label(
            RichText::new(&folder_title)
                .strong()
                .size(14.0)
                .family(theme::nav_family())
                .color(theme::ink(ui.ctx())),
        );
        ui.add_space(8.0);

        let envelopes = self.envelopes.clone();
        let selected = self.selected_message.clone();
        let mut open: Option<MessageId> = None;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(col_w);
                if envelopes.is_empty() {
                    ui.label(RichText::new("No messages").color(theme::mute(ui.ctx())));
                    return;
                }

                let mut last_group: Option<String> = None;
                for env in &envelopes {
                    let group = date_group_label(env.date);
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

                    let is_sel = selected.as_ref() == Some(&env.id);
                    let from = if env.from_name.is_empty() {
                        env.from_addr.clone()
                    } else {
                        env.from_name.clone()
                    };
                    let date = env.date.format("%b %d").to_string();
                    let name_style = if env.seen {
                        RichText::new(&from).size(12.0)
                    } else {
                        RichText::new(&from)
                            .strong()
                            .size(12.0)
                            .color(theme::ink(ui.ctx()))
                    };
                    let subject = if env.seen {
                        RichText::new(&env.subject)
                            .small()
                            .color(theme::mute(ui.ctx()))
                    } else {
                        RichText::new(&env.subject)
                            .small()
                            .color(theme::unread(ui.ctx()))
                    };

                    let avatar = self.resolve_avatar(ui.ctx(), &env.from_addr, &env.from_name);

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
                                avatars::avatar_widget(ui, &avatar, 26.0);
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
                                            RichText::new(&env.snippet)
                                                .small()
                                                .color(theme::mute(ui.ctx())),
                                        )
                                        .truncate(),
                                    );
                                });
                            });
                        });

                    if row.response.interact(Sense::click()).clicked() {
                        open = Some(env.id.clone());
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
        ui.add_space(12.0);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.style_mut().spacing.item_spacing.y = 8.0;
                ui.add(
                    egui::Label::new(
                        RichText::new(&msg.body_text)
                            .size(13.0)
                            .line_height(Some(19.0))
                            .color(theme::ink(ui.ctx())),
                    )
                    .wrap()
                    .selectable(true),
                );
            });
    }

    /// Todo → Desk compose (right side panel, same chrome as calendar).
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
                self.show_calendar = true;
                self.refresh_desk_tasks();
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
            ui.label(
                RichText::new(format!(
                    "{} · {} · {}",
                    account_label, self.selected_folder, self.status
                ))
                .small()
                .color(theme::mute(ui.ctx())),
            );
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
            let mut side = egui::SidePanel::left("nav_v4")
                .resizable(!collapsed)
                .show_separator_line(false)
                .frame(layout::panel_frame(egui::Margin::symmetric(8.0, 8.0), panel));
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
                    egui::Stroke::new(1.0, theme::mute(ui.ctx()).linear_multiply(0.45)),
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

        // Far-right: calendar (when open). Inward: Todo compose panel.
        if self.show_calendar {
            egui::SidePanel::right("calendar_v2")
                .resizable(true)
                .default_width(240.0)
                .width_range(210.0..=320.0)
                .show_separator_line(false)
                .frame(layout::panel_frame(egui::Margin::symmetric(12.0, 10.0), panel))
                .show(ctx, |ui| {
                    layout::begin_panel(ui, panel);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Desk calendar")
                                .strong()
                                .color(theme::accent(ui.ctx())),
                        );
                        ui.add_space(ui.available_width() - 28.0);
                        if ui.small_button("↻").on_hover_text("Refresh Desk").clicked() {
                            self.refresh_desk_tasks();
                        }
                    });
                    ui.add_space(6.0);
                    let _ = calendar::show_month(ui, &mut self.calendar);
                    layout::end_panel(ui);
                });
        }

        if self.show_todo {
            egui::SidePanel::right("todo_v1")
                .resizable(true)
                .default_width(380.0)
                .width_range(320.0..=520.0)
                .show_separator_line(false)
                .frame(layout::panel_frame(egui::Margin::symmetric(14.0, 12.0), panel))
                .show(ctx, |ui| {
                    layout::begin_panel(ui, panel);
                    self.todo_pane(ui);
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
