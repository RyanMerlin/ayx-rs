/// Email OTP authentication flow for Alteryx One.
///
/// `email_otp_login` tries a pure-reqwest path first (no browser, no Python):
/// sendPasscode → validatePasscode → follow the workspace-entry redirect chain
/// (Accept: text/html required — the BFF serves 302 to OIDC only for HTML
/// requests) → POST /session with workspace password → resume OIDC →
/// local-auth-workspace cookie → mint 30-day PAT.
///
/// Falls back to a Python/Playwright subprocess automatically on any failure.
/// `AYX_ONE_AUTH_FORCE_BROWSER=1` skips the pure-HTTP path entirely.
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use reqwest::cookie::{CookieStore as _, Jar};
use reqwest::redirect::Policy;
use serde_json::Value;

const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36";

/// Embedded Python/Playwright script — written to a temp file at runtime.
///
/// Argument order: email base_url workspace_gid otp_file result_file
/// Env:            AYX_WORKSPACE_PASSWORD (optional)
const PLAYWRIGHT_SCRIPT: &str = r#"#!/usr/bin/env python3
"""Alteryx One browser-based workspace auth (called by ayx-rs)."""
import asyncio, base64, json, os, pathlib, sys

EMAIL          = sys.argv[1]
BASE           = sys.argv[2].rstrip("/")
WORKSPACE_GID  = sys.argv[3]
WORKDIR        = pathlib.Path(sys.argv[4])   # private 0o700 temp directory
OTP_FILE       = WORKDIR / "otp.txt"
RESULT_FILE    = WORKDIR / "result.json"
READY_FILE     = WORKDIR / "otp-ready.txt"
PW_NEED_FILE   = WORKDIR / "password-needed.txt"
WS_PASSWORD    = os.environ.get("AYX_WORKSPACE_PASSWORD", "")

def _secure_write(path: pathlib.Path, data: str) -> None:
    """Write data to path with O_NOFOLLOW | O_CREAT, mode 0o600."""
    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW
    fd = os.open(path, flags, 0o600)
    try:
        os.write(fd, data.encode())
    finally:
        os.close(fd)

CHROME_PATHS = [
    "/opt/google/chrome/chrome",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium-browser",
    "/usr/bin/chromium",
]

def _chrome():
    return next((p for p in CHROME_PATHS if pathlib.Path(p).exists()), None)

async def _wait_file(path: pathlib.Path, timeout: int = 300) -> str:
    for _ in range(timeout):
        if path.exists():
            txt = path.read_text().strip()
            if txt:
                path.unlink(missing_ok=True)
                return txt
        await asyncio.sleep(1)
    sys.exit(f"Timed out waiting for {path}")

