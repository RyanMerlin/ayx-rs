use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use ayx_core::profile::{Config, MongoMode, SqlServerConnectionProfile, SqlServerProfile};

pub fn status_envelope(config: &Config) -> Result<Value> {
    Ok(json!({
        "profile": config.profile_name,
        "configured": config.sqlserver.is_some(),
        "backend": config.sqlserver.as_ref().map(summarize_sql_profile),
        "mongo_mode": match config.mongo.mode {
            MongoMode::Embedded => "embedded",
            MongoMode::Managed => "managed",
        },
    }))
}

pub fn inventory_envelope(config: &Config) -> Result<Value> {
    let sql = sql_profile(config)?;
    let controller = sql
        .controller
        .as_ref()
        .ok_or_else(|| anyhow!("sqlserver.controller is missing"))?;
    let server_ui = sql
        .server_ui
        .as_ref()
        .ok_or_else(|| anyhow!("sqlserver.server_ui is missing"))?;

    Ok(json!({
        "profile": config.profile_name,
        "database_names": {
            "controller": controller.database.as_deref().unwrap_or("AlteryxService"),
            "server_ui": server_ui.database.as_deref().unwrap_or("AlteryxServerUI"),
        },
        "connection_profile": summarize_sql_profile(sql),
        "recommended_separation": {
            "controller_and_server_ui_should_use_separate_databases": true,
        }
    }))
}

pub fn precheck_envelope(config: &Config, collation: Option<&str>) -> Result<Value> {
    let sql = sql_profile(config)?;
    let controller = sql
        .controller
        .as_ref()
        .ok_or_else(|| anyhow!("sqlserver.controller is missing"))?;
    let collation = collation.unwrap_or("Latin1_General_100_CI_AS_SC_UTF8");
    let collation_ok = is_collation_acceptable(collation);

    Ok(json!({
        "profile": config.profile_name,
        "collation": {
            "value": collation,
            "acceptable": collation_ok,
            "notes": [
                "Collation is a critical precheck because Alteryx Server SQL DB setups are sensitive to it",
                "Prefer a case-insensitive, accent-sensitive collation unless your environment standardizes differently"
            ]
        },
        "connection": summarize_sql_connection(controller),
        "supported_sql_versions": ["2019", "2022", "Amazon RDS for SQL Server"],
        "checklist": [
            "SQL Server reachable from the Alteryx host",
            "ODBC Driver 17 installed on the Alteryx host",
            "Separate controller and Server UI databases are recommended",
            "Database account can create and update schemas",
            "Encryption and trust settings are decided explicitly",
            "MultiSubnetFailover=True when using a clustered SQL Server",
            "Server UI connection string must not specify a Driver",
        ],
        "issues": collect_precheck_issues(controller, collation),
    }))
}

pub fn validate_connection_strings_envelope(config: &Config) -> Result<Value> {
    let sql = sql_profile(config)?;
    Ok(json!({
        "profile": config.profile_name,
        "controller": validate_connection_profile("sqlserver.controller", sql.controller.as_ref())?,
        "server_ui": validate_connection_profile("sqlserver.server_ui", sql.server_ui.as_ref())?,
    }))
}

