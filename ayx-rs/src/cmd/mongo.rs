//! Dispatch for `ayx mongo ...`.
//!
//! Pattern matches sqlserver.rs: load_profile_with_env shim, then delegate
//! to helpers in `ayx_server::mongo`. The mutate path's TTY confirmation
//! prompt is raised here, not inside `ayx_server::mongo::mutate_envelope` —
//! `require_tty_confirmation` lives in this crate (`cmd::confirm`), not in
//! `ayx-server`.
//!
//! `--apply` is a **prepare / confirm / execute** split (plan Task 4's
//! required restructure), not a single call into `mutate_envelope`:
//!
//!   1. `prepare_mutation_apply` (read-only: CLI gates, template
//!      resolution, backup/approval artifact loading + validation) runs
//!      FIRST, before any prompt.
//!   2. Only once that has succeeded — so the confirmation message can name
//!      the *real* target database/collection and the approved
//!      matched-document count, not just the raw request — does
//!      `require_tty_confirmation` fire.
//!   3. `execute_mutation_apply` (the only phase that writes anything: the
//!      prepared execution audit artifact, then `mongosh`, then the
//!      terminal status update) runs last.
//!
//! This replaces Task 3's interim shape, where confirmation fired before
//! the template even resolved and could only echo back the raw
//! `--backup-audit-artifact`/`--approval-artifact` *paths* rather than
//! anything a loaded artifact actually proved. `mutate_envelope` itself
//! still runs the whole apply flow end-to-end (prepare then execute, no
//! confirmation) for print/preview modes and any other direct caller.
//!
//! `mongo undo --apply` follows the exact same prepare/confirm/execute
//! split — `prepare_undo_apply` (loading + validating the source mutation
//! artifact, deriving the guarded inverse, a live staleness re-check, and
//! approval-artifact validation) runs before `require_tty_confirmation`,
//! which then names the real restore target and candidate count, before
//! `execute_undo_apply` runs.

use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_server::mongo::{
    MongoMutateRequest, MongoUndoRequest, PreparedMutationApply, PreparedUndoApply,
    backup_envelope, doctor_envelope as mongo_doctor_envelope, execute_mutation_apply,
    execute_undo_apply, inventory_envelope, mutate_envelope, parse_mutation_params,
    prepare_mutation_apply, prepare_undo_apply, query_envelope as mongo_query_envelope,
    restore_envelope, status_envelope, undo_envelope,
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
                // Read-only first: CLI gates, template resolution, and
                // backup/approval artifact loading + validation. Nothing
                // has been written and mongosh has not been touched by the
                // time this returns.
                let prepared = prepare_mutation_apply(&profile_cfg, &request)?;
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &mongo_mutation_apply_warning(&profile_cfg.profile_name, &prepared),
                )?;
                execute_mutation_apply(&profile_cfg, prepared, &request)
            } else {
                mutate_envelope(&profile_cfg, &request)
            }
        }
        MongoCommand::Undo {
            profile,
            mutation_audit_artifact,
            print,
            apply,
            accept_mutation_risk,
            approval_artifact,
            approve,
            audit_dir,
        } => {
            let profile_cfg = runtime.load_profile(profile.as_deref())?;
            let request = MongoUndoRequest {
                mutation_audit_artifact,
                print,
                apply,
                accept_mutation_risk,
                approval_artifact,
                approve,
                audit_dir,
            };

            if request.apply {
                // Read-only first: CLI gates, source mutation artifact
                // loading + validation, the guarded inverse, a live
                // staleness re-check, and approval-artifact validation.
                // Nothing has been written and mongosh has not been used to
                // write anything by the time this returns.
                let prepared = prepare_undo_apply(&profile_cfg, &request)?;
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &mongo_undo_apply_warning(&profile_cfg.profile_name, &prepared),
                )?;
                execute_undo_apply(&profile_cfg, prepared, &request)
            } else {
                undo_envelope(&profile_cfg, &request)
            }
        }
    }
}

