# Design — `ayx one api coverage` (Alteryx One API coverage diff)

Status: design (approved 2026-07-06)
Scope: **Alteryx One only.** Alteryx Server (`ayx-server-api`) is a separate
product and explicitly out of scope for this tool.

## Problem

The implemented One `/v4` surface is broad — `ayx-one-api/src/inventory.rs`
catalogs 156 endpoints, all mapped to commands. But "what's still missing" is
not statically knowable from the repo: the `partial` surface notes describe
deferred, *uncataloged* endpoints. The authoritative gap list requires comparing
what the **live** One API actually exposes against what the CLI has wired.

We already ship both halves of the comparison:

- **What the API exposes** — `GET /v4/open-api-spec`, surfaced today as
  `ayx one platform api open-api-spec` (`ayx-rs/src/cmd/one_platform/api.rs`).
  Returns the One OpenAPI document.
- **What the CLI wires** — `ayx_one_api::inventory::SURFACES`, a
  `&[SurfaceSpec]` of `EndpointSpec { method, path, command }`.

This tool diffs them and reports `covered` / `missing` / `stale`, so choosing
the next surface to build is driven by real data, not guesswork.

## Command surface

New command: **`ayx one api coverage`**.

### Placement (accounts for the `platform` reorg)

The broader `ayx one` reorg dissolves `platform` and promotes primitives to the
top level under `one`. Rather than bury a new command under a doomed node, this
change lands the API-introspection group at its **post-reorg home** and finishes
it:

- Introduce a top-level `one api` group.
- Move `open-api-spec` from `one platform api` → `one api open-api-spec`.
- Add `one api coverage` alongside it.
- Keep a **hidden, deprecated** `one platform api open-api-spec` alias
  (`#[command(hide = true)]`) that dispatches to the new path, so existing
  scripts/users don't break. The alias is removed when the global reorg lands
  its deprecation policy.

Net: the `api` group becomes a complete, primitive-first group the reorg never
has to touch. Blast radius is two commands.

### Flags

| Flag | Purpose |
|------|---------|
| `--spec <FILE>` | Diff against a saved OpenAPI spec JSON instead of fetching live. Enables offline/CI use. |
| `--check` | Exit non-zero if `missing` is non-empty (regression gate; pairs with `--spec <committed-snapshot>` in CI). |
| `--profile <name>` | Profile for the live fetch (standard across `one` commands). |
| `--output json\|text` | Global flag. `json` → the envelope below; `text` → human table. |

Live fetch is the default (no `--spec`). `--check` works with either source.

## Diff algorithm

Operate on a canonical operation key `(METHOD, canonical_path)`.

**Canonicalization** (applied to both sides):
1. Uppercase the method.
2. Replace every `{param}` path segment with a literal `{}` — so
   `/v4/flows/{flowId}` (spec) and `/v4/flows/{id}` (inventory) collapse to the
   same key. Param-name drift must not create false gaps.
3. Reconcile the base path: OpenAPI `paths` keys may omit `/v4` when it lives in
   `servers[].url` / a `basePath`. Canonicalize both to the `/v4/…` form. If a
   spec path cannot be confidently anchored to `/v4`, it is **not** silently
   dropped — it goes to an `unmatched_spec_paths` bucket in the report.
4. Strip trailing slashes and any query/fragment.

**Buckets:**
- `covered` = spec ∩ inventory.
- `missing` = spec − inventory — endpoints the API exposes with no wired
  command. **The build backlog.**
- `stale` = inventory − spec — inventory claims an endpoint the live spec does
  not expose (removed/renamed/wrong template). **A correctness signal**, not a
  backlog item.
- `unmatched_spec_paths` = spec paths that couldn't be canonicalized to `/v4`
  (surfaced so nothing is hidden).

`missing` entries are enriched from the spec's operation object (`summary`,
`operationId`, `tags`) and grouped by resource (first `/v4/<segment>`), so the
output reads as an actionable backlog.

## Output

`--output json` envelope `data`:

```json
{
  "coverage_pct": 91.2,
  "spec_operations": 171,
  "inventory_operations": 156,
  "covered": 156,
  "missing": [
    { "method": "POST", "path": "/v4/importedDatasets", "resource": "importedDatasets",
      "summary": "Upload an imported dataset", "operation_id": "createImportedDataset" }
  ],
  "stale": [
    { "method": "GET", "path": "/v4/flows/{id}/validate", "command": "one flows validate" }
  ],
  "unmatched_spec_paths": []
}
```

`--output text`: a grouped, aligned table — a `covered N/M (pct)` header, then
`MISSING` grouped by resource, then `STALE`, then any `UNMATCHED`. Counts are
always printed even when a bucket is empty (no silent truncation).

## Architecture — reusable core

The diff is a **pure function** in `ayx-one-api`, next to the inventory it
consumes:

```rust
// ayx-one-api/src/coverage.rs
pub struct CoverageReport { /* covered, missing, stale, unmatched, counts */ }
pub fn coverage(spec: &serde_json::Value) -> CoverageReport;
```

- Input: the parsed OpenAPI JSON (the `data` payload of `open-api-spec`, or a
  `--spec` file). No I/O, no auth — trivially unit-testable.
- Reads `inventory::SURFACES` (add a `pub fn all_endpoints()` accessor if the
  const isn't already reachable).

The `ayx-rs` command (`cmd/one_api/coverage.rs`) is a thin shell:
fetch-live-or-load-`--spec` → `coverage(&spec)` → render envelope/table →
apply `--check` exit code.

This keeps the logic testable and lets a future CI job call the same core
against a committed spec snapshot without going through the binary.

## Testing

- **Unit (`ayx-one-api`)**: synthetic OpenAPI fixtures under `docs/fixtures/`
  covering:
  - param-name drift (`{flowId}` vs `{id}`) → still `covered`;
  - a spec-only op → `missing`;
  - an inventory-only op → `stale`;
  - `servers`/basePath-relative paths → correctly anchored;
  - a non-`/v4` path → `unmatched`, not dropped;
  - method-case normalization.
- **Inventory hygiene**: assert `SURFACES` has no duplicate canonical
  `(METHOD, path)` keys.
- **cli_smoke** (all platforms, un-gated): `one api coverage --help` renders;
  `one api coverage --spec <fixture> --output json` emits a valid envelope with
  the expected buckets; `--check` against a fixture with a known `missing`
  exits non-zero; the hidden `one platform api open-api-spec` alias still works.

## Non-goals

- **Server API.** `ayx-server-api` is a different product; this tool never reads
  or reports on it.
- Auto-generating command stubs for `missing` endpoints (report only).
- Suggesting endpoint semantics beyond what the spec's `summary`/`operationId`
  provides.

## Exit criteria

- `ayx one api coverage` reports covered/missing/stale/unmatched against a live
  workspace and a `--spec` file.
- `--check` gates on `missing`.
- The `api` group is primitive-first (`one api …`) with the deprecated alias in
  place; no other command paths change.
- Core diff is a pure, unit-tested function in `ayx-one-api`.
