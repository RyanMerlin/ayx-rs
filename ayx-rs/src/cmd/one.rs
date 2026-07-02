//! Dispatch for `ayx one ...`.
//!
//! The largest single dispatch arm in the original main.rs — ~2000 LOC
//! covering platform / workspace / role / person / token / api / auth /
//! plans / scheduling / billing / flows / connections / connector
//! metadata / job groups / output objects / webhook flow tasks / write
//! settings / doctor / auto-insights / desktop-exec.
//!
//! Each arm is verbatim from the original dispatch, wrapped in
//! `Ok(match command { ... })` so the function returns `Result<Envelope>`.
//! The `load_profile` closure replaces the same-named captured closure
//! in main.rs by delegating to the shared profile loader.

use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_core::profile::Config;
use ayx_one::{api_inventory_envelope, api_status_envelope};

use crate::OneCommand;

/// Returns `true` when the profile has an Alteryx One section but no
/// Server API section. In this case `status`/`inventory` must redirect
/// to `ayx one doctor platform` rather than attempting a Server API call.
fn is_one_only_profile(config: &Config) -> bool {
    config.alteryx_one.is_some() && config.api.is_none()
}

/// Run the default email-OTP login for the currently active profile.
///
/// A thin entry point for `onboard`'s opt-in "log in now" step: it dispatches
/// the same `one platform auth login` a user would run (default OTP flow, no
/// flags), routing through the public platform dispatcher so `onboard` needs no
/// visibility into the private `one_platform` module.
pub(crate) fn run_active_profile_otp_login(environment: Option<&str>) -> Result<Envelope> {
    let runtime = crate::cmd::RuntimeCtx::new(environment);
    let command = Some(crate::OnePlatformCommand::Auth {
        command: crate::OnePlatformAuthCommand::Login {
            profile: None,
            client_id: None,
            browser: false,
            device: false,
            refresh_token: None,
            access_token: None,
            token_endpoint: None,
            workspace_id: None,
            workspace_gid: None,
        },
    });
    // apply/yes are irrelevant to login (it is neither a dry-runnable mutating
    // One API call nor a destructive operation with a TTY confirm).
    super::one_platform::execute(&runtime, false, false, command)
}

/// Borrow Cli's apply + yes for the TTY confirm prompts inside delete arms.
pub struct Ctx<'a> {
    pub apply: bool,
    pub yes: bool,
    pub environment: Option<&'a str>,
}

