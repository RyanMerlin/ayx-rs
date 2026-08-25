use anyhow::{Context, Result, bail};
use ayx_core::envelope::{Envelope, ErrorCode};

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
        OneAuthCommand::Protocol { request } => {
            let raw = if request == std::path::Path::new("-") {
                use std::io::Read as _;
                let mut raw = String::new();
                std::io::stdin()
                    .read_to_string(&mut raw)
                    .context("failed to read agent auth request from stdin")?;
                raw
            } else {
                std::fs::read_to_string(&request).with_context(|| {
                    format!("failed to read agent auth request '{}'", request.display())
                })?
            };
            let request = ayx_core::auth::agent_protocol::decode(&raw)
                .context("agent auth request was not valid JSON")?;
            request
                .validate()
                .map_err(|err| anyhow::anyhow!("invalid agent auth request: {err}"))?;
            let state = ayx_core::auth::AuthState::default();
            let rollout = ayx_core::auth::AuthRollout::from_environment()
                .map_err(|err| anyhow::anyhow!(err))?;
            Envelope::err_coded(
                ErrorCode::Validation,
                "agent auth request validated, but execution is not enabled in the selected rollout",
                serde_json::json!({
                    "protocol_version": ayx_core::auth::AGENT_AUTH_PROTOCOL_VERSION,
                    "request_id": request.request_id,
                    "validated": true,
                    "executed": false,
                    "error_code": "execution_unavailable",
                    "rollout": rollout,
                    "phase": state.phase,
                    "state": state,
                    "secret_values_returned": false,
                }),
            )
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
    base_url_arg: Option<String>,
    workspace_id: Option<String>,
    workspace_gid_arg: Option<String>,
    save_workspace_password: bool,
    secret_policy_arg: Option<String>,
) -> Result<Envelope> {
    use ayx_core::auth::{AuthRollout, SecretPersistencePolicy};
    use ayx_core::profile::{normalize_alteryx_one_token_endpoint, profile_storage_path};
    use ayx_one_api::{
        exchange_auth_code, generate_pkce_challenge, generate_random_state, initiate_device_auth,
        poll_device_token, refresh_one_access_token,
    };
    use serde_json::json;

    // Parse rollout before reading credentials or starting the OTP transport.
    // A typo must not be discovered after an irreversible OTP/PAT side effect.
    let rollout = AuthRollout::from_environment().map_err(|err| anyhow::anyhow!(err))?;
    let mut config = runtime.load_profile_lenient(profile.as_deref())?;
    let mut auth_machine = None;
    {
        let one = config
            .alteryx_one
            .as_mut()
            .context("no alteryx_one section in profile — run `ayx onboard` first or add alteryx_one.account_email to your config")?;

        if let Some(id) = client_id {
            one.oauth_client_id = Some(id);
        }
        if let Some(ep) = token_endpoint_arg {
            let endpoint = ayx_core::one_endpoint::OneEndpoint::parse(&ep)
                .context("invalid --token-endpoint; expected an HTTPS Alteryx One Ping endpoint")?;
            one.token_endpoint_url = Some(normalize_alteryx_one_token_endpoint(endpoint.as_str()));
        }
        if let Some(base_url) = base_url_arg {
            let endpoint = ayx_core::one_endpoint::OneEndpoint::parse(&base_url)
                .context("invalid --base-url; expected an HTTPS Alteryx One regional URL")?;
            one.base_url = Some(endpoint.into_string().trim_end_matches('/').to_string());
        }
    }

    let http = reqwest::blocking::Client::new();

    // Resolve the token endpoint from the profile (or the default Ping issuer).
    let token_endpoint = config
        .alteryx_one
        .as_ref()
        .and_then(|o| o.effective_token_endpoint_url())
        .unwrap_or_else(|| "https://pingauth.alteryxcloud.com/as/token".to_string());
    let token_endpoint = ayx_core::one_endpoint::OneEndpoint::parse(&token_endpoint)
        .context("configured token endpoint failed Alteryx One trust validation")?
        .into_string();

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
    if let (Some(ws_id), Some(requested_gid)) =
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

    // Validate any existing bound credential before the login flow can consume
    // a stored refresh token, client secret, or workspace password. The later
    // write-boundary validation remains necessary because this command may add
    // fresh tokens and persist them under a new binding.
    let existing_binding = auth_credential_binding(&config, workspace_id.as_deref())?;
    crate::onboard::validate_auth_credential_bindings(&config, &existing_binding)?;

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

        let mut auth_url = url::Url::parse(&auth_endpoint)
            .context("configured authorization endpoint was not a valid URL")?;
        auth_url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", client_id_val.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_challenge", pkce.code_challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("scope", "openid"),
            ("state", csrf_state.as_str()),
        ]);
        let auth_url: String = auth_url.into();

        eprintln!("Opening browser for authentication...");
        eprintln!("If the browser doesn't open, visit:\n  {auth_url}");
        open_browser(&auth_url).context("failed to open the authentication browser")?;

        // Wait for the callback (5 minute timeout).
        listener.set_nonblocking(true).ok();
        let start = std::time::Instant::now();
        let code = loop {
            if start.elapsed().as_secs() > 300 {
                bail!("timed out waiting for browser callback");
            }
            match listener.accept() {
                Ok((mut stream, _)) => match parse_browser_callback(&mut stream, &csrf_state)? {
                    BrowserCallback::Code(code) => break code,
                    BrowserCallback::Ignore => continue,
                    BrowserCallback::CsrfMismatch => {
                        bail!("OAuth state mismatch in browser callback — possible CSRF attack")
                    }
                },
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
        let mut machine = ayx_core::auth::AuthStateMachine::default();
        machine
            .apply(ayx_core::auth::AuthEvent::Begin)
            .map_err(|err| anyhow::anyhow!(err))?;
        machine
            .apply(ayx_core::auth::AuthEvent::OtpSent)
            .map_err(|err| anyhow::anyhow!(err))?;
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
            let (result, password) = ayx_one_api::LegacyOtpAdapter.login_with_password(
                &base_url,
                &email,
                &ws_gid,
                workspace_password,
                get_otp,
            )?;
            (result, Some(password))
        } else {
            let result = ayx_one_api::LegacyOtpAdapter.login(
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

        for event in [
            ayx_core::auth::AuthEvent::OtpAccepted,
            ayx_core::auth::AuthEvent::WorkspaceResolved,
            ayx_core::auth::AuthEvent::WorkspacePasswordAccepted,
            ayx_core::auth::AuthEvent::TokenExchanged,
        ] {
            machine.apply(event).map_err(|err| anyhow::anyhow!(err))?;
        }
        auth_machine = Some(machine);

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
        if let Err(err) = open_browser(uri) {
            eprintln!("warning: could not open browser automatically: {err}");
        }

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
    let remembered_policy = ayx_core::auth::load_persistence_policy(&path);
    let requested_policy = secret_policy_arg
        .as_deref()
        .map(|value| {
            SecretPersistencePolicy::parse(value)
                .ok_or_else(|| anyhow::anyhow!("invalid --secret-policy"))
        })
        .transpose()
        .map_err(|_| {
            anyhow::anyhow!("invalid --secret-policy; use secure, plaintext, or session")
        })?;
    let persistence_policy = requested_policy
        .or(remembered_policy)
        .unwrap_or(SecretPersistencePolicy::Secure);

    if let Some(machine) = auth_machine.as_mut() {
        machine
            .apply(ayx_core::auth::AuthEvent::PersistStarted)
            .map_err(|err| anyhow::anyhow!(err))?;
    }

    if persistence_policy == SecretPersistencePolicy::SessionOnly {
        if let Some(machine) = auth_machine.as_mut() {
            machine
                .apply(ayx_core::auth::AuthEvent::PersistSucceeded)
                .map_err(|err| anyhow::anyhow!(err))?;
        }
        return Ok(Envelope::ok_with_data(
            "credentials kept for this session",
            json!({
                "action": "auth.login",
                "status": "ok",
                "profile": profile_name,
                "persistence": "session_only",
                "token_length": final_access_token.len(),
                "has_refresh_token": final_refresh_token.is_some(),
                "rollout": format!("{rollout:?}").to_ascii_lowercase(),
                "auth_state": auth_machine.as_ref().map(|machine| machine.state()),
            }),
        ));
    }

    let binding = auth_credential_binding(&config, workspace_id.as_deref())?;
    crate::onboard::validate_auth_credential_bindings(&config, &binding)?;
    let explicit_plaintext = requested_policy == Some(SecretPersistencePolicy::PlaintextFallback);
    let allow_inline = persistence_policy == SecretPersistencePolicy::PlaintextFallback
        && (!save_workspace_password
            || explicit_plaintext
            || rollout.uses_new_orchestration()
            || remembered_policy == Some(SecretPersistencePolicy::PlaintextFallback));
    let secret_policy = if allow_inline {
        crate::onboard::InlineSecretPolicy::Allow
    } else {
        // Secure is the default for the new wizard and remains fail-closed
        // unless the interactive fallback below receives explicit consent.
        crate::onboard::InlineSecretPolicy::Forbid
    };
    let secretize_result =
        crate::onboard::write_config_with_binding(&path, &config, secret_policy, Some(&binding));
    let secretize = match secretize_result {
        Ok(output) => output,
        Err(_err)
            if persistence_policy == SecretPersistencePolicy::Secure
                && interactive_secret_fallback()? =>
        {
            let output = crate::onboard::write_config_with_binding(
                &path,
                &config,
                crate::onboard::InlineSecretPolicy::Allow,
                Some(&binding),
            )?;
            let _ = ayx_core::auth::save_persistence_policy(
                &path,
                SecretPersistencePolicy::PlaintextFallback,
            );
            eprintln!("warning: secure credential storage was unavailable; using the profile-file fallback by explicit consent");
            output
        }
        Err(err) => {
            return Err(err).context(
                "secure credential storage is unavailable; pass --secret-policy plaintext or --secret-policy session explicitly",
            )
        }
    };

    if requested_policy == Some(SecretPersistencePolicy::PlaintextFallback)
        && remembered_policy != Some(SecretPersistencePolicy::PlaintextFallback)
        && let Err(err) = ayx_core::auth::save_persistence_policy(
            &path,
            SecretPersistencePolicy::PlaintextFallback,
        )
    {
        eprintln!(
            "warning: credentials were stored, but the plaintext-fallback choice could not be remembered: {err}"
        );
    }
    if let Some(msg) = crate::onboard::inline_secret_warning(&secretize.inline_fields)
        && remembered_policy != Some(SecretPersistencePolicy::PlaintextFallback)
    {
        eprintln!("warning: {msg}");
    }
    if let Some(machine) = auth_machine.as_mut() {
        machine
            .apply(ayx_core::auth::AuthEvent::PersistSucceeded)
            .map_err(|err| anyhow::anyhow!(err))?;
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
            "persistence": format!("{persistence_policy:?}").to_ascii_lowercase(),
            "rollout": format!("{rollout:?}").to_ascii_lowercase(),
            "auth_state": auth_machine.as_ref().map(|machine| machine.state()),
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
        // Logout must never re-persist unrelated resolved secrets inline just
        // because this command cleared One credentials. Fail closed when the
        // secure store is unavailable; an explicit login/fallback decision is
        // the only place allowed to change persistence policy.
        crate::onboard::InlineSecretPolicy::Forbid,
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

enum BrowserCallback {
    Code(String),
    Ignore,
    CsrfMismatch,
}

/// Read one small HTTP callback request.  A local callback listener is still a
/// network-facing parser: bound the header size and read duration, tolerate
/// browser noise such as `/favicon.ico`, and use the URL parser rather than
/// hand-splitting percent-encoded query parameters.
fn parse_browser_callback(
    stream: &mut std::net::TcpStream,
    expected_state: &str,
) -> Result<BrowserCallback> {
    use std::io::Read;

    const MAX_CALLBACK_HEADER: usize = 8 * 1024;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .context("failed to set browser callback read deadline")?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(3)))
        .context("failed to set browser callback write deadline")?;

    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                request.extend_from_slice(&chunk[..n]);
                if request.len() > MAX_CALLBACK_HEADER {
                    write_callback_response(
                        stream,
                        "431 Request Header Fields Too Large",
                        "Request too large.",
                    );
                    return Ok(BrowserCallback::Ignore);
                }
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(BrowserCallback::Ignore);
            }
            Err(err) => return Err(err).context("failed to read browser callback"),
        }
    }
    let Ok(request) = std::str::from_utf8(&request) else {
        write_callback_response(stream, "400 Bad Request", "Malformed callback.");
        return Ok(BrowserCallback::Ignore);
    };
    let mut parts = request.lines().next().unwrap_or("").split_whitespace();
    let method = parts.next();
    let target = parts.next();
    if method != Some("GET") || parts.next().is_none() {
        write_callback_response(stream, "400 Bad Request", "Malformed callback.");
        return Ok(BrowserCallback::Ignore);
    }
    let Some(target) = target else {
        write_callback_response(stream, "400 Bad Request", "Malformed callback.");
        return Ok(BrowserCallback::Ignore);
    };
    let Ok(url) = url::Url::parse(&format!("http://localhost{target}")) else {
        write_callback_response(stream, "400 Bad Request", "Malformed callback.");
        return Ok(BrowserCallback::Ignore);
    };
    if url.path() != "/callback" {
        write_callback_response(stream, "404 Not Found", "Not found.");
        return Ok(BrowserCallback::Ignore);
    }
    let mut state = None;
    let mut code = None;
    for (name, value) in url.query_pairs() {
        match name.as_ref() {
            "state" => state = Some(value.into_owned()),
            "code" => code = Some(value.into_owned()),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        write_callback_response(
            stream,
            "400 Bad Request",
            "State mismatch. Please try again.",
        );
        return Ok(BrowserCallback::CsrfMismatch);
    }
    let Some(code) = code.filter(|code| !code.is_empty()) else {
        write_callback_response(stream, "400 Bad Request", "No authorization code.");
        return Ok(BrowserCallback::Ignore);
    };
    write_callback_response(stream, "200 OK", "Authenticated. You can close this tab.");
    Ok(BrowserCallback::Code(code))
}

