# Discovery Substrate And Command Surface

Status: active

## Current Scope

- `ayx discover` is the primary live-tree entry point.
- Command, capability, tactic, and workflow discovery should stay aligned.
- `catalog` remains a supporting registry index until discovery exposes the
  same stable concepts directly.

## Next Steps

- Keep the command catalog aligned with the live `clap` tree.
- Expose richer command metadata for safety and agent-friendly discovery.
- Avoid splitting the command surface into multiple canonical top-level
  discovery models.

## Exit Criteria

- `discover` can walk from command to capability to tactic to workflow without
  guesswork.
- Discovery output and generated docs report the same surface truth.
- Any deprecated helper commands have a compatibility story.