#[allow(clippy::too_many_lines)]
pub fn execute(cli: Ctx<'_>, command: Option<OneCommand>) -> Result<Envelope> {
    // Capture `environment` up-front so `cli.environment` reads through the
    // helper don't conflict with `cli` itself being borrowed by other arms.
    let environment = cli.environment;
    let runtime = crate::cmd::RuntimeCtx::new(environment);
    macro_rules! load_profile {
        ($profile:expr, $environment:expr) => {
            runtime.load_profile_lenient($profile)
        };
    }
    Ok(match command {
        None => Envelope::ok(
            "one commands available: platform, plans, scheduling, billing, auto-insights, desktop-exec",
        ),
        Some(OneCommand::Doctor { command }) => super::one_doctor::execute(&runtime, command)?,
        Some(OneCommand::Platform { command }) => {
            super::one_platform::execute(&runtime, cli.apply, cli.yes, command)?
        }
        Some(OneCommand::JobGroups { command }) => {
            super::one_job_groups::execute(&runtime, command)?
        }
        Some(OneCommand::OutputObjects { command }) => {
            super::one_output_objects::execute(&runtime, command)?
        }
        Some(OneCommand::WebhookFlowTasks { command }) => {
            super::one_webhook_flow_tasks::execute(&runtime, command)?
        }
        Some(OneCommand::WriteSettings { command }) => {
            super::one_write_settings::execute(&runtime, command)?
        }
        Some(OneCommand::Status { profile }) => {
            let config = load_profile!(profile.as_deref(), environment)?;
            if is_one_only_profile(&config) {
                Envelope::ok(
                    "ayx one status shows Server API status. For Alteryx One profiles, use `ayx one doctor platform` to check auth and connectivity.",
                )
            } else {
                api_status_envelope(&config, "one")?
            }
        }
        Some(OneCommand::Inventory { profile }) => {
            let config = load_profile!(profile.as_deref(), environment)?;
            if is_one_only_profile(&config) {
                Envelope::ok(
                    "ayx one inventory shows Server API inventory. For Alteryx One profiles, use `ayx one doctor platform` to check auth and connectivity.",
                )
            } else {
                api_inventory_envelope(&config, "one")?
            }
        }
        Some(OneCommand::Connections { command }) => {
            super::one_connections::execute(&runtime, cli.apply, cli.yes, command)?
        }
        Some(OneCommand::Flows { command }) => {
            super::one_flows::execute(&runtime, cli.apply, cli.yes, command)?
        }
        Some(OneCommand::Plans { command }) => {
            super::one_plans::execute(&runtime, cli.apply, cli.yes, command)?
        }
        Some(OneCommand::Scheduling { command }) => {
            super::one_scheduling::execute(&runtime, command)?
        }
        Some(OneCommand::Billing { command }) => super::one_billing::execute(&runtime, command)?,
        Some(OneCommand::Ui { command }) => super::one_ui::execute(&runtime, command)?,
        Some(OneCommand::AutoInsights { profile }) => {
            super::one_auto_insights::execute(&runtime, profile)?
        }
        Some(OneCommand::DesktopExec { profile }) => {
            super::one_desktop_exec::execute(&runtime, profile)?
        }
    })
}

#[cfg(test)]
mod tests {
    use ayx_core::profile::{
        AlteryxOneProfile, ApiAuth, ApiAuthMode, ApiProfile, Config, MongoDatabases, MongoMode,
        MongoProfile,
    };

    use super::is_one_only_profile;

    fn base_mongo() -> MongoProfile {
        MongoProfile {
            mode: MongoMode::Embedded,
            databases: MongoDatabases {
                gallery_name: "g".into(),
                service_name: "s".into(),
            },
            embedded: None,
            managed: None,
        }
    }

    fn minimal_config() -> Config {
        Config {
            profile_name: "test".into(),
            mongo: base_mongo(),
            alteryx_one: None,
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    #[test]
    fn one_only_profile_true_when_one_present_and_no_api() {
        let mut config = minimal_config();
        config.alteryx_one = Some(AlteryxOneProfile {
            account_email: "t@e.com".into(),
            ..Default::default()
        });
        assert!(is_one_only_profile(&config));
    }

    #[test]
    fn one_only_profile_false_when_both_one_and_api_present() {
        let mut config = minimal_config();
        config.alteryx_one = Some(AlteryxOneProfile {
            account_email: "t@e.com".into(),
            ..Default::default()
        });
        config.api = Some(ApiProfile {
            base_url: "https://srv".into(),
            auth: ApiAuth {
                mode: ApiAuthMode::Pat,
                pat: Some("tok".into()),
                client_id: None,
                client_secret: None,
                client_secret_ref: None,
                scope: None,
            },
            timeout_ms: None,
            derived: false,
        });
        assert!(!is_one_only_profile(&config));
    }

    #[test]
    fn one_only_profile_false_when_neither_present() {
        let config = minimal_config();
        assert!(!is_one_only_profile(&config));
    }

    #[test]
    fn one_only_profile_false_when_only_api_present() {
        let mut config = minimal_config();
        config.api = Some(ApiProfile {
            base_url: "https://srv".into(),
            auth: ApiAuth {
                mode: ApiAuthMode::Pat,
                pat: Some("tok".into()),
                client_id: None,
                client_secret: None,
                client_secret_ref: None,
                scope: None,
            },
            timeout_ms: None,
            derived: false,
        });
        assert!(!is_one_only_profile(&config));
    }
}
