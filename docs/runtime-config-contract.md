## Runtime Config Contract

Runtime config is central-only.

- Runtime commands resolve configuration from `AYX_CONFIG_HOME`, not from cwd-local files.
- Runtime selection order is `--profile <name>`, then `AYX_PROFILE=<name>`, then the active profile in central state, then the central `default` profile.
- Runtime `--profile` and `AYX_PROFILE` accept central profile names only. Filesystem paths are invalid.
- `config.yaml`, `environments.yaml`, and `workspace.yaml` are onboarding and migration inputs, not runtime selectors.
- `ayx profile migrate --profile <path>` is the supported bridge from legacy files into the central store.

Editor/onboarding exception:

- The TUI and onboarding/migration flows may open or edit explicit files and workspaces.
- Those explicit paths are inspection/edit targets only; they are not runtime selectors.
- Any path-based helper used by the TUI must stay visibly separate from the central runtime loader.

Forbidden patterns:

- `#[arg(long, default_value = "config.yaml")]` on runtime commands
- `PathBuf::from("config.yaml")` in runtime dispatch paths
- Docs that describe cwd-local `config.yaml` as a normal runtime mode

When adding a new runtime command:

- Use the shared runtime loader in the CLI dispatch/runtime layer plus `ayx-core::profile`.
- Treat any file-path config input as import, migrate, or onboard-only.
- If the command returns config metadata, include the selected central profile and resolved central profile path.

## Server credentials and keyring references

Server API credentials support both setup-friendly plaintext and protected
references:

- `server.api.client_secret` is the literal secret value. It is accepted for
  ease of onboarding and should produce an inline-secret warning in diagnostic
  output.
- `server.api.client_secret_ref` is a reference, not a place to paste the
  secret. It must use `keyring:<account>`, `env:<variable>`, or
  `inline:<value>`.

Keyring accounts are exact operating-system account names; the keyring does
not know which YAML profile referred to an account. For example,
`keyring:default/server.storage.mongo.managed.password` is shared by every
profile that uses that exact reference, including both `default` and
`local-dev`. Use distinct account names such as
`keyring:default/server.storage.mongo.managed.password` and
`keyring:local-dev/server.storage.mongo.managed.password` when the profiles
must have separate secrets.
