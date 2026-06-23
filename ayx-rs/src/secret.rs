//! `ayx secret prune` — legacy keyring account cleanup.
//!
//! Detects and optionally deletes keyring accounts written by ayx < v0.11.0 that
//! used the mutable `profile_name` as the keyring scope.  v0.11.0+ uses the stable
//! on-disk file stem.  When these differ, old accounts become orphaned.
//!

use std::{
    collections::HashSet,
    fs,
    path::Path,
};

use anyhow::Result;
use ayx_core::secrets::keyring_account;

/// Secretizable fields that `secretize_config` may write for a given scope.
/// Dynamic workspace-credential fields are derived at runtime from the profile
/// YAML; these are the eight static ones.
const STATIC_FIELDS: &[&str] = &[
    "alteryx_one.access_token",
    "alteryx_one.refresh_token",
    "alteryx_one.client_secret",
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
    let mut accounts: Vec<String> = STATIC_FIELDS
        .iter()
        .map(|f| keyring_account(old_scope, f))
        .collect();
    for ws_id in workspace_ids {
        for suffix in ["access_token", "refresh_token", "client_secret"] {
            let field = format!(
                "alteryx_one.workspace_credentials['{ws_id}'].{suffix}"
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
            Ok(text) => { refs.extend(keyring_refs_from_text(&text)); }
            Err(e) => {
                eprintln!("warning: could not read '{}' for keyring ref scan: {}", path.display(), e);
            }
        }
    }
    Ok(refs)
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
        let Some(profile_name) = yaml_value
            .get("profile_name")
            .and_then(|v| v.as_str())
        else {
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
fn apply_prune_with_deleter<F>(
    candidates: Vec<PruneCandidate>,
    mut deleter: F,
) -> Vec<ApplyResult>
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
    fn static_fields_count() {
        assert_eq!(STATIC_FIELDS.len(), 8);
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
        // static fields only, no workspace creds
        assert_eq!(accounts.len(), STATIC_FIELDS.len());
        assert!(accounts.iter().any(|a| a == "old_name/alteryx_one.access_token"));
    }

    #[test]
    fn dynamic_workspace_fields_included() {
        let accounts = legacy_accounts_for_mismatch("old", "new", &["ws1"]);
        // 8 static + 3 per workspace
        assert_eq!(accounts.len(), STATIC_FIELDS.len() + 3);
        assert!(accounts.iter().any(|a| {
            a == "old/alteryx_one.workspace_credentials['ws1'].access_token"
        }));
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
        let results = apply_prune_with_deleter(candidates, |_| {
            panic!("should not delete a live ref")
        });
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
        assert!(candidates.is_empty(), "expected no candidates, got {candidates:?}");
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
        assert_eq!(would_delete.len(), STATIC_FIELDS.len());
        assert!(would_delete
            .iter()
            .any(|c| c.account == "My_Profile/alteryx_one.access_token"));
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
        assert!(live
            .iter()
            .any(|c| c.account == "My_Profile/alteryx_one.access_token"));
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
        // 8 static + 3 fields × 2 workspaces = 14
        assert_eq!(candidates.len(), STATIC_FIELDS.len() + 6);
    }
}
