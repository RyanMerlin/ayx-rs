use anyhow::{Context, Result, bail};
use ayx_core::envelope::Envelope;

use crate::{
    OneAuthCommand, cmd::RuntimeCtx, one_platform_auth_diagnose_envelope,
    one_platform_auth_status_envelope,
};

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, command: OneAuthCommand) -> Result<Envelope> {
    Ok(match command {
        OneAuthCommand::Status { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_platform_auth_status_envelope(&config)?
        }
        OneAuthCommand::Diagnose { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_platform_auth_diagnose_envelope(&config)?
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn login(
    runtime: &RuntimeCtx<'_>,
    profile: Option<String>,
    client_id: Option<String>,
    browser: bool,
    device: bool,
    refresh_token_arg: Option<String>,
    access_token_arg: Option<String>,
    token_endpoint_arg: Option<String>,
    workspace_id: Option<String>,
    workspace_gid_arg: Option<String>,
    save_workspace_password: bool,
) -> Result<Envelope> {
    use ayx_core::profile::{normalize_alteryx_one_token_endpoint, profile_storage_path};
    use ayx_one_api::{
        exchange_auth_code, generate_pkce_challenge, generate_random_state, initiate_device_auth,
        poll_device_token, refresh_one_access_token,
    };
    use serde_json::json;

    let mut config = runtime.load_profile_lenient(profile.as_deref())?;
    {
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

    if save_workspace_password
        && (browser || device || refresh_token_arg.is_some() || access_token_arg.is_some())
    {
        bail!("--save-workspace-password applies only to the default email-OTP login flow");
    }

    let one = config.alteryx_one.as_mut().unwrap();
    if save_workspace_password
        && let (Some(ws_id), Some(requested_gid)) =
            (workspace_id.as_deref(), workspace_gid_arg.as_deref())
        && let Some(existing_gid) = one
            .workspace_credentials
            .get(ws_id)
            .and_then(|credential| credential.workspace_gid.as_deref())
        && existing_gid != requested_gid
    {
        bail!(
            "--workspace-gid '{}' does not match the selected workspace credential '{}' (which is bound to '{}')",
            requested_gid,
            ws_id,
            existing_gid
        );
    }

    // A workspace-scoped login must keep its numeric workspace credential and
    // workspace GID together. Without this, `--workspace-id B` could still
    // resolve the active profile GID for workspace A.
    if let Some(gid) = workspace_gid_arg.as_deref() {
        if let Some(ws_id) = workspace_id.as_deref() {
            one.workspace_credentials
                .entry(ws_id.to_string())
                .or_default()
                .workspace_gid = Some(gid.to_string());
        } else {
            one.workspace_gid = Some(gid.to_string());
        }
    }

    let password_workspace_id = if save_workspace_password {
        workspace_id.clone().or_else(|| {
            config
                .alteryx_one
                .as_ref()
                .and_then(|one| one.active_workspace_id().map(str::to_string))
        })
    } else {
        None
    };
    if save_workspace_password && password_workspace_id.is_none() {
        bail!(
            "--save-workspace-password requires a workspace-scoped login — pass --workspace-id or configure an active workspace credential"
        );
    }

    let mut workspace_password_to_save = None;

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
        let ws_gid = if let Some(ws_id) = workspace_id.as_deref() {
            config
                .alteryx_one
                .as_ref()
                .and_then(|one| one.workspace_credentials.get(ws_id))
                .and_then(|credential| credential.workspace_gid.as_deref())
                .map(str::to_string)
                .unwrap_or_default()
        } else {
            config
                .alteryx_one
                .as_ref()
                .and_then(|o| o.resolved_workspace_gid())
                .map(str::to_string)
                .unwrap_or_default()
        };
        let workspace_password = config
            .alteryx_one
            .as_ref()
            .and_then(|one| workspace_password_for_login(one, workspace_id.as_deref()));

        if ws_gid.is_empty() {
            anyhow::bail!(
                "workspace_gid is required — set alteryx_one.workspace_gid in your profile \
                 or pass --workspace-gid"
            );
        }

        eprintln!("Sending one-time passcode to {}...", email);
        eprintln!("(Check your inbox for a 6-digit code)");

        let get_otp = || {
            use std::io::Write as _;
            eprint!("Enter the 6-digit passcode: ");
            let _ = std::io::stderr().flush();
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| anyhow::anyhow!("failed to read OTP: {e}"))?;
            Ok(line.trim().to_string())
        };
        let (result, captured_workspace_password) = if save_workspace_password {
            let (result, password) = ayx_one_api::email_otp_login_with_password(
                &base_url,
                &email,
                &ws_gid,
                workspace_password,
                get_otp,
            )?;
            (result, Some(password))
        } else {
            let result = ayx_one_api::email_otp_login(
                &base_url,
                &email,
                &ws_gid,
                workspace_password,
                get_otp,
            )?;
            (result, None)
        };

        workspace_password_to_save = captured_workspace_password;

        // Sync workspace_gid into the profile in case it was resolved from
        // the active workspace credential rather than the top-level field.
        config.alteryx_one.as_mut().unwrap().workspace_gid = Some(result.workspace_gid.clone());
        if let Some(ws_id) = password_workspace_id.as_deref() {
            config
                .alteryx_one
                .as_mut()
                .unwrap()
                .workspace_credentials
                .entry(ws_id.to_string())
                .or_default()
                .workspace_gid = Some(result.workspace_gid.clone());
        }

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
                bail!("device code expired — run `ayx one login` again");
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
    if let Some(password) = workspace_password_to_save {
        let ws_id = password_workspace_id
            .as_deref()
            .expect("workspace-scoped password validation should run before login");
        one.workspace_credentials
            .entry(ws_id.to_string())
            .or_default()
            .workspace_password = Some(password);
    }
    if let Some(ws_id) = workspace_id.as_deref() {
        let cred = one
            .workspace_credentials
            .entry(ws_id.to_string())
            .or_default();
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
    let secret_policy = if save_workspace_password {
        // An explicitly requested workspace-password save must never fall back
        // to plaintext YAML. Tokens retain the historical inline fallback when
        // this flag is not used.
        crate::onboard::InlineSecretPolicy::Forbid
    } else {
        crate::onboard::InlineSecretPolicy::Allow
    };
    let secretize = crate::onboard::write_config_with_policy(&path, &config, secret_policy)
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

pub(crate) fn logout(runtime: &RuntimeCtx<'_>, profile: Option<&str>) -> Result<Envelope> {
    use ayx_core::profile::profile_storage_path;
    use serde_json::json;

    let mut config = runtime.load_profile_lenient(profile)?;
    let one = config
        .alteryx_one
        .as_mut()
        .context("no alteryx_one section in profile")?;

    let top_level_cleared = one.access_token.is_some()
        || one.access_token_ref.is_some()
        || one.refresh_token.is_some()
        || one.refresh_token_ref.is_some()
        || one.workspace_password.is_some()
        || one.workspace_password_ref.is_some();
    one.access_token = None;
    one.access_token_ref = None;
    one.refresh_token = None;
    one.refresh_token_ref = None;
    one.workspace_password = None;
    one.workspace_password_ref = None;

    let mut workspace_credentials_cleared = 0usize;
    for credential in one.workspace_credentials.values_mut() {
        let had_credential = credential.access_token.is_some()
            || credential.access_token_ref.is_some()
            || credential.refresh_token.is_some()
            || credential.refresh_token_ref.is_some()
            || credential.workspace_password.is_some()
            || credential.workspace_password_ref.is_some();
        credential.access_token = None;
        credential.access_token_ref = None;
        credential.refresh_token = None;
        credential.refresh_token_ref = None;
        credential.workspace_password = None;
        credential.workspace_password_ref = None;
        if had_credential {
            workspace_credentials_cleared += 1;
        }
    }

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

    Ok(Envelope::ok_with_data(
        "one credentials cleared",
        json!({
            "action": "auth.logout",
            "status": "ok",
            "profile": profile_name,
            "top_level_credentials_cleared": top_level_cleared,
            "workspace_credentials_cleared": workspace_credentials_cleared,
            "remote_revocation": "not attempted",
            "notes": [
                "Cleared stored Alteryx One access/refresh credentials, workspace passwords, and credential refs from the profile",
                "External secret-store entries referenced by the previous profile were not deleted",
            ],
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

/// Resolve the password for the workspace the login will actually target.
///
/// An explicit workspace ID is a hard boundary: do not fall back to the
/// active credential or profile-level legacy password, because those may
/// belong to a different workspace. With no explicit ID, retain the existing
/// active-workspace/profile fallback for the default single-workspace flow.
fn workspace_password_for_login(
    one: &ayx_core::profile::AlteryxOneProfile,
    workspace_id: Option<&str>,
) -> Option<String> {
    let password = if let Some(workspace_id) = workspace_id {
        one.workspace_credentials
            .get(workspace_id)
            .and_then(|credential| credential.workspace_password.as_deref())
    } else {
        one.resolved_workspace_password()
    }?;
    (!password.trim().is_empty()).then(|| password.to_string())
}

#[cfg(test)]
mod tests {
    use super::workspace_password_for_login;
    use ayx_core::profile::{AlteryxOneProfile, WorkspaceCredential};

    #[test]
    fn explicit_workspace_password_does_not_fall_back_to_active_workspace() {
        let mut one = AlteryxOneProfile {
            workspace_password: Some("profile-password".to_string()),
            expected_workspace_id: Some("workspace-a".to_string()),
            ..AlteryxOneProfile::default()
        };
        one.workspace_credentials.insert(
            "workspace-a".to_string(),
            WorkspaceCredential {
                workspace_password: Some("workspace-a-password".to_string()),
                ..WorkspaceCredential::default()
            },
        );
        one.workspace_credentials
            .insert("workspace-b".to_string(), WorkspaceCredential::default());

        assert_eq!(
            workspace_password_for_login(&one, Some("workspace-b")),
            None,
            "workspace B must not inherit workspace A or profile-level password"
        );
        assert_eq!(
            workspace_password_for_login(&one, Some("workspace-a")),
            Some("workspace-a-password".to_string())
        );
        assert_eq!(
            workspace_password_for_login(&one, None),
            Some("workspace-a-password".to_string())
        );
    }
}
