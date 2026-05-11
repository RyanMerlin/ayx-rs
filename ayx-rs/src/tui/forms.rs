//! Form-related helpers extracted from `tui/app.rs`.
//!
//! These functions translate between the `Config`-shaped profile types and
//! the flat `ConfigFieldState` arrays the TUI uses for rendering and
//! editing. Pulling them into their own module keeps `app.rs` focused on
//! the event loop and state mutations rather than the long pile of
//! per-field defaults / parsers.
//!
//! Every function here is `pub(super)` so the App can call into them
//! without exposing them outside the tui module.

use anyhow::{anyhow, Result};
use ayx_core::definitions::DEFAULT_RUNTIME_SETTINGS_PATH;
use ayx_core::profile::{
    ApiProfile, Config, MongoEmbedded, MongoManaged, MongoProfile, ServerApiProfile, ServerProfile,
    SqlServerConnectionProfile, SqlServerProfile, TlsConfig,
};

use super::app::{ConfigFieldKind, ConfigFieldState};

pub(super) fn api_profile_to_server_api_ref(api: &ApiProfile) -> Option<ServerApiProfile> {
    let client_id = api.auth.client_id.as_ref()?.clone();
    let client_secret = api.auth.client_secret.as_ref()?.clone();
    Some(ServerApiProfile {
        base_url: api.base_url.clone(),
        client_id,
        client_secret,
    })
}

pub(super) fn server_profile_to_server_api_ref(server: &ServerProfile) -> Option<ServerApiProfile> {
    Some(ServerApiProfile {
        base_url: server.webapi_url.clone(),
        client_id: server.curator_api_key.clone(),
        client_secret: server.curator_api_secret.clone(),
    })
}

#[allow(clippy::type_complexity)]
pub(super) fn mongo_values(
    mongo: &MongoProfile,
) -> (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
) {
    let embedded = mongo.embedded.as_ref();
    let managed = mongo.managed.as_ref();
    (
        embedded
            .and_then(|value| value.runtime_settings_path.clone())
            .unwrap_or_default(),
        embedded
            .and_then(|value| value.alteryx_service_path.clone())
            .unwrap_or_default(),
        embedded
            .and_then(|value| value.restore_target_path.clone())
            .unwrap_or_default(),
        managed
            .and_then(|value| value.url.clone())
            .unwrap_or_default(),
        managed
            .and_then(|value| value.host.clone())
            .unwrap_or_default(),
        managed
            .map(|value| value.port.to_string())
            .unwrap_or_default(),
        managed
            .and_then(|value| value.auth_database.clone())
            .unwrap_or_default(),
        managed
            .and_then(|value| value.username.clone())
            .unwrap_or_default(),
        managed
            .and_then(|value| value.password.clone())
            .unwrap_or_default(),
        managed
            .map(|value| value.tls.enabled.to_string())
            .unwrap_or_else(|| "false".to_string()),
        managed
            .and_then(|value| value.tls.ca_path.clone())
            .unwrap_or_default(),
        managed
            .and_then(|value| value.tls.cert_path.clone())
            .unwrap_or_default(),
        managed
            .and_then(|value| value.tls.key_path.clone())
            .unwrap_or_default(),
        managed
            .and_then(|value| value.tls.allow_invalid_hostnames)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "false".to_string()),
        managed
            .and_then(|value| value.timeout_ms)
            .map(|value| value.to_string())
            .unwrap_or_default(),
        managed
            .and_then(|value| value.retry_count)
            .map(|value| value.to_string())
            .unwrap_or_default(),
        managed
            .and_then(|value| value.max_pool_size)
            .map(|value| value.to_string())
            .unwrap_or_default(),
    )
}

