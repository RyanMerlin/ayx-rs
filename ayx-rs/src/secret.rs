//! `ayx secret prune` — legacy keyring account cleanup.
//!
//! Detects and optionally deletes keyring accounts written by ayx < v0.11.0 that
//! used the mutable `profile_name` as the keyring scope.  v0.11.0+ uses the stable
//! on-disk file stem.  When these differ, old accounts become orphaned.
//!

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use ayx_core::{
    auth::OneSecretSlot,
    profile::Config,
    secrets::{keyring_account, resolve_secret_ref_with},
};
use serde::Serialize;

use crate::onboard::{self, InlineSecretPolicy};

/// Stable, user-facing names for secrets AYX can manage without interpreting
/// arbitrary YAML paths. The canonical field matches the existing keyring
/// naming contract used by `secretize_config`.
#[derive(Debug, Clone, Copy)]
struct SecretSlot {
    name: &'static str,
    field: &'static str,
    env: &'static str,
}

const SLOTS: &[SecretSlot] = &[
    SecretSlot {
        name: "server.api.client-secret",
        field: "server.api.client_secret",
        env: "AYX_SERVER_API_CLIENT_SECRET",
    },
    SecretSlot {
        name: "mongo.managed.password",
        field: "server.storage.mongo.managed.password",
        env: "AYX_MONGO_MANAGED_PASSWORD",
    },
    SecretSlot {
        name: "sql.controller.password",
        field: "server.storage.sqlserver.controller.password",
        env: "AYX_SQL_CONTROLLER_PASSWORD",
    },
    SecretSlot {
        name: "sql.server-ui.password",
        field: "server.storage.sqlserver.server_ui.password",
        env: "AYX_SQL_SERVER_UI_PASSWORD",
    },
    SecretSlot {
        name: "one.client-secret",
        field: "alteryx_one.client_secret",
        env: "AYX_ONE_CLIENT_SECRET",
    },
    SecretSlot {
        name: "one.service-principal-client-secret",
        field: "alteryx_one.sp_client_secret",
        env: "AYX_ONE_SP_CLIENT_SECRET",
    },
];

#[derive(Debug, Serialize)]
pub struct SlotReport {
    pub slot: String,
    pub source: &'static str,
    pub configured: bool,
    pub resolved: bool,
    pub validation: &'static str,
    pub remediation: Option<&'static str>,
}

pub enum SecretInput {
    Prompt(String),
    Stdin(String),
    Environment(String),
}

pub struct SetResult {
    pub slot: &'static str,
    pub source: &'static str,
    /// Set when the secret had to be stored as plaintext in the profile YAML
    /// because no secure store was available. Callers must surface this.
    pub warning: Option<String>,
}

pub struct UnsetResult {
    pub slot: &'static str,
    pub keyring_entry_deleted: bool,
}

