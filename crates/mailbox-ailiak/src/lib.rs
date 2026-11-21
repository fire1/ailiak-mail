//! Thin bridge: selected mail → Ailiak Desk tasks via MCP over HTTPS.
//!
//! Mail stays in Mailbox; Ailiak stays the calm desk.

mod client;
mod compose;
mod map;

pub use client::{AiliakClient, AiliakError, CreatedTask, DeskTask, WhoAmI};
pub use compose::{
    compose_hint, parse_compose_commands, parse_for_date_arg, shift_day, today_key, ForDate,
    ParsedCompose,
};
pub use map::{
    build_payload, map_message_to_task, short_task_number, todo_defaults, CreateTaskPayload,
};
