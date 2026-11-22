//! `ailiakmail-mcp` — stdio MCP server for the local AiliakMail store.

use anyhow::Result;
use mailbox_mcp::{MailboxMcp, MailboxService};
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Never log to stdout — it is the MCP JSON-RPC channel.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    let service = MailboxService::open_default()?;
    let server = MailboxMcp::new(service);
    let running = server.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}
