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

## When an Alteryx One command fails

If a One command fails, it's often the **credential**, not the endpoint — the `/v4` API is reached directly, and an expired or stale bearer token is a common cause. First inspect the selected method without printing secrets:

```bash
ayx one auth status
ayx one auth diagnose
ayx doctor auth
```

For an email-OTP credential, run `ayx one login` interactively. For an
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
