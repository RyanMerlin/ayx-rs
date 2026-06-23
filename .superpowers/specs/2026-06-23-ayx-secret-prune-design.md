# ayx secret prune — Design Spec (v0.11.1)

## Problem

v0.11.0 changed the keyring scope from the mutable `profile_name` field to the
stable on-disk file stem.  A profile previously named `"My Profile"` (file stem
`my-profile`) had keyring accounts under the `My_Profile/` prefix; after the
first save with v0.11.0 they are rewritten under `my-profile/` and the old
`My_Profile/*` accounts are abandoned in the OS keyring.

The migration notes in `docs/releases/v0.11.0.md` call out this gap and
reference `ayx secret prune` (issue #4) as the resolution.

## Scope

**This command is migration cleanup, not a generic orphan detector.**

It deletes accounts whose names follow the pre-v0.11.0 `sanitize(profile_name)/field`
pattern and are no longer referenced by any live `keyring:` ref in the config home.
It does NOT enumerate the full keyring (cross-platform search is not portable) and
does NOT claim to find every possible stale entry.  Call it what it is.

## Command Surface

```
ayx secret prune [--profile <name>] [--apply] [--output <text|json>]
```

| Flag | Meaning |
|------|---------|
| `--profile <name>` | Scope to one profile; default = all profiles in config home |
| `--apply` | Delete the orphan accounts; without this flag the command is a dry-run |
| `--output json\|text` | Standard ayx output flag; already wired through the root |

Dry-run is the default.  The command MUST NOT delete anything without `--apply`.

## Algorithm

```
fn prune_candidates(config_home, profile_filter):
  profiles := list_profile_yamls(config_home)
  if profile_filter: profiles = profiles where stem == profile_filter
  live_refs := collect_keyring_refs(profiles)   # Set<account_string>

  candidates := []
  for path in profiles:
    config := parse_yaml(path)        # serde_yaml::from_str, not full Config load
    file_stem := path.file_stem()
    old_scope := sanitize(config.profile_name)
    new_scope := sanitize(file_stem)
    if old_scope == new_scope: continue  # no rename, nothing to prune

    fields := static_fields()
               + dynamic_fields(config)  # one per workspace_credentials key
    for field in fields:
      account := format!("{old_scope}/{field}")
      if live_refs.contains(account): push Skipped(account, reason: "live ref")
      else: push Candidate(account)

  return candidates
```

`collect_keyring_refs` reads every YAML in the profiles dir as raw text and
extracts all `keyring:<account>` substrings.  This is a text scan, not a full
parse, which avoids failing on profiles with schema-version skew.

## Field Registry

These are the exact account name suffixes that `secretize_config` writes for a
given scope.  The prune command must target exactly this set — no more, no less.

**Fixed fields** (present if the corresponding config section is populated):
```
alteryx_one.access_token
alteryx_one.refresh_token
alteryx_one.client_secret
server.api.client_secret
server.curator_api_secret
server.storage.mongo.managed.password
server.storage.sqlserver.controller.password
server.storage.sqlserver.server_ui.password
```

**Dynamic fields** (one set per workspace_id key in `alteryx_one.workspace_credentials`):
```
alteryx_one.workspace_credentials['{workspace_id}'].access_token
alteryx_one.workspace_credentials['{workspace_id}'].refresh_token
alteryx_one.workspace_credentials['{workspace_id}'].client_secret
```

The field list is defined as a compile-time constant in `ayx-rs/src/secret.rs`.
The dynamic fields are derived by parsing the YAML to find workspace keys.

## Scope Sanitization

The sanitization rule mirrors `ayx_core::secrets::keyring_account` exactly:
every character that is not ASCII alphanumeric or one of `-`, `_`, `.` is
replaced with `_`.

```rust
fn sanitize_scope(s: &str) -> String {
    s.chars().map(|c| {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }
    }).collect()
}
```

The prune module can use this directly by calling `keyring_account(scope, field)`
for both old and new scopes (the account format is `{sanitized_scope}/{field}`).

## Output Format

**Text (dry-run):**
```
ayx secret prune — dry run (2 orphan candidate(s) found)

profile: my-profile
  my_profile/alteryx_one.access_token        [would delete]
  my_profile/alteryx_one.refresh_token       [would delete]
  my_profile/server.api.client_secret        [live ref — skip]

Run with --apply to delete.
```

**Text (--apply):**
```
ayx secret prune — applied (2 deleted, 0 failed, 1 skipped)

profile: my-profile
  my_profile/alteryx_one.access_token        [deleted]
  my_profile/alteryx_one.refresh_token       [deleted]
  my_profile/server.api.client_secret        [live ref — skip]
```

**JSON envelope** (`--output json`):
```json
{
  "ok": true,
  "data": {
    "applied": false,
    "summary": { "candidates": 2, "deleted": 0, "skipped": 1, "not_found": 0, "failed": 0 },
    "entries": [
      { "profile": "my-profile", "account": "my_profile/alteryx_one.access_token", "status": "would_delete" },
      { "profile": "my-profile", "account": "my_profile/alteryx_one.refresh_token", "status": "would_delete" },
      { "profile": "my-profile", "account": "my_profile/server.api.client_secret", "status": "live_ref" }
    ]
  }
}
```

Status values: `would_delete` | `deleted` | `not_found` | `live_ref` | `failed`

`not_found` means the legacy account does not exist in the keyring (already
cleaned up or was never written).  Report it but do not treat it as an error.

`failed` means `delete_credential()` returned an unexpected error (not `NoEntry`).
A partial-delete run reports `ok: false` when any entry has status `failed`.

## Security Invariants

1. Never read, print, or compare secret values — account names only.
2. Dry-run and `--apply` use the same candidate selection logic.
3. Abort if any profile YAML in the filter set fails to parse — do not prune
   on incomplete information.  (Exception: if the file can't be read at all and
   `--profile` was not specified for it, skip and warn; don't abort the whole run.)
4. A live `keyring:` ref in any profile pointing at a candidate account is
   grounds for `live_ref` status — never delete it.
5. Only delete accounts derivable from the known pre-v0.11.0 naming scheme.
   Do not extend the blast radius to arbitrary accounts.
6. `NoEntry` from `delete_credential()` → status `not_found`, not an error.
7. `NoDefaultStore` (no keyring backend) → surface as a clear error before
   attempting any deletes.

## Error Handling

| Condition | Behavior |
|-----------|----------|
| No profiles in config home | `ok: true`, summary all zero, message "no profiles found" |
| `--profile X` not found | `ok: false`, error "profile 'X' not found" |
| Profile YAML parse failure | `ok: false`, abort — do not prune on partial info |
| No keyring backend | `ok: false`, "keyring unavailable: no backend" |
| `delete_credential` unexpected error | entry status `failed`; final result `ok: false` |
| All candidates already `not_found` | `ok: true`, summary shows `not_found` count |

## What This Does NOT Do

- Does not enumerate the full keyring (no `Entry::search`).
- Does not detect orphans from profiles that have been deleted (file gone, scope unknowable).
- Does not detect orphans from workspace envs removed from `environments.yaml`.
- Does not detect orphans from field renames or schema evolution.
- Does not help with accounts created by future naming scheme changes.

These are P3 items.  The real solution for new profiles is the stable
`secret_scope_id` roadmap item (UUID-keyed accounts that survive renames).

## Testing Strategy

**Unit tests** (in `ayx-rs/src/secret.rs`):
- `sanitize_scope` round-trips for common inputs
- `static_fields()` returns the expected 8-element list
- `legacy_accounts_for_profile` returns correct accounts when profile_name ≠ file_stem
- `legacy_accounts_for_profile` returns empty when profile_name == file_stem (sanitized)
- `collect_keyring_refs` correctly extracts `keyring:` prefixes from YAML text

**Integration tests** (in `ayx-rs/src/secret.rs` cfg(test)):
- Profile with `profile_name = "old name"` and file `old_name.yaml`: no candidates
  (sanitized profile_name already equals file_stem)
- Profile with `profile_name = "Old Name"` and file `old_name.yaml`: candidates detected
- Live ref in another profile pointing at candidate account → `live_ref` status
- `--apply` dry path (using `AYX_CONFIG_HOME` temp dir, no actual keyring): verifies
  correct candidate list without needing a live backend

All tests use `AYX_CONFIG_HOME` pointing at a temp dir.  No real keyring access
required for the candidate-detection tests.  Actual delete behavior is
covered by unit-level mock or gated behind `#[cfg(not(ci))]` where live Secret
Service is required.

## Crate Placement

All prune logic lives in `ayx-rs/src/secret.rs` (new file).  No new crate.
The `ayx_core::secrets` module already exports `keyring_account` and
`ensure_keyring_store` — use those directly.

## Implementation Notes

- Parsing profiles for the workspace key scan uses `serde_yaml::from_str::<serde_yaml::Value>`
  (not the full `Config` struct) to avoid breaking on future schema additions.
- The raw-text keyring ref scan uses a simple `contains("keyring:")` + split, not
  a full YAML parse, to be resilient to malformed or newer-schema files.
- `Entry::delete_credential()` in keyring-core 1.0.0 returns `Err(Error::NoEntry)`
  for a missing entry — handle this explicitly as `not_found`, not an error.
- The `Secret` command group is added to `Command` enum in `ayx-rs/src/main.rs`.
  Only one subcommand for now: `Prune`.
