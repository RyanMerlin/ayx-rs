# API Surface And Observability

Status: active

## Current Scope

- Server auth work should stay focused on diagnosis and simulation.
- Public API branches should remain product-scoped under `server`, `license`,
  and `one`.
- One transport and observability need to stay hardened as the surface grows.

## Next Steps

- Complete shared JSONL API logging for `license` commands (they still return
  static envelopes and emit no `record_api_event`).
- Move the generic license helpers out of `ayx-one-api` so HTTP/auth helper
  placement is product-pure.

## Exit Criteria

- Product-specific command trees stay cleanly separated.
- API logging is consistent across products and opt-in where appropriate.
- Transport retries, envelopes, and error reporting are handled once.
