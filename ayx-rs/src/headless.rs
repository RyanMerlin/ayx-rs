//! Bounded client for a product-owned local Headless Alteryx MCP server.
//!
//! The product server owns Designer semantics and authentication. This module
//! only owns the STDIO JSON-RPC lifecycle, bounded transport handling, and
//! redacted diagnostics needed to discover and invoke the published contract.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};
use url::Url;

use ayx_core::observability::redact_text;

pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOOL_PAGES: usize = 100;
const MAX_TOOLS: usize = 2_000;

#[derive(Debug, Clone)]
pub struct ServerSpec {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub timeout: Duration,
}

impl ServerSpec {
    pub fn new(executable: PathBuf, args: Vec<String>, timeout: Duration) -> Self {
        Self {
            executable,
            args,
            timeout,
        }
    }
}

#[derive(Debug)]
pub struct ServerInfo {
    pub protocol_version: Option<String>,
    pub server_info: Value,
    pub capabilities: Value,
}

/// The small common contract needed by the CLI's read/discover/call flows.
/// Both transports are deliberately generic: the product or Gateway owns the
/// actual workflow, dataset, and ability tool names and schemas.
pub trait McpClient {
    fn initialize(&mut self) -> Result<ServerInfo>;
    fn list_tools(&mut self) -> Result<Vec<Value>>;
    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value>;
}

#[derive(Debug)]
pub struct McpSession {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Result<String, String>>,
    next_id: u64,
    timeout: Duration,
    initialized: bool,
}

