# API Surface And Observability

Status: active

## Current Scope

- Server auth work should stay focused on diagnosis and simulation.
- Public API branches should remain product-scoped under `server`, `license`,
  and `one`.
- One transport and observability need to stay hardened as the surface grows.

## Next Steps

- Continue SAML-first diagnosis helpers for Server auth.
- Keep shared HTTP/auth helpers internal to the product crates.
- Standardize a single JSONL API event log across Server, License, and One.
- Keep secrets, request bodies, and raw responses redacted by default.

## Exit Criteria

- Product-specific command trees stay cleanly separated.
- API logging is consistent across products and opt-in where appropriate.
- Transport retries, envelopes, and error reporting are handled once.

