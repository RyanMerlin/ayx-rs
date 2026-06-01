# Alteryx One Roadmap Checkpoint

This is the focused next-phase task list for the One surface. The goal is to keep the
surface reliable for humans and agents while we widen coverage in measured steps.

## Task List

- [x] Validate the live auth and workspace smoke checks against env-backed credentials.
- [ ] Keep the machine-readable catalog and generated command surface aligned with the live CLI.
- [ ] Expand read-only One coverage before adding more mutating or bulk workflows.
- [ ] Reduce the remaining stack-pressure workaround only after the One dispatch path is flatter.
- [ ] Update docs and examples whenever a new One branch becomes first-class.

## Validation Gates

- `cargo-fmt.exe --all --check`
- `cargo nextest run -p ayx-rs --test one_live_smoke`
- `AYX_ONE_LIVE_SMOKE=1 cargo nextest run -p ayx-rs --test one_live_smoke`
- `cargo nextest run --workspace`
- `cargo run -q -p xtask -- refresh-command-surface`

## Recommended Next Sequence

1. Run the live smoke suite with `AYX_ONE_LIVE_SMOKE=1`.
2. Confirm the One auth, platform API status, and workspace list envelopes all return `ok: true`.
3. Keep the catalog and `docs/command-surface.md` in sync with any new One command leaves.
4. Only then consider deeper refactors to the One dispatcher or profile-loading path.
