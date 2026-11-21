//! Main chrome panes: top bar, folders, list, reader, status, shell layout.

use egui::{
    Align, Color32, CursorIcon, Id, Layout, Order, Rect, RichText, ScrollArea, Sense, Ui,
};

use crate::app::MailboxApp;
use crate::calendar;
use crate::icons::{self, FolderIcon};
use crate::layout;
use crate::theme;
use mailbox_core::types::MessageId;

impl MailboxApp {
    pub(crate) fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new((self.ailiak_mark.id(), egui::vec2(22.0, 22.0)))
                    .sense(Sense::hover()),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new("Mailbox")
                    .strong()
                    .size(18.0)
                    .color(theme::accent(ui.ctx())),
            );
            ui.add_space(10.0);
            ui.label(
                RichText::new("read locally · send work to Ailiak")
                    .small()
                    .color(theme::mute(ui.ctx())),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Help").clicked() {
                    self.show_help = !self.show_help;
                }
                if ui.button("Settings").clicked() {
                    self.show_settings = !self.show_settings;
                }
                if ui
                    .add_enabled(self.open_message.is_some(), egui::Button::new("Todo"))
                    .on_hover_text("Turn this message into an Ailiak Desk task (a)")
                    .clicked()
                {
                    self.open_todo();
                }
                if ui
                    .button("Calendar")
                    .on_hover_text("Ailiak Desk calendar (c)")
                    .clicked()
                {
                    self.show_calendar = !self.show_calendar;
                    if self.show_calendar {
                        self.refresh_desk_tasks();
                    }
                }
                if ui
                    .button("Sync")
                    .on_hover_text("Fetch folders + messages for the selected folder (g)")
                    .clicked()
                {
                    self.start_sync();
                }
            });
        });
    }

    pub(crate) fn folder_pane(&mut self, ui: &mut Ui) {
        let collapsed = self.folders_collapsed;
        let mute = theme::mute(ui.ctx());

        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            if collapsed {
                if icons::collapse_button(ui, true, mute).clicked() {
                    self.folders_collapsed = false;
                }
            } else {
                ui.label(RichText::new("Folders").small().color(mute));
                ui.add_space(ui.available_width() - 28.0);
                if icons::collapse_button(ui, false, mute).clicked() {
                    self.folders_collapsed = true;
                }
            }
        });
        ui.add_space(6.0);

        let folders = self.folders.clone();
        let mut clicked: Option<String> = None;
        for folder in &folders {
            let selected = folder.path == self.selected_folder;
            let label = if folder.unread > 0 {
                format!("{} ({})", folder.name, folder.unread)
            } else {
                folder.name.clone()
            };
            let kind = FolderIcon::from_folder(&folder.path, &folder.name);
            if icons::folder_nav_button(ui, kind, &label, selected, collapsed).clicked() {
                clicked = Some(folder.path.clone());
            }
            ui.add_space(2.0);
        }
        if let Some(path) = clicked {
            self.selected_folder = path;
            self.selected_message = None;
            self.open_message = None;
            if let Err(e) = self.refresh_envelopes() {
                self.status = e.to_string();
            } else if let Some(first) = self.envelopes.first().map(|e| e.id.clone()) {
                let _ = self.open_id(&first);
            }
        }
    }

    pub(crate) fn list_pane(&mut self, ui: &mut Ui) {
        let col_w = ui.available_width();
        // Cap content to the panel — never set_min_width larger than the
        // resized panel or egui will persist a wider PanelState and snap back.
        ui.set_max_width(col_w);

        ui.horizontal(|ui| {
            ui.label(RichText::new("Search").small().color(theme::mute(ui.ctx())));
            let search_w = (ui.available_width() - 8.0).max(80.0);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("subject, from, body…")
                    .desired_width(search_w)
                    .clip_text(true),
            );
            if resp.changed() {
                if let Err(e) = self.refresh_envelopes() {
                    self.status = e.to_string();
                }
            }
        });
        ui.add_space(6.0);

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
                for env in &envelopes {
                    let is_sel = selected.as_ref() == Some(&env.id);
                    let from = if env.from_name.is_empty() {
                        env.from_addr.clone()
                    } else {
                        env.from_name.clone()
                    };
                    let date = env.date.format("%b %d").to_string();
                    let subject = if env.seen {
                        RichText::new(&env.subject)
                    } else {
                        RichText::new(&env.subject)
                            .strong()
                            .color(theme::unread(ui.ctx()))
                    };

                    let row = egui::Frame::none()
                        .fill(if is_sel {
                            theme::select(ui.ctx())
                        } else {
                            Color32::TRANSPARENT
                        })
                        .inner_margin(egui::Margin::symmetric(10.0, 9.0))
                        .rounding(8.0)
                        .show(ui, |ui| {
                            ui.set_width(col_w - 4.0);
                            ui.horizontal(|ui| {
                                if !env.seen {
                                    ui.label(RichText::new("●").color(theme::accent(ui.ctx())));
                                } else {
                                    ui.add_space(12.0);
                                }
                                ui.add(egui::Label::new(subject).truncate());
                            });
                            // Inside horizontal so RTL can't stretch row height.
                            ui.horizontal(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(&date).small().color(theme::mute(ui.ctx())),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&from)
                                                .small()
                                                .color(theme::mute(ui.ctx())),
                                        )
                                        .truncate(),
                                    );
                                });
                            });
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&env.snippet)
                                        .small()
                                        .color(theme::mute(ui.ctx())),
                                )
                                .truncate(),
                            );
                        });

                    if row.response.interact(Sense::click()).clicked() {
                        open = Some(env.id.clone());
                    }
                    ui.add_space(3.0);
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

        ui.label(RichText::new(&msg.envelope.subject).heading());
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!("From  {}", msg.display_from())).color(theme::mute(ui.ctx())),
        );
        if !msg.to.is_empty() {
            ui.label(
                RichText::new(format!("To    {}", msg.to.join(", "))).color(theme::mute(ui.ctx())),
            );
        }
        ui.label(
            RichText::new(format!(
                "Date  {}",
                msg.envelope.date.format("%Y-%m-%d %H:%M UTC")
            ))
            .color(theme::mute(ui.ctx())),
        );
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.add(
                    egui::Label::new(RichText::new(&msg.body_text))
                        .wrap()
                        .selectable(true),
                );
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
                self.show_settings = true;
            }

            ui.separator();
            ui.label(
                RichText::new(format!(
                    "{} · {} · {}",
                    self.account_id, self.selected_folder, self.status
                ))
                .small()
                .color(theme::mute(ui.ctx())),
            );
        });
    }

    /// Draw the main three-column shell (folders · list · reader [+ calendar]).
    pub(crate) fn show_shell(&mut self, ctx: &egui::Context) {
        let panel = theme::panel(ctx);
        let paper = theme::paper(ctx);

        egui::TopBottomPanel::top("top")
            .exact_height(44.0)
            .frame(layout::panel_frame(egui::Margin::symmetric(10.0, 0.0), panel))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                self.top_bar(ui);
            });

        egui::TopBottomPanel::bottom("status")
            .exact_height(30.0)
            .frame(layout::panel_frame(egui::Margin::symmetric(10.0, 0.0), panel))
            .show_separator_line(false)
            .show(ctx, |ui| {
                self.status_bar(ui);
            });

        {
            let collapsed = self.folders_collapsed;
            // New ids reset any bad persisted panel widths from earlier builds.
            let mut side = egui::SidePanel::left("folders_v2")
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
                self.folder_pane(ui);
                layout::end_panel(ui);
            });
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

        egui::CentralPanel::default()
            .frame(layout::panel_frame(egui::Margin::same(16.0), paper))
            .show(ctx, |ui| {
                self.reader_pane(ui);
            });
    }
}
