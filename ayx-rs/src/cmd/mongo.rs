//! Dispatch for `ayx mongo ...`.
//!
//! Pattern matches sqlserver.rs: load_profile_with_env shim, then delegate
//! to helpers in `ayx_server::mongo`. The mutate path keeps its
//! accept-mutation-risk gate; the executor itself is in
//! `ayx_server::mongo::mutate_envelope`.

use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_server::mongo::{
    backup_envelope, doctor_envelope as mongo_doctor_envelope, inventory_envelope,
    query_envelope as mongo_query_envelope, restore_envelope, status_envelope,
};

use crate::{load_profile_with_env, MongoCommand};

pub fn execute(environment: Option<&str>, command: MongoCommand) -> Result<Envelope> {
    fn load_profile<'a, P>(p: P, environment: Option<&str>) -> Result<ayx_core::profile::Config>
    where
        P: Into<crate::ProfileInput<'a>>,
    {
        load_profile_with_env(p, environment)
    }
    match command {
        MongoCommand::Status { profile } => {
            let profile = load_profile(profile.as_deref(), environment)?;
            status_envelope(&profile)
        }
        MongoCommand::Inventory { profile } => {
            let profile = load_profile(profile.as_deref(), environment)?;
            inventory_envelope(&profile)
        }
        MongoCommand::Backup {
            profile,
            output_dir,
            apply,
            audit_dir,
        } => {
            let profile = load_profile(profile.as_deref(), environment)?;
            backup_envelope(&profile, &output_dir, apply, &audit_dir)
        }
        MongoCommand::Restore {
            profile,
            input_path,
            apply,
            audit_dir,
        } => {
            let profile = load_profile(profile.as_deref(), environment)?;
            restore_envelope(&profile, &input_path, apply, &audit_dir)
        }
        MongoCommand::Query {
            profile,
            database,
            collection,
            filter,
            projection,
            sort,
            limit,
            print,
            apply,
            template,
        } => {
            let profile = load_profile(profile.as_deref(), environment)?;
            let spec = ayx_server::mongo::resolve_query_spec(
                &profile,
                database.as_deref(),
                collection.as_deref(),
                filter.as_deref(),
                projection.as_deref(),
                sort.as_deref(),
                limit,
                template.as_deref(),
            )?;
            mongo_query_envelope(&profile, &spec, print, apply)
        }
        MongoCommand::Doctor { profile } => {
            let profile = load_profile(profile.as_deref(), environment)?;
            mongo_doctor_envelope(&profile)
        }
        MongoCommand::Mutate {
            profile,
            database,
            collection,
            filter,
            update,
            template,
            print,
            apply,
            accept_mutation_risk,
        } => {
            let profile = load_profile(profile.as_deref(), environment)?;
            ayx_server::mongo::mutate_envelope(
                &profile,
                database.as_deref(),
                collection.as_deref(),
                filter.as_deref(),
                update.as_deref(),
                template.as_deref(),
                print,
                apply,
                accept_mutation_risk,
            )
        }
    }
}
