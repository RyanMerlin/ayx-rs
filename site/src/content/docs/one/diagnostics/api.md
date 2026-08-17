---
title: API introspection
description: Fetch the live Alteryx One OpenAPI spec and diff it against the CLI's wired commands.
sidebar:
  order: 2
---

`ayx one api` has four leaves, and they are not four of a kind. `open-api-spec` and `coverage`
make real calls and are the most useful commands on this page; `status` and `diagnose` are
config-posture stubs that make no call to Alteryx One at all. See [Known
limitations](#known-limitations) before relying on the latter two.

## Quick reference

| Command | Key options | What it does |
|---|---|---|
| `ayx one api open-api-spec` | `--profile` | Fetch the live Alteryx One OpenAPI specification |
| `ayx one api coverage` | `--profile`, `--spec <FILE>`, `--check` | Diff the live spec against wired commands: covered / missing / stale |
| `ayx one api status` | `--profile` | Config-posture summary — does not call Alteryx One (see limitations) |
| `ayx one api diagnose` | `--profile` | Config-posture validation — does not call Alteryx One (see limitations) |

## Fetching the spec

```bash
ayx one api open-api-spec
ayx --output json one api open-api-spec --profile prod
```

Returns the full OpenAPI document Alteryx One's gateway serves at `GET /v4/open-api-spec`. Save it
to diff against later without a second network call:

```bash
ayx --output json one api open-api-spec | jq '.data.response' > spec.json
```

## Coverage

`coverage` is the useful command here: it diffs the live spec (or a saved one via `--spec`)
against every endpoint the CLI's `one` surface actually dispatches, and reports the gap.

```bash
ayx one api coverage
ayx --output json one api coverage | jq '{coverage_pct, missing: (.data.missing | length)}'

# Diff a saved spec instead of fetching live
ayx one api coverage --spec spec.json

# CI regression gate: exit non-zero if any spec-documented endpoint is unwired
ayx one api coverage --check
```

The envelope's field names are easy to misread — each is scoped more narrowly than it looks:

- **`coverage_pct`** is the percentage of *comparable* spec operations the CLI wires — scoped to
  the `/v4` namespace both sides can express. It's `null` when the spec contributes nothing
  comparable, not `100.0`, so an empty or malformed spec can never read as full coverage.
- **`inventory_operations`** counts distinct *canonical* inventory keys comparable against the
  spec. Query-only variants (`/v4/people` vs. `/v4/people?role=admin`) collapse to one canonical
  key here even though they're two distinct wired commands.
- **`inventory_total`** counts every distinct wired `(method, path)` row, comparable or not —
  always `>=` `inventory_operations` plus the size of `outside_spec_namespace`.
- **`missing`** — spec-documented endpoints the CLI doesn't wire yet.
- **`stale`** — CLI-wired `/v4` endpoints absent from the spec. Not necessarily broken: a stale
  endpoint can work fine and simply predate or postdate what the spec currently documents.
- **`outside_spec_namespace`** — wired endpoints on sibling services the gateway spec can't
  describe at all (`/svc-workflow`, keyed differently from `/v4`). This is a namespace
  classification only: it says a path isn't under `/v4`, not whether the command works. Liveness
  evidence for these rows lives in [the endpoint
  matrix](https://github.com/RyanMerlin/ayx-rs/blob/main/docs/one-endpoint-matrix.md), not here.

`--check` gates on `missing` only — `stale` and `outside_spec_namespace` rows don't fail it, since
neither represents a wiring regression by itself.

## Known limitations

`ayx one api status` and `ayx one api diagnose` read the profile's `api`/`server_api`-shaped
config section — the same section Alteryx **Server** commands use — and report on it. They make
**no network call to Alteryx One**. On a One-only profile with no `server_api` section configured
(the normal case for most users of this CLI), both fail outright:

```bash
$ ayx one api status
error_code: config_missing
error: config missing api/server_api section
```

If you're checking whether Alteryx One itself is reachable, these two commands won't tell you.
Use [`ayx one doctor auth`](/one/diagnostics/) instead — it makes a real call and validates actual
credentials against the tenant.

## Related

- [Diagnostics](/one/diagnostics/) — `ayx one doctor` / `ayx one inventory` health checks that do make live calls
- [Alteryx One overview](/one/) — all `ayx one` areas
- [Command surface reference](/reference/command-surface/)
