//! Context header data: the always-visible "where am I" strip
//! (Profile · Workspace · User). Derived from the loaded Config.

use ayx_core::profile::Config;

#[derive(Debug, Clone)]
pub struct Context {
    pub profile: String,
    pub workspace: String,
    pub user: String,
}

impl Context {
    pub fn from_config(config: &Config, active_profile: Option<&str>) -> Self {
        let one = config.alteryx_one.as_ref();
        let workspace = one
            .and_then(|profile| profile.active_workspace_id())
            .map(str::to_string)
            .unwrap_or_else(|| "(no workspace)".to_string());
        let user = one
            .map(|profile| profile.account_email.as_str())
            .filter(|email| !email.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "(no identity)".to_string());

        Self {
            profile: active_profile.unwrap_or("(none)").to_string(),
            workspace,
            user,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{AlteryxOneProfile, Config};
    use std::collections::BTreeMap;

    #[test]
    fn context_reads_profile_workspace_user() {
        let mut config = Config {
            profile_name: String::new(),
            mongo: Default::default(),
            alteryx_one: None,
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        };
        config.alteryx_one = Some(AlteryxOneProfile {
            account_email: "ryan@alteryx.com".into(),
            expected_workspace_id: Some("w_marketing".into()),
            workspace_credentials: BTreeMap::from([("w_marketing".into(), Default::default())]),
            ..Default::default()
        });

        let ctx = Context::from_config(&config, Some("wyatt"));
        assert_eq!(ctx.profile, "wyatt");
        assert_eq!(ctx.workspace, "w_marketing");
        assert_eq!(ctx.user, "ryan@alteryx.com");
    }

    #[test]
    fn context_degrades_gracefully_without_one_profile() {
        let config = Config {
            profile_name: String::new(),
            mongo: Default::default(),
            alteryx_one: None,
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        };
        let ctx = Context::from_config(&config, None);
        assert_eq!(ctx.profile, "(none)");
        assert_eq!(ctx.workspace, "(no workspace)");
        assert_eq!(ctx.user, "(no identity)");
    }
}
