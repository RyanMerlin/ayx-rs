/// Email OTP authentication flow for Alteryx One.
///
/// `email_otp_login` performs the pure-HTTP OTP→OIDC→PAT flow: sendPasscode →
/// validatePasscode → follow the workspace-entry redirect chain (Accept: text/html
/// required — the BFF serves 302 to OIDC only for HTML requests) → POST /session
/// with the workspace password → resume OIDC → local-auth-workspace cookie →
/// mint 30-day PAT.
use std::io::Write as _;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use ayx_core::observability::redact_text;
use reqwest::blocking::{Client, Response};
use reqwest::cookie::{CookieStore as _, Jar};
use reqwest::redirect::Policy;
use serde_json::Value;

const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36";

/// Attempts for HTTP calls where repeating the same request has no
/// duplication risk (either it's read-only, or a retried POST like
/// validatePasscode/session has no side effect beyond the first success).
/// Calls where a retry COULD duplicate a side effect (sendPasscode,
/// apiAccessTokens mint) use a narrower retry predicate instead of a
/// separate constant — see `is_pre_send_failure`.
const TRANSIENT_RETRY_ATTEMPTS: u32 = 3;
/// Local re-prompt attempts against a single passcodeReferenceId before
/// falling back to sending a fresh passcode.
const OTP_ATTEMPTS_PER_REFERENCE: u32 = 3;
/// Total passcode emails sent per login() call before giving up entirely.
const MAX_OTP_SENDS: u32 = 2;

/// Retries `attempt_once` up to `max_attempts` times. `should_retry_err`
/// decides whether a returned error is worth retrying; `should_retry_ok`
/// does the same for a value that came back successfully but still
/// warrants another try (e.g. an HTTP 429/5xx that round-tripped fine at
/// the transport level). Sleeps between attempts using the crate's
/// existing jittered backoff (`crate::retry_delay`), the same pacing
/// already used by the rest of this crate's One API request loop.
///
/// Generic over `T`/`E` so the retry mechanics can be unit tested without
/// constructing real `reqwest` types (which have no public test
/// constructors) — callers plug in `reqwest::blocking::Response` /
/// `reqwest::Error` at the call site.
fn retry_transient<T, E>(
    max_attempts: u32,
    should_retry_err: impl Fn(&E) -> bool,
    should_retry_ok: impl Fn(&T) -> bool,
    mut attempt_once: impl FnMut() -> Result<T, E>,
) -> Result<T, E> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match attempt_once() {
            Ok(value) => {
                if attempt >= max_attempts || !should_retry_ok(&value) {
                    return Ok(value);
                }
            }
            Err(err) => {
                if attempt >= max_attempts || !should_retry_err(&err) {
                    return Err(err);
                }
            }
        }
        std::thread::sleep(crate::retry_delay(attempt, None));
    }
}

/// A transport failure where we're confident the request never reached the
/// server — an immediate, synchronous connect-phase failure (DNS resolution
/// failure, TCP connection refused, TLS handshake rejected). Safe to retry
/// even for calls with a side effect (an OTP email send, a PAT mint) because
/// there is no risk the server already processed the request.
///
/// Deliberately checks connection-phase only (`reqwest::Error::is_connect`),
/// NOT `is_timeout`: for the client this crate builds (a general `.timeout()`
/// with no separate `.connect_timeout()` — see `email_otp_login`), a
/// connect-phase timeout does not set `is_connect()`. reqwest's overall
/// `.timeout()` fires ahead of hyper_util's connect-wrapped error path, so a
/// connect attempt that itself times out (e.g. TCP SYN never ACKed) reports
/// as a bare `is_timeout() == true` / `is_connect() == false` failure under
/// this client's configuration, not as `is_connect()`. That case is
/// deliberately excluded from this predicate — it is NOT known to be
/// pre-send, since a request could plausibly have been in flight when the
/// overall timeout fired. Only genuinely synchronous, immediate connect
/// failures are covered here. If this client ever adds `.connect_timeout()`,
/// this predicate's coverage should be re-verified.
fn is_pre_send_failure(err: &reqwest::Error) -> bool {
    err.is_connect()
}

/// Any transport-level failure — connect or timeout, pre-send or
/// post-send — treated as retryable for calls with no duplication risk
/// (repeating the request has no side effect beyond the first successful
/// attempt). Deliberately broader than `is_pre_send_failure`: it also
/// covers a post-send timeout (waiting on a response after the connection
/// was already established), which is fine here because the caller has
/// already been classified as duplication-safe to retry regardless of
/// whether the prior attempt's request reached the server.
fn is_transient_transport_error(err: &reqwest::Error) -> bool {
    err.is_connect() || err.is_timeout()
}

