use anyhow::{bail, Result};
use ayx_core::envelope::Envelope;
use ayx_core::profile::Config;
use serde_json::json;

pub fn api_status_envelope(config: &Config, product: &str) -> Result<Envelope> {
    let api = config
        .api
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("config missing api/server_api section"))?;

    Ok(Envelope::ok_with_data(
        format!("{} api status", product),
        json!({
            "product": product,
            "profile": config.profile_name,
            "base_url": api.base_url,
            "has_credentials": {
                "client_id": api.auth.client_id.as_ref().is_some_and(|v| !v.trim().is_empty()),
                "client_secret": api.auth.client_secret.as_ref().is_some_and(|v| !v.trim().is_empty()),
                "pat": api.auth.pat.as_ref().is_some_and(|v| !v.trim().is_empty()),
            },
            "timeout_ms": api.timeout_ms,
            "message": format!("{} api surface ready", product),
        }),
    ))
}

pub fn api_inventory_envelope(config: &Config, product: &str) -> Result<Envelope> {
    let api = config
        .api
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("config missing api/server_api section"))?;

    Ok(Envelope::ok_with_data(
        format!("{} api inventory", product),
        json!({
            "product": product,
            "profile": config.profile_name,
            "base_url": api.base_url,
            "inventory": [
                "connection posture",
                "auth posture",
                "follow-on command candidates",
            ],
            "message": format!("{} api inventory ready", product),
        }),
    ))
}

pub fn api_diagnose_envelope(config: &Config, product: &str) -> Result<Envelope> {
    let api = config
        .api
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("config missing api/server_api section"))?;

    if api.base_url.trim().is_empty() {
        bail!("{} api base_url cannot be empty", product);
    }

    let normalized = if api.base_url.ends_with('/') {
        api.base_url.clone()
    } else {
        format!("{}/", api.base_url)
    };

    Ok(Envelope::ok_with_data(
        format!("{} api diagnose", product),
        json!({
            "product": product,
            "profile": config.profile_name,
            "base_url": normalized,
            "auth_mode": format!("{:?}", api.auth.mode),
            "checks": [
                "base URL present",
                "credential fields present",
                "token acquisition should be attempted by the CLI caller when a live endpoint is available"
            ],
            "next_step": format!("wire {}-specific reachability and endpoint checks once the API surface is defined", product),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{
        ApiAuth, ApiAuthMode, ApiProfile, Config, MongoDatabases, MongoEmbedded, MongoMode,
        MongoProfile, ServerApiProfile,
    };

    fn config() -> Config {
        Config {
            profile_name: "test".to_string(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "AlteryxGallery".to_string(),
                    service_name: "AlteryxService".to_string(),
                },
                embedded: Some(MongoEmbedded {
                    runtime_settings_path: None,
                    alteryx_service_path: None,
                    restore_target_path: None,
                }),
                managed: None,
            },
            alteryx_one: None,
            observability: None,
            server_api: Some(ServerApiProfile {
                base_url: "http://localhost/webapi/".to_string(),
                client_id: "cid".to_string(),
                client_secret: "secret".to_string(),
            }),
            api: Some(ApiProfile {
                base_url: "http://localhost/webapi/".to_string(),
                auth: ApiAuth {
                    mode: ApiAuthMode::Oauth2ClientCredentials,
                    pat: None,
                    client_id: Some("cid".to_string()),
                    client_secret: Some("secret".to_string()),
                    scope: None,
                },
                timeout_ms: Some(15000),
            }),
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    #[test]
    fn status_envelope_includes_base_url_and_product() {
        let env = api_status_envelope(&config(), "one").expect("status envelope");
        assert!(env.ok);
        assert_eq!(env.data["product"], "one");
        assert_eq!(env.data["base_url"], "http://localhost/webapi/");
    }

    #[test]
    fn inventory_envelope_includes_product_and_inventory() {
        let env = api_inventory_envelope(&config(), "one").expect("inventory envelope");
        assert!(env.ok);
        assert_eq!(env.data["product"], "one");
        assert!(env.data["inventory"].is_array());
    }

    #[test]
    fn diagnose_envelope_includes_checks_and_next_step() {
        let env = api_diagnose_envelope(&config(), "one").expect("diagnose envelope");
        assert!(env.ok);
        assert_eq!(env.data["product"], "one");
        assert!(env.data["checks"].is_array());
        assert!(env.data["next_step"].is_string());
    }

    #[test]
    fn status_envelope_can_be_reused_for_license() {
        let env = api_status_envelope(&config(), "license").expect("status envelope");
        assert!(env.ok);
        assert_eq!(env.data["product"], "license");
    }
}
