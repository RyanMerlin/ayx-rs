# AnswerAI Handoff

This document captures the reverse-engineering work on Alteryx AnswerAI and the recommended path for integrating it into `ayx-rs`.

## Goal

Enable direct, scriptable use of Alteryx AnswerAI chat from the CLI, while keeping browser interaction limited to auth bootstrap and fallback recovery.

The desired outcome is:

- browser login only when needed
- persistent local auth state after login
- direct API calls for chat creation, prompt submission, polling, and message retrieval

## Current Findings

### Surface

- Chat UI: `https://answer.alteryx.com/chat`
- API origin: `https://api.answer.alteryx.com`
- App model: React frontend backed by Axios/fetch calls to the API origin
- Auth model: browser session plus short-lived JWT access token

### Confirmed API Endpoints

- `GET /api/v1/me/`
- `GET /api/v1/user-privileges/`
- `GET /api/v1/agents/`
- `GET /api/v1/conversations/`
- `POST /api/v1/conversations/`
- `GET /api/v1/conversations/{conversation_id}/`
- `PATCH /api/v1/conversations/{conversation_id}/`
- `DELETE /api/v1/conversations/{conversation_id}/`
- `GET /api/v1/conversations/{conversation_id}/messages/`
- `POST /api/v1/conversations/{conversation_id}/messages/`
- `POST /api/v1/conversations/{conversation_id}/messages/{message_id}/feedback/`
- `GET /api/v1/conversations/{conversation_id}/status/`
- `GET /api/v1/conversations/search/`
- `POST /oidc/refresh/`

### Confirmed Conversation Flow

The browser emits these requests in order:

1. create a conversation
2. post the first message
3. poll task status
4. fetch conversation messages

Observed payloads:

```json
POST /api/v1/conversations/
{
  "name": "ping",
  "position": 0,
  "agent_id": "answer_ai"
}
```

```json
POST /api/v1/conversations/{conversation_id}/messages/
{
  "content": "ping",
  "filters_applied": {}
}
```

Observed message response:

```json
{
  "task_id": "d18f32ce-6523-4ade-a0ae-f8d9deee24da"
}
```

Observed status response:

```json
{
  "result_ready": true,
  "state": "SUCCESS",
  "info": ""
}
```

Observed message list response:

```json
[
  {
    "id": 215430,
    "role": "user",
    "content": "ping"
  },
  {
    "id": 215431,
    "role": "assistant",
    "content": "pong"
  }
]
```

### Confirmed Agent Catalog

`GET /api/v1/agents/` returns agent metadata such as:

- `slug`
- `display_name`
- `description`
- `chat_completion_llm_model`
- `active`
- `conversation_starters`
- `filters`

The active agent discovered in the UI is:

- slug: `answer_ai`
- display name: `Answer AI`

### Auth Behavior

Important auth facts:

- the browser session can call the API origin successfully
- the API origin has its own cookies, including:
  - `sessionid`
  - `access_token`
- `POST /oidc/refresh/` works when called from the authenticated browser context
- the access token is short-lived and rotates
- raw shell calls without browser-backed auth do not currently reproduce the working session

## Recommendation

Do not force this into the deterministic CLI core as a browser automation feature.

Best boundary:

- `codex-plugins` holds the reverse-engineered playbook and browser bootstrap guidance
- `ayx-rs` holds the eventual direct client and command surface

The browser should be used only for:

- first-time sign-in
- MFA
- token refresh fallback

After bootstrap, the CLI should call the API directly.

## Why This Belongs in `ayx-rs`

This is a product workflow with a stable HTTP contract, so the direct client fits the CLI better than a plugin once auth is solved.

It is a good candidate for `ayx-rs` because:

- it can be exposed as a clean command family
- it should return structured JSON
- it benefits from the same config/auth patterns as other Alteryx One surfaces
- it is not browser-specific after bootstrap

## Why This Also Belongs in `codex-plugins`

The plugin repo is the right place for:

- browser bootstrap notes
- prompt/playbook guidance
- endpoint reconnaissance
- reverse-engineering breadcrumbs

That material is operational knowledge, not CLI execution logic.

## Proposed CLI Shape

Suggested future command family in `ayx-rs`:

- `ayx one answer auth login`
- `ayx one answer auth status`
- `ayx one answer chat list`
- `ayx one answer chat create`
- `ayx one answer chat send`
- `ayx one answer chat messages`

Suggested config fields:

- `alteryx_one.answer.base_url`
- `alteryx_one.answer.api_url`
- `alteryx_one.answer.agent_id`
- `alteryx_one.answer.access_token`
- `alteryx_one.answer.refresh_token`
- `alteryx_one.answer.session_cookie`
- `alteryx_one.answer.browser_profile_dir`
- `alteryx_one.answer.browser_storage_state`
- `alteryx_one.answer.browser_remote_debug_port`

Suggested behavior:

- bootstrap auth interactively in a browser if no usable token exists
- persist the resulting token state locally
- refresh on demand
- fall back to browser re-auth if refresh fails
- reuse the persisted browser profile on relaunch when available
- treat browser-profile reuse as best-effort session resume, not a guarantee

## Browser Profile Contract

The CLI should support a persistent browser profile directory so auth can survive between runs.

Recommended semantics:

- `browser_profile_dir` points to the Chrome/Edge user data directory used for AnswerAI auth
- the CLI launches or attaches to that profile during bootstrap
- the profile is reused on subsequent runs when the session remains valid
- if the session is invalid or expired, the CLI reopens the browser and re-authenticates
- direct API calls remain the primary execution path after bootstrap

Suggested operational shape:

1. check whether a persisted profile directory exists
2. attach to the profile or open a browser with that profile
3. verify the API session by calling `/api/v1/me/`
4. if valid, continue without prompting
5. if invalid, send the user through browser login/MFA
6. persist the new session state back into the same profile
7. use direct HTTP calls for AnswerAI operations

## CLI Auth Bootstrap Model

Yes, the CLI can launch a browser session for the user and then become fully API-driven afterward.

That is the recommended near-term design.

Practical flow:

1. CLI starts browser auth bootstrap
2. user signs in and completes MFA
3. CLI captures auth state
4. CLI stores auth state locally
5. CLI uses direct HTTP calls for all chat operations
6. CLI re-opens browser only when refresh/bootstrap fails or the persisted profile no longer contains a valid session

## Implementation Plan for `ayx-rs`

### Phase 1: Auth and Session Plumbing

- add AnswerAI config fields to the Alteryx One branch
- define local storage for auth state
- add a browser bootstrap helper for first sign-in
- add a refresh helper that can renew the session
- add an auth status command that validates the session against `/api/v1/me/`

### Phase 2: Direct Client

- implement a thin HTTP client for `api.answer.alteryx.com`
- implement conversation list/read/create/send/status methods
- return structured envelopes in the same style as the rest of `ayx`
- keep response bodies redacted in logs by default

### Phase 3: Command Surface

- add `one answer` commands
- wire each command to the direct client
- expose JSON output for automation
- keep browser use optional and isolated to auth bootstrap

### Phase 4: Hardening

- add token refresh retry logic
- add clear auth failure messages
- add tests for request payloads and response parsing
- capture a stable error path when the browser session expires

## Reverse-Engineering Notes

Useful observations from the live site:

- the browser sends `POST /api/v1/conversations/` when starting a new prompt
- the browser sends `POST /api/v1/conversations/{id}/messages/` with the prompt content
- the response is asynchronous and returns a `task_id`
- status polling happens on `/api/v1/conversations/{task_id}/status/`
- the final assistant message is available through the conversation messages endpoint
- the UI currently shows `Answer AI` with a GPT-4.1 model metadata entry

## Open Questions

- whether the `agent_id` accepted by `POST /api/v1/conversations/` is always the slug or sometimes a numeric PK alias
- whether the refresh flow can be reproduced outside the browser with a persistent token pair
- whether the UI exposes any additional hidden filters that should be modeled in the CLI

## Files Changed In `codex-plugins`

- `plugins/alteryx-cloud/skills/playbooks/answer-ai.md`
- `plugins/alteryx-cloud/README.md`

## Suggested Immediate Next Step In `ayx-rs`

Start with the auth/session layer first. If that lands cleanly, the direct client is straightforward.

The minimum useful first command would be:

- `ayx one answer auth status`

followed by:

- `ayx one answer chat list`
- `ayx one answer chat send --chat-id <id> --message \"...\"`