/// Whether a *successfully received* response (any status) is worth
/// retrying. Reuses this crate's existing status-based retry policy
/// (`crate::should_retry_status`, already covered by
/// `retry_policy_retries_gets_but_not_mutations` in `lib.rs`) with
/// `mutating = false` — every call site that uses this predicate has
/// already been classified as duplication-safe to retry.
fn retryable_status_response(response: &Response) -> bool {
    crate::should_retry_status(response.status(), false)
}

/// Result of a successful email OTP login.
pub struct OtpAuthResult {
    /// PAT `tokenValue` returned by `/v4/apiAccessTokens`.
    pub access_token: String,
    /// Workspace GID from the argument passed in.
    pub workspace_gid: String,
    /// ISO 8601 expiry from `tokenInfo.expiredAt`, if present.
    pub token_expires_at: Option<String>,
}

/// `POST /v4/auth/sendPasscode`. Retries only on a pre-send failure — see
/// `is_pre_send_failure` — because a retry after the request reached the
/// server risks sending a second passcode email for the same login attempt.
fn send_passcode(client: &Client, base: &str, email: &str) -> Result<String> {
    let response = retry_transient(
        TRANSIENT_RETRY_ATTEMPTS,
        is_pre_send_failure,
        |_: &Response| false,
        || {
            client
                .post(format!("{base}/v4/auth/sendPasscode"))
                .json(&serde_json::json!({ "email": email }))
                .send()
        },
    )
    .context("sendPasscode request failed")?
    .error_for_status()
    .context("sendPasscode returned an error status")?;
    let send: Value = response
        .json()
        .context("sendPasscode response was not JSON")?;
    let reference_id = send["passcodeReferenceId"]
        .as_str()
        .context("sendPasscode response missing passcodeReferenceId")?
        .to_string();
    Ok(reference_id)
}

/// `POST /v4/auth/validatePasscode`. Retries on any transient transport
/// failure or retryable status — resubmitting the same code has no
/// duplication risk, unlike sendPasscode.
fn validate_passcode(
    client: &Client,
    base: &str,
    email: &str,
    reference_id: &str,
    code: &str,
) -> Result<()> {
    let response = retry_transient(
        TRANSIENT_RETRY_ATTEMPTS,
        is_transient_transport_error,
        retryable_status_response,
        || {
            client
                .post(format!("{base}/v4/auth/validatePasscode"))
                .json(&serde_json::json!({
                    "email": email,
                    "passcode": code.trim(),
                    "passcodeReferenceId": reference_id,
                }))
                .send()
        },
    )
    .context("validatePasscode request failed")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        bail!(
            "validatePasscode failed: HTTP {status}: {}",
            redact_text(&body.chars().take(200).collect::<String>())
        );
    }
    Ok(())
}

/// `POST /session` (workspace password). Retries on any transient
/// transport failure or retryable status — resubmitting the same password
/// has no duplication risk.
fn submit_workspace_password(
    client: &Client,
    base: &str,
    email: &str,
    password: &str,
) -> Result<()> {
    let response = retry_transient(
        TRANSIENT_RETRY_ATTEMPTS,
        is_transient_transport_error,
        retryable_status_response,
        || {
            client
                .post(format!("{base}/session"))
                .form(&[("email", email), ("password", password)])
                .send()
        },
    )
    .context("POST /session (workspace password) failed")?;
    if !response.status().is_success() {
        bail!(
            "workspace password rejected: POST /session returned HTTP {}",
            response.status()
        );
    }
    Ok(())
}

/// What to do after a `validate_passcode` rejection, given how many local
/// attempts have been made against the current reference (`attempt`, 1-
/// indexed, already includes the failing one) and how many passcodes have
/// been sent so far (`sends`, 1-indexed).
#[derive(Debug, PartialEq, Eq)]
enum OtpAction {
    /// Attempts remain against the current reference — ask for the code again.
    Reprompt,
    /// The local attempt budget for this reference is exhausted, but
    /// there's sends budget left — send a fresh passcode and reset.
    Resend,
    /// Both budgets are exhausted — bail.
    GiveUp,
}

fn next_otp_action(attempt: u32, sends: u32) -> OtpAction {
    if attempt < OTP_ATTEMPTS_PER_REFERENCE {
        OtpAction::Reprompt
    } else if sends < MAX_OTP_SENDS {
        OtpAction::Resend
    } else {
        OtpAction::GiveUp
    }
}

