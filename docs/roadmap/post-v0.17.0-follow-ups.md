# Post-v0.17.0 Follow-ups

Status: active

The stable `v0.17.0` release shipped on 2026-08-27. The release itself is
complete; the items below are follow-up work identified during the release
and documentation sweep.

## Security and release hygiene

- [ ] Revoke the unused `PUBLIC_RELEASE_TOKEN` PAT in GitHub account settings.
  Removing the repository secret alone does not revoke the credential.
- [ ] Decide on macOS code signing and notarization. Until the Apple signing
  secrets and required gate are configured, macOS artifacts remain unsigned
  and users may need to clear Gatekeeper quarantine.
- [ ] Review and clean local `.temp/` and ignored handoff artifacts. Inspect
  for credential-bearing runtime state before removal or revocation; these
  files are not part of the public repository.
- [ ] Resolve the stale remote `codex/release-v0.9.10` branch: either land its
  environment-mutation test serialization and documentation changes, or
  delete the branch deliberately.

## Documentation and behavior

- [ ] Update or archive `docs/announcements/v0.17.0-internal-rollout.md`,
  which still describes the RC3 internal rollout.
- [ ] Resolve the known `env:`-backed secret reference load/save limitation:
  loading a profile and saving it can materialize the resolved value instead
  of preserving the `env:` reference.
- [ ] Decide whether the workspace template writer should stop emitting
  editable placeholder secrets and move fully to env/keyring-first guidance.

## Dependency maintenance

Review the open dependency pull requests after the stable release:

- [ ] [#169](https://github.com/RyanMerlin/ayx-rs/pull/169) — patch dependency updates
- [ ] [#153](https://github.com/RyanMerlin/ayx-rs/pull/153) — GitHub Actions install-action update
- [ ] [#144](https://github.com/RyanMerlin/ayx-rs/pull/144) — `serial_test` update

## Product backlog

These open One-surface issues were explicitly carried forward rather than
treated as release blockers:

- [ ] [#134](https://github.com/RyanMerlin/ayx-rs/issues/134) — live-verify plans/schedules paths
- [ ] [#133](https://github.com/RyanMerlin/ayx-rs/issues/133) — custom-role creation endpoint
- [ ] [#132](https://github.com/RyanMerlin/ayx-rs/issues/132) — workflow composability into Plan nodes
- [ ] [#125](https://github.com/RyanMerlin/ayx-rs/issues/125) — enforce registry `schema_version`
- [ ] [#123](https://github.com/RyanMerlin/ayx-rs/issues/123) — gate CI on `ayx actions validate`
- [ ] [#111](https://github.com/RyanMerlin/ayx-rs/issues/111) — plans import/export round-trip
- [ ] [#110](https://github.com/RyanMerlin/ayx-rs/issues/110) — dataset import/upload
- [ ] [#109](https://github.com/RyanMerlin/ayx-rs/issues/109) — scheduling mutations

## Release closure

- [x] Align workspace, lockfile, CLI output, release tag, and docs at
  `0.17.0`.
- [x] Pass CI and publish signed/provenanced multi-platform artifacts.
- [x] Deploy and verify the stable documentation site.