/// Build the TTY confirmation prompt for `mongo mutate --apply`. Built from
/// the *prepared* apply (loaded and validated backup/approval artifacts, a
/// resolved template, and the approved candidate snapshot) — so, unlike
/// Task 3's interim version, this names the real target database and
/// collection and the real approved matched-document count, not just the
/// raw request. Only called once `prepare_mutation_apply` has already
/// succeeded — see the module doc comment for the full prepare/confirm/
/// execute ordering.
fn mongo_mutation_apply_warning(profile_name: &str, prepared: &PreparedMutationApply) -> String {
    format!(
        "About to APPLY mongo mutation template '{template}' (rev {revision}) on profile \
         '{profile_name}': {matched_count} approved candidate document(s) in \
         '{database}.{collection}'. Approving digest: {digest}. Backup artifact: {backup} \
         (recorded {backup_age}). Approval artifact: {approval}. This runs a single no-retry \
         transactional Mongo write and cannot be undone without a guarded `mongo undo`. Review \
         the approved preview diff before proceeding.",
        template = prepared.mutation.template_id,
        revision = prepared.mutation.template_revision,
        matched_count = prepared.snapshot.matched_count,
        database = prepared.mutation.database,
        collection = prepared.mutation.collection,
        digest = prepared.approval_digest,
        backup = prepared.backup_audit_artifact.display(),
        backup_age = prepared.backup_timestamp_utc,
        approval = prepared.approval_artifact.display(),
    )
}

