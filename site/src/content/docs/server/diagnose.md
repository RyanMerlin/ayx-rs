---
title: Diagnose & auth
description: Use ayx server diagnose, ayx server auth, ayx server doctor, ayx server api, ayx server system-info, ayx server runtime-settings, and ayx server ayx-paths to investigate Alteryx Server health.
sidebar:
  order: 2
---

These commands let you investigate Alteryx Server configuration, connectivity, and auth without touching any running state. All commands here are read-only.

## Quick reference

| Command | What it does |
|---------|-------------|
| `ayx server diagnose startup` | Diagnose startup failures |
| `ayx server diagnose logs` | Diagnose log configuration |
| `ayx server diagnose network` | Diagnose network connectivity |
| `ayx server diagnose tls` | Diagnose TLS/certificate issues |
| `ayx server diagnose runtime-settings` | Diagnose `RuntimeSettings.xml` |
| `ayx server auth status` | Show current auth posture |
| `ayx server auth diagnose` | Diagnose auth configuration |
| `ayx server auth simulate` | Simulate an auth flow without committing |
| `ayx server doctor startup` | Structured health sweep — startup |
| `ayx server doctor logs` | Structured health sweep — logs |
| `ayx server doctor network` | Structured health sweep — network |
| `ayx server doctor runtime-settings` | Structured health sweep — runtime settings |
| `ayx server api status` | Check Gallery API reachability |
| `ayx server api diagnose` | Diagnose API connectivity |
| `ayx server api call` | Make a raw authenticated API call |
| `ayx server api import-swagger` | Import a Swagger spec |
| `ayx server system-info` | Dump system info snapshot |
| `ayx server runtime-settings` | Read `RuntimeSettings.xml` |
| `ayx server ayx-paths` | Show resolved Alteryx filesystem paths |

## Diagnose vs. doctor

Both cover startup, logs, network, and runtime-settings. Choose based on intent:

- `ayx server diagnose <domain>` — investigate a known symptom. Focuses on root-cause evidence for that domain.
- `ayx server doctor <domain>` — run a structured sweep. Good for routine checks or before a change window.

The `diagnose` branch also covers TLS (`ayx server diagnose tls`), which `doctor` does not.

## Diagnosing startup issues

When the Alteryx Server service fails to start or appears unhealthy:

```sh
ayx server diagnose startup --profile prod
```

Then run the doctor sweep to confirm no other issues:

```sh
ayx server doctor startup --profile prod
```

## Network and TLS

Check network connectivity from the admin host to the server:

```sh
ayx server diagnose network --profile prod
```

Investigate certificate issues:

```sh
ayx server diagnose tls --profile prod
```

## Auth

Snapshot the current auth configuration:

```sh
ayx --output json server auth status --profile prod
```

Diagnose a broken auth setup (SAML, Windows auth, API key):

```sh
ayx server auth diagnose --profile prod
```

Simulate a login flow without making any actual changes — useful for validating a config before applying it:

```sh
ayx server auth simulate --profile prod
```

For SAML-specific diagnosis, use the bundled tactic directly:

```sh
ayx tactics describe server.auth.saml-diagnose
ayx tactics run server.auth.saml-diagnose
```

## API health and raw calls

Check that the Gallery API is reachable and your credentials authenticate:

```sh
ayx server api status --profile prod
```

Investigate connectivity failures in detail:

```sh
ayx server api diagnose --profile prod
```

Make a raw authenticated call — useful for checking an endpoint not yet surfaced in the CLI:

```sh
ayx server api call --profile prod
```

## Runtime settings and paths

Read the active `RuntimeSettings.xml`:

```sh
ayx server runtime-settings --profile prod
```

Override the default path (`C:\ProgramData\Alteryx\RuntimeSettings.xml`):

```sh
ayx server runtime-settings --path "D:\Alteryx\RuntimeSettings.xml"
```

Diagnose settings-related issues:

```sh
ayx server diagnose runtime-settings --profile prod
```

Show where Alteryx resolves its filesystem paths:

```sh
ayx server ayx-paths --profile prod
```

## System info snapshot

Capture a full system info bundle, typically before a support case or upgrade:

```sh
ayx server system-info
```

Output goes to `system_info.json` by default. Override with `--output-file <path>`.

## JSON output

All commands accept `--output json` as a global flag:

```sh
ayx --output json server auth status --profile prod
```

The envelope is always `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`.

## Related

- [Alteryx Server overview](/server/)
- [Logs & diagnostics](/server/logs/)
- [Upgrade](/server/upgrade/)
- [Tactics & workflows](/telemetry/tactics/)
