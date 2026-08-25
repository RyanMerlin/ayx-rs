//! Platform-neutral authentication contracts shared by the human CLI and
//! automation clients.
//!
//! The concrete One email-OTP transport deliberately does not live here.  It
//! is a compatibility adapter in `ayx-one-api`; this module owns the pieces
//! that must remain deterministic when the transport, UI, or operating system
//! changes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::sensitive::write_sensitive_file;

pub const AUTH_STATE_MACHINE_VERSION: u16 = 1;
pub const AGENT_AUTH_PROTOCOL_VERSION: u16 = 1;
pub const DEFAULT_OTP_ATTEMPTS_PER_REFERENCE: u32 = 3;
pub const DEFAULT_OTP_SENDS: u32 = 2;
pub const DEFAULT_WORKSPACE_PASSWORD_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialHealth {
    Fresh,
    Stale,
    UnknownExpiry,
}

pub fn credential_health(
    expires_at_unix_seconds: Option<i64>,
    now_unix_seconds: i64,
) -> CredentialHealth {
    match expires_at_unix_seconds {
        Some(expires_at) if expires_at <= now_unix_seconds => CredentialHealth::Stale,
        Some(_) => CredentialHealth::Fresh,
        None => CredentialHealth::UnknownExpiry,
    }
}

