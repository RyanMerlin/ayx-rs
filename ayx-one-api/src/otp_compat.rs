//! Versioned compatibility contract for the legacy email-OTP transport.
//!
//! The transport in `email_otp.rs` remains the emergency rollback adapter. The
//! contract is intentionally data-only: orchestration code can select it and
//! characterization tests can assert the externally visible sequence without
//! duplicating (and accidentally changing) the HTTP implementation.

use anyhow::Result;
use ayx_core::auth::{AuthFailureKind, OperationOutcome, WizardAction, WizardEngine, WizardStep};
use serde::{Deserialize, Serialize};

use crate::email_otp::{
    OtpAuthResult, OtpValidationRejected, email_otp_login, email_otp_login_with_password,
};

pub const LEGACY_OTP_COMPATIBILITY_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyOtpRetryBudget {
    pub transient_attempts: u32,
    pub otp_attempts_per_reference: u32,
    pub otp_sends: u32,
    pub workspace_password_attempts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyOtpRejectionMapping {
    pub reprompt_same_reference: bool,
    pub resend_after_attempts: u32,
    pub terminal_after_sends: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LegacyTransportErrorAction {
    Retry,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyWorkspacePasswordMapping {
    pub fixed_source: LegacyTransportErrorAction,
    pub interactive_source: LegacyTransportErrorAction,
    pub interactive_attempts: u32,
}

/// Machine-checkable mapping for the legacy adapter's externally visible
/// rejection behavior. Keeping this typed prevents the compatibility gate
/// from becoming a prose-only claim that can drift from the transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyOtpErrorMapping {
    pub otp_rejection: LegacyOtpRejectionMapping,
    pub transient_transport: LegacyTransportErrorAction,
    pub terminal_http_status: LegacyTransportErrorAction,
    pub workspace_password: LegacyWorkspacePasswordMapping,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyOtpCompatibilityContract {
    pub version: u16,
    pub endpoint_order: Vec<String>,
    pub redirect_policy: String,
    pub cookie_names: Vec<String>,
    pub retry_budget: LegacyOtpRetryBudget,
    pub password_timing: String,
    pub error_mapping: LegacyOtpErrorMapping,
}

impl Default for LegacyOtpCompatibilityContract {
    fn default() -> Self {
        Self {
            version: LEGACY_OTP_COMPATIBILITY_VERSION,
            endpoint_order: [
                "GET /v4/platformAuth/session",
                "POST /v4/auth/sendPasscode",
                "POST /v4/auth/validatePasscode",
                "GET /v4/auth/accounts",
                "GET /?workspace=<name>&workspaceGid=<gid>",
                "GET /authorize?interaction_id=<id>",
                "POST /session",
                "GET /token/<interaction_id>/resume",
                "POST /v4/apiAccessTokens",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            redirect_policy: "manual; allow base host, parent, and sibling subdomains only"
                .to_string(),
            cookie_names: ["local-auth-workspace", "x-csrf-token"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            retry_budget: LegacyOtpRetryBudget {
                transient_attempts: 3,
                otp_attempts_per_reference: 3,
                otp_sends: 2,
                workspace_password_attempts: 3,
            },
            password_timing: "resolve after OTP and workspace redirect; masked interactive input"
                .to_string(),
            error_mapping: LegacyOtpErrorMapping {
                otp_rejection: LegacyOtpRejectionMapping {
                    reprompt_same_reference: true,
                    resend_after_attempts: crate::email_otp::OTP_ATTEMPTS_PER_REFERENCE,
                    terminal_after_sends: crate::email_otp::MAX_OTP_SENDS,
                },
                transient_transport: LegacyTransportErrorAction::Retry,
                terminal_http_status: LegacyTransportErrorAction::Terminal,
                workspace_password: LegacyWorkspacePasswordMapping {
                    fixed_source: LegacyTransportErrorAction::Terminal,
                    interactive_source: LegacyTransportErrorAction::Retry,
                    interactive_attempts: crate::email_otp::WORKSPACE_PASSWORD_ATTEMPTS,
                },
            },
        }
    }
}

impl LegacyOtpCompatibilityContract {
    pub fn validate(&self) -> Result<()> {
        let expected = Self::default();
        anyhow::ensure!(
            self.version == expected.version,
            "unsupported OTP contract version"
        );
        anyhow::ensure!(
            self.endpoint_order == expected.endpoint_order,
            "legacy OTP endpoint order changed"
        );
        anyhow::ensure!(
            self.redirect_policy == expected.redirect_policy,
            "legacy OTP redirect policy changed"
        );
        anyhow::ensure!(
            self.cookie_names == expected.cookie_names,
            "legacy OTP cookie contract changed"
        );
        anyhow::ensure!(
            self.retry_budget == expected.retry_budget,
            "legacy OTP retry budget changed"
        );
        anyhow::ensure!(
            self.password_timing == expected.password_timing,
            "legacy OTP password timing changed"
        );
        anyhow::ensure!(
            self.error_mapping == expected.error_mapping,
            "legacy OTP error mapping changed"
        );
        Ok(())
    }
}

/// Emergency rollback adapter. It delegates to the existing transport rather
/// than reimplementing any request, cookie, redirect, or error behavior.
#[derive(Debug, Clone, Copy, Default)]
pub struct LegacyOtpAdapter;

/// Experimental v0.17 stepwise adapter. Legacy remains independently
/// implemented for rollback; this adapter is enabled only by the explicit
/// Wizard rollout after its differential tests pass.
#[derive(Debug, Clone, Copy, Default)]
pub struct WizardOtpAdapter;

impl WizardOtpAdapter {
    pub fn login<F>(
        &self,
        base_url: &str,
        email: &str,
        workspace_gid: &str,
        workspace_password: Option<String>,
        get_otp: F,
    ) -> Result<OtpAuthResult>
    where
        F: Fn() -> Result<String>,
    {
        let mut engine = WizardEngine::default();
        let mut session = crate::WizardOtpSession::new(base_url, email, workspace_gid)?;
        expect(engine.start()?, WizardStep::SendOtp)?;
        let reference = match session.send_otp() {
            Ok(reference) => reference,
            Err(err) => {
                return Err(err.context(
                    "Wizard OTP-send outcome is unknown; do not retry automatically. \
                     Inspect/reconcile the attempted login before retrying.",
                ));
            }
        };
        if engine.record(OperationOutcome::Accepted)? != WizardAction::PromptOtp {
            anyhow::bail!("Wizard did not prompt for OTP after send")
        }
        loop {
            let otp = get_otp()?;
            expect(engine.submit_otp()?, WizardStep::ValidateOtp)?;
            match session.validate_otp(&reference, &otp) {
                Ok(()) => break,
                Err(err) if err.downcast_ref::<OtpValidationRejected>().is_some() => {
                    match engine.record(OperationOutcome::Rejected {
                        kind: AuthFailureKind::InvalidOtp,
                    })? {
                        WizardAction::PromptOtp => continue,
                        WizardAction::Invoke {
                            step: WizardStep::SendOtp,
                        } => {
                            anyhow::bail!(
                                "Wizard OTP reference exhausted; restart through AYX_AUTH_ROLLOUT=legacy"
                            )
                        }
                        action => anyhow::bail!(
                            "Wizard OTP validation stopped at {action:?}; retry through AYX_AUTH_ROLLOUT=legacy"
                        ),
                    }
                }
                Err(err) => {
                    let action = engine.record(OperationOutcome::Rejected {
                        kind: AuthFailureKind::TransientTransport,
                    })?;
                    anyhow::bail!(
                        "Wizard OTP validation could not be classified; action={action:?}: {err:#}"
                    );
                }
            }
        }
        expect(
            engine.record(OperationOutcome::Accepted)?,
            WizardStep::ResolveWorkspace,
        )?;
        session.resolve_workspace()?;
        let action = engine.record(OperationOutcome::Accepted)?;
        if action != WizardAction::PromptWorkspacePassword {
            anyhow::bail!("Wizard workspace transition drifted: {action:?}");
        }
        let password =
            match workspace_password.or_else(|| std::env::var("AYX_ONE_WS_PASSWORD").ok()) {
                Some(value) if !value.trim().is_empty() => value,
                _ => {
                    eprint!("Workspace password: ");
                    rpassword::read_password()?
                }
            };
        expect(
            engine.submit_workspace_password()?,
            WizardStep::SubmitWorkspacePassword,
        )?;
        session.submit_workspace_password(&password)?;
        expect(
            engine.record(OperationOutcome::Accepted)?,
            WizardStep::ResumeOidc,
        )?;
        session.resume_oidc()?;
        expect(
            engine.record(OperationOutcome::Accepted)?,
            WizardStep::MintPat,
        )?;
        let result = match session.mint_pat() {
            Ok(result) => result,
            Err(err) => {
                return Err(err.context(
                    "Wizard PAT-mint outcome is unknown; do not retry automatically. \
                     Inspect/reconcile the recent PAT inventory before retrying.",
                ));
            }
        };
        expect(
            engine.record(OperationOutcome::Accepted)?,
            WizardStep::Persist,
        )?;
        let _ = engine.record(OperationOutcome::Accepted)?;
        Ok(result)
    }
}

fn expect(action: WizardAction, step: WizardStep) -> Result<()> {
    match action {
        WizardAction::Invoke { step: actual } if actual == step => Ok(()),
        other => anyhow::bail!("Wizard transition drifted: expected {step:?}, got {other:?}"),
    }
}

impl LegacyOtpAdapter {
    pub fn contract() -> LegacyOtpCompatibilityContract {
        LegacyOtpCompatibilityContract::default()
    }

    pub fn login<F>(
        &self,
        base_url: &str,
        email: &str,
        workspace_gid: &str,
        workspace_password: Option<String>,
        get_otp: F,
    ) -> Result<OtpAuthResult>
    where
        F: Fn() -> Result<String> + Send + 'static,
    {
        Self::contract().validate()?;
        email_otp_login(base_url, email, workspace_gid, workspace_password, get_otp)
    }

    pub fn login_with_password<F>(
        &self,
        base_url: &str,
        email: &str,
        workspace_gid: &str,
        workspace_password: Option<String>,
        get_otp: F,
    ) -> Result<(OtpAuthResult, String)>
    where
        F: Fn() -> Result<String> + Send + 'static,
    {
        Self::contract().validate()?;
        email_otp_login_with_password(base_url, email, workspace_gid, workspace_password, get_otp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_contract_is_self_consistent() {
        let contract = LegacyOtpCompatibilityContract::default();
        contract.validate().unwrap();
        assert_eq!(contract.version, LEGACY_OTP_COMPATIBILITY_VERSION);
    }

    #[test]
    fn legacy_contract_pins_security_sensitive_order_and_limits() {
        let contract = LegacyOtpAdapter::contract();
        assert_eq!(
            contract.endpoint_order,
            [
                "GET /v4/platformAuth/session",
                "POST /v4/auth/sendPasscode",
                "POST /v4/auth/validatePasscode",
                "GET /v4/auth/accounts",
                "GET /?workspace=<name>&workspaceGid=<gid>",
                "GET /authorize?interaction_id=<id>",
                "POST /session",
                "GET /token/<interaction_id>/resume",
                "POST /v4/apiAccessTokens",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
        assert_eq!(contract.retry_budget.otp_attempts_per_reference, 3);
        assert_eq!(contract.retry_budget.otp_sends, 2);
        assert_eq!(contract.retry_budget.workspace_password_attempts, 3);
        assert_eq!(
            contract.redirect_policy,
            "manual; allow base host, parent, and sibling subdomains only"
        );
        assert_eq!(
            contract.cookie_names,
            vec!["local-auth-workspace", "x-csrf-token"]
        );
        assert_eq!(
            contract.password_timing,
            "resolve after OTP and workspace redirect; masked interactive input"
        );
        assert!(contract.error_mapping.otp_rejection.reprompt_same_reference);
        assert_eq!(
            contract.error_mapping.otp_rejection.resend_after_attempts,
            3
        );
        assert_eq!(contract.error_mapping.otp_rejection.terminal_after_sends, 2);
        assert_eq!(
            contract.error_mapping.transient_transport,
            LegacyTransportErrorAction::Retry
        );
        assert_eq!(
            contract.error_mapping.terminal_http_status,
            LegacyTransportErrorAction::Terminal
        );
        assert_eq!(
            contract.error_mapping.workspace_password.fixed_source,
            LegacyTransportErrorAction::Terminal
        );
        assert_eq!(
            contract
                .error_mapping
                .workspace_password
                .interactive_attempts,
            3
        );
    }
}
