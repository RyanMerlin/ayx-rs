//! CLI-level gate/dispatch tests for `ayx mongo mutate` (plan Tasks 3-4).
//!
//! Every test spawns the compiled `ayx` binary against a throwaway
//! `AYX_CONFIG_HOME` with a `managed`-mode Mongo profile, so connection-
//! detail resolution never touches the filesystem beyond the fixture (no
//! embedded-mode `RuntimeSettings.xml` dependency).
//!
//! None of these reach a real `mongosh` process. The only mutation template
//! Task 1 shipped (`user_email_domain_migration`, in
//! `ayx-server/knowledge/mongo/mutations.yaml`) is `preview_only`, so
//! `resolve_mutation_template` always rejects it — for print, preview, and
//! apply alike — before the dispatcher ever gets near
//! `preview_mutation`/`apply_mutation`. That rejection is itself the proof
//! each mode's wiring reached the server crate; see
//! `ayx-server/src/mongo.rs`'s `mutate_envelope_*` unit tests for the same
//! guarantee at the library level.
//!
//! **Task 4 ordering change:** `--apply` now runs `prepare_mutation_apply`
//! (CLI gates → template resolution → backup/approval artifact loading) in
//! full BEFORE the TTY confirmation prompt — see
//! `ayx-rs/src/cmd/mongo.rs`'s module doc comment. Because the shipped
//! registry has no `executable` template, `prepare_mutation_apply` can
//! never succeed via this black-box harness, which means the confirmation
//! prompt itself is now structurally unreachable from these tests — it
//! always fails at template resolution first, `--yes` or not. That is
//! itself the invariant `apply_without_yes_reaches_prepare_before_confirmation`
//! below proves. The confirmation message's *content* (real target
//! database/collection/matched-count) is unit-tested directly in
//! `ayx-rs/src/cmd/mongo.rs`'s own `#[cfg(test)]` module instead, using a
//! hand-built `PreparedMutationApply` — see
//! `mongo_mutation_apply_warning_names_the_real_target_and_matched_count`.

use std::fs;
use std::process::{Command, Stdio};

use ayx_core::profile::{AyxState, Config, MongoDatabases, MongoManaged, MongoMode, MongoProfile};
use tempfile::TempDir;

const PROFILE_NAME: &str = "mongotest";
const PREVIEW_ONLY_TEMPLATE: &str = "user_email_domain_migration";
const PREVIEW_ONLY_REJECTION: &str = "cannot be resolved for live preview/apply";

struct MongoFixture {
    home: TempDir,
}

impl MongoFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path();
        fs::create_dir_all(home.join("profiles")).expect("profiles dir");

        let state = AyxState {
            active_profile: Some(PROFILE_NAME.to_string()),
            active_workspace: None,
        };
        fs::write(
            home.join("state.yaml"),
            serde_yaml::to_string(&state).expect("state yaml"),
        )
        .expect("write state");

        let profile = Config {
            profile_name: PROFILE_NAME.to_string(),
            mongo: MongoProfile {
                mode: MongoMode::Managed,
                databases: MongoDatabases {
                    gallery_name: "AlteryxGallery".to_string(),
                    service_name: "AlteryxService".to_string(),
                },
                embedded: None,
                managed: Some(MongoManaged {
                    url: Some("mongodb://localhost:27017".to_string()),
                    host: None,
                    // `Default::default()` for a bare `u16` is 0, which
                    // config validation rejects (`mongo.managed.port must
                    // be greater than 0`) even though `url` alone is
                    // sufficient to connect — the serde `default =
                    // "default_mongo_port"` attribute only applies on
                    // deserialization, not on `Default::default()`, so it
                    // must be set explicitly here.
                    port: 27017,
                    auth_database: None,
                    username: None,
                    password: None,
                    password_ref: None,
                    tls: Default::default(),
                    timeout_ms: None,
                    retry_count: None,
                    max_pool_size: None,
                }),
            },
            alteryx_one: None,
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        };
        fs::write(
            home.join("profiles").join(format!("{PROFILE_NAME}.yaml")),
            serde_yaml::to_string(&profile).expect("profile yaml"),
        )
        .expect("write profile");

        Self { home: temp }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_ayx"))
            .args(args)
            .env("AYX_CONFIG_HOME", self.home.path())
            .env("AYX_PROFILE", PROFILE_NAME)
            // Ensure stdin is never a TTY regardless of the parent test
            // process's own stdin, and never blocks waiting for input.
            .stdin(Stdio::null())
            .output()
            .expect("ayx binary should run")
    }
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn assert_never_touched_mongosh(text: &str) {
    // A real mongosh spawn attempt would surface as either a PATH lookup
    // failure naming the binary or (if mongosh happened to be installed in
    // the test environment) a connection error. Every assertion in this
    // file instead pins an exact, deterministic Task-3-owned error string,
    // but this is a belt-and-suspenders guard against a future edit
    // accidentally deepening the call path.
    assert!(
        !text.contains("failed to execute 'mongosh"),
        "test reached a real mongosh spawn attempt: {text}"
    );
}

