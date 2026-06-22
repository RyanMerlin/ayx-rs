---
title: Releases
description: Versioned release notes for ayx.
sidebar:
  order: 0
---

Release notes for each tagged version of `ayx`. For current behavior, use the live docs above; for a specific binary, read the notes for that version.

## v0.10.0

**Workspace model clarified — the token determines the workspace.**  The `x-alteryx-workspace-gid` header is ignored server-side; switching workspaces requires `workspace switch` (re-points to an already-authenticated credential) or `auth login` (authenticates a new one).

**`workspace switch --workspace-id <id>`** — new command that instantly makes an already-authenticated workspace credential active.  Errors with guidance to run `auth login` if the credential doesn't exist yet.

**`workspace people` and `workspace admins` are now argless.**  `--workspace-id` has been removed; both commands are scoped to the active workspace via the token.

**Membership mutations reject a mismatched `--workspace-id`.**  `invite-users`, `remove-user`, `suspend-users`, `unsuspend-users`, `transfer`, and `transfer-assets` now error if an explicit `--workspace-id` doesn't match the active workspace.  Omit the flag and use `workspace switch` to change workspaces first.

**`auth login` warns on inline secret storage.**  When no OS keyring backend is available, the command prints a warning that credentials will be stored in the config file as plaintext.  Configuring a keyring backend (macOS Keychain, `libsecret`, Windows Credential Manager) eliminates the plaintext-at-rest risk.

**`connections connector-metadata template` placeholder output.**  When the connection type cannot be confidently inferred, `type` now emits a `<jdbc|remotefile|…>` placeholder and a `_note` field explaining the ambiguity, instead of always defaulting to `remotefile`.

- [v0.9.14](/releases/v0914/)
- [v0.9.13](/releases/v0913/)
- [v0.9.12](/releases/v0912/)
- [v0.9.10](/releases/v0910/)
- [v0.9.9](/releases/v099/)
