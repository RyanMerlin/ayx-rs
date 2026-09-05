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

If you are new to command-line tools, follow these four steps:

1. Open **PowerShell** on Windows, or **Terminal** on macOS/Linux.
2. Type `ayx onboard` and press Enter.
3. Say **y** when it asks whether to log in. Type the 6-digit code sent to
   your email, then type your Alteryx One workspace password.
4. When it asks whether to save the password, press Enter to choose **Yes**.

You are connected. Try this to see your workspace:

```bash
ayx one workspace current
```

If you were given an OAuth2.0 API access/refresh pair for a computer, CI job,
or agent, use the [automation checklist](#using-an-api-token-for-automation)
instead of the email-code step. You only need to do that setup once.

## Connect

Run the setup wizard:

```bash
ayx onboard
```

It asks for two things:

- **Your email** — the account you sign in to Alteryx One with.
- **Your workspace URL** — paste it straight from your browser's address bar while you're in the workspace, e.g. `https://us1.alteryxcloud.com/auth-portal/workspaces/01ABC…`. `ayx` reads your **workspace id** and **region** out of it. (You can also paste just the id, or leave it blank and add it later.)

The wizard saves this as a **profile** — your named, reusable connection — and makes it active. It then offers to log you in right away:

```text
Ready to connect. A one-time passcode will be emailed to you@example.com,
and you'll be asked for your workspace password.
Log in now [y/N]:
```

Answer **y** and you'll be prompted for the **6-digit passcode** emailed to you and your **workspace password**. `ayx` completes the sign-in and stores a 30-day token in your profile. On the first interactive login, it then asks:

```text
Save this workspace password securely for future logins? [Y/n]
```

Press Enter to save it in your operating system's secure keyring, or answer `n` to keep it for this login only. Later `ayx one login` runs reuse the securely saved password without asking again. (Prefer to do it later? Answer **n** at the onboarding prompt — the wizard prints the exact command to run when you're ready.)

For a normal human login, no auth flags are needed: `ayx one login` uses the active profile, the Wizard email-OTP flow, and secure persistence by default. Use `--profile <name>` when you want a different profile.

You won't need another passcode until the stored token expires (about 30 days); it's reused for every command in between.

For unattended automation, CI, or agents, use an OAuth2.0 API access/refresh
pair instead of OTP. Create the API-token credential in Alteryx One, configure
its client ID and token endpoint in the profile, and import the refresh token
without putting its value in command arguments:

### Using an API token for automation

This is the simple machine-login checklist:

1. Get the OAuth **client ID**, **token endpoint**, and **refresh token** from
   your Alteryx One administrator. Keep the token private, like a house key.
2. Put the refresh token in the environment variable named
   `AYX_ONE_API_REFRESH_TOKEN`, or ask your administrator to configure that
   variable for you. The client ID and token endpoint go in the selected
   profile.
3. Run the command below. It reads the token without showing it in the
   command itself:

```bash
ayx one login --auth-method oauth-refresh \
  --refresh-token-env AYX_ONE_API_REFRESH_TOKEN
```

4. Check that it worked:

```bash
ayx one workspace current
```

With secure persistence, the CLI stores the pair in the operating-system
keyring, refreshes short-lived access tokens automatically, and keeps the OAuth
method attached to the selected workspace. It does not silently fall back to
OTP when the refresh credential is unavailable. You can also pipe the token
with `--refresh-token-stdin`; see [Connecting](/connecting/#oauth-api-accessrefresh-credentials)
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

When you omit the id on `ayx one workflows detail` or `ayx one workflows delete`, or on `detail` for flows, connections, job-groups, person, or plans, `ayx` opens an interactive picker over the matching list. Off a terminal, or with `--no-input`, the same omission is a `validation` error whose `remediation.commands` names the list command to run instead.

## Where to go next

- **[Connecting to Alteryx One](/connecting/)** — the login flows in detail, and connecting Alteryx Server
- **[Common tasks](/common-tasks/)** — copy-paste recipes for the things you'll do most
- **[The safety model](/safety-model/)** — why nothing destructive runs without `--apply`
- **[Profiles & configuration](/configuration/)** — multiple workspaces, tokens, and per-run overrides