#[allow(clippy::too_many_arguments)]
pub fn connection_string_envelope(
    config: &Config,
    scope: &str,
    auth: &str,
    server: Option<&str>,
    database: Option<&str>,
    port: Option<u16>,
    encrypt: bool,
    trust_server_certificate: bool,
    multi_subnet_failover: bool,
) -> Result<Value> {
    let sql = sql_profile(config)?;
    let scope = scope.to_lowercase();
    let auth = auth.to_lowercase();
    let conn = match scope.as_str() {
        "controller" => sql
            .controller
            .as_ref()
            .ok_or_else(|| anyhow!("sqlserver.controller is missing"))?,
        "server-ui" => sql
            .server_ui
            .as_ref()
            .ok_or_else(|| anyhow!("sqlserver.server_ui is missing"))?,
        _ => return Err(anyhow!("scope must be controller or server-ui")),
    };
    let server = server
        .map(ToOwned::to_owned)
        .or_else(|| conn.host.clone())
        .unwrap_or_else(|| "localhost".to_string());
    let port = port.or(conn.port).unwrap_or(1433);
    let default_database = match scope.as_str() {
        "controller" => "AlteryxService",
        "server-ui" => "AlteryxServerUI",
        _ => unreachable!(),
    };
    let database = database
        .map(ToOwned::to_owned)
        .or_else(|| conn.database.clone())
        .unwrap_or_else(|| default_database.to_string());

    let connection_string = match (scope.as_str(), auth.as_str()) {
        ("controller", "sql") => build_controller_sql_auth(
            &server,
            port,
            &database,
            encrypt,
            trust_server_certificate,
            multi_subnet_failover,
        ),
        ("controller", "kerberos") => build_controller_kerberos(
            &server,
            port,
            &database,
            encrypt,
            trust_server_certificate,
            multi_subnet_failover,
        ),
        ("server-ui", "sql") => build_server_ui_sql_auth(
            &server,
            port,
            &database,
            encrypt,
            trust_server_certificate,
            multi_subnet_failover,
        ),
        ("server-ui", "kerberos") => build_server_ui_kerberos(
            &server,
            port,
            &database,
            encrypt,
            trust_server_certificate,
            multi_subnet_failover,
        ),
        _ => return Err(anyhow!("auth must be sql or kerberos")),
    };

    Ok(json!({
        "profile": config.profile_name,
        "scope": scope,
        "auth": auth,
        "connection_string": connection_string,
        "notes": [
            "Controller persistence includes Driver={ODBC Driver 17 for SQL Server}",
            "Server UI persistence should not specify the Driver and will add MARS automatically",
            "Do not add a MARS flag manually to the Server UI string",
        ]
    }))
}

pub fn migration_prepare_envelope(
    config: &Config,
    target_version: Option<&str>,
    dry_run: bool,
) -> Result<Value> {
    let sql = sql_profile(config)?;
    let controller = sql
        .controller
        .as_ref()
        .ok_or_else(|| anyhow!("sqlserver.controller is missing"))?;
    let server_ui = sql
        .server_ui
        .as_ref()
        .ok_or_else(|| anyhow!("sqlserver.server_ui is missing"))?;

    Ok(json!({
        "profile": config.profile_name,
        "dry_run": dry_run,
        "target_version": target_version,
        "plan": {
            "sql_supported_versions": ["2019", "2022", "Amazon RDS for SQL Server"],
            "check_collation": true,
            "validate_connectivity": true,
            "verify_separate_databases": true,
            "service_must_run_on_mongo_before_final_migration": true,
            "generate_connection_strings": {
                "controller": build_controller_sql_auth(
                    controller.host.as_deref().unwrap_or("localhost"),
                    controller.port.unwrap_or(1433),
                    controller.database.as_deref().unwrap_or("AlteryxService"),
                    controller.encrypt.unwrap_or(true),
                    controller.trust_server_certificate.unwrap_or(false),
                    controller.multi_subnet_failover.unwrap_or(false),
                ),
                "server_ui": build_server_ui_sql_auth(
                    server_ui.host.as_deref().unwrap_or("localhost"),
                    server_ui.port.unwrap_or(1433),
                    server_ui.database.as_deref().unwrap_or("AlteryxServerUI"),
                    server_ui.encrypt.unwrap_or(true),
                    server_ui.trust_server_certificate.unwrap_or(false),
                    server_ui.multi_subnet_failover.unwrap_or(false),
                ),
            },
            "review_migration_guide": "Mongo to SQL Migration Guide",
        },
        "next_steps": [
            "Confirm SQL Server collation before schema creation",
            "Create controller and Server UI databases",
            "Create or update the env-backed secret entries",
            "Apply the connection strings in System Settings",
            "Run the migration workflow after validation",
        ],
    }))
}

fn sql_profile(config: &Config) -> Result<&SqlServerProfile> {
    config
        .sqlserver
        .as_ref()
        .ok_or_else(|| anyhow!("sqlserver profile is missing; run ayx onboard first"))
}

fn summarize_sql_profile(sql: &SqlServerProfile) -> Value {
    json!({
        "controller": sql.controller.as_ref().map(summarize_sql_connection),
        "server_ui": sql.server_ui.as_ref().map(summarize_sql_connection),
        "legacy_connection_string": sql.legacy_connection_string.as_ref().map(|_| "stored"),
    })
}

fn summarize_sql_connection(conn: &SqlServerConnectionProfile) -> Value {
    json!({
        "connection_string": conn.connection_string.as_ref().map(|_| "stored"),
        "host": conn.host,
        "port": conn.port,
        "database": conn.database,
        "username": conn.username,
        "password": conn.password.as_ref().map(|_| "stored"),
        "password_env": conn.password_env,
        "integrated_security": conn.integrated_security,
        "encrypt": conn.encrypt,
        "trust_server_certificate": conn.trust_server_certificate,
        "multi_subnet_failover": conn.multi_subnet_failover,
    })
}

