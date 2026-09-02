//! CLI access to the product-owned local Headless Alteryx MCP server.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use url::Url;

use ayx_core::envelope::{Envelope, ErrorCode};

use crate::cmd::confirm;
use crate::headless::{
    GatewaySession, GatewaySpec, McpClient, McpSession, ServerInfo, ServerSpec, tool_families,
};

#[derive(Subcommand, Debug)]
pub(crate) enum HeadlessCommand {
    /// Check the local product MCP server, protocol handshake, and tool inventory.
    Doctor {
        #[command(flatten)]
        server: ServerArgs,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum McpCommand {
    /// Use an authenticated Streamable HTTP MCP Gateway endpoint.
    Gateway {
        #[command(subcommand)]
        command: GatewayCommand,
    },
    /// Discover the published product MCP tool contract.
    Tools {
        #[command(subcommand)]
        command: McpToolsCommand,
    },
    /// Invoke one product MCP tool. Execution is dry-run unless --apply is set.
    Call {
        /// Exact product-published tool name.
        name: String,
        /// JSON input file, or '-' to read JSON from stdin.
        #[arg(long, value_name = "FILE")]
        input: Option<PathBuf>,
        #[command(flatten)]
        server: ServerArgs,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum GatewayCommand {
    /// Show negotiated protocol abilities and workflow/dataset tool families.
    Abilities {
        #[command(flatten)]
        gateway: GatewayArgs,
    },
    /// Discover the Gateway's published MCP tools.
    Tools {
        #[command(subcommand)]
        command: GatewayToolsCommand,
    },
    /// Invoke one Gateway MCP tool. Execution is dry-run unless --apply is set.
    Call {
        /// Exact Gateway-published tool name.
        name: String,
        /// JSON input file, or '-' to read JSON from stdin.
        #[arg(long, value_name = "FILE")]
        input: Option<PathBuf>,
        #[command(flatten)]
        gateway: GatewayArgs,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum GatewayToolsCommand {
    /// List all tools published by the Gateway.
    List {
        /// Optional metadata family filter: workflow, dataset, ability, or analytic-app.
        #[arg(long, value_name = "FAMILY")]
        family: Option<String>,
        #[command(flatten)]
        gateway: GatewayArgs,
    },
    /// Show one published Gateway tool schema.
    Describe {
        /// Exact Gateway-published tool name.
        name: String,
        #[command(flatten)]
        gateway: GatewayArgs,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum McpToolsCommand {
    /// List all published product MCP tools.
    List {
        #[command(flatten)]
        server: ServerArgs,
    },
    /// Show one published product MCP tool schema.
    Describe {
        /// Exact product-published tool name.
        name: String,
        #[command(flatten)]
        server: ServerArgs,
    },
}

#[derive(Args, Debug, Clone)]
pub(crate) struct ServerArgs {
    /// Path to the product-owned MCP server executable.
    ///
    /// If omitted, AYX_MCP_SERVER is used. AYX-RS never searches PATH for a
    /// product server implicitly because an unvalidated executable would be a
    /// security boundary failure.
    #[arg(long, value_name = "PATH")]
    pub(crate) server: Option<PathBuf>,
    /// Argument passed to the product MCP server. Repeat for multiple args.
    #[arg(long = "arg", value_name = "ARG")]
    pub(crate) args: Vec<String>,
    /// Per-response timeout in seconds.
    #[arg(long, default_value_t = 30, value_name = "SECONDS")]
    pub(crate) timeout_seconds: u64,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct GatewayArgs {
    /// Explicit Streamable HTTP MCP Gateway endpoint.
    ///
    /// If omitted, AYX_MCP_GATEWAY_ENDPOINT is used. The endpoint must not
    /// contain credentials; the token is supplied through an environment
    /// variable or standard input.
    #[arg(long, value_name = "URL")]
    pub(crate) endpoint: Option<String>,
    /// Environment variable containing the bearer token.
    #[arg(long, default_value = "AYX_MCP_GATEWAY_TOKEN", value_name = "NAME")]
    pub(crate) token_env: String,
    /// Read the bearer token from standard input without echoing or logging it.
    #[arg(long)]
    pub(crate) token_stdin: bool,
    /// Per-response timeout in seconds.
    #[arg(long, default_value_t = 30, value_name = "SECONDS")]
    pub(crate) timeout_seconds: u64,
}

pub(crate) fn execute_headless(command: HeadlessCommand) -> Result<Envelope> {
    match command {
        HeadlessCommand::Doctor { server } => {
            let spec = match resolve_spec(&server) {
                Ok(spec) => spec,
                Err(error) => return Ok(config_error(error.to_string())),
            };
            let path = spec.executable.display().to_string();
            let mut session = McpSession::spawn(&spec)?;
            let info = session.initialize()?;
            let tools = session.list_tools()?;
            Ok(Envelope::ok_with_data(
                "headless doctor completed",
                json!({
                    "backend": "product_mcp",
                    "transport": "stdio",
                    "server": {
                        "executable": path,
                        "argument_count": spec.args.len(),
                    },
                    "protocol": server_info_value(&info),
                    "tool_count": tools.len(),
                    "tool_families": tool_families(&tools),
                    "tools": tools,
                }),
            ))
        }
    }
}

pub(crate) fn execute_mcp(apply: bool, yes: bool, command: McpCommand) -> Result<Envelope> {
    match command {
        McpCommand::Gateway { command } => execute_gateway(apply, yes, command),
        McpCommand::Tools { command } => execute_tools(command),
        McpCommand::Call {
            name,
            input,
            server,
        } => execute_call(apply, yes, &name, input.as_deref(), &server),
    }
}

fn execute_gateway(apply: bool, yes: bool, command: GatewayCommand) -> Result<Envelope> {
    match command {
        GatewayCommand::Abilities { gateway } => {
            let (mut session, endpoint) = connect_gateway(&gateway)?;
            let info = session.initialize()?;
            let tools = session.list_tools()?;
            Ok(Envelope::ok_with_data(
                "MCP Gateway abilities discovered",
                json!({
                    "backend": "mcp_gateway",
                    "transport": "streamable_http",
                    "endpoint": endpoint,
                    "protocol": server_info_value(&info),
                    "abilities": {
                        "protocol_capabilities": info.capabilities,
                        "tool_families": tool_families(&tools),
                        "tool_count": tools.len(),
                    },
                    "tools": tools,
                }),
            ))
        }
        GatewayCommand::Tools { command } => execute_gateway_tools(command),
        GatewayCommand::Call {
            name,
            input,
            gateway,
        } => execute_gateway_call(apply, yes, &name, input.as_deref(), &gateway),
    }
}

fn execute_gateway_tools(command: GatewayToolsCommand) -> Result<Envelope> {
    let (name, family, gateway) = match command {
        GatewayToolsCommand::List { family, gateway } => (None, family, gateway),
        GatewayToolsCommand::Describe { name, gateway } => (Some(name), None, gateway),
    };
    let (mut session, endpoint) = connect_gateway(&gateway)?;
    let protocol = session.initialize()?;
    let tools = session.list_tools()?;
    let selected_tools = if let Some(family) = family.as_deref() {
        let normalized = normalize_family(family)?;
        let names = tool_families(&tools).remove(normalized).unwrap_or_default();
        tools
            .iter()
            .filter(|tool| {
                tool.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| names.iter().any(|candidate| candidate == name))
            })
            .cloned()
            .collect::<Vec<_>>()
    } else {
        tools.clone()
    };
    if let Some(name) = name {
        let Some(tool) = tools.iter().find(|tool| {
            tool.get("name")
                .and_then(Value::as_str)
                .is_some_and(|candidate| candidate == name)
        }) else {
            return Ok(Envelope::err_coded(
                ErrorCode::NotFound,
                format!("MCP Gateway tool '{name}' was not found"),
                json!({
                    "backend": "mcp_gateway",
                    "transport": "streamable_http",
                    "endpoint": endpoint,
                    "tool": name,
                    "available_tool_count": tools.len(),
                }),
            ));
        };
        return Ok(Envelope::ok_with_data(
            format!("MCP Gateway tool '{name}' described"),
            json!({
                "backend": "mcp_gateway",
                "transport": "streamable_http",
                "endpoint": endpoint,
                "protocol": server_info_value(&protocol),
                "tool": tool,
            }),
        ));
    }
    Ok(Envelope::ok_with_data(
        "MCP Gateway tools listed",
        json!({
            "backend": "mcp_gateway",
            "transport": "streamable_http",
            "endpoint": endpoint,
            "protocol": server_info_value(&protocol),
            "tool_count": tools.len(),
            "selected_family": family,
            "selected_tool_count": selected_tools.len(),
            "tool_families": tool_families(&tools),
            "tools": selected_tools,
        }),
    ))
}

fn normalize_family(family: &str) -> Result<&'static str> {
    match family.trim().to_ascii_lowercase().as_str() {
        "workflow" | "workflows" => Ok("workflow"),
        "dataset" | "datasets" | "data-set" => Ok("dataset"),
        "ability" | "abilities" => Ok("ability"),
        "analytic-app" | "analytic-apps" | "analytic_app" => Ok("analytic_app"),
        _ => Err(anyhow!(
            "unknown MCP tool family '{family}'; use workflow, dataset, ability, or analytic-app"
        )),
    }
}

fn execute_gateway_call(
    apply: bool,
    yes: bool,
    name: &str,
    input: Option<&std::path::Path>,
    gateway: &GatewayArgs,
) -> Result<Envelope> {
    if gateway.token_stdin && input == Some(std::path::Path::new("-")) {
        return Ok(config_error(
            "--token-stdin and --input - cannot share standard input".to_string(),
        ));
    }
    let arguments = read_json_input(input)?;
    let spec = resolve_gateway_spec(gateway)?;
    let endpoint = spec.endpoint.to_string();
    let request = json!({
        "backend": "mcp_gateway",
        "transport": "streamable_http",
        "endpoint": endpoint,
        "tool": name,
        "arguments": arguments,
    });
    if !apply {
        return Ok(Envelope::ok_with_data(
            format!("MCP Gateway tool '{name}' dry-run"),
            json!({"dry_run": true, "mutating": true, "would_send": request}),
        ));
    }
    confirm::require_tty_confirmation(
        yes,
        &format!("About to invoke MCP Gateway tool '{name}'. Review the input carefully."),
    )?;
    let mut session = GatewaySession::connect(&spec)?;
    let protocol = session.initialize()?;
    let result = session.call_tool(name, arguments)?;
    Ok(Envelope::ok_with_data(
        format!("MCP Gateway tool '{name}' completed"),
        json!({
            "backend": "mcp_gateway",
            "transport": "streamable_http",
            "endpoint": endpoint,
            "protocol": server_info_value(&protocol),
            "tool": name,
            "result": result,
        }),
    ))
}

fn connect_gateway(args: &GatewayArgs) -> Result<(GatewaySession, String)> {
    let spec = resolve_gateway_spec(args)?;
    let endpoint = spec.endpoint.to_string();
    Ok((GatewaySession::connect(&spec)?, endpoint))
}

fn resolve_gateway_spec(args: &GatewayArgs) -> Result<GatewaySpec> {
    if args.timeout_seconds == 0 || args.timeout_seconds > 300 {
        return Err(anyhow!("--timeout-seconds must be between 1 and 300"));
    }
    let endpoint = args
        .endpoint
        .clone()
        .or_else(|| std::env::var("AYX_MCP_GATEWAY_ENDPOINT").ok())
        .ok_or_else(|| anyhow!("MCP Gateway endpoint is required; pass --endpoint URL or set AYX_MCP_GATEWAY_ENDPOINT"))?;
    let endpoint =
        Url::parse(&endpoint).map_err(|error| anyhow!("invalid MCP Gateway endpoint: {error}"))?;
    let token = if args.token_stdin {
        read_bounded_secret_stdin()?
    } else {
        std::env::var(&args.token_env).map_err(|_| {
            anyhow!(
                "MCP Gateway bearer token is missing; set {} or pass --token-stdin",
                args.token_env
            )
        })?
    };
    Ok(GatewaySpec::new(
        endpoint,
        token,
        Duration::from_secs(args.timeout_seconds),
    ))
}

fn read_bounded_secret_stdin() -> Result<String> {
    use std::io::Read;
    let mut token = String::new();
    std::io::stdin()
        .take(64 * 1024 + 1)
        .read_to_string(&mut token)
        .context("failed to read MCP Gateway token from standard input")?;
    if token.len() > 64 * 1024 {
        return Err(anyhow!("MCP Gateway token exceeds the 64 KiB safety limit"));
    }
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(anyhow!("MCP Gateway token cannot be empty"));
    }
    Ok(token)
}

fn execute_tools(command: McpToolsCommand) -> Result<Envelope> {
    let (name, server) = match command {
        McpToolsCommand::List { server } => (None, server),
        McpToolsCommand::Describe { name, server } => (Some(name), server),
    };
    let spec = match resolve_spec(&server) {
        Ok(spec) => spec,
        Err(error) => return Ok(config_error(error.to_string())),
    };
    let path = spec.executable.display().to_string();
    let mut session = McpSession::spawn(&spec)?;
    let protocol = session.initialize()?;
    let tools = session.list_tools()?;

    if let Some(name) = name {
        let Some(tool) = tools.iter().find(|tool| {
            tool.get("name")
                .and_then(Value::as_str)
                .is_some_and(|candidate| candidate == name)
        }) else {
            return Ok(Envelope::err_coded(
                ErrorCode::NotFound,
                format!("MCP tool '{name}' was not found"),
                json!({
                    "backend": "product_mcp",
                    "transport": "stdio",
                "tool": name,
                "tool_families": tool_families(&tools),
                "available_tool_count": tools.len(),
                }),
            ));
        };
        return Ok(Envelope::ok_with_data(
            format!("MCP tool '{name}' described"),
            json!({
                "backend": "product_mcp",
                "transport": "stdio",
                "server": { "executable": path },
                "protocol": server_info_value(&protocol),
                "tool": tool,
            }),
        ));
    }

    Ok(Envelope::ok_with_data(
        "MCP tools listed",
        json!({
            "backend": "product_mcp",
            "transport": "stdio",
            "server": { "executable": path },
            "protocol": server_info_value(&protocol),
            "tool_count": tools.len(),
            "tool_families": tool_families(&tools),
            "tools": tools,
        }),
    ))
}

fn execute_call(
    apply: bool,
    yes: bool,
    name: &str,
    input: Option<&std::path::Path>,
    server: &ServerArgs,
) -> Result<Envelope> {
    let arguments = read_json_input(input)?;
    let spec = match resolve_spec(server) {
        Ok(spec) => spec,
        Err(error) => return Ok(config_error(error.to_string())),
    };
    let request = json!({
        "backend": "product_mcp",
        "transport": "stdio",
        "server": { "executable": spec.executable.display().to_string() },
        "tool": name,
        "arguments": arguments,
    });

    if !apply {
        return Ok(Envelope::ok_with_data(
            format!("MCP tool '{name}' dry-run"),
            json!({
                "dry_run": true,
                "mutating": true,
                "would_send": request,
            }),
        ));
    }

    confirm::require_tty_confirmation(
        yes,
        &format!("About to invoke product MCP tool '{name}'. Review the input carefully."),
    )?;
    let mut session = McpSession::spawn(&spec)?;
    let protocol = session.initialize()?;
    let result = session.call_tool(name, arguments)?;
    Ok(Envelope::ok_with_data(
        format!("MCP tool '{name}' completed"),
        json!({
            "backend": "product_mcp",
            "transport": "stdio",
            "server": { "executable": spec.executable.display().to_string() },
            "protocol": server_info_value(&protocol),
            "tool": name,
            "result": result,
        }),
    ))
}

fn read_json_input(path: Option<&std::path::Path>) -> Result<Value> {
    let Some(path) = path else {
        return Ok(json!({}));
    };
    let content = if path == std::path::Path::new("-") {
        let mut content = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut content)?;
        content
    } else {
        use std::io::Read;
        let file = std::fs::File::open(path)?;
        let mut content = String::new();
        file.take(crate::headless::MAX_MESSAGE_BYTES as u64 + 1)
            .read_to_string(&mut content)?;
        content
    };
    if content.len() > crate::headless::MAX_MESSAGE_BYTES {
        return Err(anyhow!(
            "MCP input exceeds the {} byte safety limit",
            crate::headless::MAX_MESSAGE_BYTES
        ));
    }
    let value: Value = serde_json::from_str(&content)
        .map_err(|error| anyhow!("MCP input is not valid JSON: {error}"))?;
    let bytes = serde_json::to_vec(&value)?;
    if bytes.len() > crate::headless::MAX_MESSAGE_BYTES {
        return Err(anyhow!(
            "MCP input exceeds the {} byte safety limit",
            crate::headless::MAX_MESSAGE_BYTES
        ));
    }
    Ok(value)
}

fn resolve_spec(args: &ServerArgs) -> Result<ServerSpec> {
    if args.timeout_seconds == 0 || args.timeout_seconds > 300 {
        return Err(anyhow!("--timeout-seconds must be between 1 and 300"));
    }
    let executable = args
        .server
        .clone()
        .or_else(|| std::env::var_os("AYX_MCP_SERVER").map(PathBuf::from))
        .ok_or_else(|| {
            anyhow!("MCP server path is required; pass --server PATH or set AYX_MCP_SERVER")
        })?;
    Ok(ServerSpec::new(
        executable,
        args.args.clone(),
        Duration::from_secs(args.timeout_seconds),
    ))
}

fn server_info_value(info: &ServerInfo) -> Value {
    json!({
        "protocol_version": info.protocol_version,
        "server_info": info.server_info,
        "capabilities": info.capabilities,
    })
}

fn config_error(message: String) -> Envelope {
    Envelope::err_coded(
        ErrorCode::ConfigMissing,
        "headless MCP configuration is incomplete",
        json!({
            "error": message,
            "hint": "Pass --server PATH or set AYX_MCP_SERVER to the validated product-owned MCP executable.",
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_server_is_actionable_configuration_error() {
        let error = config_error("missing".to_string());
        assert_eq!(error.error_code, Some(ErrorCode::ConfigMissing));
        assert!(
            error.data["hint"]
                .as_str()
                .unwrap()
                .contains("AYX_MCP_SERVER")
        );
    }

    #[test]
    fn call_is_dry_run_without_apply() {
        let server = ServerArgs {
            server: Some(PathBuf::from("server.exe")),
            args: Vec::new(),
            timeout_seconds: 30,
        };
        let envelope = execute_call(false, false, "alteryx_local.inspect", None, &server)
            .expect("dry-run envelope");
        assert!(envelope.ok);
        assert_eq!(envelope.data["dry_run"], true);
        assert_eq!(envelope.data["would_send"]["tool"], "alteryx_local.inspect");
    }
}
