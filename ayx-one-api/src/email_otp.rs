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
use reqwest::blocking::Client;
use reqwest::cookie::{CookieStore as _, Jar};
use reqwest::redirect::Policy;
use serde_json::Value;

const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36";

/// Result of a successful email OTP login.
pub struct OtpAuthResult {
    /// PAT `tokenValue` returned by `/v4/apiAccessTokens`.
    pub access_token: String,
    /// Workspace GID from the argument passed in.
    pub workspace_gid: String,
    /// ISO 8601 expiry from `tokenInfo.expiredAt`, if present.
    pub token_expires_at: Option<String>,
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

    // 1. Send the passcode.
    let send: Value = client
        .post(format!("{base}/v4/auth/sendPasscode"))
        .json(&serde_json::json!({ "email": email }))
        .send()
        .context("sendPasscode request failed")?
        .error_for_status()
        .context("sendPasscode returned an error status")?
        .json()
        .context("sendPasscode response was not JSON")?;
    let reference_id = send["passcodeReferenceId"]
        .as_str()
        .context("sendPasscode response missing passcodeReferenceId")?
        .to_string();

    // 2. Prompt for the OTP and validate it.
    let otp = get_otp()?;
    let validate = client
        .post(format!("{base}/v4/auth/validatePasscode"))
        .json(&serde_json::json!({
            "email": email,
            "passcode": otp.trim(),
            "passcodeReferenceId": reference_id,
        }))
        .send()
        .context("validatePasscode request failed")?;
    let validate_status = validate.status();
    if !validate_status.is_success() {
        let body = validate.text().unwrap_or_default();
        bail!("validatePasscode failed: HTTP {validate_status}: {body}");
    }

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
    let session = client
        .post(format!("{base}/session"))
        .form(&[("email", email), ("password", ws_password.as_str())])
        .send()
        .context("POST /session (workspace password) failed")?;
    if !session.status().is_success() {
        bail!(
            "workspace password rejected: POST /session returned HTTP {}",
            session.status()
        );
    }

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

    // 7. Mint a 30-day PAT.
    let csrf = cookie_value_from_jar(&jar, &base_for_cookies, "x-csrf-token").unwrap_or_default();
    let pat: Value = client
        .post(format!("{base}/v4/apiAccessTokens"))
        .header("x-csrf-token", csrf)
        .header("x-alteryx-workspace-gid", workspace_gid)
        .bearer_auth(&bearer)
        .json(&serde_json::json!({
            "name": "ayx-rs-cli",
            "lifetimeSeconds": 2_592_000,
        }))
        .send()
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

/// Resolve the workspace display name (e.g. "alteryx-fde") from its GID by
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
    let accounts_resp = client
        .get(accounts_url)
        // The accounts endpoint identifies the caller via this header, not session cookies.
        .header("x-alteryx-auth-email", email)
        .send()
        .context("failed to fetch /v4/auth/accounts")?;
    if !accounts_resp.status().is_success() {
        let status = accounts_resp.status();
        let body = accounts_resp.text().unwrap_or_default();
        bail!("/v4/auth/accounts returned HTTP {status}: {body}");
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

/// Read the workspace password from `AYX_ONE_WS_PASSWORD`, prompting on stdin if
/// it is not set.
fn resolve_workspace_password() -> Result<String> {
    if let Ok(pw) = std::env::var("AYX_ONE_WS_PASSWORD")
        && !pw.is_empty()
    {
        return Ok(pw);
    }
    eprint!("Workspace password: ");
    std::io::stderr().flush().ok();
    let mut pw = String::new();
    std::io::stdin()
        .read_line(&mut pw)
        .context("failed to read workspace password from stdin")?;
    let pw = pw.trim().to_string();
    if pw.is_empty() {
        bail!("workspace password is required (set AYX_ONE_WS_PASSWORD or enter it when prompted)");
    }
    Ok(pw)
}

/// Follow 3xx redirects manually with the client's cookie jar, returning the
/// ordered list of every URL visited (requested URLs and redirect targets).
/// Stops at the first non-redirect response or after `max_hops` redirects.
fn follow_redirects(client: &Client, start: reqwest::Url, max_hops: usize) -> Result<Vec<String>> {
    let mut visited = vec![start.to_string()];
    let mut current = start;
    for _ in 0..max_hops {
        let resp = client
            .get(current.clone())
            .header(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
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
        current = current
            .join(location)
            .with_context(|| format!("invalid redirect Location: {location}"))?;
        visited.push(current.to_string());
    }
    bail!("exceeded {max_hops} redirects while following the auth flow")
}

/// Pull the OIDC interaction id out of the redirect chain.  Prefers the
/// `interaction_id=` query parameter; falls back to a `/token/<id>` path
/// segment (ignoring the `/token/auth/...` callback path).
fn extract_interaction_id(visited: &[String]) -> Option<String> {
    for url in visited {
        if let Some(idx) = url.find("interaction_id=") {
            let rest = &url[idx + "interaction_id=".len()..];
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if !id.is_empty() {
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
            if !seg.is_empty() && seg != "auth" {
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
