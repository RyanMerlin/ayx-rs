# Output Format

`ayx` has two JSON contracts:

- `--output json` is the compact, versioned `ayx.output.v1` presentation
  envelope. It is the default structured result for normal inspection and
  automation.
- `--output json-full` is the complete, recursively redacted envelope for
  callers that need raw API fields, export text, or every returned item.

For clean docs, scripts, and agent runs, put the global output flag after the
complete command path:

```powershell
ayx discover --output json-full
ayx catalog list --format full --scope all --output json-full
ayx actions list --output json
ayx actions workflows list --output json
```

Why this form is preferred:

- It reads naturally from the command to the selected presentation.
- It keeps examples consistent across human and agent usage.
- Leading placement remains accepted for backwards compatibility.

Resolution order for the effective mode:

1. An explicit `--output <mode>` always wins.
2. `AYX_OUTPUT=<mode>` (case-insensitive; an unknown value is a `validation`
   error, exit 2).
3. `json` when an agent host is detected (`AYX_AGENT`, `CLAUDECODE`, or
   `AI_AGENT` set to a non-empty value other than `0`) or when stdout is not a
   terminal.
4. Otherwise `text`.

Piping `ayx … | less` therefore shows JSON since 0.20.0; set `AYX_OUTPUT=text`
in your shell profile if you prefer the text renderer in pipes.

Compact list output defaults to 20 projected rows. Use `--output-limit N` to
change that limit, or `--output-limit 0` for every projected row. Use
`json-full` when a script needs unprojected/nested fields; its payload is still
redacted for credentials and secrets. In particular, use `json-full` for
`discover --deep` because compact JSON omits the command tree needed for
progressive traversal.

Notes:

- `completions` and onboarding-style flows still perform direct terminal I/O in places, so they are not pure envelope commands.
- Success documents go to stdout; selected-format failure envelopes go to
  stderr. Verbose/debug diagnostics also use stderr.
- `yaml` serializes the full redacted envelope. `table` remains the text/list
  table presentation.
- Interactive onboarding/authentication and shell completion scripts are direct-terminal workflows; structured modes return an envelope summary.
- For `workflow yxdb`, keep `--csv <path>` for export and add `--output json`
  when you want structured metadata alongside it.