pub(super) fn sqlserver_fields(config: &Config) -> Vec<ConfigFieldState> {
    let sql = config.sqlserver.as_ref();
    let controller = sql.and_then(|value| value.controller.as_ref());
    let server_ui = sql.and_then(|value| value.server_ui.as_ref());

    vec![
        ConfigFieldState {
            label: "Controller Host",
            value: controller
                .and_then(|value| value.host.clone())
                .unwrap_or_default(),
            placeholder: "localhost",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Controller Port",
            value: controller
                .and_then(|value| value.port.map(|port| port.to_string()))
                .unwrap_or_default(),
            placeholder: "1433",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Controller Database",
            value: controller
                .and_then(|value| value.database.clone())
                .unwrap_or_default(),
            placeholder: "AlteryxService",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Controller Username",
            value: controller
                .and_then(|value| value.username.clone())
                .unwrap_or_default(),
            placeholder: "sa",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Controller Password",
            value: controller
                .and_then(|value| value.password.clone())
                .unwrap_or_default(),
            placeholder: "stored in keyring on save",
            kind: ConfigFieldKind::Text,
            secret: true,
        },
        ConfigFieldState {
            label: "Controller Integrated",
            value: controller
                .and_then(|value| value.integrated_security)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "Controller Encrypt",
            value: controller
                .and_then(|value| value.encrypt)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "Controller Trust Cert",
            value: controller
                .and_then(|value| value.trust_server_certificate)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "Controller MultiSubnet",
            value: controller
                .and_then(|value| value.multi_subnet_failover)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "Controller Conn Str",
            value: controller
                .and_then(|value| value.connection_string.clone())
                .unwrap_or_default(),
            placeholder: "connection string",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Server UI Host",
            value: server_ui
                .and_then(|value| value.host.clone())
                .unwrap_or_default(),
            placeholder: "localhost",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Server UI Port",
            value: server_ui
                .and_then(|value| value.port.map(|port| port.to_string()))
                .unwrap_or_default(),
            placeholder: "1433",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Server UI Database",
            value: server_ui
                .and_then(|value| value.database.clone())
                .unwrap_or_default(),
            placeholder: "AlteryxServerUI",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Server UI Username",
            value: server_ui
                .and_then(|value| value.username.clone())
                .unwrap_or_default(),
            placeholder: "sa",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Server UI Password",
            value: server_ui
                .and_then(|value| value.password.clone())
                .unwrap_or_default(),
            placeholder: "stored in keyring on save",
            kind: ConfigFieldKind::Text,
            secret: true,
        },
        ConfigFieldState {
            label: "Server UI Integrated",
            value: server_ui
                .and_then(|value| value.integrated_security)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "Server UI Encrypt",
            value: server_ui
                .and_then(|value| value.encrypt)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "Server UI Trust Cert",
            value: server_ui
                .and_then(|value| value.trust_server_certificate)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "Server UI MultiSubnet",
            value: server_ui
                .and_then(|value| value.multi_subnet_failover)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "Server UI Conn Str",
            value: server_ui
                .and_then(|value| value.connection_string.clone())
                .unwrap_or_default(),
            placeholder: "connection string",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Legacy Conn Str",
            value: sql
                .and_then(|value| value.legacy_connection_string.clone())
                .unwrap_or_default(),
            placeholder: "legacy connection string",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
    ]
}

pub(super) fn observability_fields(config: &Config) -> Vec<ConfigFieldState> {
    let api_logging = config
        .observability
        .as_ref()
        .and_then(|value| value.api_logging.as_ref());
    vec![
        ConfigFieldState {
            label: "API Logging Enabled",
            value: api_logging
                .map(|value| value.enabled.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "API Logging Path",
            value: api_logging
                .and_then(|value| value.path.clone())
                .unwrap_or_default(),
            placeholder: "/path/to/logs",
            kind: ConfigFieldKind::Text,
            secret: false,
        },
        ConfigFieldState {
            label: "Redact Bodies",
            value: api_logging
                .and_then(|value| value.redact_bodies)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "Log Requests",
            value: api_logging
                .and_then(|value| value.log_requests)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
        ConfigFieldState {
            label: "Log Responses",
            value: api_logging
                .and_then(|value| value.log_responses)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "false".to_string()),
            placeholder: "true|false",
            kind: ConfigFieldKind::Bool,
            secret: false,
        },
    ]
}

pub(super) fn normalize_server_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{trimmed}/")
    }
}

pub(super) fn field_value<'a>(fields: &'a [ConfigFieldState], label: &str) -> &'a str {
    fields
        .iter()
        .find(|field| field.label == label)
        .map(|field| field.value.as_str())
        .unwrap_or("")
}

pub(super) fn parse_u16_field(
    fields: &[ConfigFieldState],
    label: &str,
    default: u16,
) -> Result<u16> {
    let value = field_value(fields, label).trim();
    if value.is_empty() {
        return Ok(default);
    }
    value
        .parse::<u16>()
        .map_err(|_| anyhow!("expected a whole number for {label}"))
}

pub(super) fn parse_u32_field(fields: &[ConfigFieldState], label: &str) -> Result<Option<u32>> {
    let value = field_value(fields, label).trim();
    if value.is_empty() {
        return Ok(None);
    }
    value
        .parse::<u32>()
        .map(Some)
        .map_err(|_| anyhow!("expected a whole number for {label}"))
}

pub(super) fn parse_u64_field(fields: &[ConfigFieldState], label: &str) -> Result<Option<u64>> {
    let value = field_value(fields, label).trim();
    if value.is_empty() {
        return Ok(None);
    }
    value
        .parse::<u64>()
        .map(Some)
        .map_err(|_| anyhow!("expected a whole number for {label}"))
}