pub fn binding_matches(expected: &CredentialBinding, actual: &CredentialBinding) -> bool {
    expected.validate().is_ok()
        && actual.validate().is_ok()
        && expected.canonical() == actual.canonical()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthRecoveryAction {
    RetryTransport,
    PromptOtp,
    ResendOtp,
    PromptWorkspacePassword,
    Reauthenticate,
    UseSessionOnly,
    Abort,
}

pub fn recovery_action(
    kind: AuthFailureKind,
    state: &AuthState,
    budgets: AuthBudgets,
) -> AuthRecoveryAction {
    match kind {
        AuthFailureKind::TransientTransport
            if state.transport_attempt < budgets.transport_attempts =>
        {
            AuthRecoveryAction::RetryTransport
        }
        AuthFailureKind::InvalidOtp if state.otp_attempts < budgets.otp_attempts_per_reference => {
            AuthRecoveryAction::PromptOtp
        }
        AuthFailureKind::ExpiredOtp if state.otp_sends < budgets.otp_sends => {
            AuthRecoveryAction::ResendOtp
        }
        AuthFailureKind::InvalidWorkspacePassword
            if state.workspace_password_attempts < budgets.workspace_password_attempts =>
        {
            AuthRecoveryAction::PromptWorkspacePassword
        }
        AuthFailureKind::StaleCredential => AuthRecoveryAction::Reauthenticate,
        AuthFailureKind::SecretStoreUnavailable => AuthRecoveryAction::UseSessionOnly,
        _ => AuthRecoveryAction::Abort,
    }
}

/// The state visible to both the human wizard and an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthPhase {
    Idle,
    SendingOtp,
    AwaitingOtp,
    ValidatingOtp,
    ResolvingWorkspace,
    AwaitingWorkspacePassword,
    ValidatingWorkspacePassword,
    ExchangingToken,
    Persisting,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthFailureKind {
    InvalidOtp,
    ExpiredOtp,
    InvalidWorkspacePassword,
    TransientTransport,
    StaleCredential,
    SecretStoreUnavailable,
    Cancelled,
    Protocol,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthState {
    pub version: u16,
    pub phase: AuthPhase,
    pub otp_attempts: u32,
    pub otp_sends: u32,
    pub workspace_password_attempts: u32,
    pub transport_attempt: u32,
    #[serde(default)]
    pub failure: Option<AuthFailureKind>,
}

impl Default for AuthState {
    fn default() -> Self {
        Self {
            version: AUTH_STATE_MACHINE_VERSION,
            phase: AuthPhase::Idle,
            otp_attempts: 0,
            otp_sends: 0,
            workspace_password_attempts: 0,
            transport_attempt: 0,
            failure: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthBudgets {
    pub otp_attempts_per_reference: u32,
    pub otp_sends: u32,
    pub workspace_password_attempts: u32,
    pub transport_attempts: u32,
}

impl Default for AuthBudgets {
    fn default() -> Self {
        Self {
            otp_attempts_per_reference: DEFAULT_OTP_ATTEMPTS_PER_REFERENCE,
            otp_sends: DEFAULT_OTP_SENDS,
            workspace_password_attempts: DEFAULT_WORKSPACE_PASSWORD_ATTEMPTS,
            transport_attempts: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthEvent {
    Begin,
    OtpSent,
    OtpAccepted,
    OtpRejected { reference_expired: bool },
    WorkspaceResolved,
    WorkspacePasswordAccepted,
    WorkspacePasswordRejected,
    TokenExchanged,
    PersistStarted,
    PersistSucceeded,
    TransientFailure,
    TerminalFailure(AuthFailureKind),
    Cancel,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthTransitionError {
    #[error("authentication protocol state is already terminal ({0:?})")]
    Terminal(AuthPhase),
    #[error("event {event:?} is invalid while authentication is in phase {phase:?}")]
    InvalidEvent { phase: AuthPhase, event: AuthEvent },
    #[error("authentication state machine version {0} is unsupported")]
    UnsupportedVersion(u16),
}

/// Deterministic state machine for authentication orchestration.
///
/// It owns retry budgets and recovery decisions, but performs no I/O.  This is
/// intentional: the same transitions are used by the terminal wizard, the
/// JSON agent protocol, and deterministic tests.
#[derive(Debug, Clone)]
pub struct AuthStateMachine {
    state: AuthState,
    budgets: AuthBudgets,
}

impl Default for AuthStateMachine {
    fn default() -> Self {
        Self::new(AuthBudgets::default())
    }
}

impl AuthStateMachine {
    pub fn new(budgets: AuthBudgets) -> Self {
        Self {
            state: AuthState::default(),
            budgets,
        }
    }

    pub fn state(&self) -> &AuthState {
        &self.state
    }

    pub fn budgets(&self) -> AuthBudgets {
        self.budgets
    }

    pub fn apply(&mut self, event: AuthEvent) -> Result<AuthState, AuthTransitionError> {
        if matches!(
            self.state.phase,
            AuthPhase::Complete | AuthPhase::Failed | AuthPhase::Cancelled
        ) {
            return Err(AuthTransitionError::Terminal(self.state.phase));
        }

        let phase = self.state.phase;
        match (phase, event) {
            (AuthPhase::Idle, AuthEvent::Begin) => self.state.phase = AuthPhase::SendingOtp,
            (AuthPhase::SendingOtp, AuthEvent::OtpSent) => {
                self.state.otp_sends += 1;
                self.state.otp_attempts = 0;
                self.state.phase = AuthPhase::AwaitingOtp;
            }
            (AuthPhase::AwaitingOtp, AuthEvent::OtpAccepted) => {
                self.state.phase = AuthPhase::ResolvingWorkspace;
            }
            (AuthPhase::AwaitingOtp, AuthEvent::OtpRejected { reference_expired }) => {
                self.state.otp_attempts += 1;
                if reference_expired {
                    if self.state.otp_sends >= self.budgets.otp_sends {
                        return self.fail(AuthFailureKind::ExpiredOtp);
                    }
                    self.state.phase = AuthPhase::SendingOtp;
                } else if self.state.otp_attempts >= self.budgets.otp_attempts_per_reference {
                    if self.state.otp_sends >= self.budgets.otp_sends {
                        return self.fail(AuthFailureKind::InvalidOtp);
                    }
                    self.state.phase = AuthPhase::SendingOtp;
                }
            }
            (AuthPhase::ResolvingWorkspace, AuthEvent::WorkspaceResolved) => {
                self.state.phase = AuthPhase::AwaitingWorkspacePassword;
                self.state.workspace_password_attempts = 0;
            }
            (AuthPhase::AwaitingWorkspacePassword, AuthEvent::WorkspacePasswordAccepted) => {
                self.state.phase = AuthPhase::ExchangingToken;
            }
            (AuthPhase::AwaitingWorkspacePassword, AuthEvent::WorkspacePasswordRejected) => {
                self.state.workspace_password_attempts += 1;
                if self.state.workspace_password_attempts
                    >= self.budgets.workspace_password_attempts
                {
                    return self.fail(AuthFailureKind::InvalidWorkspacePassword);
                }
            }
            (AuthPhase::ExchangingToken, AuthEvent::TokenExchanged) => {
                self.state.phase = AuthPhase::Persisting;
            }
            (AuthPhase::Persisting, AuthEvent::PersistStarted) => {}
            (AuthPhase::Persisting, AuthEvent::PersistSucceeded) => {
                self.state.phase = AuthPhase::Complete;
            }
            (_, AuthEvent::TransientFailure) => {
                self.state.transport_attempt += 1;
                if self.state.transport_attempt >= self.budgets.transport_attempts {
                    return self.fail(AuthFailureKind::TransientTransport);
                }
            }
            (_, AuthEvent::TerminalFailure(kind)) => return self.fail(kind),
            (_, AuthEvent::Cancel) => {
                self.state.failure = Some(AuthFailureKind::Cancelled);
                self.state.phase = AuthPhase::Cancelled;
            }
            _ => {
                return Err(AuthTransitionError::InvalidEvent { phase, event });
            }
        }
        self.state.failure = None;
        Ok(self.state.clone())
    }

    fn fail(&mut self, kind: AuthFailureKind) -> Result<AuthState, AuthTransitionError> {
        self.state.failure = Some(kind);
        self.state.phase = AuthPhase::Failed;
        Ok(self.state.clone())
    }
}

/// Secret persistence is explicit for automation.  The interactive wizard
/// may resolve `Secure` to `PlaintextFallback` after explaining the limitation
/// and receiving consent; no non-interactive caller may do that implicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SecretPersistencePolicy {
    #[default]
    Secure,
    PlaintextFallback,
    SessionOnly,
}

impl SecretPersistencePolicy {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "secure" | "keyring" | "credential_manager" => Some(Self::Secure),
            "plaintext" | "inline" | "profile" => Some(Self::PlaintextFallback),
            "session" | "session_only" | "none" => Some(Self::SessionOnly),
            _ => None,
        }
    }

    pub fn is_automation_safe(self) -> bool {
        !matches!(self, Self::PlaintextFallback)
    }
}

fn policy_sidecar_path(profile_path: &Path) -> PathBuf {
    let mut name = profile_path.file_name().unwrap_or_default().to_os_string();
    name.push(".auth-policy");
    profile_path.with_file_name(name)
}

/// Load a previously-consented fallback choice. The sidecar contains policy
/// metadata only, never a credential.
pub fn load_persistence_policy(profile_path: &Path) -> Option<SecretPersistencePolicy> {
    let value = fs::read_to_string(policy_sidecar_path(profile_path)).ok()?;
    SecretPersistencePolicy::parse(value.trim())
}

pub fn save_persistence_policy(
    profile_path: &Path,
    policy: SecretPersistencePolicy,
) -> Result<(), crate::sensitive::SensitiveIoError> {
    let value = match policy {
        SecretPersistencePolicy::Secure => "secure",
        SecretPersistencePolicy::PlaintextFallback => "plaintext",
        SecretPersistencePolicy::SessionOnly => "session",
    };
    write_sensitive_file(&policy_sidecar_path(profile_path), value.as_bytes())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CredentialBindingError {
    #[error("unsupported credential binding version {0}")]
    UnsupportedVersion(u16),
    #[error("credential binding field '{0}' cannot be empty")]
    EmptyField(&'static str),
    #[error("credential binding field '{0}' is too long")]
    FieldTooLong(&'static str),
}

/// Non-secret identity context used to namespace a credential.
///
/// The keyring account is derived from all fields, so a token/password from a
/// different tenant, region, base URL, or workspace cannot be selected merely
/// because its profile name or email matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialBinding {
    pub version: u16,
    pub account: String,
    pub issuer: String,
    pub region: String,
    pub base_url: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub workspace_gid: Option<String>,
}

impl CredentialBinding {
    pub const VERSION: u16 = 1;

    pub fn new(
        account: impl Into<String>,
        issuer: impl Into<String>,
        region: impl Into<String>,
        base_url: impl Into<String>,
        workspace_id: Option<String>,
        workspace_gid: Option<String>,
    ) -> Result<Self, CredentialBindingError> {
        let binding = Self {
            version: Self::VERSION,
            account: account.into(),
            issuer: issuer.into(),
            region: region.into(),
            base_url: base_url.into(),
            workspace_id,
            workspace_gid,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), CredentialBindingError> {
        if self.version != Self::VERSION {
            return Err(CredentialBindingError::UnsupportedVersion(self.version));
        }
        for (name, value) in [
            ("account", &self.account),
            ("issuer", &self.issuer),
            ("region", &self.region),
            ("base_url", &self.base_url),
        ] {
            if value.trim().is_empty() {
                return Err(CredentialBindingError::EmptyField(name));
            }
            if value.len() > 512 {
                return Err(CredentialBindingError::FieldTooLong(name));
            }
        }
        for (name, value) in [
            ("workspace_id", self.workspace_id.as_ref()),
            ("workspace_gid", self.workspace_gid.as_ref()),
        ] {
            if let Some(value) = value {
                if value.trim().is_empty() {
                    return Err(CredentialBindingError::EmptyField(name));
                }
                if value.len() > 512 {
                    return Err(CredentialBindingError::FieldTooLong(name));
                }
            }
        }
        Ok(())
    }

    pub fn canonical(&self) -> String {
        fn component(value: Option<&str>) -> String {
            let value = value.map(str::trim).unwrap_or_default();
            format!("{}:{value}", value.len())
        }
        format!(
            "v{};account={};issuer={};region={};base_url={};workspace_id={};workspace_gid={}",
            self.version,
            component(Some(&self.account)),
            component(Some(&self.issuer)),
            component(Some(&self.region)),
            component(Some(&self.base_url)),
            component(self.workspace_id.as_deref()),
            component(self.workspace_gid.as_deref()),
        )
    }

    pub fn fingerprint(&self) -> String {
        let digest = Sha256::digest(self.canonical().as_bytes());
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn keyring_account(&self, field: &str) -> String {
        self.keyring_account_in_namespace(None, field)
    }

    pub fn keyring_account_in_namespace(&self, namespace: Option<&str>, field: &str) -> String {
        let field = field
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let fingerprint = namespace
            .map(|namespace| {
                let namespaced = format!("namespace={namespace};binding={}", self.canonical());
                let digest = Sha256::digest(namespaced.as_bytes());
                digest.iter().map(|byte| format!("{byte:02x}")).collect()
            })
            .unwrap_or_else(|| self.fingerprint());
        format!("v{}/{}-{field}", self.version, fingerprint)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<Vec<u8>>,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let url = self.url.split(['?', '#']).next().unwrap_or(&self.url);
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &url)
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .field("body_len", &self.body.as_ref().map(Vec::len))
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Vec<u8>,
}

impl std::fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .field("body_len", &self.body.len())
            .finish()
    }
}

pub trait HttpTransport {
    type Error;

    fn send(&mut self, request: HttpRequest) -> Result<HttpResponse, Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretStoreAvailability {
    Available,
    Unavailable,
}

pub trait SecureStorage {
    type Error;

    fn availability(&self) -> SecretStoreAvailability;
    fn get(&mut self, account: &str) -> Result<Option<String>, Self::Error>;
    fn set(&mut self, account: &str, secret: &str) -> Result<(), Self::Error>;
    fn delete(&mut self, account: &str) -> Result<(), Self::Error>;
}

pub trait BrowserLauncher {
    type Error;

    fn open(&mut self, url: &str) -> Result<(), Self::Error>;
}

pub trait DeviceInteraction {
    type Error;

    fn wait_for_approval(
        &mut self,
        verification_uri: &str,
        user_code: &str,
    ) -> Result<(), Self::Error>;
}

pub trait Clock {
    fn now_unix_seconds(&self) -> i64;
    fn sleep_millis(&mut self, millis: u64);
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_seconds(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default()
    }

    fn sleep_millis(&mut self, millis: u64) {
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemBrowser;

impl BrowserLauncher for SystemBrowser {
    type Error = std::io::Error;

    fn open(&mut self, url: &str) -> Result<(), Self::Error> {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/c", "start", url])
                .spawn()
                .map(|_| ())
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(url)
                .spawn()
                .map(|_| ())
        }
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(url)
                .spawn()
                .map(|_| ())
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            let _ = url;
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "no native browser adapter for this platform",
            ))
        }
    }
}

pub trait UserInteraction {
    type Error;

    fn prompt(&mut self, message: &str, secret: bool) -> Result<String, Self::Error>;
    fn confirm(&mut self, message: &str, default: bool) -> Result<bool, Self::Error>;
    fn notice(&mut self, message: &str);
}

/// Rollout values for the versioned authentication orchestration boundary.
/// v0.16.1 keeps the complete legacy adapter as the default and rollback
/// lane. Wizard remains an explicitly selected, pre-release implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthRollout {
    #[default]
    Legacy,
    Wizard,
    Canary,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid authentication rollout '{value}'; use legacy, wizard, or canary")]
pub struct AuthRolloutError {
    value: String,
}

/// The authoritative One-profile secret slots.  Callers that inspect or
/// persist credentials use these names rather than maintaining divergent
/// token/password/client-secret lists.  Workspace fields use the same suffix
/// with their workspace path prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneSecretSlot {
    AccessToken,
    RefreshToken,
    WorkspacePassword,
    ClientSecret,
    ServicePrincipalClientSecret,
}

impl OneSecretSlot {
    pub const ALL: [Self; 5] = [
        Self::AccessToken,
        Self::RefreshToken,
        Self::WorkspacePassword,
        Self::ClientSecret,
        Self::ServicePrincipalClientSecret,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::AccessToken => "access_token",
            Self::RefreshToken => "refresh_token",
            Self::WorkspacePassword => "workspace_password",
            Self::ClientSecret => "client_secret",
            Self::ServicePrincipalClientSecret => "sp_client_secret",
        }
    }
}

/// Report inline secret fields without reading or returning their values.
/// Existing profiles with inline credentials remain loadable; this function
/// gives doctor/migration callers a stable, secret-free inventory.
pub fn inline_secret_fields(config: &crate::profile::Config) -> Vec<String> {
    let mut fields = Vec::new();
    if let Some(one) = config.alteryx_one.as_ref() {
        for slot in OneSecretSlot::ALL {
            let (value, reference) = top_level_secret_slot(one, slot);
            if value.as_ref().is_some_and(|value| !value.trim().is_empty()) && reference.is_none() {
                fields.push(format!("alteryx_one.{}", slot.name()));
            }
        }
        for (workspace_id, credential) in &one.workspace_credentials {
            for slot in OneSecretSlot::ALL {
                let (value, reference) = workspace_secret_slot(credential, slot);
                if value.as_ref().is_some_and(|value| !value.trim().is_empty())
                    && reference.is_none()
                {
                    fields.push(format!(
                        "alteryx_one.workspace_credentials['{workspace_id}'].{}",
                        slot.name()
                    ));
                }
            }
        }
    }
    fields
}

fn top_level_secret_slot(
    one: &crate::profile::AlteryxOneProfile,
    slot: OneSecretSlot,
) -> (&Option<String>, &Option<String>) {
    match slot {
        OneSecretSlot::AccessToken => (&one.access_token, &one.access_token_ref),
        OneSecretSlot::RefreshToken => (&one.refresh_token, &one.refresh_token_ref),
        OneSecretSlot::WorkspacePassword => (&one.workspace_password, &one.workspace_password_ref),
        OneSecretSlot::ClientSecret => (&one.client_secret, &one.client_secret_ref),
        OneSecretSlot::ServicePrincipalClientSecret => {
            (&one.sp_client_secret, &one.sp_client_secret_ref)
        }
    }
}

fn workspace_secret_slot(
    credential: &crate::profile::WorkspaceCredential,
    slot: OneSecretSlot,
) -> (&Option<String>, &Option<String>) {
    match slot {
        OneSecretSlot::AccessToken => (&credential.access_token, &credential.access_token_ref),
        OneSecretSlot::RefreshToken => (&credential.refresh_token, &credential.refresh_token_ref),
        OneSecretSlot::WorkspacePassword => (
            &credential.workspace_password,
            &credential.workspace_password_ref,
        ),
        OneSecretSlot::ClientSecret => (&credential.client_secret, &credential.client_secret_ref),
        OneSecretSlot::ServicePrincipalClientSecret => (
            &credential.sp_client_secret,
            &credential.sp_client_secret_ref,
        ),
    }
}

impl AuthRollout {
    pub fn parse(value: &str) -> Result<Self, AuthRolloutError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "legacy" | "otp" => Ok(Self::Legacy),
            "canary" | "internal" => Ok(Self::Canary),
            "wizard" | "default" => Ok(Self::Wizard),
            _ => Err(AuthRolloutError {
                value: value.trim().to_string(),
            }),
        }
    }

    /// Reads the rollout selector without silently changing lanes.  An invalid
    /// deployment setting is an operational error, not permission to enable a
    /// newer authentication implementation.
    pub fn from_environment() -> Result<Self, AuthRolloutError> {
        match std::env::var("AYX_AUTH_ROLLOUT").or_else(|_| std::env::var("AUTH_ROLLOUT")) {
            Ok(value) => Self::parse(&value),
            Err(_) => Ok(Self::default()),
        }
    }

    pub fn uses_new_orchestration(self) -> bool {
        matches!(self, Self::Canary | Self::Wizard)
    }
}

/// Versioned, secret-free JSON protocol for agents. Secret material is
/// supplied through an out-of-band channel described by `SecretInput`; it is
/// never embedded in the JSON request or echoed in a response.
pub mod agent_protocol {
    use super::{
        AGENT_AUTH_PROTOCOL_VERSION, AuthPhase, AuthState, CredentialBinding,
        SecretPersistencePolicy,
    };
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Operation {
        Start,
        SubmitOtp,
        SubmitWorkspacePassword,
        Cancel,
        Doctor,
        Migrate,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case", tag = "type")]
    pub enum SecretInput {
        Stdin,
        Environment { name: String },
        FileDescriptor { fd: u32 },
    }

    #[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Request {
        pub protocol_version: u16,
        pub request_id: String,
        pub operation: Operation,
        pub profile: String,
        pub binding: CredentialBinding,
        pub persistence: SecretPersistencePolicy,
        #[serde(default)]
        pub secret_input: Option<SecretInput>,
    }

    impl std::fmt::Debug for Request {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Request")
                .field("protocol_version", &self.protocol_version)
                .field("request_id", &self.request_id)
                .field("operation", &self.operation)
                .field("profile", &self.profile)
                .field("binding", &self.binding)
                .field("persistence", &self.persistence)
                .field("secret_input", &self.secret_input)
                .finish()
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Response {
        pub protocol_version: u16,
        pub request_id: String,
        pub state: AuthState,
        pub phase: AuthPhase,
        pub ok: bool,
        #[serde(default)]
        pub retryable: bool,
        #[serde(default)]
        pub error_code: Option<String>,
        #[serde(default)]
        pub message: Option<String>,
    }

    impl Request {
        pub fn validate(&self) -> Result<(), String> {
            if self.protocol_version != AGENT_AUTH_PROTOCOL_VERSION {
                return Err(format!(
                    "unsupported auth protocol version {} (expected {})",
                    self.protocol_version, AGENT_AUTH_PROTOCOL_VERSION
                ));
            }
            if self.request_id.trim().is_empty() {
                return Err("request_id cannot be empty".to_string());
            }
            if self.profile.trim().is_empty() {
                return Err("profile cannot be empty".to_string());
            }
            self.binding
                .validate()
                .map_err(|err| format!("invalid credential binding: {err}"))?;
            let needs_secret = matches!(
                self.operation,
                Operation::SubmitOtp | Operation::SubmitWorkspacePassword
            );
            if needs_secret && self.secret_input.is_none() {
                return Err("this operation requires an out-of-band secret_input".to_string());
            }
            if !needs_secret && self.secret_input.is_some() {
                return Err("secret_input is only valid for credential submission".to_string());
            }
            if let Some(SecretInput::Environment { name }) = &self.secret_input
                && name.trim().is_empty()
            {
                return Err("secret_input environment name cannot be empty".to_string());
            }
            if let Some(SecretInput::FileDescriptor { fd }) = self.secret_input
                && fd == 0
            {
                return Err("secret_input file descriptor must be greater than zero".to_string());
            }
            Ok(())
        }
    }

    pub fn encode(request: &Request) -> Result<String, serde_json::Error> {
        serde_json::to_string(request)
    }

    pub fn decode(input: &str) -> Result<Request, serde_json::Error> {
        serde_json::from_str(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine_reprompts_otp_before_resending() {
        let mut machine = AuthStateMachine::default();
        machine.apply(AuthEvent::Begin).unwrap();
        machine.apply(AuthEvent::OtpSent).unwrap();
        machine
            .apply(AuthEvent::OtpRejected {
                reference_expired: false,
            })
            .unwrap();
        assert_eq!(machine.state().phase, AuthPhase::AwaitingOtp);
        assert_eq!(machine.state().otp_sends, 1);
        assert_eq!(machine.state().otp_attempts, 1);
    }

    #[test]
    fn state_machine_resends_expired_reference_with_budget() {
        let mut machine = AuthStateMachine::default();
        machine.apply(AuthEvent::Begin).unwrap();
        machine.apply(AuthEvent::OtpSent).unwrap();
        machine
            .apply(AuthEvent::OtpRejected {
                reference_expired: true,
            })
            .unwrap();
        assert_eq!(machine.state().phase, AuthPhase::SendingOtp);
        machine.apply(AuthEvent::OtpSent).unwrap();
        assert_eq!(machine.state().otp_sends, 2);
    }

    #[test]
    fn state_machine_fails_after_password_budget_without_restarting_otp() {
        let mut machine = AuthStateMachine::default();
        for event in [
            AuthEvent::Begin,
            AuthEvent::OtpSent,
            AuthEvent::OtpAccepted,
            AuthEvent::WorkspaceResolved,
        ] {
            machine.apply(event).unwrap();
        }
        for _ in 0..DEFAULT_WORKSPACE_PASSWORD_ATTEMPTS {
            machine.apply(AuthEvent::WorkspacePasswordRejected).unwrap();
        }
        assert_eq!(machine.state().phase, AuthPhase::Failed);
        assert_eq!(
            machine.state().failure,
            Some(AuthFailureKind::InvalidWorkspacePassword)
        );
        assert_eq!(machine.state().otp_sends, 1);
    }

    #[test]
    fn credential_binding_separates_workspaces_and_regions() {
        let a = CredentialBinding::new(
            "person@example.com",
            "https://issuer.example/as",
            "us1",
            "https://us1.example",
            Some("42".to_string()),
            Some("gid-a".to_string()),
        )
        .unwrap();
        let b = CredentialBinding::new(
            "person@example.com",
            "https://issuer.example/as",
            "us1",
            "https://us1.example",
            Some("43".to_string()),
            Some("gid-b".to_string()),
        )
        .unwrap();
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_ne!(
            a.keyring_account("access_token"),
            b.keyring_account("access_token")
        );
        assert_ne!(
            a.keyring_account_in_namespace(Some("canary"), "access_token"),
            a.keyring_account("access_token")
        );
    }

    #[test]
    fn credential_binding_canonicalization_is_length_delimited_and_case_safe() {
        let left =
            CredentialBinding::new("a|issuer=b", "issuer", "us1", "https://example", None, None)
                .unwrap();
        let right =
            CredentialBinding::new("a", "issuer=b|issuer", "us1", "https://example", None, None)
                .unwrap();
        assert_ne!(left.canonical(), right.canonical());

        let upper = CredentialBinding::new(
            "person@example.com",
            "https://Issuer.example/AS",
            "us1",
            "https://example",
            None,
            None,
        )
        .unwrap();
        let lower = CredentialBinding::new(
            "person@example.com",
            "https://issuer.example/as",
            "us1",
            "https://example",
            None,
            None,
        )
        .unwrap();
        assert_ne!(upper.fingerprint(), lower.fingerprint());
    }

    #[test]
    fn debug_output_redacts_protocol_and_http_secret_values() {
        let binding = CredentialBinding::new(
            "person@example.com",
            "https://issuer.example",
            "us1",
            "https://us1.example",
            None,
            None,
        )
        .unwrap();
        let request = agent_protocol::Request {
            protocol_version: AGENT_AUTH_PROTOCOL_VERSION,
            request_id: "req-secret".to_string(),
            operation: agent_protocol::Operation::SubmitOtp,
            profile: "default".to_string(),
            binding,
            persistence: SecretPersistencePolicy::SessionOnly,
            secret_input: Some(agent_protocol::SecretInput::Stdin),
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("123456"));
        assert!(debug.contains("secret_input"));
        assert!(!debug.contains("value"));

        let http = HttpRequest {
            method: "POST".to_string(),
            url: "https://example.test/session?password=secret".to_string(),
            headers: BTreeMap::from([("authorization".to_string(), "Bearer secret".to_string())]),
            body: Some(b"password=secret".to_vec()),
        };
        let debug = format!("{http:?}");
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("password="));
    }

    #[test]
    fn agent_protocol_is_versioned_and_does_not_serialize_secret_values_by_name() {
        let binding = CredentialBinding::new(
            "person@example.com",
            "https://issuer.example",
            "us1",
            "https://us1.example",
            None,
            None,
        )
        .unwrap();
        let request = agent_protocol::Request {
            protocol_version: AGENT_AUTH_PROTOCOL_VERSION,
            request_id: "req-1".to_string(),
            operation: agent_protocol::Operation::Start,
            profile: "default".to_string(),
            binding,
            persistence: SecretPersistencePolicy::SessionOnly,
            secret_input: None,
        };
        let json = agent_protocol::encode(&request).unwrap();
        assert!(json.contains("protocol_version"));
        assert!(!json.contains("access_token"));
        assert!(!json.contains("refresh_token"));
        assert!(agent_protocol::decode(&json).unwrap().validate().is_ok());
    }

    #[test]
    fn agent_protocol_rejects_inline_secret_payloads_and_implicit_persistence() {
        let binding = CredentialBinding::new(
            "person@example.com",
            "https://issuer.example",
            "us1",
            "https://us1.example",
            None,
            None,
        )
        .unwrap();
        let request = serde_json::json!({
            "protocol_version": AGENT_AUTH_PROTOCOL_VERSION,
            "request_id": "req-legacy-secret",
            "operation": "submit_otp",
            "profile": "default",
            "binding": binding,
            "persistence": "session_only",
            "value": "123456"
        });
        assert!(agent_protocol::decode(&request.to_string()).is_err());

        let missing_persistence = serde_json::json!({
            "protocol_version": AGENT_AUTH_PROTOCOL_VERSION,
            "request_id": "req-missing-policy",
            "operation": "start",
            "profile": "default",
            "binding": request["binding"].clone()
        });
        assert!(agent_protocol::decode(&missing_persistence.to_string()).is_err());
    }

    #[test]
    fn rollout_defaults_to_legacy() {
        assert_eq!(AuthRollout::default(), AuthRollout::Legacy);
    }

    #[test]
    fn legacy_rollout_remains_an_explicit_rollback() {
        assert_eq!(AuthRollout::parse("legacy"), Ok(AuthRollout::Legacy));
        assert_eq!(AuthRollout::parse("otp"), Ok(AuthRollout::Legacy));
        assert_eq!(AuthRollout::parse("wizard"), Ok(AuthRollout::Wizard));
        assert_eq!(AuthRollout::parse("canary"), Ok(AuthRollout::Canary));
        assert!(AuthRollout::parse("oops").is_err());
    }

    #[test]
    fn persistence_policy_sidecar_round_trips_without_secret_material() {
        let dir = tempfile::tempdir().expect("tempdir");
        let profile = dir.path().join("default.yaml");
        save_persistence_policy(&profile, SecretPersistencePolicy::PlaintextFallback)
            .expect("save policy");
        assert_eq!(
            load_persistence_policy(&profile),
            Some(SecretPersistencePolicy::PlaintextFallback)
        );
        let sidecar = std::fs::read_to_string(dir.path().join("default.yaml.auth-policy"))
            .expect("read policy");
        assert_eq!(sidecar, "plaintext");
        assert!(!sidecar.contains("secret"));
    }

    #[test]
    fn inline_inventory_is_secret_free_and_workspace_specific() {
        let mut one = crate::profile::AlteryxOneProfile {
            account_email: "person@example.com".to_string(),
            access_token: Some("secret-access".to_string()),
            ..Default::default()
        };
        one.workspace_credentials.insert(
            "42".to_string(),
            crate::profile::WorkspaceCredential {
                workspace_password: Some("secret-password".to_string()),
                ..Default::default()
            },
        );
        let config = crate::profile::Config {
            profile_name: "default".to_string(),
            mongo: Default::default(),
            alteryx_one: Some(one),
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        };
        let fields = inline_secret_fields(&config);
        assert!(
            fields
                .iter()
                .any(|field| field == "alteryx_one.access_token")
        );
        assert!(fields.iter().any(|field| {
            field == "alteryx_one.workspace_credentials['42'].workspace_password"
        }));
        assert!(!fields.iter().any(|field| field.contains("secret-")));
    }

    #[test]
    fn stale_and_unknown_expiry_are_distinguished() {
        assert_eq!(credential_health(Some(99), 100), CredentialHealth::Stale);
        assert_eq!(credential_health(Some(101), 100), CredentialHealth::Fresh);
        assert_eq!(
            credential_health(None, 100),
            CredentialHealth::UnknownExpiry
        );
    }

    #[test]
    fn recovery_actions_are_budgeted_and_do_not_restart_password_as_otp() {
        let budgets = AuthBudgets::default();
        let mut state = AuthState {
            otp_attempts: 1,
            ..Default::default()
        };
        assert_eq!(
            recovery_action(AuthFailureKind::InvalidOtp, &state, budgets),
            AuthRecoveryAction::PromptOtp
        );
        state.workspace_password_attempts = budgets.workspace_password_attempts;
        assert_eq!(
            recovery_action(AuthFailureKind::InvalidWorkspacePassword, &state, budgets),
            AuthRecoveryAction::Abort
        );
    }
}
