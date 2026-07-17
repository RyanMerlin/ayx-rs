# Output Format

`ayx` supports structured envelopes through `--output json` and `--output yaml`.
For clean docs, scripts, and agent runs, use the command-local trailing flag form:

```powershell
ayx discover --output json
ayx catalog list --format full --scope all --output json
ayx actions list --output json
ayx actions workflows list --output json
```

Why this form is canonical:

- It keeps the command path readable and groups output formatting with the command being executed.
- It gives agents one documented invocation shape to follow.
- It matches the live examples and smoke-tested command recipes.

Do not document the legacy root-flag placement (`ayx --output json ...`). It may remain accepted for compatibility, but new examples, skills, tests, and generated documentation must use the trailing form.

Notes:

- `tui`, `completions`, and onboarding-style flows still perform direct terminal I/O in places, so they are not pure envelope commands.
- For `workflow yxdb`, keep `--csv <path>` for export and add `--output json` when you want the structured metadata alongside it.
