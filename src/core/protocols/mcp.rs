//! Minimal MCP client over STDIO (JSON-RPC 2.0).
//! Connect → initialize → tools/list → tools/call.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
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
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
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
            reader: BufReader::new(stdout),
            next_id: 1,
        };
        // MCP initialize
        let init = sess.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "EvilPostman", "version": "0.1.0" }
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
        // Content-Length framing (MCP STDIO)
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin
            .write_all(header.as_bytes())
            .and_then(|_| self.stdin.write_all(&body))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("write: {e}"))
    }

    fn read_response(&mut self, expect_id: u64) -> Result<JsonValue, String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            let mut headers = String::new();
            // Read headers until blank line
            loop {
                let mut line = String::new();
                let n = self
                    .reader
                    .read_line(&mut line)
                    .map_err(|e| format!("read: {e}"))?;
                if n == 0 {
                    return Err("mcp stdout eof".into());
                }
                if line == "\r\n" || line == "\n" {
                    break;
                }
                headers.push_str(&line);
            }
            let mut content_len = 0usize;
            for line in headers.lines() {
                if let Some(v) = line
                    .to_ascii_lowercase()
                    .strip_prefix("content-length:")
                {
                    content_len = v.trim().parse().unwrap_or(0);
                }
            }
            if content_len == 0 {
                continue;
            }
            let mut buf = vec![0u8; content_len];
            use std::io::Read;
            self.reader
                .read_exact(&mut buf)
                .map_err(|e| format!("body: {e}"))?;
            let v: JsonValue =
                serde_json::from_slice(&buf).map_err(|e| format!("json: {e}"))?;
            // Skip notifications
            if v.get("id").is_none() && v.get("method").is_some() {
                continue;
            }
            if v.get("id").and_then(|i| i.as_u64()) == Some(expect_id)
                || v.get("id").and_then(|i| i.as_i64()) == Some(expect_id as i64)
            {
                return Ok(v);
            }
        }
        Err("mcp response timeout".into())
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