fn validate_connection_profile(
    field: &str,
    conn: Option<&SqlServerConnectionProfile>,
) -> Result<Value> {
    let conn = conn.ok_or_else(|| anyhow!("{field} is missing"))?;
    Ok(json!({
        "field": field,
        "host": conn.host,
        "port": conn.port,
        "database": conn.database,
        "integrated_security": conn.integrated_security,
        "encrypt": conn.encrypt,
        "trust_server_certificate": conn.trust_server_certificate,
        "multi_subnet_failover": conn.multi_subnet_failover,
        "secret_source": conn
            .password_env
            .as_deref()
            .or_else(|| conn.password.as_ref().map(|_| "yaml")),
        "connection_string_present": conn.connection_string.is_some(),
    }))
}

fn collect_precheck_issues(sql: &SqlServerConnectionProfile, collation: &str) -> Vec<Value> {
    let mut issues = Vec::new();
    if !is_collation_acceptable(collation) {
        issues.push(json!({
            "check": "collation",
            "severity": "high",
            "message": format!("Collation '{collation}' is not in the preferred SQL Server set for Alteryx Server"),
        }));
    }
    if sql.host.as_deref().unwrap_or("").trim().is_empty() {
        issues.push(json!({"check": "host", "severity": "high", "message": "SQL host is missing"}));
    }
    if sql.port.unwrap_or(0) == 0 {
        issues.push(json!({"check": "port", "severity": "high", "message": "SQL port must be greater than zero"}));
    }
    if sql.database.as_deref().unwrap_or("").trim().is_empty() {
        issues.push(json!({"check": "database", "severity": "high", "message": "SQL database name is missing"}));
    }
    issues.push(json!({
        "check": "db-separation",
        "severity": "info",
        "message": "Separate controller and Server UI databases are recommended, but not strictly required"
    }));
    issues.push(json!({
        "check": "secret-storage",
        "severity": "info",
        "message": "Passwords should be stored via env placeholders when possible"
    }));
    issues
}

fn is_collation_acceptable(collation: &str) -> bool {
    let normalized = collation.trim().to_ascii_lowercase();
    normalized == "latin1_general_100_ci_as_sc_utf8"
        || normalized == "latin1_general_100_ci_as_sc"
        || normalized == "sql_latin1_general_cp1_ci_as"
}

fn build_controller_sql_auth(
    server: &str,
    port: u16,
    database: &str,
    encrypt: bool,
    trust_server_certificate: bool,
    multi_subnet_failover: bool,
) -> String {
    let mut parts = vec![
        "Driver={ODBC Driver 17 for SQL Server}".to_string(),
        format!("Server=tcp:{server},{port}"),
        "Integrated Security=False".to_string(),
        format!("Database={database}"),
        "UId=[user]".to_string(),
        "PWD=[user password]".to_string(),
    ];
    if encrypt {
        parts.push("Encrypt=yes".to_string());
    }
    if trust_server_certificate {
        parts.push("TrustServerCertificate=yes".to_string());
    }
    if multi_subnet_failover {
        parts.push("MultiSubnetFailover=True".to_string());
    }
    parts.join(";") + ";"
}

fn build_controller_kerberos(
    server: &str,
    port: u16,
    database: &str,
    encrypt: bool,
    trust_server_certificate: bool,
    multi_subnet_failover: bool,
) -> String {
    let mut parts = vec![
        "Driver={ODBC Driver 17 for SQL Server}".to_string(),
        format!("Server=tcp:{server},{port}"),
        "Trusted_Connection=yes".to_string(),
        format!("Database={database}"),
    ];
    if encrypt {
        parts.push("Encrypt=yes".to_string());
    }
    if trust_server_certificate {
        parts.push("TrustServerCertificate=yes".to_string());
    }
    if multi_subnet_failover {
        parts.push("MultiSubnetFailover=True".to_string());
    }
    parts.join(";") + ";"
}

