# Discovery Substrate And Command Surface

Status: active

## Current Scope

- `ayx discover` is the primary live-tree entry point.
- Command, capability, action, and workflow discovery should stay aligned.
- `catalog` remains a supporting registry index until discovery exposes the
  same stable concepts directly.

## Next Steps

- Add an automated parity check between the live `clap` tree and the static
  `COMMAND_SPECS` catalog so `discover` and `catalog` cannot drift
  independently.
- Resolve the two-model split: `discover` (live tree) vs `catalog`
  (`COMMAND_SPECS`) are still two canonical surfaces; decide the long-term
  compatibility story.

## Exit Criteria

- `discover` can walk from command to capability to action to workflow without
  guesswork.
- Discovery output and generated docs report the same surface truth.
- Any deprecated helper commands have a compatibility story.
