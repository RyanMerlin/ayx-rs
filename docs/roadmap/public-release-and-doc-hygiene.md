# Public Release Hygiene

Status: active

## Current Scope

- `RyanMerlin/ayx-rs` is the public source of truth for code, releases, and
  issue tracking.
- Public fixtures and generated artifacts must stay sanitized.
- Release plumbing should continue to point install, update, and publish flows
  at the public GitHub repository.

## Next Steps

- **Revoke the `PUBLIC_RELEASE_TOKEN` PAT.** It is a repo Actions secret last
  updated 2026-04-03 with zero references anywhere in the tree — no workflow or
  script consumes it. Two separate actions, and only the second one matters:
  deleting the repo secret removes it from Actions' reach, but **revoking the
  token itself in GitHub account settings** is what actually closes the
  credential. Release plumbing does not need it; `build-release.yml` uses OIDC
  and `GITHUB_TOKEN`.
- **Decide on macOS code signing.** Release binaries are currently unsigned and
  un-notarized: none of the `AYX_MACOS_*` secrets exist, so v0.14.0 published
  darwin binaries that Gatekeeper blocks. This is now loud rather than silent —
  the build reports its signing posture in the job summary and raises a warning
  annotation — and README plus the getting-started page document the `xattr -d
  com.apple.quarantine` workaround. To make signing mandatory once an Apple
  Developer account exists, populate the `AYX_MACOS_*` secrets and set the repo
  variable `AYX_REQUIRE_MACOS_SIGNING=true`; the gate is already wired and needs
  no workflow edit.
- **Resolve the `codex/release-v0.9.10` branch.** It is the last stale remote
  branch and was kept deliberately when the others were pruned, because unlike
  them it carries genuinely unmerged content: a `Mutex` serializing environment
  mutations in `onboard.rs` tests (main has no equivalent) and a 39-file doc
  sweep moving `--output json` to a trailing position in examples. Either land
  the test-serialization fix or delete the branch consciously, rather than
  leaving it to rot.
- Decide whether the workspace template writer should stop emitting editable
  placeholder secrets and move fully to env/keyring-first guidance.

## Exit Criteria

- No docs or scripts point to private or retired distribution channels.
- Public release checks are documented and repeatable.
- Sanitization sweeps stay green before release cuts.
- No unreferenced credentials remain configured on the repository.