fn slot(name: &str) -> Result<SecretSlot> {
    SLOTS
        .iter()
        .copied()
        .find(|candidate| candidate.name == name)
        .with_context(|| {
            format!(
                "unknown secret slot '{name}'; use one of: {}",
                SLOTS
                    .iter()
                    .map(|slot| slot.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn source_and_values(config: &Config, slot: SecretSlot) -> Result<(Option<&str>, Option<&str>)> {
    match slot.name {
        "server.api.client-secret" => {
            if let Some(server_api) = config.server_api.as_ref() {
                Ok((
                    Some(server_api.client_secret.as_str()).filter(|v| !v.is_empty()),
                    server_api.client_secret_ref.as_deref(),
                ))
            } else if let Some(server) = config.server.as_ref() {
                Ok((
                    Some(server.curator_api_secret.as_str()).filter(|v| !v.is_empty()),
                    server.curator_api_secret_ref.as_deref(),
                ))
            } else {
                Ok((None, None))
            }
        }
        "mongo.managed.password" => {
            Ok(config.mongo.managed.as_ref().map_or((None, None), |mongo| {
                (mongo.password.as_deref(), mongo.password_ref.as_deref())
            }))
        }
        "sql.controller.password" => Ok(config
            .sqlserver
            .as_ref()
            .and_then(|sql| sql.controller.as_ref())
            .map_or((None, None), |connection| {
                (
                    connection.password.as_deref(),
                    connection.password_ref.as_deref(),
                )
            })),
        "sql.server-ui.password" => Ok(config
            .sqlserver
            .as_ref()
            .and_then(|sql| sql.server_ui.as_ref())
            .map_or((None, None), |connection| {
                (
                    connection.password.as_deref(),
                    connection.password_ref.as_deref(),
                )
            })),
        "one.client-secret" => Ok(config.alteryx_one.as_ref().map_or((None, None), |one| {
            (
                one.client_secret.as_deref(),
                one.client_secret_ref.as_deref(),
            )
        })),
        "one.service-principal-client-secret" => {
            Ok(config.alteryx_one.as_ref().map_or((None, None), |one| {
                (
                    one.sp_client_secret.as_deref(),
                    one.sp_client_secret_ref.as_deref(),
                )
            }))
        }
        _ => unreachable!("slot registry is exhaustive"),
    }
}

fn set_value(
    config: &mut Config,
    slot: SecretSlot,
    value: Option<String>,
    reference: Option<String>,
) -> Result<()> {
    match slot.name {
        "server.api.client-secret" => {
            if let Some(server_api) = config.server_api.as_mut() {
                server_api.client_secret = value.unwrap_or_default();
                server_api.client_secret_ref = reference;
            } else if let Some(server) = config.server.as_ref() {
                config.server_api = Some(ayx_core::profile::ServerApiProfile {
                    base_url: server.webapi_url.clone(),
                    client_id: server.curator_api_key.clone(),
                    client_secret: value.unwrap_or_default(),
                    client_secret_ref: reference,
                });
            } else {
                bail!(
                    "{} requires server_api or server configuration; run `ayx onboard` first",
                    slot.name
                );
            }
        }
        "mongo.managed.password" => {
            let mongo = config.mongo.managed.as_mut().context("mongo.managed.password requires managed Mongo configuration; run `ayx onboard` first")?;
            mongo.password = value;
            mongo.password_ref = reference;
        }
        "sql.controller.password" => {
            let connection = config.sqlserver.as_mut().and_then(|sql| sql.controller.as_mut()).context("sql.controller.password requires SQL controller configuration; run `ayx onboard` first")?;
            connection.password = value;
            connection.password_ref = reference;
        }
        "sql.server-ui.password" => {
            let connection = config.sqlserver.as_mut().and_then(|sql| sql.server_ui.as_mut()).context("sql.server-ui.password requires SQL Server UI configuration; run `ayx onboard` first")?;
            connection.password = value;
            connection.password_ref = reference;
        }
        "one.client-secret" => {
            let one = config.alteryx_one.as_mut().context(
                "one.client-secret requires Alteryx One configuration; run `ayx onboard` first",
            )?;
            one.client_secret = value;
            one.client_secret_ref = reference;
        }
        "one.service-principal-client-secret" => {
            let one = config.alteryx_one.as_mut().context("one.service-principal-client-secret requires Alteryx One configuration; run `ayx onboard` first")?;
            one.sp_client_secret = value;
            one.sp_client_secret_ref = reference;
        }
        _ => unreachable!("slot registry is exhaustive"),
    }
    Ok(())
}

fn report_secret(
    slot: impl Into<String>,
    value: Option<&str>,
    reference: Option<&str>,
    can_set_directly: bool,
    env_files: &HashMap<String, String>,
) -> SlotReport {
    let slot = slot.into();
    let unavailable_remediation = if can_set_directly {
        "set the referenced secret, or run `ayx secret set <slot>`"
    } else {
        "set the referenced secret, or run `ayx secret migrate` after updating the credential"
    };
    let invalid_reference_remediation = if can_set_directly {
        "use keyring:<account>, env:<variable>, or run `ayx secret set <slot>`"
    } else {
        "use a keyring:<account> or env:<variable> reference, or run `ayx secret migrate`"
    };

    match reference {
        Some(reference) if reference.starts_with("inline:") => SlotReport {
            slot,
            source: "inline",
            configured: true,
            resolved: true,
            validation: "warning",
            remediation: Some("run `ayx secret migrate` when secure storage is available"),
        },
        Some(reference) => match resolve_secret_ref_with(reference, env_files) {
            Ok(Some(_)) => SlotReport {
                slot,
                source: if reference.starts_with("keyring:") {
                    "keyring"
                } else if reference.starts_with("env:") {
                    "env"
                } else {
                    "reference"
                },
                configured: true,
                resolved: true,
                validation: "passed",
                remediation: None,
            },
            Ok(None) => SlotReport {
                slot,
                source: if reference.starts_with("keyring:") {
                    "keyring"
                } else if reference.starts_with("env:") {
                    "env"
                } else {
                    "reference"
                },
                configured: true,
                resolved: false,
                validation: "error",
                remediation: Some(unavailable_remediation),
            },
            Err(_) => SlotReport {
                slot,
                source: "invalid_reference",
                configured: true,
                resolved: false,
                validation: "error",
                remediation: Some(invalid_reference_remediation),
            },
        },
        None if value.is_some_and(|value| !value.is_empty()) => SlotReport {
            slot,
            source: "plaintext",
            configured: true,
            resolved: true,
            validation: "warning",
            remediation: Some("run `ayx secret migrate` to move this value into the OS keyring"),
        },
        None => SlotReport {
            slot,
            source: "missing",
            configured: false,
            resolved: false,
            validation: "not_configured",
            remediation: None,
        },
    }
}

fn one_secret_values(
    slot: OneSecretSlot,
    one: &ayx_core::profile::AlteryxOneProfile,
) -> (&Option<String>, &Option<String>) {
    match slot {
        OneSecretSlot::AccessToken => (&one.access_token, &one.access_token_ref),
        OneSecretSlot::RefreshToken => (&one.refresh_token, &one.refresh_token_ref),
        OneSecretSlot::WorkspacePassword => (&one.workspace_password, &one.workspace_password_ref),
        OneSecretSlot::ClientSecret => (&one.client_secret, &one.client_secret_ref),
        OneSecretSlot::ServicePrincipalClientSecret => {
            (&one.sp_client_secret, &one.sp_client_secret_ref)
        }
    }
}

fn workspace_secret_values(
    slot: OneSecretSlot,
    credential: &ayx_core::profile::WorkspaceCredential,
) -> (&Option<String>, &Option<String>) {
    match slot {
        OneSecretSlot::AccessToken => (&credential.access_token, &credential.access_token_ref),
        OneSecretSlot::RefreshToken => (&credential.refresh_token, &credential.refresh_token_ref),
        OneSecretSlot::WorkspacePassword => (
            &credential.workspace_password,
            &credential.workspace_password_ref,
        ),
        OneSecretSlot::ClientSecret => (&credential.client_secret, &credential.client_secret_ref),
        OneSecretSlot::ServicePrincipalClientSecret => (
            &credential.sp_client_secret,
            &credential.sp_client_secret_ref,
        ),
    }
}

/// Inspect every persisted AYX secret slot without returning a secret value or
/// the account/variable used by its reference.  `secret set` owns only the
/// stable named slots; login-managed and workspace credentials are included so
/// status and validation cannot overlook their storage posture.
pub fn inspect_profile(path: &Path) -> Result<Vec<SlotReport>> {
    // Secret maintenance edits the selected file only. Do not overlay active
    // profile state, which could otherwise cause a targeted mutation to write
    // credentials from another profile into this one.
    let config = Config::load_from_path_lenient_without_active_overlay(path)?;
    // Resolve `env:` references against the same `.env` view the loader used,
    // so a credential supplied through a `.env` file is not reported as an
    // unresolvable reference.
    let env_files = ayx_core::profile::env_file_values(path);
    let mut reports: Vec<_> = SLOTS
        .iter()
        .copied()
        .map(|slot| {
            let (value, reference) = source_and_values(&config, slot)?;
            Ok(report_secret(slot.name, value, reference, true, &env_files))
        })
        .collect::<Result<_>>()?;

    if let Some(one) = config.alteryx_one.as_ref() {
        // Client-secret fields above are named `secret set` slots. The other
        // three top-level fields are login-managed and still need inventory.
        for one_slot in [
            OneSecretSlot::AccessToken,
            OneSecretSlot::RefreshToken,
            OneSecretSlot::WorkspacePassword,
        ] {
            let (value, reference) = one_secret_values(one_slot, one);
            reports.push(report_secret(
                format!("one.{}", one_slot.name().replace('_', "-")),
                value.as_deref(),
                reference.as_deref(),
                false,
                &env_files,
            ));
        }
        for (workspace_id, credential) in &one.workspace_credentials {
            for one_slot in OneSecretSlot::ALL {
                let (value, reference) = workspace_secret_values(one_slot, credential);
                reports.push(report_secret(
                    format!(
                        "one.workspace.{workspace_id}.{}",
                        one_slot.name().replace('_', "-")
                    ),
                    value.as_deref(),
                    reference.as_deref(),
                    false,
                    &env_files,
                ));
            }
        }
    }
    Ok(reports)
}

pub fn set_slot(path: &Path, name: &str, input: SecretInput) -> Result<SetResult> {
    let slot = slot(name)?;
    // Write path: start from the file as written, never from the env-augmented
    // read view. Loading the augmented view here persisted an `env:NAME`
    // reference for every credential-shaped variable that happened to be
    // exported, silently rebinding slots the caller never named.
    let mut config = Config::load_from_path_for_write(path)?;
    match input {
        SecretInput::Environment(name) => {
            if !is_valid_env_name(&name) {
                bail!(
                    "invalid environment variable name '{name}'; use letters, digits, and underscores, beginning with a letter or underscore"
                );
            }
            set_value(&mut config, slot, None, Some(format!("env:{name}")))?;
            onboard::write_config_exact(path, &config, None, &BTreeSet::new())?;
            Ok(SetResult {
                slot: slot.name,
                source: "env",
                warning: None,
            })
        }
        SecretInput::Prompt(value) | SecretInput::Stdin(value) => {
            if value.is_empty() {
                bail!("secret input cannot be empty");
            }
            let profile_stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(&config.profile_name);
            let account = keyring_account(profile_stem, slot.field);

            // Attempt secure storage first. Writability cannot be probed
            // reliably up front: on macOS with no default keychain the entry
            // opens fine and only the password operation fails, so the failure
            // has to be observed. `write_config_exact` rolls back completely on
            // error, so retrying afterwards starts from a clean profile.
            let keyring_ref = format!("keyring:{account}");
            let mut attempt = config.clone();
            set_value(&mut attempt, slot, None, Some(keyring_ref))?;
            let stored = onboard::write_config_exact(
                path,
                &attempt,
                Some((&account, &value)),
                &BTreeSet::new(),
            );
            match stored {
                Ok(()) => {
                    return Ok(SetResult {
                        slot: slot.name,
                        source: "keyring",
                        warning: None,
                    });
                }
                Err(err) => {
                    if !is_keyring_failure(&err) {
                        return Err(err);
                    }
                }
            }

            // No secure store. Store plaintext and say so loudly rather than
            // refusing: a hard failure here strands anyone bootstrapping on a
            // host without a keyring, and the alternative they reach for is
            // hand-editing the same plaintext into the YAML with no warning at
            // all. `doctor config` and `secret status` keep reporting the
            // posture until it is resolved.
            set_value(&mut config, slot, None, Some(format!("inline:{value}")))?;
            onboard::write_config_exact(path, &config, None, &BTreeSet::new())?;
            Ok(SetResult {
                slot: slot.name,
                source: "inline",
                warning: Some(format!(
                    "Stored '{}' as plaintext in the profile YAML because no OS keyring was \
                     available. Anyone who can read the file can read the secret. Configure a \
                     keyring backend and run `ayx secret migrate`, or use `ayx secret set {} \
                     --from-env NAME` to reference an environment variable instead.",
                    slot.name, slot.name
                )),
            })
        }
    }
}

pub fn unset_slot(path: &Path, name: &str, profiles_dir: &Path) -> Result<UnsetResult> {
    let slot = slot(name)?;
    let mut config = Config::load_from_path_lenient_without_active_overlay(path)?;
    let (_, existing_ref) = source_and_values(&config, slot)?;
    let profile_stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(&config.profile_name);
    let private_account = keyring_account(profile_stem, slot.field);
    let delete_accounts = if existing_ref == Some(format!("keyring:{private_account}").as_str()) {
        let candidates = HashSet::from([private_account.clone()]);
        unreferenced_keyring_accounts_excluding_profile(profiles_dir, path, &candidates)?
            .into_iter()
            .collect()
    } else {
        BTreeSet::new()
    };
    set_value(&mut config, slot, None, None)?;
    onboard::write_config_exact(path, &config, None, &delete_accounts)?;
    Ok(UnsetResult {
        slot: slot.name,
        keyring_entry_deleted: !delete_accounts.is_empty(),
    })
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(character) if character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Whether `err` reports a failure to **store** into the OS keyring.
///
/// This decides whether the secret is written as cleartext instead, so it must
/// be exact. It used to fall back to substring-matching the rendered message,
/// because `secretize_config` re-wrapped its failures with
/// `anyhow!("{field}: {err}")` and erased the type. That fallback matched far
/// more than it meant to:
///
///   * a keyring *read* denial — a locked macOS keychain, or a user clicking
///     Deny on the Secret Service prompt — on a host whose keyring is present
///     and perfectly writable, and
///   * a *rollback* failure, where the profile write failed for an unrelated
///     reason (`EPERM`, `ENOSPC`, a serialization error) and the keyring
///     rollback failed too, concatenating "keyring entry" into the message.
///
/// Both then offered to rewrite every credential as plaintext. The wrap now
/// preserves the typed error, so classification is by cause only.
fn is_keyring_failure(err: &anyhow::Error) -> bool {
    err.downcast_ref::<ayx_core::profile::ProfileError>()
        .is_some_and(ayx_core::secrets::is_keyring_storage_error)
}

pub struct MigrateResult {
    pub migrated: Vec<String>,
    /// Set when plaintext was found but no secure store was available to move
    /// it into. Callers must surface this.
    pub warning: Option<String>,
}

pub fn migrate_profile(path: &Path) -> Result<MigrateResult> {
    let config = Config::load_from_path_lenient_without_active_overlay(path)?;
    let mut plaintext_fields: BTreeSet<String> = SLOTS
        .iter()
        .copied()
        .filter_map(|slot| {
            source_and_values(&config, slot)
                .ok()
                .filter(|(value, reference)| {
                    ayx_core::secrets::holds_plaintext_secret(*value, *reference)
                })
                .map(|_| slot.field.to_string())
        })
        .collect();
    plaintext_fields.extend(ayx_core::auth::inline_secret_fields(&config));
    if plaintext_fields.is_empty() {
        return Ok(MigrateResult {
            migrated: Vec::new(),
            warning: None,
        });
    }
    // Migration moves plaintext *into* secure storage, so `Forbid` is correct:
    // rewriting it as inline plaintext would accomplish nothing. But a missing
    // keyring is an environment condition, not a user error, so report it as an
    // unfinished no-op instead of failing. The secrets stay exactly where they
    // were and `doctor config` keeps flagging them.
    let output = match onboard::write_config_with_policy(path, &config, InlineSecretPolicy::Forbid)
    {
        Ok(output) => output,
        Err(err) if is_keyring_failure(&err) => {
            let mut fields: Vec<String> = plaintext_fields.into_iter().collect();
            fields.sort();
            return Ok(MigrateResult {
                migrated: Vec::new(),
                warning: Some(format!(
                    "No OS keyring is available, so {} plaintext secret(s) were left in the \
                     profile YAML: {}. They remain readable by anyone who can read the file. \
                     Configure a keyring backend and re-run `ayx secret migrate`.",
                    fields.len(),
                    fields.join(", ")
                )),
            });
        }
        Err(err) => return Err(err),
    };
    Ok(MigrateResult {
        migrated: output
            .refs
            .into_keys()
            .filter(|field| plaintext_fields.contains(field))
            .collect(),
        warning: None,
    })
}

/// Compatibility projection for the original `migrated_slots` response field.
/// The richer `migrated_fields` result can include login and workspace fields
/// that have no `secret set` slot, so only stable named slots belong here.
pub fn migrated_slot_names(fields: &[String]) -> Vec<&'static str> {
    SLOTS
        .iter()
        .filter(|slot| fields.iter().any(|field| field == slot.field))
        .map(|slot| slot.name)
        .collect()
}

pub fn env_template(_path: &Path) -> Result<Vec<&'static str>> {
    Ok(SLOTS.iter().map(|slot| slot.env).collect())
}

/// Secretizable fields that `secretize_config` may write for a given scope.
/// Dynamic workspace-credential fields are derived at runtime from the profile
/// YAML; the One slots come from `OneSecretSlot`, leaving these five
/// non-One static fields.
const STATIC_FIELDS: &[&str] = &[
    "server.api.client_secret",
    "server.curator_api_secret",
    "server.storage.mongo.managed.password",
    "server.storage.sqlserver.controller.password",
    "server.storage.sqlserver.server_ui.password",
];

/// A candidate orphaned keyring account identified by `prune_candidates`.
#[derive(Debug, Clone)]
pub struct PruneCandidate {
    /// Profile on-disk file stem (current scope identity).
    pub profile_stem: String,
    /// Full keyring account string, e.g. `"my_profile/alteryx_one.access_token"`.
    pub account: String,
    /// Detection status — does not change after apply (apply returns a separate list).
    pub status: CandidateStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateStatus {
    /// No live `keyring:` ref points at this account — safe to delete.
    WouldDelete,
    /// A `keyring:` ref in a current config file points at this account — skip.
    LiveRef,
    /// Account does not exist in the keyring (already cleaned up or never written).
    /// `prune_candidates` never sets this; callers may inject it and `apply_prune` handles it
    /// as a no-op passthrough.
    #[allow(dead_code)]
    NotFound,
}

/// Returns the old (pre-v0.11.0) keyring account names for a profile where
/// `old_scope != new_scope`.  Empty when the scopes are equal (no rename).
///
/// `old_scope` is `sanitize(profile_name)` from the YAML field;
/// `new_scope` is `sanitize(file_stem)` from the on-disk path.
/// `workspace_ids` are the keys from `alteryx_one.workspace_credentials`.
fn legacy_accounts_for_mismatch(
    old_scope: &str,
    new_scope: &str,
    workspace_ids: &[&str],
) -> Vec<String> {
    // keyring_account already applies the same sanitization — compare the outputs.
    // Use a throwaway field to compare just the scope prefix.
    if keyring_account(old_scope, "") == keyring_account(new_scope, "") {
        return vec![];
    }
    let mut accounts: Vec<String> = OneSecretSlot::ALL
        .iter()
        .map(|slot| keyring_account(old_scope, &format!("alteryx_one.{}", slot.name())))
        .collect();
    accounts.extend(STATIC_FIELDS.iter().map(|f| keyring_account(old_scope, f)));
    for ws_id in workspace_ids {
        for slot in OneSecretSlot::ALL {
            let field = format!(
                "alteryx_one.workspace_credentials['{ws_id}'].{}",
                slot.name()
            );
            accounts.push(keyring_account(old_scope, &field));
        }
    }
    accounts
}

/// Scan YAML text and return all account strings referenced by `keyring:` refs.
fn keyring_refs_from_text(text: &str) -> HashSet<String> {
    let mut refs = HashSet::new();
    // skip(1): the first segment is text *before* the first "keyring:" occurrence.
    for part in text.split("keyring:").skip(1) {
        // Everything after "keyring:" until whitespace or end-of-token is the account.
        let account: String = part
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
            .collect();
        if !account.is_empty() {
            refs.insert(account);
        }
    }
    refs
}

/// Scan all YAML files in `profiles_dir` and return the union of all
/// `keyring:` account references.
fn collect_all_keyring_refs(profiles_dir: &Path) -> Result<HashSet<String>> {
    let mut refs = HashSet::new();
    let entries = fs::read_dir(profiles_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(text) => {
                refs.extend(keyring_refs_from_text(&text));
            }
            Err(e) => {
                eprintln!(
                    "warning: could not read '{}' for keyring ref scan: {}",
                    path.display(),
                    e
                );
            }
        }
    }
    Ok(refs)
}

/// Return candidates that no profile other than `excluded_profile` references.
///
/// Authentication keyring accounts are binding-derived and may deliberately be
/// shared by profiles for the same identity and workspace. Logout uses this
/// check before deletion so it cannot break another profile. Unlike the
/// best-effort reporting scan used by `prune`, unreadable profiles are an error:
/// failing closed is the only safe choice before deleting a credential.
pub fn unreferenced_keyring_accounts_excluding_profile(
    profiles_dir: &Path,
    excluded_profile: &Path,
    candidates: &HashSet<String>,
) -> Result<HashSet<String>> {
    let mut live_refs = HashSet::new();
    for entry in fs::read_dir(profiles_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == excluded_profile
            || path.extension().and_then(|extension| extension.to_str()) != Some("yaml")
        {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        live_refs.extend(keyring_refs_from_text(&text));
    }
    Ok(candidates
        .iter()
        .filter(|account| !live_refs.contains(*account))
        .cloned()
        .collect())
}

/// Extract workspace credential keys from a parsed YAML value.
/// Returns an empty vec when the field is absent or has an unexpected shape.
fn workspace_ids_from_value(value: &serde_yaml::Value) -> Vec<String> {
    value
        .get("alteryx_one")
        .and_then(|o| o.get("workspace_credentials"))
        .and_then(|m| m.as_mapping())
        .map(|m| {
            m.keys()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Detect orphaned pre-v0.11.0 keyring accounts across all profiles in
/// `config_home/profiles/`.
///
/// `profile_filter` — if `Some(stem)`, only inspect that profile file.
///
/// Returns `Err` if the profiles directory cannot be read or if a filtered
/// profile's YAML fails to parse.  Unfiltered profiles that fail to read are
/// warned and skipped.
pub fn prune_candidates(
    config_home: &Path,
    profile_filter: Option<&str>,
) -> Result<Vec<PruneCandidate>> {
    let profiles_dir = config_home.join("profiles");
    let live_refs = collect_all_keyring_refs(&profiles_dir)?;

    let mut candidates = Vec::new();

    for entry in fs::read_dir(&profiles_dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some(filter) = profile_filter
            && stem != filter
        {
            continue;
        }

        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                if profile_filter.is_some() {
                    anyhow::bail!("cannot read profile '{}': {}", stem, e);
                }
                eprintln!("warning: skipping unreadable profile '{}': {}", stem, e);
                continue;
            }
        };

        // Extract profile_name from YAML without full Config deserialize.
        let yaml_value: serde_yaml::Value = match serde_yaml::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                if profile_filter.is_some() {
                    anyhow::bail!("failed to parse profile '{}': {}", stem, e);
                }
                eprintln!("warning: skipping unparseable profile '{}': {}", stem, e);
                continue;
            }
        };
        let Some(profile_name) = yaml_value.get("profile_name").and_then(|v| v.as_str()) else {
            continue; // no profile_name field — skip
        };

        let ws_ids = workspace_ids_from_value(&yaml_value);
        let ws_id_refs: Vec<&str> = ws_ids.iter().map(String::as_str).collect();

        let old_accounts = legacy_accounts_for_mismatch(profile_name, stem, &ws_id_refs);

        for account in old_accounts {
            let status = if live_refs.contains(&account) {
                CandidateStatus::LiveRef
            } else {
                CandidateStatus::WouldDelete
            };
            candidates.push(PruneCandidate {
                profile_stem: stem.to_string(),
                account,
                status,
            });
        }
    }

    Ok(candidates)
}

/// Result of attempting to delete a single orphan account.
#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub profile_stem: String,
    pub account: String,
    pub status: ApplyStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyStatus {
    Deleted,
    NotFound,
    LiveRef,
    Failed(String),
}

/// Delete orphaned keyring accounts identified by `prune_candidates`.
///
/// `LiveRef` candidates are never touched.  `WouldDelete` candidates have
/// `Entry::delete_credential()` called; `NoEntry` maps to `NotFound`.
pub fn apply_prune(candidates: Vec<PruneCandidate>) -> Vec<ApplyResult> {
    use ayx_core::secrets::ensure_keyring_store;
    use keyring_core::Entry;

    ensure_keyring_store();

    apply_prune_with_deleter(candidates, |account| {
        let entry = Entry::new("ayx", account)?;
        entry.delete_credential()
    })
}

/// Testable core of `apply_prune`: accepts an injectable deleter so unit tests
/// can exercise routing logic without a live keyring.
fn apply_prune_with_deleter<F>(candidates: Vec<PruneCandidate>, mut deleter: F) -> Vec<ApplyResult>
where
    F: FnMut(&str) -> Result<(), keyring_core::Error>,
{
    candidates
        .into_iter()
        .map(|c| {
            let status = match c.status {
                CandidateStatus::LiveRef => ApplyStatus::LiveRef,
                CandidateStatus::NotFound => ApplyStatus::NotFound,
                CandidateStatus::WouldDelete => match deleter(&c.account) {
                    Ok(()) => ApplyStatus::Deleted,
                    Err(keyring_core::Error::NoEntry) => ApplyStatus::NotFound,
                    Err(e) => ApplyStatus::Failed(e.to_string()),
                },
            };
            ApplyResult {
                profile_stem: c.profile_stem,
                account: c.account,
                status,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_slots_are_stable_and_reject_yaml_paths() {
        assert_eq!(
            slot("mongo.managed.password").unwrap().env,
            "AYX_MONGO_MANAGED_PASSWORD"
        );
        assert!(slot("mongo.managed.password_ref").is_err());
    }

    #[test]
    fn automation_template_never_contains_secret_values() {
        let template = env_template(Path::new("profile.yaml")).unwrap();
        assert!(template.contains(&"AYX_ONE_CLIENT_SECRET"));
        assert!(template.iter().all(|entry| entry.starts_with("AYX_")));
    }

    #[test]
    fn environment_names_are_safe_and_portable() {
        assert!(is_valid_env_name("AYX_SERVER_SECRET"));
        assert!(is_valid_env_name("_AYX_SECRET_2"));
        assert!(!is_valid_env_name("2AYX_SECRET"));
        assert!(!is_valid_env_name("AYX-SECRET"));
        assert!(!is_valid_env_name("AYX_SECRET=bad"));
    }

    #[test]
    fn set_stores_only_a_keyring_reference_in_the_profile() {
        ayx_core::secrets::install_test_keyring_store();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("demo.yaml");
        let mut config = crate::onboard::default_config();
        config.server = Some(ayx_core::profile::ServerProfile {
            webapi_url: "https://server.example.test".to_string(),
            curator_api_key: "client".to_string(),
            curator_api_secret: String::new(),
            curator_api_secret_ref: None,
            verify_tls: Some(true),
            derived: false,
        });
        crate::onboard::write_config_with_policy(&path, &config, InlineSecretPolicy::Forbid)
            .unwrap();

        set_slot(
            &path,
            "server.api.client-secret",
            SecretInput::Stdin("not-in-yaml".to_string()),
        )
        .unwrap();

        let profile = fs::read_to_string(&path).unwrap();
        assert!(!profile.contains("not-in-yaml"));
        assert!(profile.contains("keyring:demo/server.api.client_secret"));
        let report = inspect_profile(&path).unwrap();
        assert!(report.iter().any(|entry| {
            entry.slot == "server.api.client-secret"
                && entry.source == "keyring"
                && entry.validation == "passed"
        }));
    }

    #[test]
    fn unset_does_not_delete_an_external_or_shared_reference() {
        let temp = tempfile::tempdir().unwrap();
        let profiles = temp.path().join("profiles");
        fs::create_dir_all(&profiles).unwrap();
        let path = profiles.join("demo.yaml");
        let mut config = crate::onboard::default_config();
        config.server = Some(ayx_core::profile::ServerProfile {
            webapi_url: "https://server.example.test".to_string(),
            curator_api_key: "client".to_string(),
            curator_api_secret: String::new(),
            curator_api_secret_ref: Some("env:AYX_SHARED_SERVER_SECRET".to_string()),
            verify_tls: Some(true),
            derived: false,
        });
        crate::onboard::write_config_with_policy(&path, &config, InlineSecretPolicy::Forbid)
            .unwrap();

        let result = unset_slot(&path, "server.api.client-secret", &profiles).unwrap();
        assert!(!result.keyring_entry_deleted);
        let profile = fs::read_to_string(&path).unwrap();
        assert!(!profile.contains("AYX_SHARED_SERVER_SECRET"));
    }

    #[test]
    fn set_does_not_migrate_unrelated_plaintext_fields() {
        ayx_core::secrets::install_test_keyring_store();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("demo.yaml");
        let mut config = crate::onboard::default_config();
        config.server = Some(ayx_core::profile::ServerProfile {
            webapi_url: "https://server.example.test".to_string(),
            curator_api_key: "client".to_string(),
            curator_api_secret: String::new(),
            curator_api_secret_ref: None,
            verify_tls: Some(true),
            derived: false,
        });
        config.mongo.managed = Some(ayx_core::profile::MongoManaged {
            password: Some("unrelated-plaintext".to_string()),
            ..Default::default()
        });
        crate::onboard::write_config_exact(&path, &config, None, &BTreeSet::new()).unwrap();

        set_slot(
            &path,
            "server.api.client-secret",
            SecretInput::Stdin("new-server-secret".to_string()),
        )
        .unwrap();

        let profile = fs::read_to_string(&path).unwrap();
        assert!(profile.contains("password: unrelated-plaintext"));
        assert!(!profile.contains("keyring:demo/server.storage.mongo.managed.password"));
    }

    #[test]
    fn inspection_includes_login_and_workspace_secret_slots_without_values() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("demo.yaml");
        let mut config = crate::onboard::default_config();
        let mut one = ayx_core::profile::AlteryxOneProfile {
            access_token: Some("top-level-token-must-not-appear".to_string()),
            ..Default::default()
        };
        one.workspace_credentials.insert(
            "42".to_string(),
            ayx_core::profile::WorkspaceCredential {
                workspace_password: Some("workspace-password-must-not-appear".to_string()),
                ..Default::default()
            },
        );
        config.alteryx_one = Some(one);
        crate::onboard::write_config_exact(&path, &config, None, &BTreeSet::new()).unwrap();

        let report = inspect_profile(&path).unwrap();
        assert!(report.iter().any(|entry| {
            entry.slot == "one.access-token"
                && entry.source == "plaintext"
                && entry.validation == "warning"
        }));
        assert!(report.iter().any(|entry| {
            entry.slot == "one.workspace.42.workspace-password"
                && entry.source == "plaintext"
                && entry.validation == "warning"
        }));
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("top-level-token-must-not-appear"));
        assert!(!serialized.contains("workspace-password-must-not-appear"));
    }

    #[test]
    fn migration_covers_auth_only_plaintext_and_reports_persisted_field() {
        ayx_core::secrets::install_test_keyring_store();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("demo.yaml");
        let mut config = crate::onboard::default_config();
        let one = ayx_core::profile::AlteryxOneProfile {
            access_token: Some("auth-token-must-not-remain-in-yaml".to_string()),
            ..Default::default()
        };
        config.alteryx_one = Some(one);
        crate::onboard::write_config_exact(&path, &config, None, &BTreeSet::new()).unwrap();

        let result = migrate_profile(&path).unwrap();
        assert_eq!(result.migrated, vec!["alteryx_one.access_token"]);
        assert!(result.warning.is_none(), "a keyring was available");
        let profile = fs::read_to_string(&path).unwrap();
        assert!(!profile.contains("auth-token-must-not-remain-in-yaml"));
        assert!(profile.contains("keyring:demo/alteryx_one.access_token"));
    }

    #[test]
    fn migration_does_not_report_existing_secure_references_as_migrated() {
        ayx_core::secrets::install_test_keyring_store();
        ayx_core::secrets::store_keyring_secret("demo/alteryx_one.client_secret", "secure")
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("demo.yaml");
        let mut config = crate::onboard::default_config();
        let one = ayx_core::profile::AlteryxOneProfile {
            access_token: Some("only-this-field-migrates".to_string()),
            client_secret_ref: Some("keyring:demo/alteryx_one.client_secret".to_string()),
            ..Default::default()
        };
        config.alteryx_one = Some(one);
        crate::onboard::write_config_exact(&path, &config, None, &BTreeSet::new()).unwrap();

        let result = migrate_profile(&path).unwrap();
        assert_eq!(result.migrated, vec!["alteryx_one.access_token"]);
        assert!(result.warning.is_none(), "a keyring was available");
    }

    #[test]
    fn migration_compatibility_slots_exclude_login_managed_fields() {
        let fields = vec![
            "alteryx_one.access_token".to_string(),
            "server.storage.mongo.managed.password".to_string(),
        ];
        assert_eq!(migrated_slot_names(&fields), vec!["mongo.managed.password"]);
    }

    #[test]
    fn static_fields_count() {
        assert_eq!(STATIC_FIELDS.len(), 5);
    }

    #[test]
    fn logout_cleanup_keeps_accounts_referenced_by_another_profile() {
        let tmp = make_config_home();
        let profiles = tmp.path().join("profiles");
        let logout_profile = profiles.join("logout.yaml");
        let shared = "v1/shared-binding/access_token";
        let orphan = "v1/orphan-binding/access_token";
        fs::write(
            &logout_profile,
            format!(
                "profile_name: logout\nalteryx_one:\n  access_token_ref: keyring:{shared}\n  refresh_token_ref: keyring:{orphan}\n"
            ),
        )
        .expect("write logout profile");
        fs::write(
            profiles.join("other.yaml"),
            format!("profile_name: other\nalteryx_one:\n  access_token_ref: keyring:{shared}\n"),
        )
        .expect("write other profile");
        let candidates = HashSet::from([shared.to_string(), orphan.to_string()]);

        let deletable = unreferenced_keyring_accounts_excluding_profile(
            &profiles,
            &logout_profile,
            &candidates,
        )
        .expect("scan profiles");

        assert!(!deletable.contains(shared));
        assert!(deletable.contains(orphan));
    }

    #[test]
    fn keyring_account_sanitizes_spaces() {
        let account = keyring_account("My Profile", "some.field");
        assert_eq!(account, "My_Profile/some.field");
    }

    #[test]
    fn keyring_account_sanitizes_slashes() {
        let account = keyring_account("a/b", "field");
        assert_eq!(account, "a_b/field");
    }

    #[test]
    fn no_candidates_when_scopes_equal() {
        // file_stem = "default", profile_name = "default" → sanitize(both) equal → no orphans
        let accounts = legacy_accounts_for_mismatch("default", "default", &[]);
        assert!(accounts.is_empty());
    }

    #[test]
    fn candidates_when_scopes_differ() {
        let accounts = legacy_accounts_for_mismatch("old_name", "my-profile", &[]);
        // Five One fields plus five non-One static fields.
        assert_eq!(
            accounts.len(),
            OneSecretSlot::ALL.len() + STATIC_FIELDS.len()
        );
        assert!(
            accounts
                .iter()
                .any(|a| a == "old_name/alteryx_one.access_token")
        );
    }

    #[test]
    fn dynamic_workspace_fields_included() {
        let accounts = legacy_accounts_for_mismatch("old", "new", &["ws1"]);
        // One fields + non-One static fields + one set of workspace fields.
        assert_eq!(
            accounts.len(),
            OneSecretSlot::ALL.len() + STATIC_FIELDS.len() + OneSecretSlot::ALL.len()
        );
        assert!(
            accounts
                .iter()
                .any(|a| { a == "old/alteryx_one.workspace_credentials['ws1'].access_token" })
        );
    }

    // apply_prune tests — these run without a live keyring; they verify the
    // routing logic for LiveRef and WouldDelete candidates.
    // Actual keyring delete is exercised by the integration test in Task 4.

    #[test]
    fn apply_skips_live_refs() {
        let candidates = vec![PruneCandidate {
            profile_stem: "p".into(),
            account: "old/field".into(),
            status: CandidateStatus::LiveRef,
        }];
        let results =
            apply_prune_with_deleter(candidates, |_| panic!("should not delete a live ref"));
        assert_eq!(results[0].status, ApplyStatus::LiveRef);
    }

    #[test]
    fn apply_reports_not_found() {
        use keyring_core::Error as KError;
        let candidates = vec![PruneCandidate {
            profile_stem: "p".into(),
            account: "old/field".into(),
            status: CandidateStatus::WouldDelete,
        }];
        // Simulate NoEntry response from the keyring.
        let results = apply_prune_with_deleter(candidates, |_| Err(KError::NoEntry));
        assert_eq!(results[0].status, ApplyStatus::NotFound);
    }

    #[test]
    fn apply_reports_deleted() {
        let candidates = vec![PruneCandidate {
            profile_stem: "p".into(),
            account: "old/field".into(),
            status: CandidateStatus::WouldDelete,
        }];
        let results = apply_prune_with_deleter(candidates, |_| Ok(()));
        assert_eq!(results[0].status, ApplyStatus::Deleted);
    }

    #[test]
    fn collect_keyring_refs_extracts_accounts() {
        let yaml = "access_token_ref: keyring:my_profile/alteryx_one.access_token\n\
                    refresh_token_ref: keyring:my_profile/alteryx_one.refresh_token\n\
                    other: plain_value\n";
        let refs = keyring_refs_from_text(yaml);
        assert!(refs.contains("my_profile/alteryx_one.access_token"));
        assert!(refs.contains("my_profile/alteryx_one.refresh_token"));
        assert!(!refs.contains("plain_value"));
        assert_eq!(refs.len(), 2);
    }

    // Integration tests: use a temp dir as AYX_CONFIG_HOME.
    // No live keyring access required — candidate detection only.

    fn make_config_home() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("profiles")).unwrap();
        dir
    }

    fn write_profile(dir: &tempfile::TempDir, stem: &str, profile_name: &str, extra: &str) {
        let path = dir.path().join("profiles").join(format!("{stem}.yaml"));
        std::fs::write(path, format!("profile_name: {profile_name}\n{extra}")).unwrap();
    }

    #[test]
    fn no_candidates_when_stem_matches_profile_name() {
        let tmp = make_config_home();
        write_profile(&tmp, "default", "default", "");
        let candidates = prune_candidates(tmp.path(), None).unwrap();
        assert!(
            candidates.is_empty(),
            "expected no candidates, got {candidates:?}"
        );
    }

    #[test]
    fn detects_orphans_when_profile_name_differs_from_stem() {
        let tmp = make_config_home();
        write_profile(&tmp, "my_profile", "My Profile", "");
        let candidates = prune_candidates(tmp.path(), None).unwrap();
        let would_delete: Vec<_> = candidates
            .iter()
            .filter(|c| c.status == CandidateStatus::WouldDelete)
            .collect();
        assert_eq!(
            would_delete.len(),
            OneSecretSlot::ALL.len() + STATIC_FIELDS.len()
        );
        assert!(
            would_delete
                .iter()
                .any(|c| c.account == "My_Profile/alteryx_one.access_token")
        );
    }

    #[test]
    fn live_ref_marks_candidate_as_live() {
        let tmp = make_config_home();
        write_profile(&tmp, "my_profile", "My Profile", "");
        write_profile(
            &tmp,
            "other",
            "other",
            "access_token_ref: \"keyring:My_Profile/alteryx_one.access_token\"\n",
        );
        let candidates = prune_candidates(tmp.path(), None).unwrap();
        let live: Vec<_> = candidates
            .iter()
            .filter(|c| c.status == CandidateStatus::LiveRef)
            .collect();
        assert!(
            live.iter()
                .any(|c| c.account == "My_Profile/alteryx_one.access_token")
        );
    }

    #[test]
    fn profile_filter_scopes_to_one_profile() {
        let tmp = make_config_home();
        write_profile(&tmp, "my_profile", "My Profile", "");
        write_profile(&tmp, "other_profile", "Other Profile", "");
        let candidates = prune_candidates(tmp.path(), Some("my_profile")).unwrap();
        assert!(candidates.iter().all(|c| c.profile_stem == "my_profile"));
    }

    #[test]
    fn profile_filter_unknown_returns_ok_empty() {
        let tmp = make_config_home();
        write_profile(&tmp, "default", "default", "");
        let candidates = prune_candidates(tmp.path(), Some("nonexistent")).unwrap();
        assert!(candidates.is_empty());
    }

    #[test]
    fn workspace_credentials_produce_dynamic_fields() {
        let tmp = make_config_home();
        write_profile(
            &tmp,
            "my_profile",
            "My Profile",
            "alteryx_one:\n  workspace_credentials:\n    ws1: {}\n    ws2: {}\n",
        );
        let candidates = prune_candidates(tmp.path(), None).unwrap();
        // One fields + non-One static fields + one field set per workspace.
        assert_eq!(
            candidates.len(),
            OneSecretSlot::ALL.len() + STATIC_FIELDS.len() + OneSecretSlot::ALL.len() * 2
        );
    }
}