impl McpSession {
    pub fn spawn(spec: &ServerSpec) -> Result<Self> {
        if !spec.executable.is_file() {
            bail!(
                "MCP server executable does not exist: {}",
                spec.executable.display()
            );
        }

        let mut command = Command::new(&spec.executable);
        command.args(&spec.args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        // Keep product diagnostics out of the JSON-RPC stdout stream while
        // consuming them on a separate bounded reader so a noisy child cannot
        // deadlock and raw credentials do not bypass redaction.
        command.stderr(Stdio::piped());
        apply_safe_environment(&mut command);

        let mut child = command.spawn().with_context(|| {
            format!(
                "failed to start product MCP server '{}'",
                spec.executable.display()
            )
        })?;
        let stdout = child
            .stdout
            .take()
            .context("MCP server stdout pipe was not created")?;
        let stdin = child
            .stdin
            .take()
            .context("MCP server stdin pipe was not created")?;
        let stderr = child
            .stderr
            .take()
            .context("MCP server stderr pipe was not created")?;
        let (sender, lines) = mpsc::channel();

        if let Err(error) = thread::Builder::new()
            .name("ayx-mcp-stderr".to_string())
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                loop {
                    match read_bounded_line(&mut reader) {
                        Ok(Some(line)) => eprintln!("{}", redact_text(line.trim_end())),
                        Ok(None) => break,
                        Err(error) => {
                            eprintln!("{}", redact_text(&error));
                            break;
                        }
                    }
                }
            })
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow::anyhow!(
                "failed to start MCP stderr reader: {error}"
            ));
        }

        if let Err(error) = thread::Builder::new()
            .name("ayx-mcp-stdout".to_string())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    match read_bounded_line(&mut reader) {
                        Ok(Some(line)) => {
                            if sender.send(Ok(line)).is_err() {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            let _ = sender.send(Err(error));
                            break;
                        }
                    }
                }
            })
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow::anyhow!(
                "failed to start MCP stdout reader: {error}"
            ));
        }

        Ok(Self {
            child,
            stdin,
            lines,
            next_id: 1,
            timeout: spec.timeout,
            initialized: false,
        })
    }

    pub fn initialize(&mut self) -> Result<ServerInfo> {
        if self.initialized {
            bail!("MCP session is already initialized");
        }

        let response = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {
                    "name": "ayx-rs",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )?;
        let result = response
            .get("result")
            .cloned()
            .context("MCP initialize response omitted result")?;
        let info = ServerInfo {
            protocol_version: result
                .get("protocolVersion")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            server_info: result.get("serverInfo").cloned().unwrap_or(Value::Null),
            capabilities: result
                .get("capabilities")
                .cloned()
                .unwrap_or_else(|| json!({})),
        };

        self.send_notification("notifications/initialized", json!({}))?;
        self.initialized = true;
        Ok(info)
    }

    pub fn list_tools(&mut self) -> Result<Vec<Value>> {
        self.require_initialized()?;
        let mut cursor: Option<String> = None;
        let mut tools = Vec::new();

        for _ in 0..MAX_TOOL_PAGES {
            let mut params = serde_json::Map::new();
            if let Some(cursor) = cursor.as_ref() {
                params.insert("cursor".to_string(), Value::String(cursor.clone()));
            }
            let response = self.request("tools/list", Value::Object(params))?;
            let result = response
                .get("result")
                .and_then(Value::as_object)
                .context("MCP tools/list response omitted an object result")?;
            let page = result
                .get("tools")
                .and_then(Value::as_array)
                .context("MCP tools/list result omitted a tools array")?;
            tools.extend(page.iter().cloned());
            if tools.len() > MAX_TOOLS {
                bail!(
                    "MCP tool inventory exceeds the {} tool safety limit",
                    MAX_TOOLS
                );
            }
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if cursor.is_none() {
                return Ok(tools);
            }
        }

        bail!("MCP tool inventory exceeded the {MAX_TOOL_PAGES} page safety limit")
    }

    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        self.require_initialized()?;
        let response = self.request(
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments,
            }),
        )?;
        let result = response
            .get("result")
            .cloned()
            .context("MCP tools/call response omitted result")?;
        ensure_bounded_json(&result, MAX_RESULT_BYTES, "MCP tool result")?;
        Ok(result)
    }

    fn require_initialized(&self) -> Result<()> {
        if !self.initialized {
            bail!("MCP session must be initialized before tool operations");
        }
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .context("MCP request id overflow")?;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write_message(&request)?;

        loop {
            let line = self.read_line()?;
            let message: Value = serde_json::from_str(&line).with_context(|| {
                format!("MCP server emitted invalid JSON: {}", redact_text(&line))
            })?;
            ensure_bounded_json(&message, MAX_MESSAGE_BYTES, "MCP message")?;
            if message.get("id") != Some(&Value::from(id)) {
                // Notifications and unrelated responses may be interleaved. They
                // are not a response to this request and must not shift IDs.
                continue;
            }
            if let Some(error) = message.get("error") {
                let safe = redact_text(
                    &serde_json::to_string(error)
                        .unwrap_or_else(|_| "unserializable MCP error".to_string()),
                );
                bail!("MCP request '{method}' failed: {safe}");
            }
            return Ok(message);
        }
    }

    fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn write_message(&mut self, message: &Value) -> Result<()> {
        let encoded = serde_json::to_vec(message).context("failed to encode MCP message")?;
        if encoded.len() > MAX_MESSAGE_BYTES {
            bail!(
                "MCP request exceeds the {} byte safety limit",
                MAX_MESSAGE_BYTES
            );
        }
        self.stdin
            .write_all(&encoded)
            .context("failed to write MCP request")?;
        self.stdin
            .write_all(b"\n")
            .context("failed to frame MCP request")?;
        self.stdin.flush().context("failed to flush MCP request")?;
        Ok(())
    }

    fn read_line(&mut self) -> Result<String> {
        match self.lines.recv_timeout(self.timeout) {
            Ok(Ok(line)) => Ok(line),
            Ok(Err(error)) => bail!("{error}"),
            Err(RecvTimeoutError::Timeout) => {
                bail!(
                    "MCP server did not respond within {} seconds",
                    self.timeout.as_secs()
                )
            }
            Err(RecvTimeoutError::Disconnected) => {
                let status = self
                    .child
                    .try_wait()
                    .ok()
                    .flatten()
                    .map(|status| status.to_string())
                    .unwrap_or_else(|| "unknown child status".to_string());
                bail!("MCP server stdout closed before a response arrived ({status})")
            }
        }
    }
}

impl McpClient for McpSession {
    fn initialize(&mut self) -> Result<ServerInfo> {
        Self::initialize(self)
    }

    fn list_tools(&mut self) -> Result<Vec<Value>> {
        Self::list_tools(self)
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        Self::call_tool(self, name, arguments)
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn ensure_bounded_json(value: &Value, limit: usize, label: &str) -> Result<()> {
    let size = serde_json::to_vec(value)
        .with_context(|| format!("failed to measure {label}"))?
        .len();
    if size > limit {
        bail!("{label} exceeds the {limit} byte safety limit");
    }
    Ok(())
}

fn read_bounded_line(reader: &mut impl BufRead) -> Result<Option<String>, String> {
    let mut bytes = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|error| format!("failed to read MCP server stdout: {error}"))?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            return String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "MCP server emitted non-UTF-8 stdout".to_string());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |position| position + 1);
        if bytes.len().saturating_add(take) > MAX_MESSAGE_BYTES {
            return Err(format!(
                "MCP message exceeds the {} byte safety limit",
                MAX_MESSAGE_BYTES
            ));
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "MCP server emitted non-UTF-8 stdout".to_string());
        }
    }
}

