//! MCP (Model Context Protocol) server over stdio.
//!
//! Speaks JSON-RPC 2.0, one message per line. Exposes the Desktop-DB read tools
//! (list_chats / read_chat / search). Live/send tools come from the presage
//! side once that integration lands.

use std::io::IsTerminal;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use signalkit_core::{live, DesktopBundle};
use uuid::Uuid;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "signalkit";

#[derive(Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

pub async fn run(bundle: DesktopBundle) -> Result<()> {
    if std::io::stdin().is_terminal() {
        eprintln!(
            "signalkit serve: speaking MCP over stdio.\n\
             This is meant to be driven by an MCP client (e.g. Claude Desktop).\n\
             For human use, try `signalkit list`, `read`, or `search`.\n"
        );
    }

    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                write_response(
                    &mut stdout,
                    &Response {
                        jsonrpc: "2.0",
                        id: Value::Null,
                        result: None,
                        error: Some(RpcError {
                            code: -32700,
                            message: format!("parse error: {e}"),
                        }),
                    },
                )
                .await?;
                continue;
            }
        };

        let is_notification = req.id.is_none();
        let id = req.id.clone().unwrap_or(Value::Null);
        let result = dispatch(&bundle, &req).await;

        if is_notification {
            // No response for notifications, per JSON-RPC 2.0.
            continue;
        }
        let resp = match result {
            Ok(value) => Response {
                jsonrpc: "2.0",
                id,
                result: Some(value),
                error: None,
            },
            Err(message) => Response {
                jsonrpc: "2.0",
                id,
                result: None,
                error: Some(RpcError {
                    code: -32000,
                    message,
                }),
            },
        };
        write_response(&mut stdout, &resp).await?;
    }
    Ok(())
}

async fn write_response<W: AsyncWriteExt + Unpin>(out: &mut W, resp: &Response) -> Result<()> {
    let mut buf = serde_json::to_vec(resp)?;
    buf.push(b'\n');
    out.write_all(&buf).await?;
    out.flush().await?;
    Ok(())
}

async fn dispatch(bundle: &DesktopBundle, req: &Request) -> Result<Value, String> {
    match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "serverInfo": {
                "name": SERVER_NAME,
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": { "tools": {} },
        })),
        // Notifications: no result expected; we just ack internally.
        "notifications/initialized" | "notifications/cancelled" => Ok(Value::Null),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list()),
        "tools/call" => tools_call(bundle, &req.params).await,
        other => Err(format!("method not found: {other}")),
    }
}

fn tools_list() -> Value {
    json!({
        "tools": [
            tool_def(
                "list_chats",
                "List Signal chats sorted by recent activity (Desktop DB).",
                json!({
                    "type": "object",
                    "properties": {
                        "include_empty": {
                            "type": "boolean",
                            "description": "Include chats with no activity. Default false.",
                            "default": false
                        }
                    }
                })
            ),
            tool_def(
                "read_chat",
                "Read messages from a chat. Newest first.",
                json!({
                    "type": "object",
                    "properties": {
                        "chat": { "type": "string", "description": "Chat display name from list_chats." },
                        "limit": { "type": "integer", "description": "Max messages.", "default": 50 },
                        "offset": { "type": "integer", "description": "Skip N before returning.", "default": 0 },
                        "from_ms": { "type": ["integer", "null"], "description": "Earliest sent_at (epoch ms)." },
                        "to_ms": { "type": ["integer", "null"], "description": "Latest sent_at (epoch ms)." }
                    },
                    "required": ["chat"]
                })
            ),
            tool_def(
                "search",
                "Substring search across messages. Optionally scope to a chat or a date range.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Substring to find in message bodies." },
                        "chat": { "type": ["string", "null"], "description": "Limit to this chat by display name." },
                        "limit": { "type": "integer", "default": 100 },
                        "from_ms": { "type": ["integer", "null"] },
                        "to_ms": { "type": ["integer", "null"] }
                    },
                    "required": ["query"]
                })
            ),
            tool_def(
                "find_recipient",
                "Find a contact's ACI UUID by name — use this to resolve a recipient before calling `send`. Searches across display name, profile names, and e164. Returns top matches sorted by message count (the contact you've spoken to most comes first). Only 1:1 contacts with a real ACI are returned (groups are filtered out).",
                json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Name or substring to search for (case-insensitive)." },
                        "limit": { "type": "integer", "default": 5 }
                    },
                    "required": ["name"]
                })
            ),
            tool_def(
                "whoami",
                "Return the linked account's ACI / PNI / number. Requires `signalkit link` first.",
                json!({ "type": "object", "properties": {} })
            ),
            tool_def(
                "send",
                "Send a 1:1 text message via presage. `to` is an ACI UUID; find it via list_chats (service_id field) or find_recipient. Returns send timestamp.",
                json!({
                    "type": "object",
                    "properties": {
                        "to": { "type": "string", "description": "Recipient ACI UUID." },
                        "body": { "type": "string", "description": "Message text." }
                    },
                    "required": ["to", "body"]
                })
            ),
            tool_def(
                "send_group",
                "Send a text message to a Signal group v2. Requires the group's 32-byte master key (base64). Master key lookup helpers are not yet wired — for now you must obtain it externally.",
                json!({
                    "type": "object",
                    "properties": {
                        "master_key_b64": { "type": "string", "description": "Group master key as base64 (32 bytes decoded)." },
                        "body": { "type": "string", "description": "Message text." }
                    },
                    "required": ["master_key_b64", "body"]
                })
            )
        ]
    })
}

