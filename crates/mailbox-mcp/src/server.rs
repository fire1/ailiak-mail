//! rmcp tool surface over [`crate::service::MailboxService`].

use rmcp::{
    handler::server::wrapper::Parameters,
    model::*,
    schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::service::MailboxService;

fn tool_err(e: impl ToString) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

fn json_ok(value: impl serde::Serialize) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(&value).map_err(tool_err)?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AccountIdArgs {
    /// Account id from `list_accounts`. Omit to use the primary / first account.
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListMessagesArgs {
    #[serde(default)]
    pub account_id: Option<String>,
    /// Folder path (default `INBOX`).
    #[serde(default)]
    pub folder: Option<String>,
    /// Max envelopes to return (newest first).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    #[serde(default)]
    pub account_id: Option<String>,
    /// Substring match against subject, from, snippet, body.
    pub query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MessageIdArgs {
    /// Local message id (`account:folder:uid`).
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarkSeenArgs {
    pub id: String,
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub seen: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SyncArgs {
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub max_messages: Option<u32>,
}

#[derive(Clone)]
pub struct MailboxMcp {
    service: MailboxService,
}

#[tool_router]
impl MailboxMcp {
    pub fn new(service: MailboxService) -> Self {
        Self { service }
    }

    #[tool(
        description = "List configured mail accounts (id, name, email, protocol). Call first when account_id is unknown."
    )]
    fn list_accounts(&self) -> Result<CallToolResult, McpError> {
        json_ok(self.service.list_accounts().map_err(tool_err)?)
    }

    #[tool(description = "List folders for an account (Inbox, Sent, …) with unread counts.")]
    fn list_folders(
        &self,
        Parameters(args): Parameters<AccountIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        json_ok(
            self.service
                .list_folders(args.account_id.as_deref())
                .map_err(tool_err)?,
        )
    }

    #[tool(
        description = "List message envelopes in a folder (newest first). Returns id, subject, from, date, seen, snippet."
    )]
    fn list_messages(
        &self,
        Parameters(args): Parameters<ListMessagesArgs>,
    ) -> Result<CallToolResult, McpError> {
        json_ok(
            self.service
                .list_messages(
                    args.account_id.as_deref(),
                    args.folder.as_deref(),
                    args.limit.map(|n| n as usize),
                )
                .map_err(tool_err)?,
        )
    }

    #[tool(description = "Search local mail cache by subject/from/snippet/body substring.")]
    fn search_messages(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        json_ok(
            self.service
                .search_messages(args.account_id.as_deref(), &args.query)
                .map_err(tool_err)?,
        )
    }

    #[tool(description = "Fetch a full message (headers + body_text) by local id.")]
    fn get_message(
        &self,
        Parameters(args): Parameters<MessageIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        json_ok(self.service.get_message(&args.id).map_err(tool_err)?)
    }

    #[tool(description = "Mark a local message seen or unseen.")]
    fn mark_seen(
        &self,
        Parameters(args): Parameters<MarkSeenArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.service
            .mark_seen(&args.id, args.seen)
            .map_err(tool_err)?;
        json_ok(json!({ "id": args.id, "seen": args.seen }))
    }

    #[tool(
        description = "Sync one folder from the IMAP/POP3 server into the local cache (uses OS keyring password)."
    )]
    fn sync_folder(
        &self,
        Parameters(args): Parameters<SyncArgs>,
    ) -> Result<CallToolResult, McpError> {
        json_ok(
            self.service
                .sync_folder(
                    args.account_id.as_deref(),
                    args.folder.as_deref(),
                    args.max_messages,
                )
                .map_err(tool_err)?,
        )
    }
}

#[tool_handler(
    name = "ailiakmail",
    version = "0.1.0",
    instructions = "AiliakMail local mail MCP. Prefer list_accounts → list_folders → list_messages / search_messages → get_message. Sync only when the user asks to fetch from the server."
)]
impl ServerHandler for MailboxMcp {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_router_exposes_expected_tools() {
        let router = MailboxMcp::tool_router();
        for name in [
            "list_accounts",
            "list_folders",
            "list_messages",
            "search_messages",
            "get_message",
            "mark_seen",
            "sync_folder",
        ] {
            assert!(router.has_route(name), "missing tool {name}");
        }
    }
}
