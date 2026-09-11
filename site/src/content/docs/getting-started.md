---
title: Getting started
description: Install ayx, connect it to your Alteryx One workspace, and run your first command in a couple of minutes.
sidebar:
  order: 1
---

`ayx` is a single binary. Install it, run the setup wizard, and you're connected to your Alteryx One workspace in a couple of minutes.

## Install

**macOS / Linux**

```bash
curl -fsSL https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.sh | bash
```

**Windows (PowerShell)**

```powershell
iwr https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.ps1 | iex
```

The installer downloads the latest release, verifies its SHA-256 checksum, and puts the `ayx` binary on your PATH. On macOS and Linux it adds the install directory to `~/.profile`, so open a new terminal (or run the `export PATH=…` line it prints) before your first command.

**macOS Gatekeeper:** release binaries aren't signed or notarized yet, so Gatekeeper will refuse to run the downloaded binary with something like "cannot be opened because the developer cannot be verified" or "is damaged and can't be opened". Clear the quarantine attribute to run it:

```bash
xattr -d com.apple.quarantine <path-to-binary-or-archive>
```

If you extracted a directory instead, add `-r` to clear it recursively:

```bash
xattr -dr com.apple.quarantine <path-to-directory>
```

Signing and notarization are planned.

Confirm it's installed:

```bash
ayx --version
```

## The very short version

If you are new to command-line tools, follow these five steps:

1. Open **PowerShell** on Windows, or **Terminal** on macOS/Linux.
2. Type `ayx onboard` and press Enter.
3. Answer its questions — your email, then your workspace URL.
4. Press Enter at `Log in now [Y/n]:` to log in. Type the 6-digit code sent to
   your email, then type your Alteryx One workspace password.
5. When it asks whether to save the password, press Enter to choose **Yes**.

You are connected. Try this to see your workspace:

```bash
ayx one workspace current
```