// ── Default preview mode (no --print, no --apply) ──────────────────────────

#[test]
fn default_preview_reaches_template_resolution_without_touching_mongosh() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&[
        "mongo",
        "mutate",
        "--template",
        PREVIEW_ONLY_TEMPLATE,
        "--param",
        "new_email=a@b.com",
    ]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(
        err.contains(PREVIEW_ONLY_REJECTION),
        "expected the preview-only rejection, got: {err}"
    );
    assert_never_touched_mongosh(&err);
}

#[test]
fn mutate_without_template_is_rejected() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&["mongo", "mutate"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("requires --template"));
}

// ── --print: static rendering, mutually exclusive with the apply flags ─────

#[test]
fn print_reaches_template_resolution_without_touching_mongosh() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&[
        "mongo",
        "mutate",
        "--template",
        PREVIEW_ONLY_TEMPLATE,
        "--param",
        "new_email=a@b.com",
        "--print",
    ]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(
        err.contains(PREVIEW_ONLY_REJECTION),
        "expected the preview-only rejection, got: {err}"
    );
    assert_never_touched_mongosh(&err);
}

#[test]
fn print_conflicts_with_apply() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&[
        "mongo",
        "mutate",
        "--template",
        PREVIEW_ONLY_TEMPLATE,
        "--print",
        "--apply",
    ]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(
        err.contains("cannot be used with"),
        "expected a clap conflict error, got: {err}"
    );
}

#[test]
fn print_conflicts_with_approve() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&[
        "mongo",
        "mutate",
        "--template",
        PREVIEW_ONLY_TEMPLATE,
        "--print",
        "--approve",
        "sha256:deadbeef",
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("cannot be used with"));
}

#[test]
fn print_conflicts_with_backup_audit_artifact() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&[
        "mongo",
        "mutate",
        "--template",
        PREVIEW_ONLY_TEMPLATE,
        "--print",
        "--backup-audit-artifact",
        "/tmp/backup.json",
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("cannot be used with"));
}

#[test]
fn print_conflicts_with_approval_artifact() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&[
        "mongo",
        "mutate",
        "--template",
        PREVIEW_ONLY_TEMPLATE,
        "--print",
        "--approval-artifact",
        "/tmp/approval.json",
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("cannot be used with"));
}

// ── Raw free-form mutation arguments no longer exist ────────────────────────

#[test]
fn raw_mutation_arguments_are_rejected_by_clap() {
    let fixture = MongoFixture::new();
    for flag in ["--database", "--collection", "--filter", "--update"] {
        let output = fixture.run(&["mongo", "mutate", flag, "x", "--apply"]);
        assert!(
            !output.status.success(),
            "expected {flag} to be rejected as an unknown argument"
        );
        let err = stderr(&output);
        assert!(
            err.contains("unexpected argument") || err.contains("unrecognized"),
            "expected a clap unknown-argument error for {flag}, got: {err}"
        );
    }
}

// ── --apply: complete-tuple gate validation ─────────────────────────────────

#[test]
fn apply_with_nothing_else_reports_every_missing_gate_together() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&["mongo", "mutate", "--apply"]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(err.contains("--template is required"), "{err}");
    assert!(err.contains("--accept-mutation-risk is required"), "{err}");
    assert!(err.contains("--backup-audit-artifact is required"), "{err}");
    assert!(err.contains("--approval-artifact is required"), "{err}");
    assert!(err.contains("--approve is required"), "{err}");
    assert_never_touched_mongosh(&err);
}

