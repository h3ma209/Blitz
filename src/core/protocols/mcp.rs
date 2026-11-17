//! Minimal MCP client over STDIO (JSON-RPC 2.0).
//! Connect → initialize → tools/list → tools/call.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Value as JsonValue};

#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
}

pub struct McpSession {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Result<JsonValue, String>>,
    next_id: u64,
}

fn spawn_mcp_reader(stdout: std::process::ChildStdout) -> mpsc::Receiver<Result<JsonValue, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = tx.send(Err("mcp stdout eof".into()));
                    return;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    // Support newline-delimited JSON-RPC (standard MCP stdio transport)
                    if trimmed.starts_with('{') {
                        if let Ok(v) = serde_json::from_str::<JsonValue>(trimmed) {
                            if tx.send(Ok(v)).is_err() {
                                return;
                            }
                        }
                        continue;
                    }
                    // Support LSP-style Content-Length framing
                    let mut content_len = if let Some(v) =
                        trimmed.to_ascii_lowercase().strip_prefix("content-length:")
                    {
                        v.trim().parse().unwrap_or(0)
                    } else {
                        // Skip non-JSON diagnostic text lines on stdout
                        continue;
                    };
                    loop {
                        let mut hdr = String::new();
                        match reader.read_line(&mut hdr) {
                            Ok(0) => {
                                let _ = tx.send(Err("mcp stdout eof".into()));
                                return;
                            }
                            Ok(_) => {
                                if hdr == "\r\n" || hdr == "\n" {
                                    break;
                                }
                                if let Some(v) = hdr.trim().to_ascii_lowercase().strip_prefix("content-length:") {
                                    content_len = v.trim().parse().unwrap_or(content_len);
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(format!("read: {e}")));
                                return;
                            }
                        }
                    }
                    if content_len == 0 {
                        continue;
                    }
                    const MAX_MCP_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
                    if content_len > MAX_MCP_MESSAGE_BYTES {
                        let _ = tx.send(Err(format!("mcp message too large: {content_len} bytes")));
                        return;
                    }
                    let mut buf = vec![0u8; content_len];
                    if let Err(e) = reader.read_exact(&mut buf) {
                        let _ = tx.send(Err(format!("body: {e}")));
                        return;
                    }
                    match serde_json::from_slice::<JsonValue>(&buf) {
                        Ok(v) => {
                            if tx.send(Ok(v)).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(format!("json: {e}")));
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("read: {e}")));
                    return;
                }
            }
        }
    });
    rx
}

impl McpSession {
    pub fn connect(command: &str, args: &[String]) -> Result<Self, String> {
        if command.trim().is_empty() {
            return Err("mcp command empty".into());
        }
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn {command}: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let mut sess = Self {
            child,
            stdin,
            rx: spawn_mcp_reader(stdout),
            next_id: 1,
        };
        // MCP initialize
        let init = sess.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "Blitz", "version": "0.1.0" }
            }),
        )?;
        if init.get("error").is_some() {
            return Err(format!("initialize error: {init}"));
        }
        // notifications/initialized (no id)
        sess.notify(
            "notifications/initialized",
            json!({}),
        )?;
        Ok(sess)
    }

    pub fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, String> {
        let res = self.request("tools/list", json!({}))?;
        if let Some(err) = res.get("error") {
            return Err(format!("tools/list: {err}"));
        }
        let tools = res
            .pointer("/result/tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(tools
            .iter()
            .map(|t| McpToolInfo {
                name: t
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: t
                    .get("description")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
            .collect())
    }

    pub fn call_tool(&mut self, name: &str, args: JsonValue) -> Result<String, String> {
        let res = self.request(
            "tools/call",
            json!({
                "name": name,
                "arguments": args
            }),
        )?;
        if let Some(err) = res.get("error") {
            return Err(format!("tools/call: {err}"));
        }
        Ok(serde_json::to_string_pretty(&res.get("result").cloned().unwrap_or(JsonValue::Null))
            .unwrap_or_else(|_| "{}".into()))
    }

    fn request(&mut self, method: &str, params: JsonValue) -> Result<JsonValue, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.write_message(&msg)?;
        self.read_response(id)
    }

    fn notify(&mut self, method: &str, params: JsonValue) -> Result<(), String> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.write_message(&msg)
    }

    fn write_message(&mut self, msg: &JsonValue) -> Result<(), String> {
        let body = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
        // Newline-delimited JSON-RPC (standard MCP STDIO transport)
        self.stdin
            .write_all(&body)
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("write: {e}"))
    }

    fn read_response(&mut self, expect_id: u64) -> Result<JsonValue, String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err("mcp response timeout".into());
            }
            let remaining = deadline.saturating_duration_since(now);
            let msg = match self.rx.recv_timeout(remaining) {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => return Err(e),
                Err(mpsc::RecvTimeoutError::Timeout) => return Err("mcp response timeout".into()),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err("mcp stdout eof".into()),
            };
            // Skip notifications
            if msg.get("id").is_none() && msg.get("method").is_some() {
                continue;
            }
            if msg.get("id").and_then(|i| i.as_u64()) == Some(expect_id)
                || msg.get("id").and_then(|i| i.as_i64()) == Some(expect_id as i64)
            {
                return Ok(msg);
            }
        }
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One-shot: connect, list tools, disconnect.
pub fn mcp_list_tools(command: &str, args: &[String]) -> Result<Vec<McpToolInfo>, String> {
    let (cmd, argv) = resolve_command(command, args);
    let mut s = McpSession::connect(&cmd, &argv)?;
    s.list_tools()
}

/// One-shot: connect, call tool, disconnect.
pub fn mcp_call_tool(
    command: &str,
    args: &[String],
    tool: &str,
    tool_args: JsonValue,
) -> Result<String, String> {
    let (cmd, argv) = resolve_command(command, args);
    let mut s = McpSession::connect(&cmd, &argv)?;
    s.call_tool(tool, tool_args)
}

pub fn split_mcp_args(s: &str) -> Vec<String> {
    s.split_whitespace().map(|x| x.to_string()).collect()
}

fn resolve_command(command: &str, args: &[String]) -> (String, Vec<String>) {
    if args.is_empty() && command.contains(char::is_whitespace) {
        let mut parts = split_mcp_args(command);
        if parts.is_empty() {
            return (command.to_string(), vec![]);
        }
        let cmd = parts.remove(0);
        (cmd, parts)
    } else {
        (command.to_string(), args.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_args() {
        assert_eq!(
            split_mcp_args("npx -y foo"),
            vec!["npx".to_string(), "-y".to_string(), "foo".to_string()]
        );
    }
}
