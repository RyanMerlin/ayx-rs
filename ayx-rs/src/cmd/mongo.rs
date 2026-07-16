//! Dispatch for `ayx mongo ...`.
//!
//! Pattern matches sqlserver.rs: load_profile_with_env shim, then delegate
//! to helpers in `ayx_server::mongo`. The mutate path's TTY confirmation
//! prompt is raised here, not inside `ayx_server::mongo::mutate_envelope` —
//! `require_tty_confirmation` lives in this crate (`cmd::confirm`), not in
//! `ayx-server`. Confirmation deliberately does not depend on the template
//! actually resolving: it fires as soon as the CLI-level gate tuple is
//! complete (`validate_mutation_apply_gates`), and `mutate_envelope` does
//! its own template resolution afterward as the real source of truth. This
//! keeps "did the operator agree to attempt this" independent of "did the
//! template turn out to be valid" — the latter can fail for reasons (a
//! demoted template, a typo, an unbound parameter) that have nothing to do
//! with whether the request should have been confirmed in the first place.

use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_server::mongo::{
    MongoMutateRequest, backup_envelope, doctor_envelope as mongo_doctor_envelope,
    inventory_envelope, mutate_envelope, parse_mutation_params,
    query_envelope as mongo_query_envelope, restore_envelope, status_envelope,
    validate_mutation_apply_gates,
};

use crate::MongoCommand;
use crate::cmd;

pub fn execute(environment: Option<&str>, yes: bool, command: MongoCommand) -> Result<Envelope> {
    let runtime = crate::cmd::RuntimeCtx::new(environment);
    match command {
        MongoCommand::Status { profile } => {
            let profile = runtime.load_profile(profile.as_deref())?;
            status_envelope(&profile)
        }
        MongoCommand::Inventory { profile } => {
            let profile = runtime.load_profile(profile.as_deref())?;
            inventory_envelope(&profile)
        }
        MongoCommand::Backup {
            profile,
            output_dir,
            apply,
            audit_dir,
        } => {
            let profile = runtime.load_profile(profile.as_deref())?;
            backup_envelope(&profile, &output_dir, apply, &audit_dir)
        }
        MongoCommand::Restore {
            profile,
            input_path,
            apply,
            audit_dir,
        } => {
            let profile = runtime.load_profile(profile.as_deref())?;
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
            let profile = runtime.load_profile(profile.as_deref())?;
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
            let profile = runtime.load_profile(profile.as_deref())?;
            mongo_doctor_envelope(&profile)
        }
        MongoCommand::Mutate {
            profile,
            template,
            param,
            print,
            apply,
            accept_mutation_risk,
            backup_audit_artifact,
            approval_artifact,
            approve,
            audit_dir,
        } => {
            let profile_cfg = runtime.load_profile(profile.as_deref())?;
            let params = parse_mutation_params(&param)?;
            let request = MongoMutateRequest {
                template,
                params,
                print,
                apply,
                accept_mutation_risk,
                backup_audit_artifact,
                approval_artifact,
                approve,
                audit_dir,
            };

            if request.apply {
                // Reject an incomplete apply tuple before ever prompting —
                // the operator should not be asked to confirm a request
                // that can't execute anyway.
                validate_mutation_apply_gates(&request)?;
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &mongo_mutation_apply_warning(&request, &profile_cfg.profile_name),
                )?;
            }

            mutate_envelope(&profile_cfg, &request)
        }
        MongoCommand::Undo { .. } => {
            anyhow::bail!(
                "mongo undo is not yet implemented; the guarded-rollback executor and the \
                 mutation execution audit artifact it reads from land in a follow-up task"
            );
        }
    }
}

/// Build the TTY confirmation prompt for `mongo mutate --apply`. Names the
/// template (which uniquely determines the target database/collection in
/// the remediation registry), the bound-parameter count, and the approval
/// digest being confirmed. Deliberately built from the raw request only —
/// see the module doc comment for why this must not depend on the template
/// actually resolving. The real matched-document count isn't shown because
/// it isn't independently re-verified yet; that lands with the backup/
/// approval artifact loader (plan Task 4), which can enrich this message
/// once it has a loaded snapshot to read the count from.
fn mongo_mutation_apply_warning(request: &MongoMutateRequest, profile_name: &str) -> String {
    format!(
        "About to APPLY mongo mutation template '{template}' on profile '{profile_name}' \
         ({param_count} bound parameter(s)). Approving digest: {digest}. Backup artifact: \
         {backup}. Approval artifact: {approval}. This runs a single no-retry transactional \
         Mongo write against the template's configured database/collection and cannot be \
         undone without a guarded `mongo undo`. Review the approved preview diff before \
         proceeding.",
        template = request.template.as_deref().unwrap_or("<missing>"),
        param_count = request.params.len(),
        digest = request.approve.as_deref().unwrap_or("<missing>"),
        backup = request
            .backup_audit_artifact
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<missing>".to_string()),
        approval = request
            .approval_artifact
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<missing>".to_string()),
    )
}
