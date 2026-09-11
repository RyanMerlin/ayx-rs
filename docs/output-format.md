# Output Format

`ayx` has one JSON contract: `--output json` is the complete, recursively
redacted envelope for automation, raw API inspection, and export metadata.

```json
{
  "ok": true,
  "command": "one.jobs.runs",
  "message": "jobGroup jobs ok",
  "timestamp_utc": "2026-09-11T12:00:00Z",
  "data": { "...": "..." }
}
```

| Field | Present | Meaning |
|---|---|---|
| `ok` | always | `true` on stdout success, `false` on the stderr failure envelope. |
| `command` | every parsed command | Dotted id of the leaf that ran, such as `one.jobs.runs`. Compatibility aliases report the canonical id. Use it to correlate a result with its invocation. Omitted, never guessed, when no command was resolved (for example an invalid `AYX_OUTPUT`). |
| `message` | always | One human sentence. Do not parse it. |
| `timestamp_utc` | always | RFC 3339 time the envelope was built. |
| `data` | always | The complete redacted payload; may be `null`. |
| `error_code` | failures | Stable classification; see [`cli-schema.json`](cli-schema.json). |
| `remediation` | some failures | `{ summary, commands }`: the next step. |
| `retryable` | failures | Whether an identical retry may succeed. |
| `next` | some successes | Follow-up commands, such as the `--page-token` continuation. |

The machine-readable definition is [`cli-schema.json`](cli-schema.json). YAML
carries the same fields.

For clean docs, scripts, and agent runs, put the global output flag after the
complete command path:

```powershell
ayx discover --output json
ayx catalog list --format full --scope all --output json
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
   error, exit 2). An empty `AYX_OUTPUT` is ignored.
3. `json` when an agent host is detected (`AYX_AGENT`, `CLAUDECODE`, or
   `AI_AGENT` set to a non-empty value other than `0`) or when stdout is not a
   terminal.
4. Otherwise `text`.

Piping `ayx … | less` therefore shows JSON since 0.20.0; set `AYX_OUTPUT=text`
in your shell profile if you prefer the text renderer in pipes.

Human list output defaults to 20 projected rows. Use `--output-limit N` to
change that limit, or `--output-limit 0` for every projected row. JSON always
retains the complete redacted response, including nested fields and command
trees.

Notes:

- `completions` and onboarding-style flows still perform direct terminal I/O in places, so they are not pure envelope commands.
- Success documents go to stdout; selected-format failure envelopes go to
  stderr. Verbose/debug diagnostics also use stderr.
- `yaml` serializes the full redacted envelope. `table` remains the text/list
  table presentation.
- Interactive onboarding/authentication and shell completion scripts are direct-terminal workflows; structured modes return an envelope summary.
- For `workflow yxdb`, keep `--csv <path>` for export and add `--output json`
  when you want structured metadata alongside it.

## `--jq`

`--jq <FILTER>` runs a jq filter (pure-Rust `jaq`; jq 1.7 syntax and the
standard library) over the rendered JSON and prints one value per line.
`--raw-output` / `-r` prints string results without quotes. `--jq` forces
`--output json`, and it runs after redaction, so it cannot reveal anything the plain output
would not. A filter that fails to parse, compile, or run is a `validation`
error (exit 2).

The filter runs on the rendered, redacted document. The `env`/`$ENV` and
`now` builtins, and the wall-clock/timezone builtins (`strftime`,
`strflocaltime`, `gmtime`, `localtime`, `mktime`, `strptime`), are not
available, and `halt`/`halt_error` are rejected, so a filter cannot read the
process environment or host clock, or change the exit code.