/// Build the TTY confirmation prompt for `mongo undo --apply`. Mirrors
/// `mongo_mutation_apply_warning`: built from the *prepared* undo (the
/// loaded, freshness-verified source mutation and its guarded-inverse
/// candidate set), so it names the real restore target and the real
/// candidate count, not just raw request paths.
fn mongo_undo_apply_warning(profile_name: &str, prepared: &PreparedUndoApply) -> String {
    format!(
        "About to APPLY mongo undo of mutation template '{template}' (rev {revision}) on \
         profile '{profile_name}': restoring {candidate_count} document(s) in \
         '{database}.{collection}'. Approving digest: {digest}. Undoing mutation artifact: \
         {undo_of}. Approval artifact: {approval}. This runs a single no-retry transactional \
         Mongo write and cannot itself be undone. Review the approved restore diff before \
         proceeding.",
        template = prepared.loaded.audit.template_id,
        revision = prepared.loaded.audit.template_revision,
        candidate_count = prepared.candidates.len(),
        database = prepared.loaded.audit.database,
        collection = prepared.loaded.audit.collection,
        digest = prepared.approval_digest,
        undo_of = prepared.mutation_audit_artifact.display(),
        approval = prepared.approval_artifact.display(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_server::mongo::{CandidateSnapshot, ResolvedMutation};
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// A hand-built `PreparedMutationApply` — everything `mongo_mutation_apply_warning`
    /// needs, without going through `prepare_mutation_apply` (which requires
    /// an executable template; the shipped registry ships only a
    /// `preview_only` one, so this is the only way to unit test the
    /// confirmation message's content at the ayx-rs crate level).
    fn fixture_prepared() -> PreparedMutationApply {
        PreparedMutationApply {
            mutation: ResolvedMutation {
                template_id: "test_template".to_string(),
                template_revision: 3,
                template_source_digest: "sha256:aaaa".to_string(),
                database: "TestDb".to_string(),
                collection: "widgets".to_string(),
                filter: serde_json::json!({"status": "pending"}),
                update: serde_json::json!({"$set": {"value": "x"}}),
                max_affected: 10,
                max_backup_age_minutes: 60,
                parameters: BTreeMap::new(),
                parameter_digest: "sha256:bbbb".to_string(),
                purpose: "test".to_string(),
                kba_refs: vec![],
                rollback: "guarded_set_inverse".to_string(),
            },
            snapshot: CandidateSnapshot {
                matched_count: 7,
                candidate_ids: vec![],
                raw_docs: vec![],
                field_diffs: vec![],
            },
            approval_digest: "sha256:cccc".to_string(),
            connection: serde_json::json!({}),
            backup_audit_artifact: PathBuf::from("/tmp/backup.json"),
            backup_timestamp_utc: Utc::now(),
            approval_artifact: PathBuf::from("/tmp/approval.json"),
        }
    }

    #[test]
    fn mongo_mutation_apply_warning_names_the_real_target_and_matched_count() {
        // This is the crux of Task 4's required restructure: the
        // confirmation prompt must name the REAL resolved target and the
        // REAL approved matched-document count -- not just echo back raw
        // request paths (Task 3's interim shape).
        let prepared = fixture_prepared();
        let message = mongo_mutation_apply_warning("myprofile", &prepared);
        assert!(
            message.contains("TestDb.widgets"),
            "must name the real resolved database.collection: {message}"
        );
        assert!(
            message.contains("7 approved candidate"),
            "must name the real approved matched-document count: {message}"
        );
        assert!(message.contains("test_template"), "{message}");
        assert!(message.contains("rev 3"), "{message}");
        assert!(message.contains("myprofile"), "{message}");
        assert!(message.contains("sha256:cccc"), "{message}");
    }

    /// A hand-built `PreparedUndoApply` — everything `mongo_undo_apply_warning`
    /// needs, without going through `prepare_undo_apply` (which requires a
    /// live mongosh staleness check).
    fn fixture_prepared_undo() -> PreparedUndoApply {
        use ayx_server::mongo::{
            CandidateDiff, FieldDiff, LoadedMutationAudit, MutationExecutionAudit,
            MutationExecutionOutcome, UndoCandidate, UndoFieldInverse,
        };

        let candidates = ayx_server::mongo::CandidateSnapshot {
            matched_count: 1,
            candidate_ids: vec![serde_json::json!("doc1")],
            raw_docs: vec![],
            field_diffs: vec![CandidateDiff {
                id: serde_json::json!("doc1"),
                fields: vec![FieldDiff {
                    path: "value".to_string(),
                    old_present: true,
                    old_value: serde_json::json!("old"),
                    new_value: serde_json::json!("42"),
                }],
            }],
        };
        let audit = MutationExecutionAudit {
            schema_version: 1,
            command: "mongo mutate".to_string(),
            timestamp_utc: Utc::now(),
            profile: "myprofile".to_string(),
            template_id: "test_template".to_string(),
            template_revision: 3,
            template_source_digest: "sha256:aaaa".to_string(),
            parameter_digest: "sha256:bbbb".to_string(),
            database: "TestDb".to_string(),
            collection: "widgets".to_string(),
            max_affected: 10,
            rollback: "guarded_set_inverse".to_string(),
            approval_digest: "sha256:cccc".to_string(),
            approval_artifact: PathBuf::from("/tmp/mutate-approval.json"),
            backup_audit_artifact: PathBuf::from("/tmp/backup.json"),
            backup_timestamp_utc: Utc::now(),
            connection: serde_json::json!({}),
            candidates,
            outcome: MutationExecutionOutcome::Applied {
                matched_count: 1,
                modified_count: 1,
            },
            undo_artifact: None,
        };

        PreparedUndoApply {
            mutation_audit_artifact: PathBuf::from("/tmp/mutation-audit.json"),
            loaded: LoadedMutationAudit {
                audit,
                source_artifact_hash: "sha256:eeee".to_string(),
            },
            candidates: vec![UndoCandidate {
                id: serde_json::json!("doc1"),
                fields: vec![UndoFieldInverse {
                    path: "value".to_string(),
                    old_present: true,
                    old_value: serde_json::json!("old"),
                    post_value: serde_json::json!("42"),
                }],
            }],
            field_paths: vec!["value".to_string()],
            approval_digest: "sha256:ffff".to_string(),
            approval_artifact: PathBuf::from("/tmp/undo-approval.json"),
            connection: serde_json::json!({}),
        }
    }

    #[test]
    fn mongo_undo_apply_warning_names_the_real_target_and_candidate_count() {
        let prepared = fixture_prepared_undo();
        let message = mongo_undo_apply_warning("myprofile", &prepared);
        assert!(
            message.contains("TestDb.widgets"),
            "must name the real target database.collection: {message}"
        );
        assert!(
            message.contains("restoring 1 document"),
            "must name the real candidate count: {message}"
        );
        assert!(message.contains("test_template"), "{message}");
        assert!(message.contains("rev 3"), "{message}");
        assert!(message.contains("myprofile"), "{message}");
        assert!(message.contains("sha256:ffff"), "{message}");
        assert!(message.contains("/tmp/mutation-audit.json"), "{message}");
    }
}
