use std::collections::{BTreeSet, HashSet};

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
    auth_flow_arg: Option<String>,
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
    let rollout = match auth_flow_arg.as_deref() {
        Some(value) => {
            let selected = AuthRollout::parse(value).map_err(|err| anyhow::anyhow!(err))?;
            if let Ok(environment_value) =
                std::env::var("AYX_AUTH_ROLLOUT").or_else(|_| std::env::var("AUTH_ROLLOUT"))
                && let Ok(environment_rollout) = AuthRollout::parse(&environment_value)
                && environment_rollout != selected
            {
                anyhow::bail!(
                    "--auth-flow {selected:?} conflicts with AYX_AUTH_ROLLOUT={environment_value}; unset the environment override or choose the same lane"
                );
            }
            selected
        }
        None => AuthRollout::from_environment().map_err(|err| anyhow::anyhow!(err))?,
    };
    let mut config = runtime.load_profile_lenient_for_auth(profile.as_deref())?;
    let profile_name = config.profile_name.clone();
    let path = profile_storage_path(&profile_name)?;
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
    let remembered_policy = ayx_core::auth::load_persistence_policy(&path);
    let persistence_policy = requested_policy
        .or(remembered_policy)
        .unwrap_or(SecretPersistencePolicy::Secure);
    if persistence_policy == SecretPersistencePolicy::SessionOnly {
        bail!(
            "--secret-policy session is not supported by the standalone `ayx one login` command: the process exits after login and cannot retain a usable session; use secure or plaintext explicitly"
        );
    }
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

    let password_workspace_id = workspace_id.clone().or_else(|| {
        config
            .alteryx_one
            .as_ref()
            .and_then(|one| one.active_workspace_id().map(str::to_string))
    });

    // Validate any existing bound credential before the login flow can consume
    // a stored refresh token, client secret, or workspace password. The later
    // write-boundary validation remains necessary because this command may add
    // fresh tokens and persist them under a new binding.
    let existing_binding = auth_credential_binding(&config, workspace_id.as_deref())?;
    crate::onboard::validate_auth_credential_bindings_for_rollout(
        &config,
        &existing_binding,
        Some(rollout),
    )?;

    let mut workspace_password_to_save = None;
    let mut token_expires_at = None;

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
        // Password saving is a user decision, not a command-line memorization
        // exercise. If we had to prompt for it, offer one clear secure-save
        // decision after a successful login. The explicit flag remains useful
        // for scripted interactive runs and skips that second confirmation.
        let offer_workspace_password_save = should_offer_workspace_password_save(
            save_workspace_password,
            workspace_password.is_some(),
            workspace_password_is_supplied_by_environment(),
        );
        let capture_workspace_password = save_workspace_password || offer_workspace_password_save;

        if ws_gid.is_empty() {
            anyhow::bail!(
                "workspace_gid is required — set alteryx_one.workspace_gid in your profile \
                 or pass --workspace-gid"
            );
        }

        ensure_visible_line_input()?;
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
        let (result, captured_workspace_password) = if rollout.uses_new_orchestration() {
            if capture_workspace_password {
                let (result, password) = ayx_one_api::WizardOtpAdapter.login_with_password(
                    &base_url,
                    &email,
                    &ws_gid,
                    workspace_password,
                    get_otp,
                )?;
                (result, Some(password))
            } else {
                let result = ayx_one_api::WizardOtpAdapter.login(
                    &base_url,
                    &email,
                    &ws_gid,
                    workspace_password,
                    get_otp,
                )?;
                (result, None)
            }
        } else if capture_workspace_password {
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

        if let Some(password) = captured_workspace_password
            && (save_workspace_password
                || (offer_workspace_password_save && interactive_workspace_password_save()?))
        {
            workspace_password_to_save = Some(password);
        }

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

        for event in [
            ayx_core::auth::AuthEvent::OtpAccepted,
            ayx_core::auth::AuthEvent::WorkspaceResolved,
            ayx_core::auth::AuthEvent::WorkspacePasswordAccepted,
            ayx_core::auth::AuthEvent::TokenExchanged,
        ] {
            machine.apply(event).map_err(|err| anyhow::anyhow!(err))?;
        }
        auth_machine = Some(machine);
        token_expires_at = result.token_expires_at;

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
        if let Some(ws_id) = password_workspace_id.as_deref() {
            one.workspace_credentials
                .entry(ws_id.to_string())
                .or_default()
                .workspace_password = Some(password);
        } else {
            // A profile with only a workspace GID has no numeric workspace key
            // to scope a nested credential. The top-level secret remains bound
            // to that GID at the keyring write boundary, so it is safe to reuse
            // for this single-profile login on the next invocation.
            one.workspace_password = Some(password);
        }
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
    if let Some(machine) = auth_machine.as_mut() {
        machine
            .apply(ayx_core::auth::AuthEvent::PersistStarted)
            .map_err(|err| anyhow::anyhow!(err))?;
    }

    let binding = auth_credential_binding(&config, workspace_id.as_deref())?;
    crate::onboard::validate_auth_credential_bindings_for_rollout(
        &config,
        &binding,
        Some(rollout),
    )?;
    if let Some(one) = config.alteryx_one.as_mut() {
        one.auth_rollout = Some(rollout);
    }
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
    let secretize_result = crate::onboard::write_config_with_binding_for_rollout(
        &path,
        &config,
        secret_policy,
        Some(&binding),
        Some(rollout),
    );
    let mut effective_persistence_policy = persistence_policy;
    let secretize = match secretize_result {
        Ok(output) => output,
        Err(_err)
            if persistence_policy == SecretPersistencePolicy::Secure
                && interactive_secret_fallback()? =>
        {
            let output = crate::onboard::write_config_with_binding_for_rollout(
                &path,
                &config,
                crate::onboard::InlineSecretPolicy::Allow,
                Some(&binding),
                Some(rollout),
            )?;
            let _ = ayx_core::auth::save_persistence_policy(
                &path,
                SecretPersistencePolicy::PlaintextFallback,
            );
            effective_persistence_policy = SecretPersistencePolicy::PlaintextFallback;
            eprintln!("warning: secure credential storage was unavailable; using the profile-file fallback by explicit consent");
            output
        }
        Err(err) => {
            return Err(err).context(
                "secure credential storage is unavailable; pass --secret-policy plaintext or --secret-policy session explicitly",
            )
        }
    };

    if persistence_policy_to_remember(requested_policy, effective_persistence_policy).is_some()
        && let Err(err) =
            ayx_core::auth::save_persistence_policy(&path, effective_persistence_policy)
    {
        eprintln!(
            "warning: credentials were stored, but the selected credential-storage policy could not be remembered: {err}"
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
    // A confirmation must mean the complete login transaction succeeded: remote
    // authentication, state validation, and durable credential persistence.
    // Resolve workspace display metadata only after that point. The lookup is
    // bounded and best-effort, so it cannot turn a successful login into a
    // hang or a failure.
    let authenticated_workspace = lookup_authenticated_workspace(&endpoint, &final_access_token);
    eprintln!("\nAuthentication Successful!\n");
    if let Some(expires) = token_expires_at {
        eprintln!("Token expires: {expires}");
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
            "workspace_id": authenticated_workspace.as_ref().and_then(|workspace| workspace.id.as_deref()),
            "workspace_name": authenticated_workspace.as_ref().and_then(|workspace| workspace.name.as_deref()),
            "token_length": final_access_token.len(),
            "has_refresh_token": final_refresh_token.is_some(),
            "inline_secret_fields": secretize.inline_fields,
            "persistence": format!("{effective_persistence_policy:?}").to_ascii_lowercase(),
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
    let keyring_accounts = credential_keyring_accounts(one);

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
    let profiles_dir = path
        .parent()
        .context("profile storage path has no profiles directory")?;
    let cleanup_accounts: BTreeSet<String> =
        crate::secret::unreferenced_keyring_accounts_excluding_profile(
            profiles_dir,
            &path,
            &keyring_accounts,
        )?
        .into_iter()
        .collect();
    let secretize = crate::onboard::write_config_with_policy_and_delete_keyring_accounts(
        &path,
        &config,
        // Logout must never re-persist unrelated resolved secrets inline just
        // because this command cleared One credentials. Fail closed when the
        // secure store is unavailable; an explicit login/fallback decision is
        // the only place allowed to change persistence policy.
        crate::onboard::InlineSecretPolicy::Forbid,
        &cleanup_accounts,
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
            "local_keyring_entries_deleted": cleanup_accounts.len(),
            "local_keyring_entries_retained": keyring_accounts.len() - cleanup_accounts.len(),
            "remote_revocation": "not attempted",
            "notes": [
                "Cleared stored Alteryx One access/refresh credentials, workspace passwords, and credential refs from the profile",
                "Deleted local keyring entries that no other profile references; shared entries were retained",
                "Remote token revocation was not attempted",
            ],
            "inline_secret_fields": secretize.inline_fields,
        }),
    ))
}

