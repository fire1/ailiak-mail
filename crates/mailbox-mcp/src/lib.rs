//! AiliakMail MCP server — expose the local SQLite mail cache to agents.

pub mod server;
pub mod service;

pub use server::MailboxMcp;
pub use service::MailboxService;