fn apply_safe_environment(command: &mut Command) {
    const ALLOWLIST: &[&str] = &[
        "PATH",
        "SystemRoot",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "HOME",
        "XDG_CONFIG_HOME",
    ];
    let inherited: Vec<(String, String)> = ALLOWLIST
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect();
    command.env_clear();
    command.envs(inherited);
}

#[derive(Clone)]
pub struct GatewaySpec {
    pub endpoint: Url,
    pub bearer_token: String,
    pub timeout: Duration,
}

impl GatewaySpec {
    pub fn new(endpoint: Url, bearer_token: String, timeout: Duration) -> Self {
        Self {
            endpoint,
            bearer_token,
            timeout,
        }
    }
}

/// Bounded Streamable HTTP MCP client for a published Gateway endpoint.
///
/// The endpoint is always explicit and the bearer token is supplied out of
/// band. The token is held only for the request lifetime and is never included
/// in an envelope or diagnostic string.
pub struct GatewaySession {
    client: Client,
    endpoint: Url,
    bearer_token: String,
    session_id: Option<String>,
    next_id: u64,
    initialized: bool,
}

impl GatewaySession {
    pub fn connect(spec: &GatewaySpec) -> Result<Self> {
        if !matches!(spec.endpoint.scheme(), "http" | "https") {
            bail!("MCP Gateway endpoint must use http or https")
        }
        if spec.endpoint.username() != "" || spec.endpoint.password().is_some() {
            bail!("MCP Gateway endpoint must not contain embedded credentials")
        }
        if spec.bearer_token.trim().is_empty() {
            bail!("MCP Gateway bearer token cannot be empty")
        }
        if spec.bearer_token.len() > 64 * 1024 {
            bail!("MCP Gateway bearer token exceeds the 64 KiB safety limit")
        }
        let client = Client::builder()
            .timeout(spec.timeout)
            .build()
            .context("failed to build MCP Gateway HTTP client")?;
        Ok(Self {
            client,
            endpoint: spec.endpoint.clone(),
            bearer_token: spec.bearer_token.trim().to_string(),
            session_id: None,
            next_id: 1,
            initialized: false,
        })
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .context("MCP request id overflow")?;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let body = self.post(&request)?;
        let messages = parse_http_messages(&body)?;
        for message in messages {
            if message.get("id") != Some(&Value::from(id)) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let safe = redact_text(
                    &serde_json::to_string(error)
                        .unwrap_or_else(|_| "unserializable MCP error".to_string()),
                );
                bail!("MCP Gateway request '{method}' failed: {safe}");
            }
            return Ok(message);
        }
        bail!("MCP Gateway response did not contain a response for request {id}")
    }

    fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        let request = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let body = self.post(&request)?;
        if !body.is_empty() {
            let _ = parse_http_messages(&body)?;
        }
        Ok(())
    }

    fn post(&mut self, request: &Value) -> Result<Vec<u8>> {
        let body = serde_json::to_vec(request).context("failed to encode MCP Gateway request")?;
        if body.len() > MAX_MESSAGE_BYTES {
            bail!(
                "MCP Gateway request exceeds the {} byte safety limit",
                MAX_MESSAGE_BYTES
            );
        }
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        let authorization = format!("Bearer {}", self.bearer_token);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&authorization).context("invalid MCP Gateway bearer token")?,
        );
        if let Some(session_id) = self.session_id.as_deref() {
            headers.insert(
                "Mcp-Session-Id",
                HeaderValue::from_str(session_id).context("invalid MCP Gateway session id")?,
            );
        }
        let response = self
            .client
            .post(self.endpoint.clone())
            .headers(headers)
            .body(body)
            .send()
            .context("MCP Gateway request failed")?;
        if let Some(session_id) = response.headers().get("Mcp-Session-Id") {
            let session_id = session_id
                .to_str()
                .context("MCP Gateway returned an invalid session id")?;
            if session_id.len() > MAX_MESSAGE_BYTES {
                bail!("MCP Gateway session id exceeds the safety limit");
            }
            self.session_id = Some(session_id.to_string());
        }
        let status = response.status();
        let mut limited_response = response.take(MAX_RESULT_BYTES as u64 + 1);
        let mut bytes = Vec::new();
        limited_response
            .read_to_end(&mut bytes)
            .context("failed to read MCP Gateway response")?;
        if bytes.len() > MAX_RESULT_BYTES {
            bail!(
                "MCP Gateway response exceeds the {} byte safety limit",
                MAX_RESULT_BYTES
            );
        }
        if !status.is_success() {
            let detail = String::from_utf8_lossy(&bytes);
            bail!(
                "MCP Gateway returned HTTP {}: {}",
                status,
                redact_text(&detail)
            );
        }
        Ok(bytes.to_vec())
    }
}