fn build_server_ui_sql_auth(
    server: &str,
    port: u16,
    database: &str,
    encrypt: bool,
    trust_server_certificate: bool,
    multi_subnet_failover: bool,
) -> String {
    let mut parts = vec![
        format!("Server={server},{port}"),
        format!("Database={database}"),
        "User Id=[user]".to_string(),
        "Password=[user password]".to_string(),
        "MultipleActiveResultSets=True".to_string(),
    ];
    if encrypt {
        parts.push("Encrypt=yes".to_string());
    }
    if trust_server_certificate {
        parts.push("TrustServerCertificate=yes".to_string());
    }
    if multi_subnet_failover {
        parts.push("MultiSubnetFailover=True".to_string());
    }
    parts.join(";") + ";"
}

fn build_server_ui_kerberos(
    server: &str,
    port: u16,
    database: &str,
    encrypt: bool,
    trust_server_certificate: bool,
    multi_subnet_failover: bool,
) -> String {
    let mut parts = vec![
        format!("Server={server},{port}"),
        format!("Database={database}"),
        "Trusted_Connection=yes".to_string(),
        "MultipleActiveResultSets=True".to_string(),
    ];
    if encrypt {
        parts.push("Encrypt=yes".to_string());
    }
    if trust_server_certificate {
        parts.push("TrustServerCertificate=yes".to_string());
    }
    if multi_subnet_failover {
        parts.push("MultiSubnetFailover=True".to_string());
    }
    parts.join(";") + ";"
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{
        Config, MongoDatabases, MongoEmbedded, MongoMode, MongoProfile, SqlServerConnectionProfile,
        SqlServerProfile,
    };

    fn sample_config() -> Config {
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
            server_api: None,
            api: None,
            server: None,
            sqlserver: Some(SqlServerProfile {
                controller: Some(SqlServerConnectionProfile {
                    connection_string: None,
                    host: Some("sql.example.com".to_string()),
                    port: Some(1433),
                    database: Some("AlteryxService".to_string()),
                    username: Some("svc".to_string()),
                    password: None,
                    password_ref: None,
                    password_env: Some("AYX_SQL_CONTROLLER_PASSWORD".to_string()),
                    integrated_security: Some(false),
                    encrypt: Some(true),
                    trust_server_certificate: Some(false),
                    multi_subnet_failover: Some(true),
                }),
                server_ui: Some(SqlServerConnectionProfile {
                    connection_string: None,
                    host: Some("sql.example.com".to_string()),
                    port: Some(1433),
                    database: Some("AlteryxServerUI".to_string()),
                    username: Some("svc".to_string()),
                    password: None,
                    password_ref: None,
                    password_env: Some("AYX_SQL_SERVER_UI_PASSWORD".to_string()),
                    integrated_security: Some(false),
                    encrypt: Some(true),
                    trust_server_certificate: Some(false),
                    multi_subnet_failover: Some(false),
                }),
                legacy_connection_string: None,
            }),
            upgrade: None,
        }
    }

    #[test]
    fn precheck_flags_insecure_collation() {
        let cfg = sample_config();
        let data = precheck_envelope(&cfg, Some("Latin1_General_CS_AS")).expect("precheck");
        assert!(!data["collation"]["acceptable"].as_bool().unwrap());
        assert!(!data["issues"].as_array().unwrap().is_empty());
    }

    #[test]
    fn controller_connection_string_includes_driver() {
        let cfg = sample_config();
        let data = connection_string_envelope(
            &cfg,
            "controller",
            "sql",
            None,
            None,
            None,
            true,
            false,
            true,
        )
        .expect("connection string");
        let s = data["connection_string"].as_str().unwrap();
        assert!(s.contains("Driver={ODBC Driver 17 for SQL Server}"));
        assert!(s.contains("MultiSubnetFailover=True"));
    }

    #[test]
    fn server_ui_string_omits_driver() {
        let cfg = sample_config();
        let data = connection_string_envelope(
            &cfg,
            "server-ui",
            "kerberos",
            None,
            None,
            None,
            false,
            false,
            false,
        )
        .expect("connection string");
        let s = data["connection_string"].as_str().unwrap();
        assert!(!s.contains("Driver="));
        assert!(s.contains("MultipleActiveResultSets=True"));
        assert!(s.contains("Trusted_Connection=yes"));
    }

    #[test]
    fn validate_connections_returns_secret_sources() {
        let cfg = sample_config();
        let data = validate_connection_strings_envelope(&cfg).expect("validate");
        assert_eq!(
            data["controller"]["secret_source"],
            "AYX_SQL_CONTROLLER_PASSWORD"
        );
        assert_eq!(
            data["server_ui"]["secret_source"],
            "AYX_SQL_SERVER_UI_PASSWORD"
        );
    }
}
