# Workspace and output compatibility standard

The CLI has one universal workspace selector:

```text
ayx --workspace <numeric-id|gid|exact-saved-name> one <command>
```

Resolution order is the command-line selector, `AYX_WORKSPACE`, then the
profile's active workspace. An unresolved or ambiguous selector is an error.
`--no-input` disables prompts; login must then use `--access-token` or
`--refresh-token`, and destructive operations still require `--yes`.

`alteryx_one.active_workspace_id` is the active selection. It is deliberately
separate from `expected_workspace_id`, which is a mutation safety guard and is
not changed by workspace selection.

Existing leaf-level workspace flags remain supported for compatibility. New
automation should use `--workspace`; leaf flags will be deprecated after the
universal selector has covered all workspace-scoped commands. A future major
release may remove them after a warning-and-migration period.

Compact JSON uses the versioned `ayx.output.v1` envelope. `json-full` is a
sanitized diagnostic/transport view: credentials, tokens, headers, cookies,
passwords, OTPs, and secret references remain redacted.