fn write_callback_response(stream: &mut std::net::TcpStream, status: &str, body: &str) {
    use std::io::Write;
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    return std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ());
    #[cfg(target_os = "macos")]
    return std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map(|_| ());
    #[cfg(target_os = "windows")]
    return std::process::Command::new("explorer.exe")
        .arg(url)
        .spawn()
        .map(|_| ());
    #[allow(unreachable_code)]
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no browser launcher is available on this platform",
    ))
}

fn interactive_secret_fallback() -> Result<bool> {
    use std::io::{IsTerminal, Write};

    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }
    eprintln!(
        "The operating-system credential store is unavailable. The profile-file fallback is protected with owner-only permissions, but its credential is plaintext on disk."
    );
    eprint!("Store credentials in the profile file? [Y/n] ");
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("failed to read credential-storage choice")?;
    Ok(answer.trim().is_empty()
        || matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

fn auth_credential_binding(
    config: &ayx_core::profile::Config,
    workspace_id: Option<&str>,
) -> Result<ayx_core::auth::CredentialBinding> {
    crate::onboard::binding_for_auth_config(config, workspace_id)
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
    use super::{BrowserCallback, parse_browser_callback, workspace_password_for_login};
    use ayx_core::profile::{AlteryxOneProfile, WorkspaceCredential};
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn callback_request(request_target: &str, expected_state: &str) -> (BrowserCallback, String) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind callback fixture");
        let address = listener.local_addr().expect("callback fixture address");
        let target = request_target.to_owned();
        let state = expected_state.to_owned();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept callback fixture");
            parse_browser_callback(&mut stream, &state).expect("parse callback fixture")
        });

        let mut client = TcpStream::connect(address).expect("connect callback fixture");
        write!(
            client,
            "GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .expect("write callback request");
        let mut response = String::new();
        std::io::Read::read_to_string(&mut client, &mut response).expect("read callback response");
        (server.join().expect("callback fixture thread"), response)
    }

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

    #[test]
    fn browser_callback_decodes_standard_query_parameters() {
        let (result, response) = callback_request(
            "/callback?code=otp%2Bvalue%2Fpart&state=state%20value&ignored=x",
            "state value",
        );
        assert!(matches!(result, BrowserCallback::Code(code) if code == "otp+value/part"));
        assert!(response.starts_with("HTTP/1.1 200 OK"));
    }

    #[test]
    fn browser_callback_ignores_favicon_and_rejects_csrf() {
        let (favicon, favicon_response) = callback_request("/favicon.ico", "expected");
        assert!(matches!(favicon, BrowserCallback::Ignore));
        assert!(favicon_response.starts_with("HTTP/1.1 404 Not Found"));

        let (csrf, csrf_response) =
            callback_request("/callback?code=secret&state=wrong", "expected");
        assert!(matches!(csrf, BrowserCallback::CsrfMismatch));
        assert!(csrf_response.starts_with("HTTP/1.1 400 Bad Request"));
    }

    #[test]
    fn browser_callback_tolerates_malformed_requests_without_returning_code() {
        let (result, response) = callback_request("/callback?state=expected", "expected");
        assert!(matches!(result, BrowserCallback::Ignore));
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    }
}
