# Discovery Substrate And Command Surface

Status: active

## Current Scope

- `ayx discover` is the primary live-tree entry point and the canonical
  source for command identity, flags, positional arguments, aliases, and
  nested traversal.
- The live `clap` command tree is canonical. `ayx catalog` is a derived
  compatibility/metadata view over that same tree, not an independent
  registry: `catalog list --scope all` (the default) mirrors every visible
  command with clap-sourced `name`/`path`/`summary`; `catalog list --scope
  curated` narrows that to commands that also carry a hand-maintained
  `CATALOG_METADATA` row (`output`/`safety`/`mutating`/`prerequisites`/
  `notes`).
- **Resolved**: the two-model split is not being collapsed into a single
  surface. `discover` (live tree, rich per-command detail) and `catalog`
  (derived, flattened command-and-capability index with optional semantic
  metadata) are both canonical, for different audiences — `discover` for
  humans/agents walking or inspecting the tree, `catalog` for agents that
  want one flat, machine-readable list with safety/mutation classification
  where it exists.

## Next Steps

- **Decision: do not create a `discover` schema v2.** Keep schema v1 stable
  and preserve its narrow role as live command-tree discovery. If a concrete
  agent failure justifies it, make only additive v1 fields: exact long-option
  spelling beside the internal Clap id, canonical-versus-alias/deprecation
  markers, and references to catalog metadata. Do not turn discovery into a
  claim about tenant capability, authorization, safety, or provider response
  shape; those belong to catalog, doctor/capability checks, and dry-run.
- Add independent black-box discovery coverage for exact long-option spelling,
  inherited global options, and `--help` agreement. The current live-tree
  parity tests are valuable drift guards, but shared-source tests cannot catch
  a wrong rendering or an accidentally hidden command.
- Enrich `CATALOG_METADATA` for commands that currently show
  `metadata_status: unclassified` in `catalog list --scope all`,
  prioritizing mutating/destructive commands so their safety classification
  is explicit instead of defaulted.
- Later: decide whether `catalog list --scope curated` needs a formal
  deprecation window now that `--scope all` covers the full command tree, or
  whether it stays indefinitely as the stable pre-migration compatibility
  view for existing consumers.

## Exit Criteria

- `discover` can walk from command to capability to action to workflow without
  guesswork.
- Discovery output and generated docs report the same surface truth.
- Any deprecated helper commands have a compatibility story.