/// Sends a passcode and validates it, re-prompting on a wrong/expired code
/// (up to `OTP_ATTEMPTS_PER_REFERENCE` times against the same reference)
/// and automatically sending a fresh passcode if that budget is exhausted
/// (up to `MAX_OTP_SENDS` sends total). See the module-level design note
/// on why this doesn't need to parse the API's exact rejection reason.
fn otp_login_with_reprompt<F>(client: &Client, base: &str, email: &str, get_otp: &F) -> Result<()>
where
    F: Fn() -> Result<String>,
{
    let mut sends = 0u32;
    loop {
        sends += 1;
        let reference_id = send_passcode(client, base, email)?;
        if sends > 1 {
            eprintln!("Sent a new passcode to {email}.");
        }
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let otp = get_otp()?;
            match validate_passcode(client, base, email, &reference_id, &otp) {
                Ok(()) => return Ok(()),
                Err(err) => match next_otp_action(attempt, sends) {
                    OtpAction::Reprompt => {
                        eprintln!(
                            "Incorrect or expired passcode ({attempt}/{OTP_ATTEMPTS_PER_REFERENCE}) — try again."
                        );
                    }
                    OtpAction::Resend => {
                        eprintln!(
                            "Still not accepted after {OTP_ATTEMPTS_PER_REFERENCE} tries — sending a new passcode..."
                        );
                        break;
                    }
                    OtpAction::GiveUp => {
                        return Err(err.context(format!(
                            "passcode rejected {OTP_ATTEMPTS_PER_REFERENCE} times across {sends} passcode(s) sent"
                        )));
                    }
                },
            }
        }
    }
}

/// Authenticate via email OTP and return a 30-day PAT.
///
/// Performs the dependency-free pure-HTTP flow (no browser, no Python):
/// sendPasscode → validatePasscode → follow the workspace-entry redirect chain →
/// POST /session with workspace password → resume OIDC → local-auth-workspace
/// cookie → mint 30-day PAT.
///
/// `get_otp` is called once when the OTP email has been sent.  It should prompt
/// the user and return the 6-digit code.
pub fn email_otp_login<F>(
    base_url: &str,
    email: &str,
    workspace_gid: &str,
    get_otp: F,
) -> Result<OtpAuthResult>
where
    F: Fn() -> Result<String> + Send + 'static,
{
    email_otp_login_pure_http(base_url, email, workspace_gid, &get_otp)
}

