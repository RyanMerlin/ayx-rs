# Security Policy

## Reporting a vulnerability

If you believe you have found a security issue in `ayx-rs`, please report it
privately first. **Do not** open a public GitHub issue.

Please use GitHub Security Advisories for private reporting. PGP / Signal can
be arranged after the initial advisory is opened.

Please include:

- A description of the issue and its impact.
- Steps to reproduce (proof-of-concept, command line, payload, etc.).
- The version of `ayx` you tested (`ayx --version`).
- Any suggested mitigations or patches.

We aim to acknowledge reports within **3 business days** and to provide an
initial assessment within **10 business days**. Coordinated disclosure
timelines are negotiated case-by-case; the default is a **90-day** embargo
from initial report or until a fix is released, whichever is sooner.

## Scope

In scope:

- The `ayx` CLI and TUI.
- All workspace crates: `ayx-core`, `ayx-server`, `ayx-server-api`,
  `ayx-one-api`, `ayx-one`, `ayx-workflow`, `ayx-docs-schema`.
- The install scripts (`scripts/install.sh`, `scripts/install.ps1`).
- The release pipeline (`.github/workflows/`).

Out of scope:

- Issues in upstream Alteryx products (Server, Designer, Alteryx One). Please
  report those to Alteryx directly.
- Social engineering, phishing of maintainers, physical attacks.
- Vulnerabilities requiring an attacker-controlled local profile YAML on a
  trusted machine — that file is the user's authentication material and is
  treated as trusted by design.

## Secret handling expectations

`ayx` stores authentication material in the OS keyring by default
(`keyring:` references in `profile.yaml`). Inline fallback (`inline:` /
plaintext-in-YAML) is gated behind `AYX_ALLOW_INLINE_SECRETS=1` or the
caller passing `InlineSecretPolicy::Allow`. If you find a code path that
silently falls back to inline storage without that opt-in, treat it as a
security issue and report it.

## Hardening checklist for operators

- Run `ayx doctor config` after install; address every `inline:` ref.
- Pin `alteryx_one.expected_workspace_id` to enable mutation preflight (see
  the One API mutation safety gate in `ayx-one-api/src/lib.rs`).
- Set `verify_tls = true` (default) on every API profile; never use the
  global `--no-verify-tls` flag in production.
- Run mutating commands without `--apply` first to inspect the dry-run
  envelope before executing.
- Keep `audits/` on a host-local volume with 0o700 perms; treat audit
  artifacts as sensitive (they may contain workspace ids and request bodies).
- Treat `profiles/`, `workspaces/`, `state.yaml`, and observability JSONL
  logs as sensitive local artifacts; they are written with restrictive
  permissions on supported platforms, but Windows ACL review remains an
  operator responsibility.
- If you expose `ayx dashboard` beyond loopback, require dashboard auth
  (`AYX_DASHBOARD_PASSWORD` or `--auth-password`) and keep it on a trusted
  network only.
