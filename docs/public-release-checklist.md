# Public Release Checklist

`RyanMerlin/ayx-rs` is the canonical public home for `ayx`.

Use this checklist before changing repo visibility, publishing a release, or announcing the project publicly.

## Source Of Truth

- GitHub `RyanMerlin/ayx-rs` is the public source of truth for code, releases, and issue tracking.
- Any GitLab mirror is non-canonical and must not be referenced by install, update, or release documentation.
- `ayx-cli` is retired as a public distribution channel and should only exist as an archived or redirecting stub if it still exists at all.

## Release Plumbing

- `README.md`, `docs/cli-spec.md`, `scripts/install.sh`, `scripts/install.ps1`, and `ayx update` all point to `RyanMerlin/ayx-rs`.
- GitHub Actions CI runs on pull requests and pushes to `main`.
- The release workflow publishes from the current GitHub repository and does not depend on a personal access token unless there is a documented exception.
- Release artifacts include checksums and signing/provenance outputs that match the documentation.

## Sanitization Sweep

Run full-text scans for:

- secrets and token-like values
- internal domains and URLs
- Jira-style ticket IDs
- GitLab-only references
- private handoff or reverse-engineering notes

Manually review:

- `README.md`
- `docs/cli-spec.md`
- `config.yaml`
- `.env.example`
- `.github/workflows/*`
- `scripts/install.*`
- `docs/swagger-v3.json`
- `audits/*`

## GitHub Protections

Apply or verify protections for `main`:

- no force pushes
- no branch deletion
- pull request required
- at least 1 approval
- stale approval dismissal on new commits
- conversation resolution before merge
- linear history
- required status checks matching the current CI jobs

If release tags are part of the trust model, also protect `v*` tags or cover them with a ruleset.

Current required status checks should match:

- `Rustfmt`
- `Clippy`
- `Test (ubuntu-latest)`
- `Test (macos-latest)`

`cargo-audit` currently runs as advisory coverage and should remain optional unless you want it to block merges.

## Manual Verification

This repo does not assume GitHub admin API access from local tooling. Capture screenshots or export the live GitHub settings view and verify it against the protection checklist above before launch.
