//! `ayx dashboard` — local read-only operational web dashboard.
//!
//! Server-rendered HTML over axum + maud + htmx. Reuses the same telemetry
//! functions as `ayx telemetry ...` so the CLI and dashboard never drift.
//! Binds to loopback by default; `--allow-remote` is required to accept any
//! non-loopback address (the dashboard has no auth — Alteryx tokens live in
//! process memory).

use anyhow::{anyhow, Context as _, Result};
use ayx_core::envelope::Envelope;
use ayx_core::profile::{list_central_profiles, resolve_runtime_profile, RuntimeProfileResolution};
use clap::Args;
use serde_json::json;
use std::net::{IpAddr, SocketAddr};

pub mod handlers;
pub mod server;
pub mod telemetry_bridge;
pub mod views;

#[derive(Debug, Clone)]
pub(crate) struct DashboardProfileError {
    pub selected_profile: Option<String>,
    pub selection_source: String,
    pub active_profile: Option<String>,
    pub config_home: String,
    pub message: String,
}

impl DashboardProfileError {
    fn to_value(&self) -> serde_json::Value {
        json!({
            "config_home": self.config_home,
            "selected_profile": self.selected_profile,
            "selection_source": self.selection_source,
            "active_profile": self.active_profile,
            "message": self.message,
        })
    }
}

#[derive(Args, Debug, Clone)]
pub struct DashboardCommand {
    /// Central profile name. Defaults to `AYX_PROFILE` or the active central profile.
    #[arg(long)]
    pub profile: Option<String>,
    /// Address to bind. Defaults to 127.0.0.1; non-loopback requires
    /// `--allow-remote`.
    #[arg(long, default_value = "127.0.0.1")]
    pub bind: String,
    /// TCP port.
    #[arg(long, default_value_t = 8765)]
    pub port: u16,
    /// Default telemetry source for panels (overrideable per request via
    /// `?source=...`).
    #[arg(long, value_parser = ["one", "server", "auto"], default_value = "one")]
    pub source: String,
    /// Per-panel polling interval in seconds for htmx auto-refresh.
    #[arg(long, default_value_t = 10)]
    pub poll: u64,
    /// Do not auto-launch the browser on startup.
    #[arg(long)]
    pub no_open: bool,
    /// Allow binding to a non-loopback address. The dashboard has no auth —
    /// only enable on a trusted network.
    #[arg(long)]
    pub allow_remote: bool,
}

pub fn execute(environment: Option<&str>, cmd: DashboardCommand) -> Result<Envelope> {
    let addr = parse_bind(&cmd.bind, cmd.port, cmd.allow_remote)?;
    let available_profiles = list_central_profiles().unwrap_or_default();
    let selected_profile_resolution = match cmd.profile.as_deref() {
        Some(profile) => resolve_runtime_profile(Some(profile)).ok(),
        None => resolve_runtime_profile(None).ok(),
    };
    let selected_profile = selected_profile_resolution
        .as_ref()
        .map(|resolution| resolution.selected_profile.clone())
        .or_else(|| cmd.profile.clone());
    // Probe the profile at startup so the operator sees config issues
    // immediately, but don't fail hard — the dashboard shell, healthz, and
    // static assets render even when telemetry is misconfigured, and each
    // panel surfaces its own error card on request.
    if let Err(e) = crate::load_profile_with_env_lenient(cmd.profile.as_deref(), environment) {
        eprintln!("dashboard: warning — profile failed to load: {e}");
        eprintln!("dashboard: server will start; telemetry panels will report errors until the profile is fixed.");
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("dashboard: failed to start tokio runtime")?;

    let state = server::AppState {
        available_profiles,
        selected_profile,
        profile_resolution: selected_profile_resolution,
        default_source: cmd.source.clone(),
        poll_secs: cmd.poll,
        environment: environment.map(|s| s.to_owned()),
    };

    let url = format!("http://{addr}/");
    eprintln!("ayx dashboard listening on {url}  (Ctrl-C to stop)");
    if !cmd.no_open {
        let _ = open_browser(&url);
    }

    let result = runtime.block_on(server::serve(addr, state));
    result.map_err(|e| anyhow!("dashboard server exited with error: {e}"))?;

    Ok(Envelope::ok_with_data(
        "dashboard server stopped",
        json!({
            "bind": addr.to_string(),
        }),
    ))
}

fn parse_bind(host: &str, port: u16, allow_remote: bool) -> Result<SocketAddr> {
    let ip: IpAddr = host
        .parse()
        .with_context(|| format!("dashboard: invalid --bind address '{host}'"))?;
    if !ip.is_loopback() && !allow_remote {
        return Err(anyhow!(
            "validation: --bind {host} is not loopback; pass --allow-remote to confirm (no auth is enforced)"
        ));
    }
    Ok(SocketAddr::new(ip, port))
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let prog = "open";
    #[cfg(target_os = "windows")]
    let prog = "explorer";
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let prog = "xdg-open";
    std::process::Command::new(prog)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

pub(crate) fn resolve_dashboard_profile(
    state: &server::AppState,
    requested_profile: Option<&str>,
) -> Result<RuntimeProfileResolution, DashboardProfileError> {
    let selected_profile = requested_profile.or(state.selected_profile.as_deref());
    resolve_runtime_profile(selected_profile).map_err(|err| {
        let active_profile = state
            .profile_resolution
            .as_ref()
            .and_then(|resolution| resolution.active_profile.clone());
        DashboardProfileError {
            selected_profile: selected_profile.map(|profile| profile.to_owned()),
            selection_source: selected_profile
                .map(|_| "query".to_string())
                .or_else(|| {
                    state
                        .profile_resolution
                        .as_ref()
                        .map(|resolution| resolution.selection_source.clone())
                })
                .unwrap_or_else(|| "state".to_string()),
            active_profile,
            config_home: ayx_core::profile::ayx_config_home()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "unavailable".to_string()),
            message: err.to_string(),
        }
    })
}