/// Windows console state is shared by child processes. An interrupted masked
/// password prompt can leave it in raw/no-echo mode, so restore ordinary line
/// input before any visible confirmation or OTP response.
fn ensure_visible_line_input() -> Result<()> {
    #[cfg(windows)]
    {
        crossterm::terminal::disable_raw_mode()
            .context("failed to restore visible console input for the OTP prompt")?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthenticatedWorkspace {
    id: Option<String>,
    name: Option<String>,
}

/// The OTP flow identifies a workspace by GID, while the platform's current
/// workspace endpoint provides the numeric ID and operator-facing name. This
/// is best-effort confirmation only: a successful login must not become a
/// failure because a subsequent informational read is unavailable.
fn lookup_authenticated_workspace(
    base_url: &str,
    access_token: &str,
) -> Option<AuthenticatedWorkspace> {
    let endpoint = format!("{}/v4/workspaces/current", base_url.trim_end_matches('/'));
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    let response = http.get(endpoint).bearer_auth(access_token).send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: serde_json::Value = response.json().ok()?;
    Some(authenticated_workspace_from_value(&value))
}

fn authenticated_workspace_from_value(value: &serde_json::Value) -> AuthenticatedWorkspace {
    let id = value
        .get("id")
        .or_else(|| value.get("workspaceId"))
        .or_else(|| value.get("workspace_id"))
        .and_then(|id| {
            id.as_i64()
                .map(|id| id.to_string())
                .or_else(|| id.as_u64().map(|id| id.to_string()))
                .or_else(|| id.as_str().map(str::to_string))
        });
    let name = ["displayName", "name"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(serde_json::Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string);
    AuthenticatedWorkspace { id, name }
}

fn workspace_password_is_supplied_by_environment() -> bool {
    std::env::var("AYX_ONE_WS_PASSWORD")
        .ok()
        .is_some_and(|password| !password.is_empty())
}

fn should_offer_workspace_password_save(
    explicit_save: bool,
    has_stored_password: bool,
    has_environment_password: bool,
) -> bool {
    !explicit_save && !has_stored_password && !has_environment_password
}

/// Ask only after the password has been accepted by the remote workspace.
/// Secure keyring storage is the ergonomic default; decline explicitly when a
/// user does not want the password remembered on this machine.
fn interactive_workspace_password_save() -> Result<bool> {
    use std::io::Write as _;

    ensure_visible_line_input()?;
    eprint!("Save this workspace password securely for future logins? [Y/n] ");
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("failed to read workspace-password storage choice")?;
    Ok(accepts_workspace_password_save(&answer))
}

fn accepts_workspace_password_save(answer: &str) -> bool {
    !matches!(answer.trim().to_ascii_lowercase().as_str(), "n" | "no")
}

fn credential_keyring_accounts(one: &ayx_core::profile::AlteryxOneProfile) -> HashSet<String> {
    let mut accounts = HashSet::new();
    let mut add = |reference: Option<&str>| {
        if let Some(account) = reference.and_then(|reference| reference.strip_prefix("keyring:")) {
            accounts.insert(account.to_string());
        }
    };
    add(one.access_token_ref.as_deref());
    add(one.refresh_token_ref.as_deref());
    add(one.workspace_password_ref.as_deref());
    for credential in one.workspace_credentials.values() {
        add(credential.access_token_ref.as_deref());
        add(credential.refresh_token_ref.as_deref());
        add(credential.workspace_password_ref.as_deref());
    }
    accounts
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
    eprint!("Store credentials in the profile file? [y/N] ");
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("failed to read credential-storage choice")?;
    Ok(accepts_plaintext_fallback(&answer))
}

/// A plaintext fallback changes the credential-at-rest security boundary, so
/// empty input must fail closed. Keep the parser separate from terminal I/O so
/// the consent contract is directly regression-tested.
fn accepts_plaintext_fallback(answer: &str) -> bool {
    accepts_affirmative_answer(answer)
}

fn accepts_affirmative_answer(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// An explicit policy is a user decision and must replace a previously
/// remembered choice. An interactive secure-store failure is the one implicit
/// case that is remembered, and only after affirmative plaintext consent.
fn persistence_policy_to_remember(
    requested: Option<ayx_core::auth::SecretPersistencePolicy>,
    effective: ayx_core::auth::SecretPersistencePolicy,
) -> Option<ayx_core::auth::SecretPersistencePolicy> {
    (requested.is_some() || effective == ayx_core::auth::SecretPersistencePolicy::PlaintextFallback)
        .then_some(effective)
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
    use super::{
        AuthenticatedWorkspace, BrowserCallback, accepts_plaintext_fallback,
        accepts_workspace_password_save, authenticated_workspace_from_value,
        parse_browser_callback, persistence_policy_to_remember,
        should_offer_workspace_password_save, workspace_password_for_login,
    };
    use ayx_core::profile::{AlteryxOneProfile, WorkspaceCredential};
    use httpmock::prelude::*;
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
    fn plaintext_fallback_requires_affirmative_consent() {
        assert!(!accepts_plaintext_fallback(""));
        assert!(!accepts_plaintext_fallback("no"));
        assert!(accepts_plaintext_fallback("y"));
        assert!(accepts_plaintext_fallback(" YES "));
    }

    #[test]
    fn workspace_password_save_offer_is_only_for_a_new_interactive_password() {
        assert!(should_offer_workspace_password_save(false, false, false));
        assert!(!should_offer_workspace_password_save(true, false, false));
        assert!(!should_offer_workspace_password_save(false, true, false));
        assert!(!should_offer_workspace_password_save(false, false, true));
    }

    #[test]
    fn workspace_password_save_defaults_to_secure_keyring_storage() {
        assert!(accepts_workspace_password_save(""));
        assert!(accepts_workspace_password_save("y"));
        assert!(!accepts_workspace_password_save("n"));
        assert!(!accepts_workspace_password_save("NO"));
    }

    #[test]
    fn authenticated_workspace_confirmation_uses_numeric_id_and_display_name() {
        let workspace = authenticated_workspace_from_value(&serde_json::json!({
            "id": 91946,
            "name": "Machine Name",
            "displayName": "Alteryx FDE"
        }));
        assert_eq!(
            workspace,
            AuthenticatedWorkspace {
                id: Some("91946".to_string()),
                name: Some("Alteryx FDE".to_string()),
            }
        );

        let fallback = authenticated_workspace_from_value(&serde_json::json!({
            "workspaceId": "42",
            "name": "  Secondary Workspace  "
        }));
        assert_eq!(fallback.id.as_deref(), Some("42"));
        assert_eq!(fallback.name.as_deref(), Some("Secondary Workspace"));
    }

    #[test]
    fn authenticated_workspace_lookup_is_best_effort_for_http_failures() {
        let server = MockServer::start();
        let unavailable = server.mock(|when, then| {
            when.method(GET).path("/v4/workspaces/current");
            then.status(503);
        });
        assert_eq!(
            super::lookup_authenticated_workspace(&server.base_url(), "token"),
            None
        );
        unavailable.assert();

        let malformed = MockServer::start();
        let malformed_response = malformed.mock(|when, then| {
            when.method(GET).path("/v4/workspaces/current");
            then.status(200).body("not json");
        });
        assert_eq!(
            super::lookup_authenticated_workspace(&malformed.base_url(), "token"),
            None
        );
        malformed_response.assert();
    }

    #[test]
    fn explicit_secure_policy_replaces_a_remembered_plaintext_choice() {
        use ayx_core::auth::SecretPersistencePolicy::{PlaintextFallback, Secure};

        assert_eq!(
            persistence_policy_to_remember(Some(Secure), Secure),
            Some(Secure)
        );
        assert_eq!(
            persistence_policy_to_remember(Some(PlaintextFallback), PlaintextFallback),
            Some(PlaintextFallback)
        );
        assert_eq!(persistence_policy_to_remember(None, Secure), None);
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
