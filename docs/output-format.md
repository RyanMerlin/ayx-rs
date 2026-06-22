# Output Format

`ayx` supports structured envelopes through `--output json` and `--output yaml`.
For clean docs, scripts, and agent runs, prefer the canonical global-flag form:

```powershell
ayx --output json discover
ayx --output json catalog list --format full
ayx --output json tactics list
ayx --output json workflows list
```

Why this form is preferred:

- It makes the global output mode obvious before the command tree starts.
- It keeps the invocation shape consistent across human and agent usage.
- It matches the regression tests that now cover both leading and trailing placement for representative commands.

The trailing form, for example `ayx discover --output json`, is accepted for the command families covered by the CLI smoke tests. Use it when it is more convenient, but prefer putting `--output json` first in docs, examples, and agent recipes.

Notes:

- `tui`, `completions`, and onboarding-style flows still perform direct terminal I/O in places, so they are not pure envelope commands.
- For `workflow yxdb`, keep `--csv <path>` for export and add `--output json` when you want the structured metadata alongside it.