pub(super) fn parse_optional_text_field(
    fields: &[ConfigFieldState],
    label: &str,
) -> Option<String> {
    let value = field_value(fields, label).trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub(super) fn parse_bool_field(value: &str, default: bool) -> Result<bool> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(default);
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "true" | "yes" | "y" | "1" | "on" => Ok(true),
        "false" | "no" | "n" | "0" | "off" => Ok(false),
        _ => Err(anyhow!("expected true/false value")),
    }
}

pub(super) fn default_mongo_embedded() -> MongoEmbedded {
    MongoEmbedded {
        runtime_settings_path: Some(DEFAULT_RUNTIME_SETTINGS_PATH.to_string()),
        alteryx_service_path: None,
        restore_target_path: None,
    }
}

pub(super) fn default_mongo_managed() -> MongoManaged {
    MongoManaged {
        url: None,
        host: None,
        port: 27017,
        auth_database: None,
        username: None,
        password: None,
        password_ref: None,
        tls: TlsConfig {
            enabled: false,
            ca_path: None,
            cert_path: None,
            key_path: None,
            allow_invalid_hostnames: None,
        },
        timeout_ms: None,
        retry_count: None,
        max_pool_size: None,
    }
}

pub(super) fn default_sqlserver_profile() -> SqlServerProfile {
    SqlServerProfile {
        controller: Some(SqlServerConnectionProfile {
            connection_string: None,
            host: Some("localhost".to_string()),
            port: Some(1433),
            database: Some("AlteryxService".to_string()),
            username: Some("sa".to_string()),
            password: None,
            password_ref: None,
            password_env: Some("AYX_SQL_CONTROLLER_PASSWORD".to_string()),
            integrated_security: Some(true),
            encrypt: Some(true),
            trust_server_certificate: Some(false),
            multi_subnet_failover: Some(false),
        }),
        server_ui: Some(SqlServerConnectionProfile {
            connection_string: None,
            host: Some("localhost".to_string()),
            port: Some(1433),
            database: Some("AlteryxServerUI".to_string()),
            username: Some("sa".to_string()),
            password: None,
            password_ref: None,
            password_env: Some("AYX_SQL_SERVER_UI_PASSWORD".to_string()),
            integrated_security: Some(false),
            encrypt: Some(true),
            trust_server_certificate: Some(false),
            multi_subnet_failover: Some(false),
        }),
        legacy_connection_string: None,
    }
}

pub(super) fn update_sql_connection(
    existing: Option<SqlServerConnectionProfile>,
    label: &str,
    password_env: &str,
    fields: &[ConfigFieldState],
) -> Result<SqlServerConnectionProfile> {
    let mut conn = existing.unwrap_or_else(|| SqlServerConnectionProfile {
        connection_string: None,
        host: None,
        port: None,
        database: None,
        username: None,
        password: None,
        password_ref: None,
        password_env: Some(password_env.to_string()),
        integrated_security: Some(false),
        encrypt: Some(true),
        trust_server_certificate: Some(false),
        multi_subnet_failover: Some(false),
    });
    conn.host = parse_optional_text_field(fields, &format!("{label} Host"));
    conn.port = Some(parse_u16_field(
        fields,
        &format!("{label} Port"),
        conn.port.unwrap_or(1433),
    )?);
    conn.database = parse_optional_text_field(fields, &format!("{label} Database"));
    conn.username = parse_optional_text_field(fields, &format!("{label} Username"));
    conn.password = parse_optional_text_field(fields, &format!("{label} Password"));
    conn.integrated_security = Some(parse_bool_field(
        field_value(fields, &format!("{label} Integrated")),
        conn.integrated_security.unwrap_or(false),
    )?);
    conn.encrypt = Some(parse_bool_field(
        field_value(fields, &format!("{label} Encrypt")),
        conn.encrypt.unwrap_or(true),
    )?);
    conn.trust_server_certificate = Some(parse_bool_field(
        field_value(fields, &format!("{label} Trust Cert")),
        conn.trust_server_certificate.unwrap_or(false),
    )?);
    conn.multi_subnet_failover = Some(parse_bool_field(
        field_value(fields, &format!("{label} MultiSubnet")),
        conn.multi_subnet_failover.unwrap_or(false),
    )?);
    conn.connection_string = parse_optional_text_field(fields, &format!("{label} Conn Str"));
    if conn.password_env.is_none() {
        conn.password_env = Some(password_env.to_string());
    }
    Ok(conn)
}

pub(super) fn default_server_profile() -> ServerProfile {
    ServerProfile {
        webapi_url: "http://localhost/".to_string(),
        curator_api_key: String::new(),
        curator_api_secret: String::new(),
        curator_api_secret_ref: None,
        verify_tls: Some(true),
    }
}
