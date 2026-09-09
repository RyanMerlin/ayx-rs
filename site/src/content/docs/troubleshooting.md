---
title: Troubleshooting
description: Start with doctor, find the right reference, and trust the binary.
sidebar:
  order: 7
---

## Start with doctor

Most issues surface here. `doctor` checks config, auth, and connectivity without changing anything:

```bash
ayx doctor          # config, auth, and connectivity
ayx doctor auth     # just authentication
ayx doctor network  # just connectivity
```

Reading the JSON: a full run puts each row under `data.checks.<name>`, and two
of those nodes are easy to confuse. `data.checks.one` is the Alteryx One probe
row — its own `status`, `summary`, and `workspace_probe`. `data.checks.auth.one`
is the One section of the *auth* row, and it is the one that carries
`credential_kind`, `renews_automatically`, `access_token_expires_at`, and
`guidance`. A scoped run returns its own payload at the data root instead, so
the same node is `data.one` under `ayx doctor auth`.

`probes_run: false` in the `network` row speaks only for that row's targets.
The `one` check does make a live request when an access token is present.

## When an Alteryx One command fails

If a One command fails, it's often the **credential**, not the endpoint — the `/v4` API is reached directly, and an expired or stale bearer token is a common cause. First inspect the selected method without printing secrets:

```bash
ayx one auth status
ayx one auth diagnose
ayx doctor auth
```

For an email-OTP credential, run `ayx one login` interactively — its access
token lasts 30 days and does not renew itself, so this is expected rather than
a fault. If you would rather not repeat it, `ayx one login --oauth-api-token`
sets up the durable credential once and renews access silently thereafter. For an
`oauth_refresh` credential, the CLI normally refreshes automatically. If the
refresh token was revoked or expired, import a newly issued pair with
`--auth-method oauth-refresh` and `--refresh-token-env NAME` or
`--refresh-token-stdin`; it will not silently fall back to OTP.

## Where to look things up

| Question | Page |
|----------|------|
| Does this command exist, and what are its flags? | [Command surface](/reference/command-surface/) |
| How is configuration resolved? | [Runtime config contract](/reference/runtime-config-contract/) |
| What's the stable CLI contract? | [CLI spec](/reference/cli-spec/) |

## When the docs and your binary disagree

Trust the binary. The command reference is generated from the CLI's own command tree and checked for staleness in CI — it isn't rewritten automatically. Check your version with `ayx --version` and compare against the [release notes](/releases/).
