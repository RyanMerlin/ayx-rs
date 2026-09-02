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

### Environment-sourced credentials

`ayx` reads `.env` files when it loads a profile: the one beside the profile
file always, and the one in the current working directory only when
`AYX_CONFIG_HOME` is unset.

`AYX_CONFIG_HOME` is the isolation boundary. Setting it suppresses the
working-directory `.env`, so a scratch config home cannot silently inherit
credentials from whichever checkout the process happens to be standing in.
Real process environment variables still apply either way. Point
`AYX_CONFIG_HOME` at a temporary directory for tests, CI, and scripted runs.

A credential picked up from the environment is recorded as an `env:<variable>`
reference, never as a literal value. The reference names a location, so it is
safe to serialize: the profile keeps no copy of the secret, `ayx secret status`
reports `source: env` instead of the `plaintext` warning posture, and `ayx
secret migrate` leaves it alone. `env:` references resolve against the same
`.env` view the loader used, so a credential supplied through a file still
resolves on the next command.

Keyring accounts are exact operating-system account names; the keyring does
not know which YAML profile referred to an account. For example,
`keyring:default/server.storage.mongo.managed.password` is shared by every
profile that uses that exact reference, including both `default` and
`local-dev`. Use distinct account names such as
`keyring:default/server.storage.mongo.managed.password` and
`keyring:local-dev/server.storage.mongo.managed.password` when the profiles
must have separate secrets.

### When no keyring is available

Secure storage is preferred but never required. On a host without an OS keyring
— a container, a CI runner, WSL without Secret Service — `ayx secret set` stores
the value as plaintext in the profile YAML so setup can proceed, and warns:

```text
[ayx WARN] Stored 'one.client-secret' as plaintext in the profile YAML because
no OS keyring was available. Anyone who can read the file can read the secret.
```

The warning is also returned as a `warning` field in the envelope, so agents and
CI steps can act on it. The posture keeps being reported afterwards by
`ayx doctor config` (`status: warn`, with each affected field named) and
`ayx secret status` (`validation: warning`, with remediation).

`ayx secret migrate` is the exception: its purpose is moving plaintext *into*
secure storage, so it never rewrites a secret as plaintext. Without a keyring it
reports an unfinished no-op naming what was left behind and changes nothing.

To skip plaintext entirely, reference an environment variable instead —
`ayx secret set <slot> --from-env NAME` stores `env:NAME` and needs no keyring.

## Secret lifecycle

Use named secret slots rather than arbitrary YAML paths:

- `ayx secret status` shows source, presence, and resolution state for every
  AYX-managed secret, including One login and workspace credentials, without
  returning values or reference names.
- `ayx secret set <slot>` securely prompts and stores the value in the OS
  keyring, falling back to warned plaintext when no keyring is available (see
  "When no keyring is available"). `--from-stdin` is the non-interactive input
  path; secret values are never accepted as command-line arguments.
- `ayx secret set <slot> --from-env NAME` stores `env:NAME` without reading the
  value. This is the preferred CI configuration; the CI provider injects `NAME`.
- `ayx secret validate` performs offline configuration and resolution checks
  across every AYX-managed secret, exits non-zero for unresolved/invalid
  references, and leaves live connectivity to an explicit auth/network command.
- `ayx secret unset <slot>` detaches the reference and deletes only an
  AYX-created profile-scoped keyring account proven unreferenced by other
  profiles. Manually shared references are detached but never deleted.
- `ayx secret migrate` moves supported plaintext profile values, including One
  login and workspace credentials and secrets held inside an `inline:`
  reference, into the secure store and reports the persisted field paths. It
  never writes plaintext; without a keyring it reports an unfinished no-op
  naming what was left behind.

  Known gap: a plaintext value sitting beside a *non-inline* reference that no
  longer resolves — a `keyring:` account that was wiped, for instance — is not
  detected, so migrate reports completion without moving it. The credential is
  not at risk of loss (the write boundary preserves any value its reference
  cannot reproduce) and `ayx doctor config` flags the profile, but the two
  commands disagree. Do not read a clean `secret migrate` as proof that a
  profile holds no plaintext; check `ayx doctor config` as well.

For One OAuth API-token credentials, prefer the secret-free login input paths:

- `ayx one login --auth-method oauth-refresh --refresh-token-env NAME`
- `printf '%s' "$TOKEN" | ayx one login --auth-method oauth-refresh --refresh-token-stdin`
- `ayx one login --access-token-env NAME`
- `printf '%s' "$TOKEN" | ayx one login --access-token-stdin`

These paths keep the token out of command arguments and shell history. The
legacy `--refresh-token <value>` and `--access-token <value>` flags remain for
compatibility but should not be used in shared terminals or automation logs.
The selected workspace stores `credential_kind: oauth_refresh`; email OTP uses
`credential_kind: email_otp`. `auth_rollout` continues to select only the
Wizard/Legacy OTP implementation, while `auth_mode` continues to select
user/service-principal authentication.

Refresh-token exchange and local keyring persistence are not an atomic
transaction. If a process or keyring failure occurs after the provider accepts
a rotating exchange, the CLI fails closed and tells the operator not to retry
the old pair blindly; re-import a fresh provider-issued pair.

Supported slots are `server.api.client-secret`, `mongo.managed.password`,
`sql.controller.password`, `sql.server-ui.password`, `one.client-secret`, and
`one.service-principal-client-secret`. Login-managed and workspace credentials
are visible to `status` and `validate`, and are migrated in bulk, but are not
accepted by `secret set`; update them through the appropriate One auth flow.