impl McpClient for GatewaySession {
    fn initialize(&mut self) -> Result<ServerInfo> {
        if self.initialized {
            bail!("MCP Gateway session is already initialized");
        }
        let response = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {
                    "name": "ayx-rs",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )?;
        let result = response
            .get("result")
            .cloned()
            .context("MCP Gateway initialize response omitted result")?;
        let info = ServerInfo {
            protocol_version: result
                .get("protocolVersion")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            server_info: result.get("serverInfo").cloned().unwrap_or(Value::Null),
            capabilities: result
                .get("capabilities")
                .cloned()
                .unwrap_or_else(|| json!({})),
        };
        self.send_notification("notifications/initialized", json!({}))?;
        self.initialized = true;
        Ok(info)
    }

    fn list_tools(&mut self) -> Result<Vec<Value>> {
        if !self.initialized {
            bail!("MCP Gateway session must be initialized before tool operations");
        }
        let mut cursor: Option<String> = None;
        let mut tools = Vec::new();
        for _ in 0..MAX_TOOL_PAGES {
            let mut params = serde_json::Map::new();
            if let Some(cursor) = cursor.as_ref() {
                params.insert("cursor".to_string(), Value::String(cursor.clone()));
            }
            let response = self.request("tools/list", Value::Object(params))?;
            let result = response
                .get("result")
                .and_then(Value::as_object)
                .context("MCP Gateway tools/list response omitted an object result")?;
            let page = result
                .get("tools")
                .and_then(Value::as_array)
                .context("MCP Gateway tools/list result omitted a tools array")?;
            tools.extend(page.iter().cloned());
            if tools.len() > MAX_TOOLS {
                bail!(
                    "MCP Gateway tool inventory exceeds the {} tool safety limit",
                    MAX_TOOLS
                );
            }
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if cursor.is_none() {
                return Ok(tools);
            }
        }
        bail!("MCP Gateway tool inventory exceeded the {MAX_TOOL_PAGES} page safety limit")
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        if !self.initialized {
            bail!("MCP Gateway session must be initialized before tool operations");
        }
        let response = self.request("tools/call", json!({"name": name, "arguments": arguments}))?;
        let result = response
            .get("result")
            .cloned()
            .context("MCP Gateway tools/call response omitted result")?;
        ensure_bounded_json(&result, MAX_RESULT_BYTES, "MCP Gateway tool result")?;
        Ok(result)
    }
}