#[test]
fn apply_accept_mutation_risk_alone_still_reports_missing_approval() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&[
        "mongo",
        "mutate",
        "--template",
        PREVIEW_ONLY_TEMPLATE,
        "--apply",
        "--accept-mutation-risk",
    ]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(
        !err.contains("--accept-mutation-risk is required"),
        "accept-mutation-risk was passed and should not be reported missing: {err}"
    );
    assert!(err.contains("--backup-audit-artifact is required"), "{err}");
    assert!(err.contains("--approval-artifact is required"), "{err}");
    assert!(err.contains("--approve is required"), "{err}");
    assert_never_touched_mongosh(&err);
}

// ── --apply: TTY confirmation gate ──────────────────────────────────────────

/// The complete apply tuple minus `--yes`/TTY — every test below appends
/// exactly one more thing (nothing, or `--yes`) to this base.
fn complete_apply_tuple_args() -> Vec<&'static str> {
    vec![
        "mongo",
        "mutate",
        "--template",
        PREVIEW_ONLY_TEMPLATE,
        "--param",
        "new_email=a@b.com",
        "--apply",
        "--accept-mutation-risk",
        "--backup-audit-artifact",
        "/tmp/fixture-backup.json",
        "--approval-artifact",
        "/tmp/fixture-approval.json",
        "--approve",
        "sha256:deadbeef",
    ]
}

#[test]
fn apply_without_yes_reaches_prepare_before_confirmation() {
    // Task 4: prepare_mutation_apply (gates -> template resolution ->
    // backup/approval loading) now runs in full BEFORE
    // require_tty_confirmation is ever called. With a preview_only fixture
    // template, prepare always fails at template resolution -- so the
    // request fails with that error, NOT the confirmation-declined error,
    // even with no --yes and no TTY. This is the direct, observable proof
    // of the new ordering: if confirmation ran first (Task 3's interim
    // shape), this would instead see "destructive operation requires
    // confirmation".
    let fixture = MongoFixture::new();
    let output = fixture.run(&complete_apply_tuple_args());
    assert!(
        !output.status.success(),
        "a preview_only template must still fail closed"
    );
    let err = stderr(&output);
    assert!(
        err.contains(PREVIEW_ONLY_REJECTION),
        "expected to reach prepare_mutation_apply's template-resolution step \
         even without --yes/a TTY, got: {err}"
    );
    assert!(
        !err.contains("destructive operation requires confirmation"),
        "confirmation must never be reached when prepare itself already failed: {err}"
    );
    assert_never_touched_mongosh(&err);
}

#[test]
fn apply_with_yes_reaches_the_same_prepare_boundary() {
    // With --yes, the observable outcome is identical to the no --yes case
    // above: prepare_mutation_apply fails at template resolution before
    // confirmation is ever consulted, so --yes has nothing to bypass yet.
    // This is the CLI-level proof that prepare runs unconditionally ahead
    // of the confirmation gate, matching
    // `apply_without_yes_reaches_prepare_before_confirmation`.
    let fixture = MongoFixture::new();
    let mut args = complete_apply_tuple_args();
    args.push("--yes");
    let output = fixture.run(&args);
    assert!(
        !output.status.success(),
        "the only shipped template is preview_only, so this still fails at prepare"
    );
    let err = stderr(&output);
    // The old terminal message this replaces must be gone entirely.
    assert!(
        !err.contains("execution is not yet enabled"),
        "the old disabled-execution message must not resurface: {err}"
    );
    assert!(
        !err.contains("destructive operation requires confirmation"),
        "confirmation is never reached (prepare fails first): {err}"
    );
    assert!(
        !err.contains("missing required safety gate"),
        "the apply tuple was complete: {err}"
    );
    // Reached ayx_server::mongo::prepare_mutation_apply's template-resolution
    // step — proof the request crossed the CLI -> server boundary with
    // every CLI-level gate satisfied.
    assert!(
        err.contains(PREVIEW_ONLY_REJECTION),
        "expected to reach template resolution in the server crate, got: {err}"
    );
    assert_never_touched_mongosh(&err);
}

// ── mongo undo: Clap variant exists but execution is intentionally stubbed ──

#[test]
fn undo_variant_parses_and_reports_not_yet_implemented() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&[
        "mongo",
        "undo",
        "--mutation-audit-artifact",
        "/tmp/mutation-audit.json",
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("not yet implemented"));
}

#[test]
fn undo_print_conflicts_with_apply() {
    let fixture = MongoFixture::new();
    let output = fixture.run(&[
        "mongo",
        "undo",
        "--mutation-audit-artifact",
        "/tmp/mutation-audit.json",
        "--print",
        "--apply",
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("cannot be used with"));
}