async def main():
    from playwright.async_api import async_playwright

    chrome = _chrome()
    launch = {"headless": True, "args": ["--no-sandbox", "--disable-dev-shm-usage"]}
    if chrome:
        launch["executable_path"] = chrome

    async with async_playwright() as pw:
        browser = await pw.chromium.launch(**launch)
        ctx = await browser.new_context(viewport={"width": 1280, "height": 900})
        page = await ctx.new_page()

        # ── 1. Auth-portal sign-in page ──────────────────────────────────────
        await page.goto(f"{BASE}/auth-portal/sign-in",
                        wait_until="domcontentloaded", timeout=30000)
        await page.wait_for_timeout(2000)

        # ── 2. Enter email ───────────────────────────────────────────────────
        email_input = page.locator('input[type="email"]').first
        await email_input.fill(EMAIL)
        await page.wait_for_timeout(300)
        await page.locator('button[type="submit"]').first.click()
        await page.wait_for_timeout(4000)

        # Signal to Rust: OTP email has been sent (mode 0o600, O_NOFOLLOW)
        _secure_write(READY_FILE, "ready")
        print("otp-ready", flush=True)

        # ── 3. Wait for OTP from Rust ────────────────────────────────────────
        otp = await _wait_file(OTP_FILE)

        # ── 4. Enter OTP ─────────────────────────────────────────────────────
        single_digit = page.locator('input[maxlength="1"]')
        if await single_digit.count() >= 6:
            for i, ch in enumerate(otp[:6]):
                await single_digit.nth(i).fill(ch)
        else:
            inp = page.locator(
                'input:not([type="hidden"]):not([type="submit"]):not([type="email"])'
            ).first
            await inp.fill(otp)

        await page.wait_for_timeout(300)
        try:
            await page.locator('button[type="submit"]').first.click(timeout=5000)
        except Exception:
            pass
        await page.wait_for_timeout(6000)

        # ── 5. Select workspace ───────────────────────────────────────────────
        # Try clicking the workspace by GID or by navigating directly.
        clicked = False
        for sel in [
            f'[data-workspace-gid="{WORKSPACE_GID}"]',
            f'[href*="{WORKSPACE_GID}"]',
            f'li[data-id="{WORKSPACE_GID}"]',
        ]:
            try:
                await page.click(sel, timeout=2000)
                clicked = True
                break
            except Exception:
                continue

        if not clicked:
            # Navigate directly; the SPA will handle it.
            await page.goto(
                f"{BASE}/auth-portal/workspaces/{WORKSPACE_GID}",
                wait_until="domcontentloaded", timeout=15000,
            )

        await page.wait_for_timeout(8000)

        # ── 6. Handle password form (if shown) ───────────────────────────────
        body = await page.inner_text("body")
        if "password" in body.lower():
            pw_to_use = WS_PASSWORD
            if pw_to_use:
                pw_input = page.locator('input[type="password"]').first
                if await pw_input.count() > 0:
                    await pw_input.fill(pw_to_use)
                    await page.wait_for_timeout(300)
                    await page.locator('button[type="submit"]').first.click()
                    await page.wait_for_timeout(10000)
            else:
                # Signal Rust to prompt interactively; wait for it to write
                # the password back to OTP_FILE.
                _secure_write(PW_NEED_FILE, "needed")
                ws_pw = await _wait_file(OTP_FILE)
                pw_input = page.locator('input[type="password"]').first
                await pw_input.fill(ws_pw)
                await page.wait_for_timeout(300)
                await page.locator('button[type="submit"]').first.click()
                await page.wait_for_timeout(10000)

        # ── 7. Extract cookies ────────────────────────────────────────────────
        cookies = await ctx.cookies()
        cookie_dict = {c["name"]: c["value"] for c in cookies}

        law = cookie_dict.get("local-auth-workspace", "")
        if not law:
            await browser.close()
            final = page.url
            sys.exit(
                f"local-auth-workspace cookie not set (final URL: {final}). "
                f"Auth did not complete."
            )

        # Decode: base64-encoded JSON  {"accessToken":"...","refreshToken":"..."}
        try:
            padded = law + "=" * (-len(law) % 4)
            decoded = json.loads(base64.urlsafe_b64decode(padded))
            bearer = decoded.get("accessToken") or decoded.get("access_token", "")
        except Exception as exc:
            await browser.close()
            sys.exit(f"Failed to decode local-auth-workspace cookie: {exc}")

        # ── 8. Mint 30-day PAT ────────────────────────────────────────────────
        csrf = cookie_dict.get("x-csrf-token", "")
        pat_resp = await ctx.request.post(
            f"{BASE}/v4/apiAccessTokens",
            data=json.dumps({"name": "ayx-rs-cli", "lifetimeSeconds": 2_592_000}),
            headers={
                "Content-Type": "application/json",
                "x-csrf-token": csrf,
                "x-alteryx-workspace-gid": WORKSPACE_GID,
                "Authorization": f"Bearer {bearer}",
            },
        )

        if pat_resp.status not in (200, 201):
            txt = await pat_resp.text()
            await browser.close()
            sys.exit(f"Failed to mint PAT ({pat_resp.status}): {txt[:300]}")

        pat = await pat_resp.json()
        token_value = pat.get("tokenValue", "")
        expires_at  = pat.get("tokenInfo", {}).get("expiredAt", "")

        _secure_write(RESULT_FILE, json.dumps({
            "access_token": token_value,
            "expires_at": expires_at,
            "workspace_gid": WORKSPACE_GID,
        }))

        await browser.close()
        print(f"done token_len={len(token_value)}", flush=True)