/// Pure-HTTP email-OTP login — replicates the browser flow with reqwest only.
///
/// The entire OIDC dance is server-side on the us1 BFF, so this needs no PKCE,
/// client secret, or token exchange: maintain a cookie jar spanning the us1 and
/// pingauth domains, follow redirects, and POST the workspace password at the
/// sign-in step.  Sequence:
///   1. `POST /v4/auth/sendPasscode` → passcodeReferenceId
///   2. prompt OTP, `POST /v4/auth/validatePasscode` → account session cookies
///   3. resolve workspace name, `GET /?workspace=<name>&workspaceGid=<gid>` and
///      follow redirects → lands on the workspace password page; capture the
///      OIDC interaction id from the redirect chain
///   4. `POST /session` (form: email, workspace password)
///   5. `GET /token/<id>/resume` follow redirects → BFF sets local-auth-workspace
///   6. decode the local-auth-workspace cookie → accessToken
///   7. `POST /v4/apiAccessTokens` (Bearer) → 30-day PAT
fn email_otp_login_pure_http<F>(
    base_url: &str,
    email: &str,
    workspace_gid: &str,
    get_otp: &F,
) -> Result<OtpAuthResult>
where
    F: Fn() -> Result<String>,
{
    let base = base_url.trim_end_matches('/');
    let jar = Arc::new(Jar::default());
    let client = Client::builder()
        .cookie_provider(jar.clone())
        .redirect(Policy::none()) // we follow redirects manually to drive the flow
        .user_agent(BROWSER_UA)
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client for pure-HTTP auth")?;

    // Warm-up: lets the auth-portal associate the email with the session.
    if let Ok(warm_url) = reqwest::Url::parse_with_params(
        &format!("{base}/v4/platformAuth/session"),
        &[("email", email), ("includeInvited", "accounts,workspaces")],
    ) {
        let _ = client.get(warm_url).send();
    }

    // 1-2. Send the passcode and validate it, retrying wrong entries and
    //      automatically requesting a fresh passcode if the reference dies.
    otp_login_with_reprompt(&client, base, email, get_otp)?;

    // 3. Enter the workspace; follow redirects to the password page and capture
    //    the OIDC interaction id.
    let ws_name = resolve_workspace_name(&client, base, email, workspace_gid)?;
    let enter_url = format!("{base}/");
    let visited = follow_redirects(
        &client,
        reqwest::Url::parse_with_params(
            &enter_url,
            &[
                ("workspace", ws_name.as_str()),
                ("workspaceGid", workspace_gid),
            ],
        )
        .context("failed to build workspace-entry URL")?,
        25,
    )?;
    let interaction_id = extract_interaction_id(&visited).context(
        "could not find the OIDC interaction id in the redirect chain — \
         the auth flow may have changed",
    )?;

    // 4. Submit the workspace password.
    let ws_password = resolve_workspace_password()?;
    submit_workspace_password(&client, base, email, &ws_password)?;

    // 5. Resume the OIDC interaction; the BFF exchanges the code server-side and
    //    sets the local-auth-workspace cookie.
    let resume_url = reqwest::Url::parse(&format!("{base}/token/{interaction_id}/resume"))
        .context("failed to build resume URL")?;
    follow_redirects(&client, resume_url, 25)?;

    // 6. Read and decode the workspace bearer.
    let base_for_cookies = reqwest::Url::parse(base).context("base_url is not a valid URL")?;
    let law = cookie_value_from_jar(&jar, &base_for_cookies, "local-auth-workspace")
        .context("local-auth-workspace cookie was not set — authentication did not complete")?;
    let bearer = decode_local_auth_workspace(&law)?;

    // 7. Mint a 30-day PAT. Retries only on a pre-send failure — a retry
    //    after the request reached the server risks minting a second,
    //    orphaned PAT the caller never sees (see is_pre_send_failure).
    let csrf = cookie_value_from_jar(&jar, &base_for_cookies, "x-csrf-token").unwrap_or_default();
    let pat_payload = serde_json::json!({
        "name": "ayx-rs-cli",
        "lifetimeSeconds": 2_592_000,
    });
    let pat: Value = retry_transient(
        TRANSIENT_RETRY_ATTEMPTS,
        is_pre_send_failure,
        |_: &Response| false,
        || {
            client
                .post(format!("{base}/v4/apiAccessTokens"))
                .header("x-csrf-token", csrf.as_str())
                .header("x-alteryx-workspace-gid", workspace_gid)
                .bearer_auth(&bearer)
                .json(&pat_payload)
                .send()
        },
    )
    .context("apiAccessTokens request failed")?
    .error_for_status()
    .context("apiAccessTokens returned an error status")?
    .json()
    .context("apiAccessTokens response was not JSON")?;

    let access_token = pat["tokenValue"]
        .as_str()
        .context("PAT response missing tokenValue")?
        .to_string();
    let token_expires_at = pat["tokenInfo"]["expiredAt"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Ok(OtpAuthResult {
        access_token,
        workspace_gid: workspace_gid.to_string(),
        token_expires_at,
    })
}

// ── Pure-HTTP flow helpers ──────────────────────────────────────────────────

/// Resolve the workspace display name (e.g. "example-workspace") from its GID by
/// querying `/v4/auth/accounts`.  The workspace-entry URL needs the name.
fn resolve_workspace_name(
    client: &Client,
    base: &str,
    email: &str,
    workspace_gid: &str,
) -> Result<String> {
    let accounts_url = reqwest::Url::parse_with_params(
        &format!("{base}/v4/auth/accounts"),
        &[("includeInvited", "workspaces,accounts")],
    )
    .context("failed to build accounts URL")?;
    let accounts_resp = retry_transient(
        TRANSIENT_RETRY_ATTEMPTS,
        is_transient_transport_error,
        retryable_status_response,
        || {
            client
                .get(accounts_url.clone())
                // The accounts endpoint identifies the caller via this header, not session cookies.
                .header("x-alteryx-auth-email", email)
                .send()
        },
    )
    .context("failed to fetch /v4/auth/accounts")?;
    if !accounts_resp.status().is_success() {
        let status = accounts_resp.status();
        let body = accounts_resp.text().unwrap_or_default();
        bail!(
            "/v4/auth/accounts returned HTTP {status}: {}",
            redact_text(&body.chars().take(200).collect::<String>())
        );
    }
    let accounts: Value = accounts_resp
        .json()
        .context("/v4/auth/accounts response was not JSON")?;

    let accounts = accounts
        .as_array()
        .context("accounts response was not an array")?;
    for account in accounts {
        if let Some(workspaces) = account["workspaces"].as_array() {
            for ws in workspaces {
                if ws["gid"].as_str() == Some(workspace_gid)
                    && let Some(name) = ws["name"].as_str()
                {
                    return Ok(name.to_string());
                }
            }
        }
    }
    bail!("workspace {workspace_gid} not found in /v4/auth/accounts (not a member?)")
}

/// Read the workspace password from `AYX_ONE_WS_PASSWORD`, prompting on the
/// terminal (masked, no echo) if it is not set.
fn resolve_workspace_password() -> Result<String> {
    if let Ok(pw) = std::env::var("AYX_ONE_WS_PASSWORD")
        && !pw.is_empty()
    {
        return Ok(pw);
    }
    eprint!("Workspace password: ");
    std::io::stderr().flush().ok();
    let pw = rpassword::read_password().context(
        "failed to read workspace password (no interactive terminal available — \
         set AYX_ONE_WS_PASSWORD instead)",
    )?;
    let pw = pw.trim().to_string();
    if pw.is_empty() {
        bail!("workspace password is required (set AYX_ONE_WS_PASSWORD or enter it when prompted)");
    }
    Ok(pw)
}

/// Returns `true` if `host` is allowed given the auth base host.
///
/// Allowed hosts are: the base host itself, its parent domain (base host minus
/// its leftmost label), and any sibling subdomain of that parent.  The parent
/// must itself contain a dot so that a 2-label base like `foo.com` never
/// grants access to arbitrary `.com` hosts.
///
/// Example: base `us1.alteryxcloud.com` allows `us1.alteryxcloud.com`,
/// `alteryxcloud.com`, and `pingauth.alteryxcloud.com`, but rejects `evil.com`
/// and `alteryxcloud.com.evil.com`.
fn host_allowed(host: &str, base_host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let base_host = base_host.trim_end_matches('.').to_ascii_lowercase();
    if host == base_host {
        return true;
    }
    // Parent = base_host minus its leftmost label.  Require the parent to
    // still contain a dot so we never allow an entire TLD (e.g. ".com").
    if let Some((_, parent)) = base_host.split_once('.')
        && parent.contains('.')
    {
        return host == parent || host.ends_with(&format!(".{parent}"));
    }
    false
}

/// Follow 3xx redirects manually with the client's cookie jar, returning the
/// ordered list of every URL visited (requested URLs and redirect targets).
/// Stops at the first non-redirect response or after `max_hops` redirects.
///
/// Every redirect *target* is validated against an allowlist derived from the
/// starting URL's host: only the base host, its parent domain, and sibling
/// subdomains of that parent are permitted.  An off-domain target causes an
/// immediate error without sending a request (and therefore without forwarding
/// cookies).
fn follow_redirects(client: &Client, start: reqwest::Url, max_hops: usize) -> Result<Vec<String>> {
    let base_host = start
        .host_str()
        .context("auth base URL has no host")?
        .to_ascii_lowercase();
    let mut visited = vec![start.to_string()];
    let mut current = start;
    for _ in 0..max_hops {
        let resp = retry_transient(
            TRANSIENT_RETRY_ATTEMPTS,
            is_transient_transport_error,
            retryable_status_response,
            || {
                client
                    .get(current.clone())
                    .header(
                        reqwest::header::ACCEPT,
                        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                    )
                    .send()
            },
        )
        .with_context(|| format!("request to {current} failed"))?;
        let status = resp.status();
        if !status.is_redirection() {
            return Ok(visited);
        }
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .context("redirect response missing Location header")?;
        // Resolve relative Locations against the current URL.
        let next = current
            .join(location)
            .with_context(|| format!("invalid redirect Location: {location}"))?;
        // Validate the redirect target before following it.
        let next_host = next.host_str().unwrap_or("");
        if !host_allowed(next_host, &base_host) {
            let parent = base_host
                .split_once('.')
                .map(|(_, p)| p)
                .unwrap_or(&base_host);
            bail!(
                "refusing to follow auth redirect to off-domain host '{}' \
                 (expected an *.{} host); the auth flow may have changed or been tampered with",
                next_host,
                parent,
            );
        }
        current = next;
        visited.push(current.to_string());
    }
    bail!("exceeded {max_hops} redirects while following the auth flow")
}

/// Returns `true` if `s` has a valid shape for an OIDC interaction id:
/// 6–128 ASCII alphanumeric/`_`/`-` characters.
fn is_valid_interaction_id(s: &str) -> bool {
    (6..=128).contains(&s.len())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Pull the OIDC interaction id out of the redirect chain.  Prefers the
/// `interaction_id=` query parameter; falls back to a `/token/<id>` path
/// segment (ignoring the `/token/auth/...` callback path).
///
/// Candidates are validated with `is_valid_interaction_id` before being
/// returned; malformed or oversized values are skipped.
fn extract_interaction_id(visited: &[String]) -> Option<String> {
    for url in visited {
        if let Some(idx) = url.find("interaction_id=") {
            let rest = &url[idx + "interaction_id=".len()..];
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if is_valid_interaction_id(&id) {
                return Some(id);
            }
        }
    }
    for url in visited {
        if let Some(idx) = url.find("/token/") {
            let rest = &url[idx + "/token/".len()..];
            let seg: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if is_valid_interaction_id(&seg) && seg != "auth" {
                return Some(seg);
            }
        }
    }
    None
}

/// Decode the `local-auth-workspace` cookie (base64url JSON) and return the
/// access token.
fn decode_local_auth_workspace(cookie: &str) -> Result<String> {
    use base64::Engine as _;
    let mut padded = cookie.to_string();
    while !padded.len().is_multiple_of(4) {
        padded.push('=');
    }
    let bytes = base64::engine::general_purpose::URL_SAFE
        .decode(padded.as_bytes())
        .context("local-auth-workspace cookie was not valid base64url")?;
    let decoded: Value = serde_json::from_slice(&bytes)
        .context("local-auth-workspace cookie did not contain JSON")?;
    decoded["accessToken"]
        .as_str()
        .or_else(|| decoded["access_token"].as_str())
        .map(str::to_string)
        .context("local-auth-workspace JSON missing accessToken")
}

fn cookie_value_from_jar(jar: &Jar, url: &url::Url, name: &str) -> Option<String> {
    use reqwest::header::HeaderValue;
    let hv: HeaderValue = jar.cookies(url)?;
    let raw = hv.to_str().ok()?;
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once('=')
            && k.trim().eq_ignore_ascii_case(name)
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::retry_transient;
    use super::{OtpAction, resolve_workspace_password};
    use super::{extract_interaction_id, host_allowed, is_valid_interaction_id, next_otp_action};
    use serial_test::serial;

    #[test]
    fn next_otp_action_reprompts_when_attempts_remain() {
        assert_eq!(next_otp_action(1, 1), OtpAction::Reprompt);
        assert_eq!(next_otp_action(2, 1), OtpAction::Reprompt);
    }

    #[test]
    fn next_otp_action_resends_when_attempts_and_sends_exhausted_but_sends_remain() {
        assert_eq!(next_otp_action(3, 1), OtpAction::Resend);
    }

    #[test]
    fn next_otp_action_gives_up_when_attempts_and_sends_both_exhausted() {
        assert_eq!(next_otp_action(3, 2), OtpAction::GiveUp);
    }

    #[test]
    fn retry_transient_returns_ok_on_first_success() {
        let mut calls = 0u32;
        let result: Result<u32, &'static str> = retry_transient(
            3,
            |_: &&'static str| true,
            |_: &u32| false,
            || {
                calls += 1;
                Ok(7)
            },
        );
        assert_eq!(result, Ok(7));
        assert_eq!(calls, 1);
    }

    #[test]
    fn retry_transient_retries_on_retryable_err_then_succeeds() {
        let mut calls = 0u32;
        let result: Result<u32, &'static str> = retry_transient(
            3,
            |_: &&'static str| true,
            |_: &u32| false,
            || {
                calls += 1;
                if calls < 3 { Err("transient") } else { Ok(42) }
            },
        );
        assert_eq!(result, Ok(42));
        assert_eq!(calls, 3);
    }

    #[test]
    fn retry_transient_stops_at_max_attempts_on_persistent_err() {
        let mut calls = 0u32;
        let result: Result<u32, &'static str> = retry_transient(
            3,
            |_: &&'static str| true,
            |_: &u32| false,
            || {
                calls += 1;
                Err("still broken")
            },
        );
        assert_eq!(result, Err("still broken"));
        assert_eq!(calls, 3);
    }

    #[test]
    fn retry_transient_does_not_retry_non_retryable_err() {
        let mut calls = 0u32;
        let result: Result<u32, &'static str> = retry_transient(
            3,
            |_: &&'static str| false,
            |_: &u32| false,
            || {
                calls += 1;
                Err("terminal")
            },
        );
        assert_eq!(result, Err("terminal"));
        assert_eq!(calls, 1);
    }

    #[test]
    fn retry_transient_retries_retryable_ok_value_then_stops() {
        let mut calls = 0u32;
        let result: Result<u32, &'static str> = retry_transient(
            3,
            |_: &&'static str| true,
            |v: &u32| *v == 429,
            || {
                calls += 1;
                if calls < 2 { Ok(429) } else { Ok(200) }
            },
        );
        assert_eq!(result, Ok(200));
        assert_eq!(calls, 2);
    }

    fn connect_refused_error() -> reqwest::Error {
        // Port 1 on loopback is always unbound — connecting to it fails
        // immediately with ECONNREFUSED, deterministically and without any
        // real network access (loopback-only, no DNS involved).
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("client build");
        client
            .get("http://127.0.0.1:1/")
            .send()
            .expect_err("connecting to a closed loopback port must fail")
    }

    #[test]
    fn is_pre_send_failure_true_for_connection_refused() {
        assert!(super::is_pre_send_failure(&connect_refused_error()));
    }

    #[test]
    fn is_transient_transport_error_true_for_connection_refused() {
        assert!(super::is_transient_transport_error(&connect_refused_error()));
    }

    fn post_send_timeout_error() -> reqwest::Error {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        std::thread::spawn(move || {
            // Accept the connection but never write a response — the client's
            // own request timeout fires while waiting, not during connect.
            // Bind (not `let _ =`) so the accepted TcpStream stays open for
            // the sleep duration instead of being dropped immediately, which
            // would close the connection and produce a spurious
            // connection-reset error instead of the intended timeout.
            let _held_conn = listener.accept();
            std::thread::sleep(std::time::Duration::from_secs(5));
        });
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()
            .expect("client build");
        client
            .get(format!("http://{addr}/"))
            .send()
            .expect_err("a server that never responds must time out")
    }

    #[test]
    fn is_pre_send_failure_false_for_post_send_timeout() {
        let err = post_send_timeout_error();
        assert!(err.is_timeout(), "expected is_timeout() true, got: {err:?}");
        assert!(
            !err.is_connect(),
            "expected is_connect() false, got: {err:?}"
        );
        assert!(!super::is_pre_send_failure(&err));
    }

    #[test]
    fn is_transient_transport_error_true_for_post_send_timeout() {
        assert!(super::is_transient_transport_error(
            &post_send_timeout_error()
        ));
    }

    // ── host_allowed ──────────────────────────────────────────────────────────

    #[test]
    fn host_allowed_exact_base() {
        assert!(host_allowed("us1.alteryxcloud.com", "us1.alteryxcloud.com"));
    }

    #[test]
    fn host_allowed_sibling_subdomain() {
        assert!(host_allowed(
            "pingauth.alteryxcloud.com",
            "us1.alteryxcloud.com"
        ));
    }

    #[test]
    fn host_allowed_parent_domain() {
        assert!(host_allowed("alteryxcloud.com", "us1.alteryxcloud.com"));
    }

    #[test]
    fn host_allowed_rejects_unrelated() {
        assert!(!host_allowed("evil.com", "us1.alteryxcloud.com"));
    }

    #[test]
    fn host_allowed_rejects_subdomain_lookalike() {
        // "alteryxcloud.com.evil.com" ends with ".com" but should not match
        assert!(!host_allowed(
            "alteryxcloud.com.evil.com",
            "us1.alteryxcloud.com"
        ));
    }

    #[test]
    fn host_allowed_rejects_prefix_lookalike() {
        assert!(!host_allowed("notalteryxcloud.com", "us1.alteryxcloud.com"));
    }

    #[test]
    fn host_allowed_rejects_empty() {
        assert!(!host_allowed("", "us1.alteryxcloud.com"));
    }

    #[test]
    fn host_allowed_two_label_base_only_allows_exact() {
        // Parent of "foo.com" is "com" which has no dot — only exact match allowed.
        assert!(host_allowed("foo.com", "foo.com"));
        assert!(!host_allowed("bar.com", "foo.com"));
        assert!(!host_allowed("sub.foo.com", "foo.com"));
    }

    // ── is_valid_interaction_id ───────────────────────────────────────────────

    #[test]
    fn valid_typical_id() {
        assert!(is_valid_interaction_id("glqI9FpDHQkirawE3nYD5"));
    }

    #[test]
    fn invalid_too_short() {
        assert!(!is_valid_interaction_id("abc"));
    }

    #[test]
    fn invalid_too_long() {
        let long: String = "a".repeat(129);
        assert!(!is_valid_interaction_id(&long));
    }

    #[test]
    fn valid_min_length() {
        assert!(is_valid_interaction_id("abcdef"));
    }

    #[test]
    fn valid_max_length() {
        let s: String = "a".repeat(128);
        assert!(is_valid_interaction_id(&s));
    }

    // ── extract_interaction_id ────────────────────────────────────────────────

    #[test]
    fn extract_from_query_param() {
        let visited = vec![
            "https://us1.alteryxcloud.com/".to_string(),
            "https://pingauth.alteryxcloud.com/as/authorization.oauth2?interaction_id=glqI9FpDHQkirawE3nYD5&client_id=x".to_string(),
        ];
        assert_eq!(
            extract_interaction_id(&visited),
            Some("glqI9FpDHQkirawE3nYD5".to_string())
        );
    }

    #[test]
    fn extract_from_token_path() {
        let visited = vec![
            "https://us1.alteryxcloud.com/".to_string(),
            "https://us1.alteryxcloud.com/token/glqI9FpDHQkirawE3nYD5/resume".to_string(),
        ];
        assert_eq!(
            extract_interaction_id(&visited),
            Some("glqI9FpDHQkirawE3nYD5".to_string())
        );
    }

    #[test]
    fn extract_skips_token_auth_segment() {
        let visited = vec!["https://us1.alteryxcloud.com/token/auth/callback?code=abc".to_string()];
        // "/token/auth" has seg="auth" which is excluded; no valid id found.
        assert_eq!(extract_interaction_id(&visited), None);
    }

    #[test]
    fn extract_rejects_oversized_id() {
        let oversized = "a".repeat(129);
        let visited = vec![format!(
            "https://pingauth.alteryxcloud.com/as/authorization.oauth2?interaction_id={oversized}"
        )];
        assert_eq!(extract_interaction_id(&visited), None);
    }

    #[test]
    fn extract_returns_none_for_empty_chain() {
        assert_eq!(extract_interaction_id(&[]), None);
    }

    // ── resolve_workspace_password ───────────────────────────────────────────

    #[test]
    #[serial]
    fn resolve_workspace_password_env_var_short_circuit() {
        // nextest process-isolates each test; #[serial] additionally guards
        // against a non-nextest (threaded) runner racing on the shared env var.
        unsafe { std::env::set_var("AYX_ONE_WS_PASSWORD", "test-secret-pw") };
        let result = resolve_workspace_password();
        unsafe { std::env::remove_var("AYX_ONE_WS_PASSWORD") };
        assert_eq!(result.unwrap(), "test-secret-pw");
    }

    /// Returns `true` if this process has a real controlling terminal
    /// attached. Probed with a plain, read-only file open of the OS's
    /// canonical "current controlling terminal" path — `/dev/tty` on
    /// Unix, `CONIN$` on Windows (the same name `rpassword`'s own Windows
    /// backend opens internally via `CreateFileW`; see
    /// `rpassword-7.5.4/src/windows.rs::open_file_or_console`).
    ///
    /// Deliberately just an `open()`, nothing else: on Unix, only
    /// `tcsetattr` (not `open`) changes termios state, so this can never
    /// disable local echo or otherwise disturb the terminal — unlike
    /// actually calling `rpassword::read_password()`, which immediately
    /// clears `ECHO` on open and only restores it via a `Drop` impl that
    /// never runs if the read is abandoned mid-block (see the history
    /// note on the caller below). The handle is dropped (closed) right
    /// after the check.
    #[cfg(unix)]
    fn has_controlling_terminal_for_test() -> bool {
        std::fs::OpenOptions::new()
            .read(true)
            .open("/dev/tty")
            .is_ok()
    }

    #[cfg(windows)]
    fn has_controlling_terminal_for_test() -> bool {
        std::fs::OpenOptions::new()
            .read(true)
            .open("CONIN$")
            .is_ok()
    }

    // Neither this crate nor its CI matrix (ubuntu/macos/windows) targets any
    // other platform; this conservative fallback just proceeds to exercise
    // the real call, matching the only environment such a target would run
    // tests in (headless).
    #[cfg(not(any(unix, windows)))]
    fn has_controlling_terminal_for_test() -> bool {
        false
    }

    #[test]
    #[serial]
    fn resolve_workspace_password_no_tty_fails_cleanly() {
        // With AYX_ONE_WS_PASSWORD unset, this falls through to the masked
        // terminal read. In a headless environment (no controlling terminal
        // — true for CI on all three OSes this crate's test matrix covers)
        // rpassword returns an `Err` almost instantly, which is what this
        // test asserts. But if this test runs from a real interactive
        // terminal (a developer's local `cargo nextest run`), rpassword
        // successfully opens the terminal and blocks waiting for actual
        // keystrokes, with no timeout of its own.
        //
        // Two other approaches were tried and rejected during review:
        //   1. An unconditional, Unix-only `/dev/tty` probe-and-skip — wrong
        //      because this crate's CI runs on windows-latest too, where the
        //      real mechanism is `CONIN$`, not a filesystem path; a literal
        //      `/dev/tty` check would misreport "no terminal" on Windows
        //      unconditionally.
        //   2. Running the call on a worker thread bounded by
        //      `recv_timeout` — this avoided hanging the test, but left a
        //      real, reproducible bug: when a terminal *is* attached and the
        //      timeout fires, the abandoned thread is still parked inside
        //      `rpassword::read_password()`, which already cleared `ECHO`
        //      via `tcsetattr` on open; since the thread is never joined and
        //      the process exits without the read completing, `rpassword`'s
        //      `Drop`-based terminal restoration never runs. Verified with
        //      `forkpty`: after that test exited, the pty was left with
        //      `ECHO OFF (not restored)` — i.e. this "fix" would have
        //      silently broken a developer's shell the moment they ran
        //      `cargo nextest run` from their own terminal.
        //
        // The correct fix is to never call `rpassword` at all unless we've
        // first confirmed (non-destructively) that there's no controlling
        // terminal to corrupt.
        if has_controlling_terminal_for_test() {
            eprintln!(
                "note: resolve_workspace_password_no_tty_fails_cleanly skipped — \
                 a real controlling terminal is attached to this test process, \
                 so the no-TTY path can't be exercised here without risking a \
                 blocked read that leaves local echo disabled"
            );
            return;
        }

        unsafe { std::env::remove_var("AYX_ONE_WS_PASSWORD") };
        let err = resolve_workspace_password()
            .expect_err("expected an error with no TTY and no env var set");
        let msg = err.to_string();
        assert!(
            msg.contains("AYX_ONE_WS_PASSWORD"),
            "error should point at AYX_ONE_WS_PASSWORD as the alternative, got: {msg}"
        );
    }
}
