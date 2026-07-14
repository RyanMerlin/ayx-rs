//! Dispatch for `ayx sqlserver ...`.
//!
//! Every arm is a load-then-envelope wrapper around helpers in
//! `ayx_server::sqlserver`. `load_profile_with_env` is the canonical
//! profile loader (replaces the captured-closure pattern from main.rs).

use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_server::sqlserver::{
    connection_string_envelope, inventory_envelope as sqlserver_inventory_envelope,
    migration_prepare_envelope, precheck_envelope as sqlserver_precheck_envelope,
    status_envelope as sqlserver_status_envelope, validate_connection_strings_envelope,
};

use crate::SqlserverCommand;

pub fn execute(environment: Option<&str>, command: SqlserverCommand) -> Result<Envelope> {
    let runtime = crate::cmd::RuntimeCtx::new(environment);
    macro_rules! load_profile {
        ($profile:expr) => {
            runtime.load_profile($profile)
        };
    }
    match command {
        SqlserverCommand::Status { profile } => {
            let config = load_profile!(profile.as_deref())?;
            Ok(Envelope::ok_with_data(
                "sqlserver status summarized",
                sqlserver_status_envelope(&config)?,
            ))
        }
        SqlserverCommand::Inventory { profile } => {
            let config = load_profile!(profile.as_deref())?;
            Ok(Envelope::ok_with_data(
                "sqlserver inventory summarized",
                sqlserver_inventory_envelope(&config)?,
            ))
        }
        SqlserverCommand::Precheck { profile, collation } => {
            let config = load_profile!(profile.as_deref())?;
            Ok(Envelope::ok_with_data(
                "sqlserver precheck summarized",
                sqlserver_precheck_envelope(&config, collation.as_deref())?,
            ))
        }
        SqlserverCommand::ValidateStrings { profile } => {
            let config = load_profile!(profile.as_deref())?;
            Ok(Envelope::ok_with_data(
                "sqlserver connection strings validated",
                validate_connection_strings_envelope(&config)?,
            ))
        }
        SqlserverCommand::ConnectionString {
            profile,
            scope,
            auth,
            server,
            database,
            port,
            encrypt,
            trust_server_certificate,
            multi_subnet_failover,
        } => {
            let config = load_profile!(profile.as_deref())?;
            Ok(Envelope::ok_with_data(
                "sqlserver connection string generated",
                connection_string_envelope(
                    &config,
                    &scope,
                    &auth,
                    server.as_deref(),
                    database.as_deref(),
                    port,
                    encrypt,
                    trust_server_certificate,
                    multi_subnet_failover,
                )?,
            ))
        }
        SqlserverCommand::Migrate {
            profile,
            target_version,
            dry_run,
        } => {
            let config = load_profile!(profile.as_deref())?;
            Ok(Envelope::ok_with_data(
                "sqlserver migration plan generated",
                migration_prepare_envelope(&config, target_version.as_deref(), dry_run)?,
            ))
        }
        SqlserverCommand::Prepare {
            profile,
            target_version,
            dry_run,
        } => {
            let config = load_profile!(profile.as_deref())?;
            Ok(Envelope::ok_with_data(
                "sqlserver migration preparation generated",
                migration_prepare_envelope(&config, target_version.as_deref(), dry_run)?,
            ))
        }
    }
}