fn tool_def(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema
    })
}

async fn tools_call(bundle: &DesktopBundle, params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing 'name'".to_string())?;
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let text = match name {
        "list_chats" => {
            let include_empty = args
                .get("include_empty")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let chats = bundle.db.list_chats(include_empty).map_err(|e| e.to_string())?;
            serde_json::to_string(&chats).map_err(|e| e.to_string())?
        }
        "read_chat" => {
            let chat = args
                .get("chat")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'chat'".to_string())?;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as u32;
            let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as u32;
            let from = args.get("from_ms").and_then(Value::as_i64);
            let to = args.get("to_ms").and_then(Value::as_i64);
            let row = bundle
                .db
                .find_chat_by_name(chat)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no chat matched: {chat}"))?;
            let msgs = bundle
                .db
                .get_messages(&row.id, Some(limit), offset, from, to)
                .map_err(|e| e.to_string())?;
            serde_json::to_string(&msgs).map_err(|e| e.to_string())?
        }
        "search" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'query'".to_string())?;
            let chat = args.get("chat").and_then(Value::as_str);
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(100) as u32;
            let from = args.get("from_ms").and_then(Value::as_i64);
            let to = args.get("to_ms").and_then(Value::as_i64);
            let cid = match chat {
                Some(name) => Some(
                    bundle
                        .db
                        .find_chat_by_name(name)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| format!("no chat matched: {name}"))?
                        .id,
                ),
                None => None,
            };
            let msgs = bundle
                .db
                .search_messages(cid.as_deref(), query, Some(limit), from, to)
                .map_err(|e| e.to_string())?;
            serde_json::to_string(&msgs).map_err(|e| e.to_string())?
        }
        "find_recipient" => {
            let needle = args
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'name'".to_string())?
                .to_lowercase();
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(5) as usize;
            let chats = bundle.db.list_chats(false).map_err(|e| e.to_string())?;
            let mut matches: Vec<_> = chats
                .into_iter()
                .filter(|c| c.service_id.is_some())
                .filter(|c| {
                    let hit = |opt: &Option<String>| {
                        opt.as_deref()
                            .map(|s| s.to_lowercase().contains(&needle))
                            .unwrap_or(false)
                    };
                    hit(&c.name)
                        || hit(&c.profile_full_name)
                        || hit(&c.profile_name)
                        || hit(&c.e164)
                })
                .collect();
            matches.sort_by_key(|c| std::cmp::Reverse(c.total_messages));
            matches.truncate(limit);
            let projected: Vec<_> = matches
                .iter()
                .map(|c| {
                    json!({
                        "display_name": c.display_name(),
                        "service_id": c.service_id,
                        "e164": c.e164,
                        "total_messages": c.total_messages,
                    })
                })
                .collect();
            serde_json::to_string(&projected).map_err(|e| e.to_string())?
        }
        "whoami" => {
            let store_path = live::default_store_path().map_err(|e| e.to_string())?;
            let store = live::open_store(&store_path).await.map_err(|e| e.to_string())?;
            let mut mgr = live::load_registered(store).await.map_err(|e| e.to_string())?;
            let who = live::whoami(&mut mgr).await.map_err(|e| e.to_string())?;
            serde_json::to_string(&who).map_err(|e| e.to_string())?
        }
        "send" => {
            let to_str = args
                .get("to")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'to' (ACI UUID)".to_string())?;
            let to: Uuid = to_str
                .parse()
                .map_err(|e: uuid::Error| format!("bad 'to' UUID: {e}"))?;
            let body = args
                .get("body")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'body'".to_string())?
                .to_string();
            let store_path = live::default_store_path().map_err(|e| e.to_string())?;
            let store = live::open_store(&store_path).await.map_err(|e| e.to_string())?;
            let mut mgr = live::load_registered(store).await.map_err(|e| e.to_string())?;
            let ts = live::send_text(&mut mgr, to, body)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_string(&json!({"timestamp": ts})).map_err(|e| e.to_string())?
        }
        "send_group" => {
            let b64 = args
                .get("master_key_b64")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'master_key_b64'".to_string())?;
            let body = args
                .get("body")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'body'".to_string())?
                .to_string();
            let master_key = live::decode_master_key(b64).map_err(|e| e.to_string())?;
            let store_path = live::default_store_path().map_err(|e| e.to_string())?;
            let store = live::open_store(&store_path).await.map_err(|e| e.to_string())?;
            let mut mgr = live::load_registered(store).await.map_err(|e| e.to_string())?;
            let ts = live::send_text_to_group(&mut mgr, &master_key, body)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_string(&json!({"timestamp": ts})).map_err(|e| e.to_string())?
        }
        other => return Err(format!("unknown tool: {other}")),
    };
    Ok(json!({
        "content": [{ "type": "text", "text": text }]
    }))
}
