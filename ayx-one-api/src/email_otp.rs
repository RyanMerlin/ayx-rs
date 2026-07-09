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
        bail!(
            "validatePasscode failed: HTTP {validate_status}: {}",
            redact_text(&body.chars().take(200).collect::<String>())
        );
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
    let accounts_resp = client
        .get(accounts_url)
        // The accounts endpoint identifies the caller via this header, not session cookies.
        .header("x-alteryx-auth-email", email)
        .send()
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
    use super::{extract_interaction_id, host_allowed, is_valid_interaction_id};

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
}
