# Changelog

## 0.8.0 — 2026-05-14

### Dependency upgrades

- `reqwest` 0.12 → 0.13 (workspace). Feature set updated to `rustls` (was `rustls-tls`) and adds `form`.
- `zip` 0.6 → 8 (workspace). `ayx-server` and `ayx-workflow` now consume the workspace pin instead of pinning locally.
- `sha2` 0.10 → 0.11 (workspace).
- `self_update` 0.43.1 → 0.44.0 (`ayx-rs`).

### Code changes required by the upgrades

- `zip`: `FileOptions` → `SimpleFileOptions` in `ayx-server/src/upgrade/service.rs` and `ayx-workflow/src/lib.rs`.
- `sha2`: `format!("{:x}", hasher.finalize())` no longer compiles because the new `Array<u8, ...>` return type does not implement `LowerHex`. Switched `ayx-workflow/src/cloud_convert.rs::checksum` to the same byte-iter idiom already used in `ayx-server/src/upgrade/manifest.rs::compute_sha256`.

### `ayx onboard` fixes

- Skip the storage backend section entirely when "Configure Alteryx Server" is N. Previously the RuntimeSettings.xml, AlteryxService.exe, and Mongo restore-target prompts ran regardless of the server answer.
- Drop the "Designer user install" yes/no prompt. The service detector now always probes `%LOCALAPPDATA%\Alteryx\bin` in addition to `C:\Program Files\Alteryx\bin`, so per-user Designer installs are picked up without asking.
- Drop the "Embedded Mongo restore target path" prompt. The value is resolved at restore time from `RuntimeSettings.xml` (`ayx-server/src/mongo.rs::resolve_embedded_restore_target_path`); existing profile values are preserved.

### Verification

- `cargo build --workspace` clean.
- `cargo test -p ayx-workflow -p ayx-server -p ayx-rs` passes (including `workflow_canary` and `one_live_smoke` integration tests).

## 0.7.0

See commit `162fb05`.
