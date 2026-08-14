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

Run full-text scans with `git grep`, not `rg`/`grep -r`. Those tools silently skip
gitignored-but-tracked files — a file can be `git add -f`'d past its own directory's `.gitignore`
(as `docs/handoffs/` notes are, deliberately, on occasion) and still be fully present in the repo
while invisible to a plain `rg` sweep. Confirmed directly: an `rg` sweep for a real tenant
identifier found 4 files; the same search with `git grep` found 5 — the extra one was a handoff
note with real tenant data that a routine sweep had missed. `git grep` (optionally paired with `git ls-files` to be
explicit about scope) sees everything actually tracked, regardless of ignore rules.

Scan for:

- secrets and token-like values
- internal domains and URLs
- Jira-style ticket IDs
- GitLab-only references
- private handoff or reverse-engineering notes
- real tenant/customer identifiers from live-testing sessions (workspace names, ids, tier names,
  custom hostnames, real asset names) — these get pulled in easily by evidence docs recording a
  live validation pass, and have regressed back into the repo before after an earlier scrub (see
  git history around commit `d71b785`)

Manually review:

- `README.md`
- `docs/cli-spec.md`
- `docs/command-surface.md`
- `docs/roadmap/*`
- `docs/adr/*`
- `docs/fixtures/*`
- `config.yaml`
- `.github/workflows/*`
- `scripts/install.*`
- `site/scripts/sync-content.mjs`
- `audits/*`

## Functional Validation

A live validation pass per [docs/one-live-validation.md](one-live-validation.md) must be green, or have every deviation explicitly recorded, before tagging a release; the last pass for this repo's v0.15.0 release candidate ran on 2026-08-14 against a live test tenant.

## GitHub Protections

Target protections for `main` (the end state this checklist is driving toward):

- no force pushes
- no branch deletion
- pull request required
- at least 1 approval
- stale approval dismissal on new commits
- conversation resolution before merge
- linear history
- required status checks matching the current CI jobs

If release tags are part of the trust model, also protect `v*` tags or cover them with a ruleset.

### Current state

Verified against the live rulesets on `RyanMerlin/ayx-rs` via the GitHub API, 2026-07-28.

`protect-main` (branch ruleset, active, `refs/heads/main`) enforces:

- no branch deletion
- no force pushes (non-fast-forward)
- linear history
- pull request required, with 1 approving review, stale approvals dismissed on new commits, and
  conversation resolution before merge
- required status checks: all nine listed below

`protect-release-tags` (tag ruleset, active, `refs/tags/v*`) enforces no deletion and no
non-fast-forward, so a published release tag cannot be deleted or repointed. This matters because
the release trust model, sigstore keyless signing plus GitHub provenance attestations, is anchored
to the `v*` tag.

Both rulesets grant bypass to the repository-admin role and to the `merlinlabs-automation` GitHub
App, so solo maintenance and automated merges still work. That is deliberate: the rules gate outside
contributions and accidents, not the maintainer.

Current CI (`.github/workflows/ci.yml`) defines these jobs:

- `Rustfmt`
- `Clippy`
- `Test (${{ matrix.os }})`: matrix is `ubuntu-latest`, `macos-latest`, `windows-latest`, so this
  produces three separate checks: `Test (ubuntu-latest)`, `Test (macos-latest)`, `Test (windows-latest)`
- `Command surface source-of-truth`
- `Docs`
- `cargo-audit`
- `GitHub Actions lint`

**All nine are required status checks.** `cargo-audit` blocks merges; it is not advisory.
`Test (windows-latest)` is required because the project ships Windows release binaries and has
shipped a Windows-only defect before (the v0.13.1 exit panic, fixed in v0.13.2).

## Verification

Read the live rulesets rather than trusting this file, which can drift:

```bash
gh api repos/RyanMerlin/ayx-rs/rulesets
gh api repos/RyanMerlin/ayx-rs/rulesets/<id>
```

The `merlinlabs-automation` App installation holds `administration` permission, so this check is
scriptable and does not need screenshots of the settings UI. Re-run it before any launch or
visibility change, and after any ruleset edit, since a required-status-check name that no longer
matches a real CI job name silently stops gating.