asyncio.run(main())
"#;

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
/// Tries a dependency-free pure-HTTP flow first (no browser, no Python); if any
/// step fails, falls back to the Playwright subprocess.  Set
/// `AYX_ONE_AUTH_FORCE_BROWSER=1` to skip the pure-HTTP path entirely.
///
/// `get_otp` is called when the OTP email has been sent.  It should prompt the
/// user and return the 6-digit code.  It may be called twice if the pure-HTTP
/// path consumes an OTP and then fails (the browser fallback sends a fresh one).
pub fn email_otp_login<F>(
    base_url: &str,
    email: &str,
    workspace_gid: &str,
    get_otp: F,
) -> Result<OtpAuthResult>
where
    F: Fn() -> Result<String> + Send + 'static,
{
    let force_browser = std::env::var("AYX_ONE_AUTH_FORCE_BROWSER")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let no_fallback = std::env::var("AYX_ONE_AUTH_NO_FALLBACK")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if !force_browser {
        match email_otp_login_pure_http(base_url, email, workspace_gid, &get_otp) {
            Ok(result) => return Ok(result),
            Err(e) => {
                if no_fallback {
                    return Err(e.context(
                        "pure-HTTP failed (AYX_ONE_AUTH_NO_FALLBACK=1, not falling back)",
                    ));
                }
                eprintln!(
                    "Pure-HTTP authentication failed ({e:#}); \
                     falling back to browser (Playwright)..."
                );
            }
        }
    }

    email_otp_login_playwright(base_url, email, workspace_gid, get_otp)
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

/// Authenticate via email OTP using a Playwright (headless Chromium) subprocess.
///
/// `get_otp` is called once when the OTP email has been sent.  It should
/// prompt the user and return the 6-digit code.
fn email_otp_login_playwright<F>(
    base_url: &str,
    email: &str,
    workspace_gid: &str,
    get_otp: F,
) -> Result<OtpAuthResult>
where
    F: Fn() -> Result<String> + Send + 'static,
{
    // Use a private, per-invocation temp directory (mode 0o700) for all IPC
    // files.  This prevents symlink-TOCTOU attacks and stops other local users
    // from reading the OTP or PAT while they are briefly on disk.
    let workdir = tempfile::Builder::new()
        .prefix("ayx-otp-")
        .tempdir()
        .context("failed to create private temp directory for auth IPC")?;

    // Write the Playwright script inside the private workdir (not the shared
    // system temp dir) so the path is unpredictable and under 0o700 directory
    // permissions.  Use O_CREAT|O_EXCL|O_NOFOLLOW so a pre-existing symlink
    // cannot redirect the write.
    let script_path = write_script(workdir.path())?;

    let otp_file = workdir.path().join("otp.txt");
    let result_file = workdir.path().join("result.json");
    let ready_file = workdir.path().join("otp-ready.txt");
    let pw_need_file = workdir.path().join("password-needed.txt");

    // ── Verify the OTP email is actually reachable before starting the browser ─
    // sendPasscode is used here only to check the email is valid; the actual
    // OTP entry happens through the browser (which sends its own passcode).
    // We do NOT call validatePasscode from Rust — the browser handles that.

    // ── Launch Python/Playwright subprocess ───────────────────────────────────
    let ws_password = std::env::var("AYX_ONE_WS_PASSWORD").unwrap_or_default();

    // Pass the private workdir to Python; the script derives all file paths
    // from it so it never opens files outside the 0o700 directory.
    let mut child = std::process::Command::new("python3")
        .arg(&script_path)
        .arg(email)
        .arg(base_url)
        .arg(workspace_gid)
        .arg(workdir.path()) // private dir; Python builds paths inside it
        .env("AYX_WORKSPACE_PASSWORD", &ws_password)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context(
            "failed to launch Python auth subprocess — \
             is python3 with playwright installed?  \
             Run: pip install playwright && playwright install chromium",
        )?;

    // ── Wait for OTP-ready signal (browser navigated to auth portal) ──────────
    eprintln!("Opening browser for authentication...");
    {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if ready_file.exists() {
                break;
            }
            if matches!(child.try_wait(), Ok(Some(_))) {
                bail!("auth subprocess exited before sending OTP email (check stderr above)");
            }
            if Instant::now() > deadline {
                child.kill().ok();
                bail!("timed out waiting for browser to reach auth portal");
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    let _ = std::fs::remove_file(&ready_file);

    // ── Prompt user for OTP ──────────────────────────────────────────────────
    // Print the otp_file path in a machine-readable tag so an automated driver
    // can write the code directly (bypassing stdin).  For interactive use the
    // get_otp closure reads from stdin and we write it ourselves.
    eprint!("Passcode: ");
    let _ = std::io::stderr().flush();
    eprintln!("[otp-path] {}", otp_file.display());

    // Spawn a thread to read from the closure (stdin in interactive use) and
    // write to the file.  Automated drivers may write directly to the file
    // instead; whichever arrives first is used.
    let otp_file_for_thread = otp_file.clone();
    std::thread::spawn(move || {
        if let Ok(otp) = get_otp() {
            let _ = write_nofollow(&otp_file_for_thread, otp.trim().as_bytes());
        }
    });

    // Main thread polls until the otp_file appears.
    {
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            if otp_file.exists() {
                break;
            }
            if matches!(child.try_wait(), Ok(Some(_))) {
                bail!("auth subprocess exited before OTP was provided (check stderr above)");
            }
            if Instant::now() > deadline {
                child.kill().ok();
                bail!("timed out waiting for OTP");
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    // ── Wait for result; handle workspace-password prompt if it appears ───────
    {
        let deadline = Instant::now() + Duration::from_secs(240);
        loop {
            if result_file.exists() {
                break;
            }

            // Python couldn't find AYX_WORKSPACE_PASSWORD → prompt now.
            if pw_need_file.exists() {
                let _ = std::fs::remove_file(&pw_need_file);
                eprint!("Workspace password: ");
                let _ = std::io::stderr().flush();
                let mut pw = String::new();
                std::io::stdin()
                    .read_line(&mut pw)
                    .context("failed to read workspace password")?;
                write_nofollow(&otp_file, pw.trim().as_bytes())
                    .context("failed to write workspace password to temp file")?;
            }

            if matches!(child.try_wait(), Ok(Some(_))) {
                bail!("auth subprocess exited before writing result (check stderr above)");
            }
            if Instant::now() > deadline {
                child.kill().ok();
                bail!("timed out waiting for browser to complete authentication");
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    // Reap the child.
    let _ = child.wait();

    // ── Parse result ──────────────────────────────────────────────────────────
    let result_text = std::fs::read_to_string(&result_file)
        .context("auth script did not write a result file — check stderr for errors")?;
    let _ = std::fs::remove_file(&result_file);

    let result: Value =
        serde_json::from_str(&result_text).context("auth result was not valid JSON")?;

    let access_token = result["access_token"]
        .as_str()
        .context("auth result missing access_token")?
        .to_string();

    let token_expires_at = result["expires_at"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Ok(OtpAuthResult {
        access_token,
        workspace_gid: workspace_gid.to_string(),
        token_expires_at,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Write `data` to `path` with O_NOFOLLOW so a symlink cannot redirect the
/// write.  Also sets mode 0o600 so the file is owner-readable only.
fn write_nofollow(path: &PathBuf, data: &[u8]) -> Result<()> {
    use std::fs::OpenOptions;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc_nofollow())
            .open(path)
            .with_context(|| format!("open {}", path.display()))?;
        f.write_all(data)
            .with_context(|| format!("write {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        // Non-Unix: best-effort restricted write (no O_NOFOLLOW equivalent).
        std::fs::write(path, data).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn libc_nofollow() -> i32 {
    // O_NOFOLLOW is 0x20000 on Linux x86_64/aarch64, 0x100 on macOS.
    // Using the platform constant via libc would require adding a libc dep;
    // instead hard-code the Linux value and fall back gracefully on others.
    #[cfg(target_os = "linux")]
    {
        0x20000
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Write the embedded Playwright script to a temp file and return its path.
fn write_script(workdir: &std::path::Path) -> Result<PathBuf> {
    let path = workdir.join("browser-auth.py");
    // O_CREAT|O_EXCL: fail if the file already exists (the workdir is fresh
    // per invocation so this can only happen if something tampered with it).
    // O_NOFOLLOW: reject symlinks. Mode 0o700: owner-execute only.
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true) // O_CREAT|O_EXCL
            .mode(0o700)
            .custom_flags(libc_nofollow())
            .open(&path)
            .with_context(|| format!("failed to create auth script at {}", path.display()))?;
        f.write_all(PLAYWRIGHT_SCRIPT.as_bytes())
            .with_context(|| format!("failed to write auth script to {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&path, PLAYWRIGHT_SCRIPT)
            .with_context(|| format!("failed to write auth script to {}", path.display()))?;
    }
    Ok(path)
}

/// Poll until `path` exists, checking every 500 ms up to `timeout`.
/// `early_exit` returns true if the subprocess exited early (skip waiting).
#[allow(dead_code)]
fn wait_for_file<F>(path: &std::path::Path, timeout: Duration, early_exit: F) -> Result<()>
where
    F: Fn() -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return Ok(());
        }
        if early_exit() {
            bail!("auth subprocess exited before completing (check stderr above for details)");
        }
        if Instant::now() > deadline {
            bail!("timed out after {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
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

// ── Unused legacy helpers (kept for reference) ────────────────────────────────

#[allow(dead_code)]
fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).expect("OS entropy source unavailable");
    buf.iter().map(|b| format!("{b:02x}")).collect()
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
