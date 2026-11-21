use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use thiserror::Error;

use crate::map::CreateTaskPayload;

#[derive(Debug, Error)]
pub enum AiliakError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub struct WhoAmI {
    pub email: Option<String>,
    pub handle: Option<String>,
    pub display: String,
    pub raw: Value,
}

pub struct AiliakClient {
    http: Client,
    base_url: String,
    token: String,
    next_id: u64,
}

impl AiliakClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .user_agent(concat!("mailbox/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("build HTTP client")?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            next_id: 1,
        })
    }

    fn mcp_url(&self) -> String {
        format!("{}/mcp", self.base_url)
    }

    fn next_rpc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_rpc_id();
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let resp = self
            .http
            .post(self.mcp_url())
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .context("MCP HTTP request")?
            .error_for_status()
            .context("MCP HTTP status")?;

        let v: Value = resp.json().context("parse MCP JSON")?;
        if let Some(err) = v.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("MCP error");
            return Err(anyhow!(msg.to_string()));
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }

    pub fn initialize(&mut self) -> Result<()> {
        let _ = self.rpc(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "mailbox", "version": env!("CARGO_PKG_VERSION") },
            }),
        )?;
        // notifications/initialized — fire and forget (no response expected)
        let _ = self
            .http
            .post(self.mcp_url())
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            }))
            .send();
        Ok(())
    }

    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        let result = self.rpc(
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments,
            }),
        )?;
        if result.get("isError").and_then(|v| v.as_bool()) == Some(true) {
            let text = extract_text(&result).unwrap_or_else(|| "tool error".into());
            return Err(anyhow!(text));
        }
        Ok(result)
    }

    pub fn whoami(&mut self) -> Result<WhoAmI> {
        self.initialize()?;
        let result = self.call_tool("whoami", json!({}))?;
        let text = extract_text(&result).unwrap_or_else(|| result.to_string());
        // Prefer structured JSON if the tool returns it in text
        let raw: Value = serde_json::from_str(&text).unwrap_or(json!({ "text": text }));
        let email = raw
            .get("email")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let handle = raw
            .get("handle")
            .or_else(|| raw.get("displayName"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let display = handle
            .clone()
            .or_else(|| email.clone())
            .unwrap_or_else(|| text.chars().take(80).collect());
        Ok(WhoAmI {
            email,
            handle,
            display,
            raw,
        })
    }

    pub fn create_task(&mut self, payload: &CreateTaskPayload) -> Result<CreatedTask> {
        self.initialize()?;
        let mut args = json!({
            "text": payload.text,
            "title": payload.title,
            "detail": payload.detail,
        });
        match &payload.for_date {
            None => {}
            Some(s) if s.is_empty() => {
                args["forDate"] = Value::Null;
            }
            Some(s) => {
                args["forDate"] = json!(s);
            }
        }
        if let Some(url) = &payload.embed_url {
            args["embedUrl"] = json!(url);
        }
        if let Some(n) = &payload.task_number {
            args["taskNumber"] = json!(n);
        }
        if let Some(est) = payload.estimate_seconds {
            args["estimateSeconds"] = json!(est);
        }

        let result = self.call_tool("create_task", args)?;
        let text = extract_text(&result).unwrap_or_else(|| result.to_string());
        let parsed: Value = serde_json::from_str(&text).unwrap_or(json!({ "text": text }));
        let id = parsed
            .get("id")
            .or_else(|| parsed.get("taskId"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| text.split_whitespace().next().map(str::to_string))
            .unwrap_or(text);

        if let Some(spent) = payload.spent_seconds {
            if spent > 0 {
                let _ = self.call_tool("add_task_spent", json!({ "id": id, "seconds": spent }));
            }
        }
        if payload.mark_done {
            let _ = self.call_tool("mark_task_done", json!({ "id": id }));
        }

        Ok(CreatedTask { id, raw: parsed })
    }

    pub fn set_task_date(&mut self, id: &str, for_date: Option<&str>) -> Result<()> {
        self.initialize()?;
        let for_date = match for_date {
            None | Some("") => Value::Null,
            Some(s) => json!(s),
        };
        let _ = self.call_tool("set_task_date", json!({ "id": id, "forDate": for_date }))?;
        Ok(())
    }

    pub fn list_open_tasks(
        &mut self,
        for_date: Option<&str>,
        tag: Option<&str>,
    ) -> Result<Vec<DeskTask>> {
        self.initialize()?;
        let mut args = json!({});
        if let Some(d) = for_date {
            args["forDate"] = json!(d);
        }
        if let Some(t) = tag {
            args["tag"] = json!(t);
        }
        let result = self.call_tool("list_open_tasks", args)?;
        let text = extract_text(&result).unwrap_or_else(|| result.to_string());
        let parsed: Value = serde_json::from_str(&text).unwrap_or(json!({ "text": text }));

        let mut tasks = Vec::new();
        let list = parsed
            .get("tasks")
            .and_then(|t| t.as_array())
            .cloned()
            .or_else(|| parsed.as_array().cloned())
            .unwrap_or_default();

        for item in list {
            let id = item
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
            let title = item
                .get("title")
                .or_else(|| item.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)")
                .to_string();
            let for_date = item
                .get("forDate")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            tasks.push(DeskTask {
                id,
                title,
                for_date,
            });
        }
        Ok(tasks)
    }
}

#[derive(Debug, Clone)]
pub struct CreatedTask {
    pub id: String,
    pub raw: Value,
}

#[derive(Debug, Clone)]
pub struct DeskTask {
    pub id: String,
    pub title: String,
    pub for_date: Option<String>,
}

fn extract_text(tool_result: &Value) -> Option<String> {
    let content = tool_result.get("content")?.as_array()?;
    let mut parts = Vec::new();
    for item in content {
        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                parts.push(t.to_string());
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}