fn parse_http_messages(body: &[u8]) -> Result<Vec<Value>> {
    if body.len() > MAX_RESULT_BYTES {
        bail!(
            "MCP Gateway response exceeds the {} byte safety limit",
            MAX_RESULT_BYTES
        );
    }
    if let Ok(value) = serde_json::from_slice::<Value>(body)
        && value.is_object()
    {
        ensure_bounded_json(&value, MAX_RESULT_BYTES, "MCP Gateway message")?;
        return Ok(vec![value]);
    }
    let text = std::str::from_utf8(body).context("MCP Gateway response was not UTF-8")?;
    let mut messages = Vec::new();
    for line in text.lines() {
        let Some(candidate) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if candidate.is_empty() || candidate == "[DONE]" {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(candidate)
            && value.is_object()
        {
            ensure_bounded_json(&value, MAX_RESULT_BYTES, "MCP Gateway message")?;
            messages.push(value);
            // A stream can contain notifications before the response. Keep
            // collecting data events so request() can match its numeric id.
        }
    }
    if messages.is_empty() {
        bail!("MCP Gateway response did not contain a JSON-RPC message")
    }
    Ok(messages)
}

/// Categorize only what is explicitly present in tool metadata. This is a
/// demo aid, not a claim that a missing category is unsupported by the product.
pub fn tool_families(tools: &[Value]) -> BTreeMap<&'static str, Vec<String>> {
    let mut families: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for tool in tools {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        let searchable = [
            name,
            tool.get("title")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            tool.get("description")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ]
        .join(" ")
        .to_ascii_lowercase();
        for (family, needles) in [
            ("workflow", &["workflow"][..]),
            ("dataset", &["dataset", "data set"][..]),
            ("ability", &["ability", "abilities"][..]),
            (
                "analytic_app",
                &["analytic app", "analytic_app", "analyticapp"][..],
            ),
        ] {
            if needles.iter().any(|needle| searchable.contains(needle)) {
                families.entry(family).or_default().push(name.to_string());
            }
        }
    }
    families
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_spec_keeps_arguments_out_of_the_executable_path() {
        let spec = ServerSpec::new(
            PathBuf::from("C:/Program Files/Alteryx/alteryx-mcp-server.exe"),
            vec!["--stdio".to_string()],
            Duration::from_secs(30),
        );
        assert_eq!(
            spec.executable.to_string_lossy(),
            "C:/Program Files/Alteryx/alteryx-mcp-server.exe"
        );
        assert_eq!(spec.args, ["--stdio"]);
    }

    #[test]
    fn bounded_json_rejects_large_payloads() {
        let value = Value::String("x".repeat(32));
        let error = ensure_bounded_json(&value, 8, "test payload").expect_err("must reject");
        assert!(error.to_string().contains("safety limit"));
    }

    #[test]
    fn parses_json_and_sse_gateway_responses() {
        let json_messages =
            parse_http_messages(br#"{"jsonrpc":"2.0","id":1,"result":{}}"#).expect("JSON response");
        assert_eq!(json_messages[0]["id"], 1);

        let sse_messages = parse_http_messages(
            b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n\n",
        )
        .expect("SSE response");
        assert_eq!(sse_messages[0]["id"], 2);
    }

    #[test]
    fn tool_families_are_metadata_driven() {
        let tools = vec![
            json!({"name":"list_workflows"}),
            json!({"name":"read_data", "description":"Read a dataset"}),
            json!({"name":"publish", "title":"Ability management"}),
            json!({"name":"other"}),
        ];
        let families = tool_families(&tools);
        assert_eq!(families["workflow"], ["list_workflows"]);
        assert_eq!(families["dataset"], ["read_data"]);
        assert_eq!(families["ability"], ["publish"]);
        assert!(!families.contains_key("analytic_app"));
    }

    #[test]
    fn gateway_session_sends_bearer_and_reuses_session_id() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test gateway");
        let address = listener.local_addr().expect("test gateway address");
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for request_number in 0..3 {
                let (mut stream, _) = listener.accept().expect("accept gateway request");
                let mut bytes = Vec::new();
                let mut chunk = [0_u8; 4096];
                loop {
                    let read = stream.read(&mut chunk).expect("read gateway request");
                    bytes.extend_from_slice(&chunk[..read]);
                    if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                requests.push(String::from_utf8(bytes).expect("request text"));
                let body = match request_number {
                    0 => br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"gateway"}}}"#.to_vec(),
                    1 => Vec::new(),
                    _ => br#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"list_workflows"}]}}"#.to_vec(),
                };
                let status = if request_number == 1 {
                    "202 Accepted"
                } else {
                    "200 OK"
                };
                let session = if request_number == 0 {
                    "Mcp-Session-Id: demo-session\r\n"
                } else {
                    ""
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{session}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write gateway response");
                stream.write_all(&body).expect("write gateway body");
            }
            requests
        });

        let spec = GatewaySpec::new(
            Url::parse(&format!("http://{address}/mcp")).expect("test endpoint"),
            "test-token".to_string(),
            Duration::from_secs(5),
        );
        let mut session = GatewaySession::connect(&spec).expect("connect gateway");
        let info = session.initialize().expect("initialize gateway");
        assert_eq!(info.protocol_version.as_deref(), Some("2025-06-18"));
        let tools = session.list_tools().expect("list gateway tools");
        assert_eq!(tools[0]["name"], "list_workflows");
        let requests = server.join().expect("gateway server");
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("authorization: bearer test-token")
        );
        assert!(
            requests[2]
                .to_ascii_lowercase()
                .contains("mcp-session-id: demo-session")
        );
    }

    #[test]
    fn safe_environment_allowlist_excludes_secret_names() {
        let mut command = Command::new("ayx");
        apply_safe_environment(&mut command);
        let names: Vec<String> = command
            .get_envs()
            .filter_map(|(name, _)| name.to_str().map(ToOwned::to_owned))
            .collect();
        assert!(
            !names
                .iter()
                .any(|name| name.to_ascii_lowercase().contains("token"))
        );
        assert!(
            !names
                .iter()
                .any(|name| name.to_ascii_lowercase().contains("secret"))
        );
    }
}
