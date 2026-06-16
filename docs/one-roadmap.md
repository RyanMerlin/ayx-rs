# Alteryx One Roadmap Checkpoint

This is the focused next-phase task list for the One surface. The goal is to keep the
surface reliable for humans and agents while we widen coverage in measured steps.

## Task List

- [x] Validate the live auth and workspace smoke checks against env-backed credentials.
- [x] Keep the machine-readable catalog and generated command surface aligned with the live CLI.
- [x] Expand read-only One coverage before adding more mutating or bulk workflows.
- [x] Reduce the remaining stack-pressure workaround only after the One dispatch path is flatter.
- [x] Update docs and examples whenever a new One branch becomes first-class.

## Validation Gates

- `cargo fmt --all --check`
- `cargo nextest run -p ayx-rs --test one_live_smoke`
- `AYX_ONE_LIVE_SMOKE=1 cargo nextest run -p ayx-rs --test one_live_smoke`
- `cargo nextest run --workspace`
- `cargo run -q -p xtask -- refresh-command-surface`

## Recommended Next Sequence

1. Run the live smoke suite with `AYX_ONE_LIVE_SMOKE=1` after any One transport or auth change.
2. Keep `docs/command-surface.md`, `docs/cli-spec.md`, and the catalog tests synchronized with any new One command leaves.
3. Finish the progressive discovery surface so `ayx discover`-style workflows can move from command discovery to capability and tactic discovery without guesswork.
4. Decide whether `catalog` remains a long-term registry helper or becomes a compatibility alias once discovery exposes the same stable concepts directly.
5. Focus the next round on transport hardening and documented-only inventory gaps, not further dispatcher reshaping.
6. Use `docs/one-backend-inventory.md` as the source of truth for remaining One backend wiring work.
