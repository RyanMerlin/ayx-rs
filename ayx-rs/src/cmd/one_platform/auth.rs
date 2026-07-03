use anyhow::{Context, Result, bail};
use ayx_core::envelope::Envelope;

use crate::{
    OnePlatformAuthCommand, cmd::RuntimeCtx, one_platform_auth_diagnose_envelope,
    one_platform_auth_status_envelope,
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: OnePlatformAuthCommand,
) -> Result<Envelope> {
    Ok(match command {
        OnePlatformAuthCommand::Status { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_platform_auth_status_envelope(&config)?
        }
        OnePlatformAuthCommand::Diagnose { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_platform_auth_diagnose_envelope(&config)?
        }
        OnePlatformAuthCommand::Login {
            profile,
            client_id,
            browser,
            device,
            refresh_token,
            access_token,
            token_endpoint,
            workspace_id,
            workspace_gid,
        } => login(
            runtime,
            profile.as_deref(),
            client_id,
            browser,
            device,
            refresh_token,
            access_token,
            token_endpoint,
            workspace_id,
            workspace_gid,
        )?,
    })
}

#[allow(clippy::too_many_arguments)]
fn login(
    runtime: &RuntimeCtx<'_>,
    profile: Option<&str>,
    client_id: Option<String>,
    browser: bool,
    device: bool,
    refresh_token_arg: Option<String>,
    access_token_arg: Option<String>,
    token_endpoint_arg: Option<String>,
    workspace_id: Option<String>,
    workspace_gid_arg: Option<String>,
) -> Result<Envelope> {
    use ayx_core::profile::{normalize_alteryx_one_token_endpoint, profile_storage_path};
    use ayx_one_api::{
        exchange_auth_code, generate_pkce_challenge, generate_random_state, initiate_device_auth,
        poll_device_token, refresh_one_access_token,
    };
    use serde_json::json;

    let mut config = runtime.load_profile_lenient(profile)?;
    let one = config
        .alteryx_one
        .as_mut()
        .context("no alteryx_one section in profile — run `ayx onboard` first or add alteryx_one.account_email to your config")?;

    if let Some(id) = client_id {
        one.oauth_client_id = Some(id);
    }
    if let Some(ep) = token_endpoint_arg {
        one.token_endpoint_url = Some(normalize_alteryx_one_token_endpoint(&ep));
    }
    if let Some(gid) = workspace_gid_arg {
        one.workspace_gid = Some(gid);
    }

    let http = reqwest::blocking::Client::new();

    // Resolve the token endpoint from the profile (or the default Ping issuer).
    let token_endpoint = config
        .alteryx_one
        .as_ref()
        .and_then(|o| o.effective_token_endpoint_url())
        .unwrap_or_else(|| "https://pingauth.alteryxcloud.com/as/token".to_string());

    // oauth_client_id is only consumed by the --browser (PKCE) and --device
    // grants. The default email-OTP flow, and the --refresh-token/--access-token
    // bypass paths, never use it. Resolve it lazily so a brand-new user can
    // complete the default OTP login without first creating an OAuth client.
    let client_id_opt = config
        .alteryx_one
        .as_ref()
        .and_then(|o| o.resolved_oauth_client_id())
        .map(str::to_string);

    let (final_access_token, final_refresh_token) = if let Some(rt) = refresh_token_arg {
        // --- bypass: exchange a refresh token the caller already has ---
        let one = config.alteryx_one.as_mut().unwrap();
        if let Some(ws_id) = &workspace_id {
            one.workspace_credentials
                .entry(ws_id.clone())
                .or_default()
                .refresh_token = Some(rt);
        } else {
            one.refresh_token = Some(rt);
        }
        let access = refresh_one_access_token(&config, &http)
            .context("token exchange failed — check --client-id and --refresh-token")?;
        (access, None)
    } else if let Some(at) = access_token_arg {
        // --- bypass: store a token the caller already has ---
        (at, None)
    } else if browser {
        // --- PKCE authorization-code flow ---
        let client_id_val = client_id_opt.clone().context(
            "oauth_client_id is required for the --browser flow — pass --client-id or set \
             alteryx_one.oauth_client_id in your profile (the default email-OTP flow does not need it)",
        )?;
        let pkce = generate_pkce_challenge();
        // 32-byte random state guards the callback against CSRF.
        let csrf_state = generate_random_state(32);

        // Bind a random local port for the redirect callback.
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .context("failed to bind local callback server")?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://localhost:{port}/callback");

        // Derive the authorization endpoint from the token endpoint.
        let auth_endpoint = token_endpoint
            .replace("/token", "/authorize")
            .replace("/as/token", "/as/authorize");

        let auth_url = format!(
            "{auth_endpoint}?response_type=code&client_id={client_id_val}&redirect_uri={redirect_uri}&code_challenge={}&code_challenge_method=S256&scope=openid&state={csrf_state}",
            pkce.code_challenge
        );

        eprintln!("Opening browser for authentication...");
        eprintln!("If the browser doesn't open, visit:\n  {auth_url}");
        open_browser(&auth_url);

        // Wait for the callback (5 minute timeout).
        listener.set_nonblocking(false).ok();
        let start = std::time::Instant::now();
        let code = loop {
            if start.elapsed().as_secs() > 300 {
                bail!("timed out waiting for browser callback");
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    use std::io::{Read, Write};
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let req = std::str::from_utf8(&buf[..n]).unwrap_or("");

                    // Parse the request line: GET /callback?... HTTP/1.1
                    let request_line = req.lines().next().unwrap_or("");
                    let path = request_line.split_whitespace().nth(1).unwrap_or("");

                    // Validate path prefix to reject unexpected requests.
                    let ok_path = path == "/callback"
                        || path.starts_with("/callback?")
                        || path.starts_with("/callback ");
                    let qs = path.split_once('?').map(|(_, q)| q).unwrap_or("");

                    let returned_state = qs
                        .split('&')
                        .find(|p| p.starts_with("state="))
                        .map(|p| p.trim_start_matches("state="));
                    let state_ok = returned_state == Some(csrf_state.as_str());

                    let code = if ok_path && state_ok {
                        qs.split('&')
                            .find(|p| p.starts_with("code="))
                            .map(|p| p.trim_start_matches("code=").to_string())
                    } else {
                        None
                    };

                    let (status, body) = if !ok_path {
                        (
                            "404 Not Found",
                            "<html><body><h2>Not found.</h2></body></html>",
                        )
                    } else if !state_ok {
                        (
                            "400 Bad Request",
                            "<html><body><h2>State mismatch — possible CSRF. Please try again.</h2></body></html>",
                        )
                    } else if code.is_some() {
                        (
                            "200 OK",
                            "<html><body><h2>Authenticated!</h2><p>You can close this tab.</p></body></html>",
                        )
                    } else {
                        (
                            "400 Bad Request",
                            "<html><body><h2>No authorization code in callback. Please try again.</h2></body></html>",
                        )
                    };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );

                    if !ok_path {
                        // Ignore favicon, prefetch, etc.
                        continue;
                    }
                    if !state_ok {
                        bail!("OAuth state mismatch in browser callback — possible CSRF attack");
                    }
                    match code {
                        Some(c) => break c,
                        None => bail!("browser callback did not include an authorization code"),
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => return Err(e).context("callback server error"),
            }
        };

        let (access, refresh) = exchange_auth_code(
            &token_endpoint,
            &client_id_val,
            &code,
            &pkce.code_verifier,
            &redirect_uri,
            &http,
        )?;
        (access, refresh)
    } else if !device {
        // --- Email OTP flow (default) ---
        let base_url = ayx_one_api::resolve_one_base_url(&config);
        let email = config
            .alteryx_one
            .as_ref()
            .map(|o| o.account_email.clone())
            .unwrap_or_default();
        let ws_gid = config
            .alteryx_one
            .as_ref()
            .and_then(|o| o.resolved_workspace_gid())
            .map(str::to_string)
            .unwrap_or_default();

        if ws_gid.is_empty() {
            anyhow::bail!(
                "workspace_gid is required — set alteryx_one.workspace_gid in your profile \
                 or pass --workspace-gid"
            );
        }

        eprintln!("Sending one-time passcode to {}...", email);
        eprintln!("(Check your inbox for a 6-digit code)");

        let result = ayx_one_api::email_otp_login(&base_url, &email, &ws_gid, || {
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| anyhow::anyhow!("failed to read OTP: {e}"))?;
            Ok(line.trim().to_string())
        })?;

        // Sync workspace_gid into the profile in case it was resolved from
        // the active workspace credential rather than the top-level field.
        config.alteryx_one.as_mut().unwrap().workspace_gid = Some(result.workspace_gid.clone());

        if let Some(ref expires) = result.token_expires_at {
            eprintln!("Token expires: {expires}");
        }

        (result.access_token, None)
    } else {
        // --- Device authorization grant (--device flag) ---
        let client_id_val = client_id_opt.clone().context(
            "oauth_client_id is required for the --device flow — pass --client-id or set \
             alteryx_one.oauth_client_id in your profile (the default email-OTP flow does not need it)",
        )?;
        let device_auth_endpoint = token_endpoint
            .replace("/token", "/device_authorization")
            .replace("/as/token", "/as/device_authorization");

        let device_resp = initiate_device_auth(&device_auth_endpoint, &client_id_val, &http)?;

        let uri = device_resp
            .verification_uri_complete
            .as_deref()
            .unwrap_or(&device_resp.verification_uri);

        eprintln!("\nOpen this URL in your browser and complete sign-in:");
        eprintln!("  {uri}");
        if device_resp.verification_uri_complete.is_none() {
            eprintln!("\nEnter code: {}", device_resp.user_code);
        }
        eprintln!("\nWaiting for authentication...");

        // Try to open the browser automatically.
        open_browser(uri);

        let mut interval = device_resp.interval;
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(device_resp.expires_in);

        loop {
            std::thread::sleep(std::time::Duration::from_secs(interval));
            if std::time::Instant::now() >= deadline {
                bail!("device code expired — run `ayx one platform auth login` again");
            }
            match poll_device_token(
                &token_endpoint,
                &client_id_val,
                &device_resp.device_code,
                &http,
            )? {
                Some((access, refresh)) => break (access, refresh),
                None => {
                    // slow_down or authorization_pending — keep polling.
                    // Increase interval on slow_down (server may signal it via
                    // the `slow_down` error; we add 5s conservatively).
                    if interval < 30 {
                        interval += 1;
                    }
                }
            }
        }
    };

    // Store the tokens.
    let one = config.alteryx_one.as_mut().unwrap();
    if let Some(ws_id) = workspace_id {
        let cred = one.workspace_credentials.entry(ws_id).or_default();
        cred.access_token = Some(final_access_token.clone());
        if let Some(rt) = final_refresh_token.clone() {
            cred.refresh_token = Some(rt);
        }
    } else {
        one.access_token = Some(final_access_token.clone());
        if let Some(rt) = final_refresh_token.clone() {
            one.refresh_token = Some(rt);
        }
    }

    let email = one.account_email.clone();
    let endpoint = one.normalized_base_url().unwrap_or_default();
    let profile_name = config.profile_name.clone();

    let path = profile_storage_path(&profile_name)?;
    let secretize = crate::onboard::write_config_with_policy(
        &path,
        &config,
        crate::onboard::InlineSecretPolicy::Allow,
    )
    .context("failed to save profile")?;

    if let Some(msg) = crate::onboard::inline_secret_warning(&secretize.inline_fields) {
        eprintln!("warning: {msg}");
    }
    eprintln!("Credentials stored in profile '{profile_name}'.");
    Ok(Envelope::ok_with_data(
        "credentials stored",
        json!({
            "action": "auth.login",
            "status": "ok",
            "profile": profile_name,
            "account_email": email,
            "endpoint": endpoint,
            "token_length": final_access_token.len(),
            "has_refresh_token": final_refresh_token.is_some(),
            "inline_secret_fields": secretize.inline_fields,
        }),
    ))
}

fn open_browser(url: &str) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", url])
        .spawn();
}
