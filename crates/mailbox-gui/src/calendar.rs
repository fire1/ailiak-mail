//! Month calendar aligned with Ailiak Desk `forDate` (YYYY-MM-DD).

use chrono::{Datelike, Local, Months, NaiveDate};
use egui::{Color32, RichText, Sense, Ui, Vec2};
use mailbox_ailiak::DeskTask;

use crate::theme;

#[derive(Debug, Clone)]
pub struct CalendarState {
    pub month: NaiveDate,
    /// `None` = use “today” semantics for Todo; `Some("")` = unscheduled; `Some(YYYY-MM-DD)` = that day.
    pub selected: Option<String>,
    pub tasks: Vec<DeskTask>,
    pub loading: bool,
}

impl Default for CalendarState {
    fn default() -> Self {
        let today = Local::now().date_naive();
        Self {
            month: NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today),
            selected: Some(today.format("%Y-%m-%d").to_string()),
            tasks: Vec::new(),
            loading: false,
        }
    }
}

impl CalendarState {
    pub fn select_today(&mut self) {
        let today = Local::now().date_naive();
        self.selected = Some(today.format("%Y-%m-%d").to_string());
        self.month = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today);
    }

    pub fn select_tomorrow(&mut self) {
        let d = Local::now().date_naive() + chrono::Duration::days(1);
        self.selected = Some(d.format("%Y-%m-%d").to_string());
        self.month = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d);
    }

    pub fn clear_day(&mut self) {
        self.selected = Some(String::new());
    }

    /// Selected calendar day (`None` = today semantics; `Some("")` = unscheduled).
    #[allow(dead_code)] // Kept for Calendar ↔ Desk day wiring; Options uses slash `/from`/`/date`.
    pub fn todo_for_date(&self) -> Option<String> {
        self.selected.clone()
    }
}

/// Draw month grid + day chips + tasks for selected day.
/// Returns a day key when the user picks a day (`""` = unscheduled).
pub fn show_month(ui: &mut Ui, state: &mut CalendarState) -> Option<String> {
    let mut clicked: Option<String> = None;
    let today = Local::now().date_naive();
    let today_key = today.format("%Y-%m-%d").to_string();

    ui.horizontal(|ui| {
        if ui.button("‹").on_hover_text("Previous month").clicked() {
            if let Some(m) = state.month.checked_sub_months(Months::new(1)) {
                state.month = m;
            }
        }
        ui.label(
            RichText::new(state.month.format("%B %Y").to_string())
                .strong()
                .color(theme::accent(ui.ctx())),
        );
        if ui.button("›").on_hover_text("Next month").clicked() {
            if let Some(m) = state.month.checked_add_months(Months::new(1)) {
                state.month = m;
            }
        }
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.small_button("Today").clicked() {
            state.select_today();
            clicked = state.selected.clone();
        }
        if ui.small_button("Tomorrow").clicked() {
            state.select_tomorrow();
            clicked = state.selected.clone();
        }
        if ui.small_button("Unscheduled").clicked() {
            state.clear_day();
            clicked = Some(String::new());
        }
    });
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        for name in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
            ui.allocate_ui(Vec2::new(28.0, 16.0), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new(name).small().color(theme::mute(ui.ctx())));
                });
            });
        }
    });

    let first = state.month;
    let pad = first.weekday().num_days_from_monday() as i32;
    let dim = days_in_month(first.year(), first.month());
    let cells = pad + dim;
    let rows = (cells + 6) / 7;

    for row in 0..rows {
        ui.horizontal(|ui| {
            for col in 0..7 {
                let idx = row * 7 + col;
                let day_num = idx - pad + 1;
                ui.allocate_ui(Vec2::new(28.0, 26.0), |ui| {
                    if day_num < 1 || day_num > dim {
                        return;
                    }
                    let date = NaiveDate::from_ymd_opt(first.year(), first.month(), day_num as u32)
                        .unwrap_or(first);
                    let key = date.format("%Y-%m-%d").to_string();
                    let is_today = key == today_key;
                    let is_sel = state.selected.as_deref() == Some(key.as_str());
                    let has_task = state
                        .tasks
                        .iter()
                        .any(|t| t.for_date.as_deref() == Some(key.as_str()));

                    let fill = if is_sel {
                        theme::select(ui.ctx())
                    } else {
                        Color32::TRANSPARENT
                    };
                    let color = if is_today {
                        theme::accent(ui.ctx())
                    } else {
                        ui.visuals().text_color()
                    };
                    let label = if has_task {
                        format!("{day_num}·")
                    } else {
                        day_num.to_string()
                    };
                    let text = if is_today || is_sel {
                        RichText::new(label).strong().color(color)
                    } else {
                        RichText::new(label).color(color)
                    };

                    let resp = egui::Frame::none()
                        .fill(fill)
                        .rounding(8.0)
                        .show(ui, |ui| {
                            ui.centered_and_justified(|ui| {
                                ui.label(text);
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if resp.clicked() {
                        state.selected = Some(key.clone());
                        clicked = Some(key);
                    }
                });
            }
        });
    }

    ui.add_space(8.0);
    ui.separator();
    let day_label = match state.selected.as_deref() {
        None => "Desk day: (default today)".to_string(),
        Some("") => "Desk day: unscheduled".to_string(),
        Some(d) => format!("Desk day: {d}"),
    };
    ui.label(
        RichText::new(day_label)
            .small()
            .color(theme::mute(ui.ctx())),
    );

    ui.add_space(4.0);
    ui.label(RichText::new("On Desk").small().strong());
    if state.loading {
        ui.label(
            RichText::new("Loading…")
                .small()
                .color(theme::mute(ui.ctx())),
        );
    } else {
        let sel = state.selected.clone();
        let day_tasks: Vec<_> = state
            .tasks
            .iter()
            .filter(|t| match (sel.as_deref(), t.for_date.as_deref()) {
                (Some(""), None) => true,
                (Some(s), Some(fd)) if !s.is_empty() => s == fd,
                (None, Some(fd)) => fd == today_key,
                _ => false,
            })
            .cloned()
            .collect();
        if day_tasks.is_empty() {
            ui.label(
                RichText::new("No open tasks this day")
                    .small()
                    .color(theme::mute(ui.ctx())),
            );
        } else {
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    for t in &day_tasks {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("·").color(theme::accent(ui.ctx())));
                            ui.label(RichText::new(&t.title).small());
                        });
                    }
                });
        }
    }

    clicked
}

fn days_in_month(year: i32, month: u32) -> i32 {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let next = first.checked_add_months(Months::new(1)).unwrap_or(first);
    (next - first).num_days() as i32
}
