//! `--source one|server|auto` resolution.
//!
//! Picks which backend a telemetry subcommand should target based on which
//! surfaces the loaded profile defines and an optional explicit override.
//! When both surfaces are configured and no override is supplied we refuse
//! to guess — the operator gets a Validation error and a hint instead of a
//! surprise.

use anyhow::{Result, anyhow};
use ayx_core::profile::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetrySource {
    One,
    Server,
}

impl TelemetrySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            TelemetrySource::One => "one",
            TelemetrySource::Server => "server",
        }
    }
}

pub fn pick(config: &Config, override_: Option<&str>) -> Result<TelemetrySource> {
    let has_one = config.alteryx_one.is_some();
    let has_server = config.server.is_some();

    match override_.map(|s| s.to_ascii_lowercase()) {
        Some(s) if s == "one" => {
            if !has_one {
                return Err(anyhow!(
                    "validation: --source one requested but profile has no `alteryx_one` block"
                ));
            }
            Ok(TelemetrySource::One)
        }
        Some(s) if s == "server" => {
            if !has_server {
                return Err(anyhow!(
                    "validation: --source server requested but profile has no `server` block"
                ));
            }
            Ok(TelemetrySource::Server)
        }
        Some(s) if s == "auto" || s.is_empty() => pick_auto(has_one, has_server),
        Some(s) => Err(anyhow!(
            "validation: unknown --source value '{s}'; expected one of: one, server, auto"
        )),
        None => pick_auto(has_one, has_server),
    }
}

fn pick_auto(has_one: bool, has_server: bool) -> Result<TelemetrySource> {
    match (has_one, has_server) {
        (true, false) => Ok(TelemetrySource::One),
        (false, true) => Ok(TelemetrySource::Server),
        (true, true) => Err(anyhow!(
            "validation: profile has both `alteryx_one` and `server` blocks; pass --source one or --source server"
        )),
        (false, false) => Err(anyhow!(
            "config missing: no telemetry source configured; profile must define `alteryx_one` or `server` (run 'ayx onboard')"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{
        AlteryxOneProfile, MongoDatabases, MongoMode, MongoProfile, ServerProfile,
    };

    fn base() -> Config {
        Config {
            profile_name: "test".into(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "g".into(),
                    service_name: "s".into(),
                },
                embedded: None,
                managed: None,
            },
            alteryx_one: None,
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    fn cfg(one: bool, server: bool) -> Config {
        let mut c = base();
        if one {
            c.alteryx_one = Some(AlteryxOneProfile {
                account_email: "t@e.com".into(),
                base_url: None,
                oauth_client_id: None,
                client_secret: None,
                client_secret_ref: None,
                token_endpoint_url: None,
                access_token: None,
                access_token_ref: None,
                refresh_token: None,
                refresh_token_ref: None,
                workspace_credentials: Default::default(),
                expected_workspace_id: None,
                ..Default::default()
            });
        }
        if server {
            c.server = Some(ServerProfile {
                webapi_url: "http://srv".into(),
                curator_api_key: "k".into(),
                curator_api_secret: "s".into(),
                curator_api_secret_ref: None,
                verify_tls: None,
            });
        }
        c
    }

    #[test]
    fn auto_picks_only_configured_surface() {
        assert_eq!(pick(&cfg(true, false), None).unwrap(), TelemetrySource::One);
        assert_eq!(
            pick(&cfg(false, true), None).unwrap(),
            TelemetrySource::Server
        );
    }

    #[test]
    fn auto_rejects_when_both_present() {
        let err = pick(&cfg(true, true), None).unwrap_err();
        assert!(format!("{err}").contains("--source"));
    }

    #[test]
    fn auto_rejects_when_neither_present() {
        assert!(pick(&cfg(false, false), None).is_err());
    }

    #[test]
    fn explicit_override_validates_surface_present() {
        assert!(pick(&cfg(true, true), Some("one")).is_ok());
        assert!(pick(&cfg(true, true), Some("server")).is_ok());
        assert!(pick(&cfg(false, true), Some("one")).is_err());
        assert!(pick(&cfg(true, false), Some("server")).is_err());
    }

    #[test]
    fn unknown_override_rejected() {
        assert!(pick(&cfg(true, false), Some("nonsense")).is_err());
    }
}