That sign-in lasts **30 days**. It does not renew itself, so you will run
`ayx one login` again when it runs out. If you would rather not do that — or
you are setting up a computer, CI job, or agent — see
[Choose how you sign in](#choose-how-you-sign-in) for the durable alternative.
You only need to do that setup once.

## Connect

Run the setup wizard:

```bash
ayx onboard
```

It asks up to four things:

- **Your email** — the account you sign in to Alteryx One with.
- **Your workspace URL or id** — paste the URL straight from your browser's address bar while you're in the workspace, e.g. `https://us1.alteryxcloud.com/auth-portal/workspaces/01ABC…`. `ayx` reads your **workspace id** and **region** out of it. (You can also paste just the id, or leave it blank and add it later.)
- **Your Alteryx One base URL** — asked only when you gave a bare workspace id. A workspace id does not say which region it lives in, so the wizard has to ask. Press Enter to accept the `https://us1.alteryxcloud.com` default it shows, or type your own region's host. Pasting the full workspace URL supplies the region and skips this question.
- **Whether to configure Alteryx Server** — answer `n` if you only use Alteryx One. Alteryx Server is a separate product with its own host and credentials.

The wizard saves this as a **profile** — your named, reusable connection — and makes it active. It then offers to log you in right away:

```text
Ready to connect. A one-time passcode will be emailed to you@example.com,
and you'll be asked for your workspace password.
This is a time-limited login: the token lasts 30 days, does not renew
automatically, and you'll sign in again when it runs out.
Prefer not to? `ayx one login --oauth-api-token` is the durable path — paste a
Client ID and Refresh Token once from the Alteryx One UI and access tokens renew
silently from then on. It suits people as well as CI and agents.
Credentials are kept in the operating-system secure store by default (this protects
them at rest; it does not extend how long they last).
Log in now [Y/n]:
```

**The default is Yes.** Pressing Enter logs you in — a real one-time passcode is emailed to you, and you'll be prompted for that **6-digit code** and your **workspace password**. Answer `n` if you'd rather do it later; the wizard prints the exact command to run when you're ready.

On the first interactive login, it then asks:

```text
Save this workspace password securely for future logins? [Y/n]
```

Press Enter to save it in your operating system's secure keyring, or answer `n` to keep it for this login only. Later `ayx one login` runs reuse the securely saved password without asking again. The keyring protects the password and the token at rest; it does not extend how long the token lasts.

For a normal human login, no auth flags are needed: `ayx one login` uses the active profile, the Wizard email-OTP flow, and secure persistence by default. Use `--profile <name>` when you want a different profile.

## Choose how you sign in

Alteryx One gives you two kinds of user credential. Both are first-class, and
both work for a person at a keyboard:

| | **Email one-time passcode** | **OAuth API token** |
|---|---|---|
| Set up with | `ayx one login` (the default) | `ayx one login --oauth-api-token` |
| You supply | A 6-digit emailed code and your workspace password | A Client ID and Refresh Token, pasted once from the Alteryx One UI |
| Lifetime | The access token expires after **30 days** | Access tokens renew **silently** for as long as the refresh token stays valid |
| Renews itself | **No** — you sign in again | **Yes**, until the refresh token is revoked or reaches the lifetime your provider configured (up to 365 days) |
| Best for | The quickest first run | Anyone who doesn't want to re-authenticate monthly, plus CI and agents |

Email OTP is the default because it is the fastest way to a working setup: no
administration page, nothing to copy. Its cost is the 30-day cycle.

### The durable path: an OAuth API token

Do this once, and `ayx` renews access tokens for you without asking. It suits a
person on a laptop just as well as an unattended job. You should not have to
paste anything again until the refresh token is revoked or reaches the lifetime
your provider configured (up to 365 days).

1. In the Alteryx One UI, open the **OAuth2.0 API Tokens** page and generate a
   token. Note the visible **Client ID** and copy the **Refresh Token** from
   the dialog. Keep the refresh token private, like a house key.
2. Run the setup command. It shows the Client ID prompt, then a hidden prompt
   for the refresh token, verifies the pair, and stores it in your operating
   system's keyring:

```bash
ayx one login --oauth-api-token
```

3. Check that it worked:

```bash
ayx one workspace current
```

From then on, run ordinary `ayx one ...` commands — they renew access
automatically. Don't run `login` as a routine step; a bare `ayx one login`
just reports that OAuth is already configured.

### Unattended setup for CI and agents

Same credential, entered without an interactive paste. Put the refresh token in
an environment variable and import it without exposing the value in command
arguments or shell history:

```bash
ayx one login --auth-method oauth-refresh \
  --refresh-token-env AYX_ONE_API_REFRESH_TOKEN
```

The CLI stores the pair in the operating-system keyring, refreshes short-lived
access tokens automatically, and keeps the OAuth method attached to the
selected workspace. It does not silently fall back to OTP when the refresh
credential is unavailable. You can also pipe the token with
`--refresh-token-stdin`; see [Connecting](/connecting/#the-durable-path-oauth-api-accessrefresh-credentials)
for PowerShell, macOS, and Linux examples.

## Verify

Check the whole path — config, auth, and connectivity — without changing anything on the server:

```bash
ayx doctor
```

Then confirm who you are and which workspace you're pointed at:

```bash
ayx whoami
```

Want machine-readable output? Ask for JSON. `--output` is a global flag — it can appear anywhere on the command line, before or after the subcommand.

```bash
ayx one workspace current --output json
```

## Interactive helpers

`ayx one open <kind> [id]` opens the relevant page in the Alteryx One web console — a workspace or a workflow — right from your terminal. Pass `--print` to get the URL instead of launching a browser, which is handy over SSH.

When you omit the id on `ayx one workflows detail` or `ayx one workflows delete`, or on `detail` for flows, connections, person, or plans (or on the hidden `ayx one job-groups detail` compatibility alias — the canonical `ayx one jobs <JOB-ID>` requires the id), `ayx` opens an interactive picker over the matching list. Off a terminal, or with `--no-input`, the same omission is a `validation` error whose `remediation.commands` names the list command to run instead.

## Where to go next

- **[Connecting to Alteryx One](/connecting/)** — the login flows in detail, and connecting Alteryx Server
- **[Common tasks](/common-tasks/)** — copy-paste recipes for the things you'll do most
- **[The safety model](/safety-model/)** — why nothing destructive runs without `--apply`
- **[Profiles & configuration](/configuration/)** — multiple workspaces, tokens, and per-run overrides
