use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::auth::AuthRollout;
use crate::secrets::{
    env_secret_ref, recover_keyring_transaction, resolve_secret_ref, resolve_secret_ref_with,
};
use crate::sensitive::{recover_sensitive_file, write_sensitive_file};

// ---------------------------------------------------------------------------
// Task 4: mixed-state secret conflict detection
// ---------------------------------------------------------------------------

/// A diagnostic record for a single representation's resolved secret.
struct SecretCandidate {
    /// Human-readable label, e.g. `"server_api.client_secret_ref"`.
    label: String,
    /// The ref string that was resolved (e.g. `"inline:..."`, `"env:FOO"`).
    ref_form: String,
}

/// Error returned when two non-derived secret representations both resolve to
/// *different* concrete values, indicating a mixed-state configuration.
///
/// The message names the conflicting fields and their ref forms so the operator
/// can identify which value to remove.  It never includes the resolved secret
/// values themselves.
#[derive(Debug, Error)]
#[error(
    "mixed secret state: {source_a} and {source_b} resolve to different values; \
     remove one or run `ayx config edit` to consolidate. \
     ({ref_a} vs {ref_b})"
)]
pub struct MixedSecretState {
    /// Label of the first conflicting source field.
    pub source_a: String,
    /// Label of the second conflicting source field.
    pub source_b: String,
    /// Ref form of the first source (never the resolved value).
    pub ref_a: String,
    /// Ref form of the second source (never the resolved value).
    pub ref_b: String,
}

/// Check whether multiple secret representations in `config` disagree.
///
/// For each populated representation (`server_api`, non-derived `api`,
/// non-derived `server`) we attempt to resolve the secret to a concrete string.
/// When two representations both resolve **and** their values differ, we return
/// `Err(MixedSecretState)`.  If either side is unresolvable (e.g. an unset
/// `env:` var, a missing keyring entry) we cannot prove a conflict and degrade
/// gracefully to `Ok(())`.
///
/// The error message names only field labels and ref forms — never the resolved
/// secret values — to avoid leaking credentials into logs or terminal output.
pub fn detect_secret_conflict(config: &Config) -> Result<(), MixedSecretState> {
    // Collect (label, ref_form, resolved_value) for each non-derived
    // representation that has a secret to speak of.
    let mut candidates: Vec<(SecretCandidate, String)> = Vec::new();

    // 1. server_api.client_secret / client_secret_ref
    if let Some(sa) = config.server_api.as_ref()
        && let Some(val) =
            resolved_inline_or_ref(&sa.client_secret, sa.client_secret_ref.as_deref())
    {
        let ref_form = ref_form_for(
            &sa.client_secret,
            sa.client_secret_ref.as_deref(),
            "inline:***",
        );
        candidates.push((
            SecretCandidate {
                label: "server_api.client_secret".to_string(),
                ref_form,
            },
            val,
        ));
    }

    // 2. api.auth.client_secret / client_secret_ref — only when user-authored
    if let Some(api) = config.api.as_ref()
        && !api.is_derived()
        && let Some(val) = resolved_inline_or_ref(
            api.auth.client_secret.as_deref().unwrap_or(""),
            api.auth.client_secret_ref.as_deref(),
        )
    {
        let ref_form = ref_form_for(
            api.auth.client_secret.as_deref().unwrap_or(""),
            api.auth.client_secret_ref.as_deref(),
            "inline:***",
        );
        candidates.push((
            SecretCandidate {
                label: "api.auth.client_secret".to_string(),
                ref_form,
            },
            val,
        ));
    }

    // 3. server.curator_api_secret / curator_api_secret_ref — only when user-authored
    if let Some(srv) = config.server.as_ref()
        && !srv.is_derived()
        && let Some(val) = resolved_inline_or_ref(
            &srv.curator_api_secret,
            srv.curator_api_secret_ref.as_deref(),
        )
    {
        let ref_form = ref_form_for(
            &srv.curator_api_secret,
            srv.curator_api_secret_ref.as_deref(),
            "inline:***",
        );
        candidates.push((
            SecretCandidate {
                label: "server.curator_api_secret".to_string(),
                ref_form,
            },
            val,
        ));
    }

    // Compare every pair that both resolved to a concrete value.
    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            let (cand_a, val_a) = &candidates[i];
            let (cand_b, val_b) = &candidates[j];
            if val_a != val_b {
                return Err(MixedSecretState {
                    source_a: cand_a.label.clone(),
                    source_b: cand_b.label.clone(),
                    ref_a: cand_a.ref_form.clone(),
                    ref_b: cand_b.ref_form.clone(),
                });
            }
        }
    }

    Ok(())
}

/// Resolve a secret from either an inline plaintext value or a ref string.
///
/// Returns `None` if:
/// - Both `value` is empty and `ref_` is `None` (no secret configured).
/// - The ref is unresolvable (env var not set, keyring miss) — we cannot prove
///   a conflict in this case, so we degrade gracefully.
fn resolved_inline_or_ref(value: &str, ref_: Option<&str>) -> Option<String> {
    if !value.is_empty() {
        return Some(value.to_string());
    }
    if let Some(r) = ref_ {
        // Silently ignore resolution errors — an unresolvable ref cannot prove a
        // conflict and must never make the config un-loadable.
        return resolve_secret_ref(r).ok().flatten();
    }
    None
}

/// Return the ref-form string for display in the error message.
/// Never includes the resolved plaintext value — only the ref or a
/// redacted inline placeholder.
///
/// `env:` and `keyring:` refs name a *location* (not a value) and are safe to
/// print verbatim.  An `inline:` ref embeds the secret as its suffix; it is
/// treated exactly like a bare plaintext value and replaced with
/// `inline_placeholder`.
fn ref_form_for(value: &str, ref_: Option<&str>, inline_placeholder: &str) -> String {
    if let Some(r) = ref_ {
        // Allowlist: only `env:` and `keyring:` refs name a *location* (not a
        // value) and are safe to print verbatim.  Anything else — `inline:` (embeds
        // the secret as a suffix), a bare value, or any unknown future scheme —
        // is redacted to `inline_placeholder` to prevent accidental secret leaks.
        if r.starts_with("env:") || r.starts_with("keyring:") {
            return r.to_string();
        }
        return inline_placeholder.to_string();
    }
    if !value.is_empty() {
        return inline_placeholder.to_string();
    }
    String::new()
}

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("failed to read config file '{path}': {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config yaml '{path}': {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("invalid config: {0}")]
    Invalid(String),
    /// The OS keyring could not be written to.
    ///
    /// Distinct from [`ProfileError::Invalid`] purely so callers can classify
    /// by cause. Reading, writing and deleting a keyring entry all produce
    /// prose containing "keyring entry" or "keyring secret", so a substring
    /// test could not tell a *store* failure — where offering a plaintext
    /// fallback is meaningful — from a *read* denial or a failed rollback,
    /// where it is not. That ambiguity offered to rewrite every credential as
    /// cleartext on hosts whose keyring was present and working but merely
    /// locked, or when an unrelated write failed and its keyring rollback
    /// failed too.
    ///
    /// Renders identically to `Invalid`, so operator-facing text is unchanged.
    #[error("invalid config: {0}")]
    KeyringUnavailable(String),
    /// An existing OS keyring entry could not be read.
    ///
    /// The counterpart to [`ProfileError::KeyringUnavailable`], and separate
    /// from it on purpose: a *read* failure must degrade the profile load so
    /// the diagnostics can report the unresolved slot, and must never authorise
    /// the plaintext fallback, which only a *store* failure does.
    ///
    /// Renders identically to `Invalid`, so operator-facing text is unchanged.
    #[error("invalid config: {0}")]
    KeyringUnreadable(String),
    #[error("failed to write config file '{path}': {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

pub const DEFAULT_PROFILE_FILE: &str = "config.yaml";
pub const DEFAULT_ENVIRONMENTS_FILE: &str = "environments.yaml";
const LEGACY_WORKSPACE_FILE: &str = "workspace.yaml";
const LEGACY_DEFAULT_PROFILE_FILE: &str = "default.yaml";
const DEFAULT_ACTIVE_PROFILE_NAME: &str = "default";
const DEFAULT_ACTIVE_WORKSPACE_NAME: &str = "default";

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AyxState {
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub active_workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedProfilePath {
    pub requested_path: String,
    pub resolved_path: String,
    pub source: String,
    pub active_profile: Option<String>,
    pub active_workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProfileResolution {
    pub config_home: String,
    pub selected_profile: String,
    pub selection_source: String,
    pub resolved_profile_path: String,
    pub active_profile: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Config {
    pub profile_name: String,
    #[serde(default = "default_mongo_profile")]
    pub mongo: MongoProfile,
    pub alteryx_one: Option<AlteryxOneProfile>,
    #[serde(default)]
    pub observability: Option<ObservabilityProfile>,
    #[serde(default)]
    pub server_api: Option<ServerApiProfile>,
    #[serde(default)]
    pub api: Option<ApiProfile>,
    #[serde(default)]
    pub server: Option<ServerProfile>,
    #[serde(default)]
    pub sqlserver: Option<SqlServerProfile>,
    #[serde(default)]
    pub upgrade: Option<UpgradeProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceConfig {
    pub workspace_name: String,
    pub active_environment: String,
    pub environments: HashMap<String, Config>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerDeploymentProfile {
    pub api: ServerApiProfile,
    pub storage: ServerStorageProfile,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerStorageProfile {
    pub kind: ServerStorageKind,
    #[serde(default)]
    pub mongo: Option<MongoProfile>,
    #[serde(default)]
    pub sqlserver: Option<SqlServerProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServerStorageKind {
    EmbeddedMongo,
    ManagedMongo,
    SqlServer,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MongoProfile {
    #[serde(default)]
    pub mode: MongoMode,
    pub databases: MongoDatabases,
    pub embedded: Option<MongoEmbedded>,
    pub managed: Option<MongoManaged>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MongoDatabases {
    pub gallery_name: String,
    pub service_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MongoMode {
    #[default]
    Embedded,
    Managed,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MongoEmbedded {
    #[serde(default = "default_runtime_settings_path")]
    pub runtime_settings_path: Option<String>,
    pub alteryx_service_path: Option<String>,
    pub restore_target_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MongoManaged {
    pub url: Option<String>,
    pub host: Option<String>,
    #[serde(default = "default_mongo_port")]
    pub port: u16,
    pub auth_database: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub password_ref: Option<String>,
    pub tls: TlsConfig,
    pub timeout_ms: Option<u64>,
    pub retry_count: Option<u32>,
    pub max_pool_size: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TlsConfig {
    #[serde(default)]
    pub enabled: bool,
    pub ca_path: Option<String>,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub allow_invalid_hostnames: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiProfile {
    pub base_url: String,
    pub auth: ApiAuth,
    pub timeout_ms: Option<u64>,
    /// True when this profile was synthesized from `server_api` by
    /// `with_server_api_overrides`, not written directly by the user.
    /// Skipped on serialization so it never persists to disk.
    #[serde(skip, default)]
    pub derived: bool,
}

impl ApiProfile {
    pub fn is_derived(&self) -> bool {
        self.derived
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiAuth {
    pub mode: ApiAuthMode,
    pub pat: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_secret_ref: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiAuthMode {
    Pat,
    Oauth2ClientCredentials,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpgradeProfile {
    pub target_version: Option<String>,
    pub deployment: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservabilityProfile {
    pub api_logging: Option<ApiLoggingProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiLoggingProfile {
    pub enabled: bool,
    pub path: Option<String>,
    pub redact_bodies: Option<bool>,
    pub log_requests: Option<bool>,
    pub log_responses: Option<bool>,
}

/// Whether to acquire tokens via the interactive user/refresh flow or the
/// non-interactive service-principal `client_credentials` flow.  The user
/// flow is the verified default and matches the official `ayx-cli` behaviour;
/// `service-principal` is experimental until the regional-JWKS trust boundary
/// is resolved (see docs/auth-model.md).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    #[default]
    User,
    ServicePrincipal,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct WorkspaceCredential {
    /// Numeric workspace id. The map key remains a backward-compatible lookup
    /// key; this field keeps the identity distinct when credentials are
    /// imported from another source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Stable workspace GID returned by the One API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default)]
    pub access_token_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub refresh_token_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_password: Option<String>,
    #[serde(default)]
    pub workspace_password_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sp_client_secret: Option<String>,
    #[serde(default)]
    pub sp_client_secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_url: Option<String>,
    /// Service-principal client ID — distinct from the user `oauth_client_id`.
    /// When set, this credential uses `client_credentials` grant with
    /// `client_secret_post` against `token_endpoint_url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sp_client_id: Option<String>,
    /// ULID of the workspace — used as the `scope=w:<gid>` value in SP token
    /// requests.  For user flow this is informational only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_gid: Option<String>,
    /// Exact display name captured from the account/workspace directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
    /// Last known credential health; informational and never a substitute for
    /// a live token probe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_health: Option<String>,
    /// Override the API base URL for this credential (e.g. a regional cell
    /// host for SP tokens).  Falls back to the profile `base_url` default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base_url: Option<String>,
}

/// Canonical identity used when a command binds a credential to a workspace.
/// The numeric ID is the stable key; GID and name are corroborating metadata
/// used for display and fail-closed selector checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceTarget {
    pub workspace_id: String,
    pub workspace_gid: String,
    pub display_name: String,
    pub credential_key: String,
    pub resolution_source: WorkspaceResolutionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceResolutionSource {
    Cli,
    Environment,
    ActiveProfile,
    SavedCredential,
    Directory,
}

impl WorkspaceTarget {
    pub fn from_credential(
        credential_key: impl Into<String>,
        credential: &WorkspaceCredential,
        resolution_source: WorkspaceResolutionSource,
    ) -> Option<Self> {
        let credential_key = credential_key.into();
        let workspace_id = credential.workspace_id.clone()?.trim().to_string();
        if workspace_id.is_empty()
            || !workspace_id
                .chars()
                .all(|character| character.is_ascii_digit())
            || credential_key != workspace_id
        {
            return None;
        }
        let workspace_gid = credential.workspace_gid.clone()?.trim().to_string();
        let display_name = credential.workspace_name.clone()?.trim().to_string();
        if workspace_gid.is_empty() || display_name.is_empty() {
            return None;
        }
        Some(Self {
            workspace_id,
            workspace_gid,
            display_name,
            credential_key,
            resolution_source,
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct AlteryxOneProfile {
    /// Version of the serialized workspace/profile shape. Older profiles omit
    /// this field and are interpreted as the current legacy-compatible shape.
    #[serde(default = "default_profile_schema_version")]
    pub schema_version: u32,
    pub account_email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sp_client_secret: Option<String>,
    #[serde(default)]
    pub sp_client_secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default)]
    pub access_token_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub refresh_token_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_password: Option<String>,
    #[serde(default)]
    pub workspace_password_ref: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workspace_credentials: BTreeMap<String, WorkspaceCredential>,
    /// Active workspace selector, separate from `expected_workspace_id`, which
    /// remains an optional mutation safety guard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_workspace_id: Option<String>,
    /// Rollout that owns the bound credentials in this profile. Persisting
    /// the selected lane keeps an explicit one-shot CLI override aligned with
    /// later API credential consumption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_rollout: Option<AuthRollout>,
    /// Expected workspace id for mutation safety preflight.
    ///
    /// When set, every mutating One API request (after `--apply`) makes a
    /// `GET /v4/workspaces/current` call and fails closed if the returned
    /// workspace id does not match this value. Set per-environment to
    /// prevent accidentally mutating the wrong workspace when tokens are
    /// shared or stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_workspace_id: Option<String>,
    /// Account-level service-principal client ID.  Resolved workspace-first
    /// via `resolved_sp_client_id()`.  Set `auth_mode: service-principal` to
    /// activate the SP flow (see docs/auth-model.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sp_client_id: Option<String>,
    /// SP token endpoint URL at the account level (e.g. the regional Ping
    /// issuer `https://pingauth-us1-4.alteryxcloud.com/as/token`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sp_token_endpoint_url: Option<String>,
    /// Workspace ULID used as `scope=w:<gid>` in SP token requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_gid: Option<String>,
    /// Token acquisition strategy.  Defaults to `user` (refresh_token flow).
    /// Set to `service-principal` to use the `client_credentials` SP flow.
    #[serde(default, skip_serializing_if = "is_default_auth_mode")]
    pub auth_mode: AuthMode,
}

pub const CURRENT_PROFILE_SCHEMA_VERSION: u32 = 1;

fn default_profile_schema_version() -> u32 {
    CURRENT_PROFILE_SCHEMA_VERSION
}

impl Default for AlteryxOneProfile {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: String::new(),
            base_url: None,
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials: Default::default(),
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::User,
        }
    }
}

/// Profile debugging must be safe to include in an error report.  In
/// particular, `inline:` references contain the secret value, so neither the
/// values nor their references may be delegated to derived `Debug` output.
impl std::fmt::Debug for WorkspaceCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceCredential")
            .field("has_access_token", &self.access_token.is_some())
            .field("has_refresh_token", &self.refresh_token.is_some())
            .field("has_workspace_password", &self.workspace_password.is_some())
            .field("oauth_client_id", &self.oauth_client_id)
            .field("has_client_secret", &self.client_secret.is_some())
            .field("has_sp_client_secret", &self.sp_client_secret.is_some())
            .field("token_endpoint_url", &self.token_endpoint_url)
            .field("sp_client_id", &self.sp_client_id)
            .field("workspace_gid", &self.workspace_gid)
            .field("api_base_url", &self.api_base_url)
            .finish()
    }
}

impl std::fmt::Debug for AlteryxOneProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlteryxOneProfile")
            .field("account_email", &self.account_email)
            .field("base_url", &self.base_url)
            .field("oauth_client_id", &self.oauth_client_id)
            .field("has_client_secret", &self.client_secret.is_some())
            .field("has_sp_client_secret", &self.sp_client_secret.is_some())
            .field("token_endpoint_url", &self.token_endpoint_url)
            .field("has_access_token", &self.access_token.is_some())
            .field("has_refresh_token", &self.refresh_token.is_some())
            .field("has_workspace_password", &self.workspace_password.is_some())
            .field("workspace_credentials", &self.workspace_credentials)
            .field("auth_rollout", &self.auth_rollout)
            .field("expected_workspace_id", &self.expected_workspace_id)
            .field("sp_client_id", &self.sp_client_id)
            .field("sp_token_endpoint_url", &self.sp_token_endpoint_url)
            .field("workspace_gid", &self.workspace_gid)
            .field("auth_mode", &self.auth_mode)
            .finish()
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("profile_name", &self.profile_name)
            .field("mongo_mode", &self.mongo.mode)
            .field("alteryx_one", &self.alteryx_one)
            .field("has_observability", &self.observability.is_some())
            .field("has_server_api", &self.server_api.is_some())
            .field("has_api", &self.api.is_some())
            .field("has_server", &self.server.is_some())
            .field("has_sqlserver", &self.sqlserver.is_some())
            .field("has_upgrade", &self.upgrade.is_some())
            .finish()
    }
}

impl AlteryxOneProfile {
    pub fn normalized_base_url(&self) -> Option<String> {
        self.base_url
            .as_deref()
            .and_then(normalize_alteryx_one_base_url)
    }

    pub fn workspace_credential_for(
        &self,
        workspace_id: Option<&str>,
    ) -> Option<&WorkspaceCredential> {
        let workspace_id = workspace_id?;
        self.workspace_credentials.get(workspace_id)
    }

    pub fn active_workspace_id(&self) -> Option<&str> {
        if let Some(active) = self.active_workspace_id.as_deref()
            && self.workspace_credentials.contains_key(active)
        {
            return Some(active);
        }
        if let Some(expected_workspace_id) = self.expected_workspace_id.as_deref()
            && self
                .workspace_credentials
                .contains_key(expected_workspace_id)
        {
            return Some(expected_workspace_id);
        }
        if self.workspace_credentials.len() == 1 {
            return self.workspace_credentials.keys().next().map(String::as_str);
        }
        None
    }

    /// Resolve a universal workspace selector without conflating numeric IDs,
    /// GIDs, and display names. Exact name matching is intentionally strict.
    pub fn resolve_workspace_selector(&self, selector: &str) -> Result<String, String> {
        Ok(self
            .resolve_workspace_target(selector, WorkspaceResolutionSource::Cli)?
            .credential_key)
    }

    /// Resolve a selector and preserve how it was chosen for diagnostics and
    /// audit output. Matching is exact and never performs fuzzy name lookup.
    pub fn resolve_workspace_target(
        &self,
        selector: &str,
        source: WorkspaceResolutionSource,
    ) -> Result<WorkspaceTarget, String> {
        let selector = selector.trim();
        if selector.is_empty() {
            return Err("workspace selector cannot be empty".to_string());
        }
        let mut matches = self
            .workspace_credentials
            .iter()
            .filter_map(|(key, c)| {
                let id_match = key == selector || c.workspace_id.as_deref() == Some(selector);
                let gid_match = c.workspace_gid.as_deref() == Some(selector);
                let name_match = c.workspace_name.as_deref() == Some(selector);
                (id_match || gid_match || name_match).then_some(key.clone())
            })
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        match matches.as_slice() {
            [only] => WorkspaceTarget::from_credential(
                only,
                self.workspace_credentials
                    .get(only)
                    .expect("selector match must have a credential"),
                source,
            )
            .ok_or_else(|| {
                format!("workspace '{selector}' has incomplete or non-canonical identity metadata")
            }),
            [] => Err(format!(
                "workspace '{selector}' is not a saved credential workspace"
            )),
            _ => Err(format!(
                "workspace selector '{selector}' is ambiguous; use its numeric ID or GID"
            )),
        }
    }

    /// Validate all saved identities before a workspace-scoped operation.
    /// Legacy entries remain readable, but duplicate or malformed identity
    /// metadata must never be selected implicitly.
    pub fn validate_workspace_identities(&self) -> Result<(), String> {
        let mut ids = std::collections::BTreeMap::<String, String>::new();
        let mut gids = std::collections::BTreeMap::<String, String>::new();
        let mut names = std::collections::BTreeMap::<String, String>::new();
        for (key, credential) in &self.workspace_credentials {
            let Some(id) = credential.workspace_id.as_deref() else {
                continue;
            };
            if id.is_empty() || !id.chars().all(|character| character.is_ascii_digit()) {
                return Err(format!(
                    "workspace credential '{key}' has a non-numeric workspace ID"
                ));
            }
            if let Some(previous) = ids.insert(id.to_string(), key.clone())
                && previous != *key
            {
                return Err(format!(
                    "duplicate workspace ID '{id}' in credentials '{previous}' and '{key}'"
                ));
            }
            if let Some(gid) = credential.workspace_gid.as_deref()
                && let Some(previous) = gids.insert(gid.to_string(), key.clone())
                && previous != *key
            {
                return Err(format!(
                    "duplicate workspace GID '{gid}' in credentials '{previous}' and '{key}'"
                ));
            }
            if let Some(name) = credential.workspace_name.as_deref()
                && let Some(previous) = names.insert(name.to_string(), key.clone())
                && previous != *key
            {
                return Err(format!(
                    "duplicate workspace name '{name}' in credentials '{previous}' and '{key}'"
                ));
            }
        }
        Ok(())
    }

    /// Normalize legacy credential map keys in memory. Fully identified
    /// records are re-keyed by numeric workspace ID; incomplete records remain
    /// readable but are marked stale and cannot be selected as active targets.
    /// The caller decides when to persist the returned mutation.
    pub fn migrate_workspace_credentials(&mut self) -> Result<usize, String> {
        if self.schema_version > CURRENT_PROFILE_SCHEMA_VERSION {
            return Err(format!(
                "profile schema version {} is newer than supported version {}",
                self.schema_version, CURRENT_PROFILE_SCHEMA_VERSION
            ));
        }
        self.validate_workspace_identities()?;
        // Build a replacement without mutating the live map until every
        // entry has been checked. This keeps failed migrations recoverable.
        let original = self.workspace_credentials.clone();
        let mut normalized = std::collections::BTreeMap::new();
        let mut changes = usize::from(self.schema_version != CURRENT_PROFILE_SCHEMA_VERSION);
        for (key, mut credential) in original {
            let canonical_id = credential.workspace_id.as_deref().and_then(|id| {
                (!id.is_empty() && id.chars().all(|character| character.is_ascii_digit()))
                    .then_some(id.to_string())
            });
            let target_key = canonical_id.unwrap_or_else(|| {
                credential.credential_health = Some("stale".to_string());
                key.clone()
            });
            if target_key != key {
                changes += 1;
            }
            if normalized.insert(target_key.clone(), credential).is_some() {
                return Err(format!(
                    "workspace credential migration would merge duplicate key '{target_key}'"
                ));
            }
        }
        self.workspace_credentials = normalized;
        self.schema_version = CURRENT_PROFILE_SCHEMA_VERSION;
        self.validate_workspace_identities()?;
        Ok(changes)
    }

    pub fn active_workspace_credential(&self) -> Option<&WorkspaceCredential> {
        self.active_workspace_id()
            .and_then(|workspace_id| self.workspace_credential_for(Some(workspace_id)))
    }

    pub fn resolved_access_token(&self) -> Option<&str> {
        let active = self.active_workspace_id();
        let token = active.and_then(|workspace_id| {
            self.workspace_credential_for(Some(workspace_id))
                .and_then(|credential| credential.access_token.as_deref())
        });
        token.filter(|value| !value.trim().is_empty()).or_else(|| {
            (active.is_none() && self.workspace_credentials.len() <= 1)
                .then_some(self.access_token.as_deref())
                .flatten()
                .filter(|value| !value.trim().is_empty())
        })
    }

    pub fn resolved_refresh_token(&self) -> Option<&str> {
        let active = self.active_workspace_id();
        let token = active.and_then(|workspace_id| {
            self.workspace_credential_for(Some(workspace_id))
                .and_then(|credential| credential.refresh_token.as_deref())
        });
        token.filter(|value| !value.trim().is_empty()).or_else(|| {
            (active.is_none() && self.workspace_credentials.len() <= 1)
                .then_some(self.refresh_token.as_deref())
                .flatten()
                .filter(|value| !value.trim().is_empty())
        })
    }

    pub fn resolved_workspace_password(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|credential| credential.workspace_password.as_deref())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.workspace_password
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
            })
    }

    pub fn resolved_oauth_client_id(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|credential| credential.oauth_client_id.as_deref())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.oauth_client_id
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
            })
    }

    pub fn resolved_client_secret(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|credential| credential.client_secret.as_deref())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.client_secret
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
            })
    }

    pub fn resolved_sp_client_secret(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|credential| credential.sp_client_secret.as_deref())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.sp_client_secret
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
            })
            .or_else(|| self.resolved_client_secret())
    }

    /// Service-principal client ID — workspace-first, then account-level.
    pub fn resolved_sp_client_id(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|c| c.sp_client_id.as_deref())
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                self.sp_client_id
                    .as_deref()
                    .filter(|v| !v.trim().is_empty())
            })
    }

    /// Workspace ULID for SP `scope=w:<gid>` — workspace credential first,
    /// then account-level.
    pub fn resolved_workspace_gid(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|c| c.workspace_gid.as_deref())
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                self.workspace_gid
                    .as_deref()
                    .filter(|v| !v.trim().is_empty())
            })
    }

    /// SP token endpoint URL — workspace credential's `token_endpoint_url`
    /// first, then account-level `sp_token_endpoint_url`, both normalized.
    pub fn effective_sp_token_endpoint_url(&self) -> Option<String> {
        if let Some(credential) = self.active_workspace_credential()
            && let Some(url) = credential
                .token_endpoint_url
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
        {
            return Some(normalize_alteryx_one_token_endpoint(url));
        }
        self.sp_token_endpoint_url
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(normalize_alteryx_one_token_endpoint)
    }

    /// Per-credential API base URL override for SP — used when the SP token
    /// is scoped to a regional cell that differs from the global API base.
    pub fn resolved_sp_api_base_url(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|c| c.api_base_url.as_deref())
            .filter(|v| !v.trim().is_empty())
    }

    pub fn effective_token_endpoint_url_for_workspace(
        &self,
        workspace_id: Option<&str>,
    ) -> Option<String> {
        if let Some(credential) = self.workspace_credential_for(workspace_id)
            && let Some(url) = credential
                .token_endpoint_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        {
            return Some(normalize_alteryx_one_token_endpoint(url));
        }
        self.effective_token_endpoint_url()
    }

    pub fn effective_token_endpoint_url(&self) -> Option<String> {
        if let Some(url) = self
            .token_endpoint_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(normalize_alteryx_one_token_endpoint(url));
        }
        self.normalized_base_url()
            .map(|base_url| derive_alteryx_one_token_endpoint(&base_url))
    }

    pub fn canonicalize(&mut self) {
        if let Some(base_url) = self.normalized_base_url() {
            self.base_url = Some(base_url.clone());
            if self
                .token_endpoint_url
                .as_deref()
                .and_then(infer_alteryx_one_base_url)
                .is_some_and(|inferred| inferred == base_url)
            {
                self.token_endpoint_url = None;
            }
        }
        for credential in self.workspace_credentials.values_mut() {
            if let Some(url) = credential
                .token_endpoint_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                credential.token_endpoint_url = Some(normalize_alteryx_one_token_endpoint(url));
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerProfile {
    pub webapi_url: String,
    pub curator_api_key: String,
    pub curator_api_secret: String,
    #[serde(default)]
    pub curator_api_secret_ref: Option<String>,
    pub verify_tls: Option<bool>,
    /// True when this profile was synthesized from `server_api` by
    /// `with_server_api_overrides`, not written directly by the user.
    /// Skipped on serialization so it never persists to disk.
    #[serde(skip, default)]
    pub derived: bool,
}

impl ServerProfile {
    pub fn is_derived(&self) -> bool {
        self.derived
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerApiProfile {
    pub base_url: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret_ref: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SqlServerProfile {
    pub controller: Option<SqlServerConnectionProfile>,
    pub server_ui: Option<SqlServerConnectionProfile>,
    #[serde(default)]
    pub legacy_connection_string: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SqlServerConnectionProfile {
    pub connection_string: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub password_ref: Option<String>,
    pub password_env: Option<String>,
    pub integrated_security: Option<bool>,
    pub encrypt: Option<bool>,
    pub trust_server_certificate: Option<bool>,
    pub multi_subnet_failover: Option<bool>,
}

impl ServerProfile {
    pub fn verify_tls(&self) -> bool {
        self.verify_tls.unwrap_or(true)
    }
}

impl Config {
    pub fn load_from_path(path: &Path) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_path(path)?;
        Self::load_from_resolved_path(&resolved)
    }

    pub fn load_from_path_with_environment(
        path: &Path,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_or_workspace_path(path)?;
        Self::load_from_resolved_path_with_environment(&resolved, environment)
    }

    pub fn load_from_path_lenient(path: &Path) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_path(path)?;
        Self::load_from_resolved_path_lenient(&resolved)
    }

    pub fn load_from_path_lenient_without_active_overlay(
        path: &Path,
    ) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_path(path)?;
        Self::load_from_resolved_path_lenient_without_active_overlay(&resolved)
    }

    /// Load a profile for a write that still needs its references resolved.
    ///
    /// `secret migrate` has to *read* the credential behind an `inline:` or
    /// `keyring:` reference in order to move it, so it cannot use
    /// [`Config::load_from_path_for_write`]. It must still avoid
    /// `apply_env_fallbacks` and `with_server_api_overrides`, which inject
    /// configuration derived from whatever the environment happens to export;
    /// writing that back binds slots the operator never named.
    ///
    /// So: references resolve, ambient environment does not leak into the file.
    pub fn load_from_path_resolving_without_env_fallbacks(
        path: &Path,
    ) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_path(path)?;
        let (path_str, env_values, value) = Self::read_profile_value(&resolved)?;
        if is_workspace_value(&value) {
            // Fail closed. Flattening a workspace to its active environment and
            // handing that back to a caller that writes the file would discard
            // `workspace_name`, `active_environment`, and every other
            // environment — including credentials held only there — with exit 0
            // and no warning. A write path must never be the thing that decides
            // to drop them.
            return Err(ProfileError::Invalid(format!(
                "'{path_str}' is a workspace file, not a single profile. Writing one \
                 environment back would discard the other environments and the workspace \
                 metadata. Target the environment's own profile instead."
            )));
        }
        let config: Self = serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
            path: path_str,
            source,
        })?;
        config.resolve_secret_refs(&env_values)
    }

    /// Load a profile exactly as it is written on disk: no environment
    /// fallbacks applied, no secret references resolved.
    ///
    /// Read paths deliberately augment the file. `apply_env_fallbacks` injects
    /// an `env:NAME` reference for whatever the environment happens to export,
    /// and `resolve_secret_refs` hydrates the value behind each reference. That
    /// augmented view is correct to *use* and wrong to *persist*: writing it
    /// back records configuration the operator never asked for, permanently
    /// rebinding a credential to an ambient variable that happened to be set
    /// during an unrelated command.
    ///
    /// A write path must start from the operator's file and change only the
    /// field it was asked to change, so it uses this loader instead.
    pub fn load_from_path_for_write(path: &Path) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_path(path)?;
        let (path_str, _env_values, value) = Self::read_profile_value(&resolved)?;
        if is_workspace_value(&value) {
            // Fail closed. Flattening a workspace to its active environment and
            // handing that back to a caller that writes the file would discard
            // `workspace_name`, `active_environment`, and every other
            // environment — including credentials held only there — with exit 0
            // and no warning. A write path must never be the thing that decides
            // to drop them.
            return Err(ProfileError::Invalid(format!(
                "'{path_str}' is a workspace file, not a single profile. Writing one \
                 environment back would discard the other environments and the workspace \
                 metadata. Target the environment's own profile instead."
            )));
        }
        serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
            path: path_str,
            source,
        })
    }

    pub fn load_from_path_with_environment_lenient(
        path: &Path,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_or_workspace_path(path)?;
        Self::load_from_resolved_path_with_environment_lenient(&resolved, environment)
    }

    pub fn load_runtime_profile_with_environment(
        profile: Option<&str>,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let resolution = resolve_runtime_profile(profile)?;
        Self::load_from_resolved_path_with_environment(
            Path::new(&resolution.resolved_profile_path),
            environment,
        )
    }

    pub fn load_runtime_profile_with_environment_lenient(
        profile: Option<&str>,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let resolution = resolve_runtime_profile(profile)?;
        Self::load_from_resolved_path_with_environment_lenient(
            Path::new(&resolution.resolved_profile_path),
            environment,
        )
    }

    fn load_from_resolved_path(path: &Path) -> Result<Self, ProfileError> {
        let config = Self::load_from_resolved_path_lenient(path)?;
        config.validate()?;
        Ok(config)
    }

    fn load_from_resolved_path_lenient(path: &Path) -> Result<Self, ProfileError> {
        let (path_str, env_values, value) = Self::read_profile_value(path)?;
        Self::load_config_from_value(path, path_str, value, env_values, None)
    }

    fn load_from_resolved_path_lenient_without_active_overlay(
        path: &Path,
    ) -> Result<Self, ProfileError> {
        let (path_str, env_values, value) = Self::read_profile_value(path)?;
        Self::load_config_without_active_overlay(path, path_str, value, env_values, None)
    }

    fn load_from_resolved_path_with_environment(
        path: &Path,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let config = Self::load_from_resolved_path_with_environment_lenient(path, environment)?;
        config.validate()?;
        Ok(config)
    }

    fn load_from_resolved_path_with_environment_lenient(
        path: &Path,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let (path_str, env_values, value) = Self::read_profile_value(path)?;
        Self::load_config_from_value(path, path_str, value, env_values, environment)
    }

    fn read_profile_value(
        path: &Path,
    ) -> Result<(String, HashMap<String, String>, serde_yaml::Value), ProfileError> {
        let path_str = path.display().to_string();
        recover_keyring_transaction(path)?;
        recover_sensitive_file(path).map_err(|err| ProfileError::Read {
            path: path_str.clone(),
            source: std::io::Error::other(err.to_string()),
        })?;
        let content = fs::read_to_string(path).map_err(|source| ProfileError::Read {
            path: path_str.clone(),
            source,
        })?;
        let env_path = path
            .parent()
            .map(|parent| parent.join(".env"))
            .unwrap_or_else(|| Path::new(".env").to_path_buf());
        let env_values = collect_env_overrides(path).map_err(|source| ProfileError::Read {
            path: env_path.display().to_string(),
            source,
        })?;
        let expanded = expand_env_placeholders(&content, &env_values);
        let value: serde_yaml::Value =
            serde_yaml::from_str(&expanded).map_err(|source| ProfileError::Parse {
                path: path_str.clone(),
                source,
            })?;
        Ok((path_str, env_values, normalize_profile_value(value)?))
    }

    fn load_config_from_value(
        path: &Path,
        path_str: String,
        value: serde_yaml::Value,
        env_values: HashMap<String, String>,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let config = if is_workspace_value(&value) {
            let workspace: WorkspaceConfig =
                serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
                    path: path_str.clone(),
                    source,
                })?;
            let active = environment.unwrap_or(&workspace.active_environment);
            workspace.environments.get(active).cloned().ok_or_else(|| {
                ProfileError::Invalid(format!(
                    "workspace '{}' does not contain environment '{}'",
                    workspace.workspace_name, active
                ))
            })?
        } else {
            serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
                path: path_str,
                source,
            })?
        };

        Self::finalize_loaded_config(config, env_values, path)
    }

    fn load_config_without_active_overlay(
        path: &Path,
        path_str: String,
        value: serde_yaml::Value,
        env_values: HashMap<String, String>,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let config = if is_workspace_value(&value) {
            let workspace: WorkspaceConfig =
                serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
                    path: path_str.clone(),
                    source,
                })?;
            let active = environment.unwrap_or(&workspace.active_environment);
            workspace.environments.get(active).cloned().ok_or_else(|| {
                ProfileError::Invalid(format!(
                    "workspace '{}' does not contain environment '{}'",
                    workspace.workspace_name, active
                ))
            })?
        } else {
            serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
                path: path_str,
                source,
            })?
        };

        Self::finalize_loaded_config_without_overlay(config, env_values, path)
    }

    fn finalize_loaded_config(
        config: Self,
        env_values: HashMap<String, String>,
        current_path: &Path,
    ) -> Result<Self, ProfileError> {
        let config = apply_env_fallbacks(config, &env_values);
        let config = config
            .with_server_api_overrides(&env_values)?
            .resolve_secret_refs(&env_values)?;
        // Warn-only: a mixed-state config is suspicious but still loadable.
        // The write boundary enforces the hard-error; reads must proceed so the
        // operator can inspect and repair the config.
        if let Err(e) = detect_secret_conflict(&config) {
            eprintln!("[ayx WARN] {e}");
        }
        Ok(overlay_active_profile_one_from_state(config, current_path))
    }

    fn finalize_loaded_config_without_overlay(
        config: Self,
        env_values: HashMap<String, String>,
        _current_path: &Path,
    ) -> Result<Self, ProfileError> {
        let config = apply_env_fallbacks(config, &env_values);
        let config = config
            .with_server_api_overrides(&env_values)?
            .resolve_secret_refs(&env_values)?;
        // Warn-only: same rationale as finalize_loaded_config above.
        if let Err(e) = detect_secret_conflict(&config) {
            eprintln!("[ayx WARN] {e}");
        }
        Ok(config)
    }

    fn with_server_api_overrides(
        mut self,
        env_files: &HashMap<String, String>,
    ) -> Result<Self, ProfileError> {
        // Resolve an `env:`/`keyring:`/`inline:` ref on the shared `server_api`
        // secret BEFORE it is expanded into the `api`/`server` representations, so
        // all three carry the same resolved value, and propagate the ref so the
        // later secretize-on-save preserves `env:` refs across every copy.
        if let Some(shared) = self.server_api.as_mut()
            && shared.client_secret.is_empty()
            && let Some(reference) = shared.client_secret_ref.as_deref()
            && let Some(secret) = resolve_ref_for_load(reference, env_files)?
        {
            shared.client_secret = secret;
        }
        if let Some(shared) = &self.server_api {
            if self.api.is_none() {
                self.api = Some(ApiProfile {
                    base_url: normalize_alteryx_base_url(&shared.base_url),
                    auth: ApiAuth {
                        mode: ApiAuthMode::Oauth2ClientCredentials,
                        pat: None,
                        client_id: Some(shared.client_id.clone()),
                        client_secret: Some(shared.client_secret.clone()),
                        client_secret_ref: shared.client_secret_ref.clone(),
                        scope: Some(String::new()),
                    },
                    timeout_ms: None,
                    derived: true,
                });
            }

            if self.server.is_none() {
                self.server = Some(ServerProfile {
                    webapi_url: normalize_alteryx_base_url(&shared.base_url),
                    curator_api_key: shared.client_id.clone(),
                    curator_api_secret: shared.client_secret.clone(),
                    curator_api_secret_ref: shared.client_secret_ref.clone(),
                    verify_tls: None,
                    derived: true,
                });
            }
        }

        Ok(self)
    }

    fn resolve_secret_refs(
        mut self,
        env_files: &HashMap<String, String>,
    ) -> Result<Self, ProfileError> {
        if let Some(one) = self.alteryx_one.as_mut() {
            if one.access_token.is_none()
                && let Some(reference) = one.access_token_ref.as_deref()
            {
                one.access_token = resolve_ref_for_load(reference, env_files)?;
            }
            if one.refresh_token.is_none()
                && let Some(reference) = one.refresh_token_ref.as_deref()
            {
                one.refresh_token = resolve_ref_for_load(reference, env_files)?;
            }
            if one.workspace_password.is_none()
                && let Some(reference) = one.workspace_password_ref.as_deref()
            {
                one.workspace_password = resolve_ref_for_load(reference, env_files)?;
            }
            if one.client_secret.is_none()
                && let Some(reference) = one.client_secret_ref.as_deref()
            {
                one.client_secret = resolve_ref_for_load(reference, env_files)?;
            }
            if one.sp_client_secret.is_none()
                && let Some(reference) = one.sp_client_secret_ref.as_deref()
            {
                one.sp_client_secret = resolve_ref_for_load(reference, env_files)?;
            }
            for credential in one.workspace_credentials.values_mut() {
                if credential.access_token.is_none()
                    && let Some(reference) = credential.access_token_ref.as_deref()
                {
                    credential.access_token = resolve_ref_for_load(reference, env_files)?;
                }
                if credential.refresh_token.is_none()
                    && let Some(reference) = credential.refresh_token_ref.as_deref()
                {
                    credential.refresh_token = resolve_ref_for_load(reference, env_files)?;
                }
                if credential.workspace_password.is_none()
                    && let Some(reference) = credential.workspace_password_ref.as_deref()
                {
                    credential.workspace_password = resolve_ref_for_load(reference, env_files)?;
                }
                if credential.client_secret.is_none()
                    && let Some(reference) = credential.client_secret_ref.as_deref()
                {
                    credential.client_secret = resolve_ref_for_load(reference, env_files)?;
                }
                if credential.sp_client_secret.is_none()
                    && let Some(reference) = credential.sp_client_secret_ref.as_deref()
                {
                    credential.sp_client_secret = resolve_ref_for_load(reference, env_files)?;
                }
            }
            one.canonicalize();
        }

        if let Some(api) = self.api.as_mut()
            && api.auth.client_secret.is_none()
            && let Some(reference) = api.auth.client_secret_ref.as_deref()
        {
            api.auth.client_secret = resolve_ref_for_load(reference, env_files)?;
        }

        if let Some(server) = self.server.as_mut()
            && server.curator_api_secret.is_empty()
            && let Some(reference) = server.curator_api_secret_ref.as_deref()
            && let Some(secret) = resolve_ref_for_load(reference, env_files)?
        {
            server.curator_api_secret = secret;
        }

        if let Some(sqlserver) = self.sqlserver.as_mut() {
            for conn in [sqlserver.controller.as_mut(), sqlserver.server_ui.as_mut()]
                .into_iter()
                .flatten()
            {
                if conn.password.is_none()
                    && let Some(reference) = conn.password_ref.as_deref()
                {
                    conn.password = resolve_ref_for_load(reference, env_files)?;
                }
            }
        }

        if let Some(mongo) = self.mongo.managed.as_mut()
            && mongo.password.is_none()
            && let Some(reference) = mongo.password_ref.as_deref()
        {
            mongo.password = resolve_ref_for_load(reference, env_files)?;
        }

        Ok(self)
    }

    fn validate(&self) -> Result<(), ProfileError> {
        if self.profile_name.trim().is_empty() {
            return Err(ProfileError::Invalid(
                "profile_name cannot be empty".to_string(),
            ));
        }

        if self.mongo.databases.gallery_name.trim().is_empty() {
            return Err(ProfileError::Invalid(
                "mongo.databases.gallery_name cannot be empty".to_string(),
            ));
        }

        if self.mongo.databases.service_name.trim().is_empty() {
            return Err(ProfileError::Invalid(
                "mongo.databases.service_name cannot be empty".to_string(),
            ));
        }

        match self.mongo.mode {
            MongoMode::Embedded => {
                self.mongo.embedded.as_ref().ok_or_else(|| {
                    ProfileError::Invalid("mongo.mode=embedded requires mongo.embedded".to_string())
                })?;
            }
            MongoMode::Managed => {
                let managed = self.mongo.managed.as_ref().ok_or_else(|| {
                    ProfileError::Invalid("mongo.mode=managed requires mongo.managed".to_string())
                })?;

                let has_url = managed.url.as_ref().is_some_and(|u| !u.trim().is_empty());
                let has_host = managed.host.as_ref().is_some_and(|h| !h.trim().is_empty());

                if !has_url && !has_host {
                    return Err(ProfileError::Invalid(
                        "mongo.managed requires either url or host".to_string(),
                    ));
                }

                if managed.port == 0 {
                    return Err(ProfileError::Invalid(
                        "mongo.managed.port must be greater than 0".to_string(),
                    ));
                }
            }
        }

        if let Some(api) = &self.api {
            if api.base_url.trim().is_empty() {
                return Err(ProfileError::Invalid(
                    "api.base_url cannot be empty".to_string(),
                ));
            }

            match api.auth.mode {
                ApiAuthMode::Pat => {
                    let has_pat = api.auth.pat.as_ref().is_some_and(|p| !p.trim().is_empty());
                    if !has_pat {
                        return Err(ProfileError::Invalid(
                            "api.auth.mode=pat requires api.auth.pat".to_string(),
                        ));
                    }
                }
                ApiAuthMode::Oauth2ClientCredentials => {
                    let has_client_id = api
                        .auth
                        .client_id
                        .as_ref()
                        .is_some_and(|v| !v.trim().is_empty());
                    let has_client_secret = api
                        .auth
                        .client_secret
                        .as_ref()
                        .is_some_and(|v| !v.trim().is_empty());
                    if !has_client_id || !has_client_secret {
                        return Err(ProfileError::Invalid(
                            "api.auth.mode=oauth2_client_credentials requires client_id and client_secret"
                                .to_string(),
                        ));
                    }
                }
            }
        }

        if let Some(one) = &self.alteryx_one {
            if !one.account_email.contains('@') {
                return Err(ProfileError::Invalid(
                    "alteryx_one.account_email must be a valid email".to_string(),
                ));
            }
            if one.normalized_base_url().is_none() {
                return Err(ProfileError::Invalid(
                    "alteryx_one.base_url is required".to_string(),
                ));
            }
            if let Some(client_id) = &one.oauth_client_id
                && client_id.trim().is_empty()
            {
                return Err(ProfileError::Invalid(
                    "alteryx_one.oauth_client_id cannot be empty when set".to_string(),
                ));
            }
            if let Some(client_secret) = &one.client_secret {
                if client_secret.trim().is_empty() {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.client_secret cannot be empty when set".to_string(),
                    ));
                }
                if one
                    .oauth_client_id
                    .as_ref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.oauth_client_id is required when client_secret is set"
                            .to_string(),
                    ));
                }
            }
            if let Some(url) = &one.base_url
                && url.trim().is_empty()
            {
                return Err(ProfileError::Invalid(
                    "alteryx_one.base_url cannot be empty when set".to_string(),
                ));
            }
            if let Some(url) = &one.token_endpoint_url
                && url.trim().is_empty()
            {
                return Err(ProfileError::Invalid(
                    "alteryx_one.token_endpoint_url cannot be empty when set".to_string(),
                ));
            }
            if let Some(token) = &one.access_token
                && token.trim().is_empty()
            {
                return Err(ProfileError::Invalid(
                    "alteryx_one.access_token cannot be empty when set".to_string(),
                ));
            }
            if let Some(token) = &one.refresh_token {
                if token.trim().is_empty() {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.refresh_token cannot be empty when set".to_string(),
                    ));
                }
                if one
                    .oauth_client_id
                    .as_ref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.oauth_client_id is required when refresh_token is set"
                            .to_string(),
                    ));
                }
            }
            for (workspace_id, credential) in &one.workspace_credentials {
                let access_token_present = credential
                    .access_token
                    .as_ref()
                    .is_some_and(|token| !token.trim().is_empty());
                if !access_token_present {
                    return Err(ProfileError::Invalid(format!(
                        "alteryx_one.workspace_credentials['{workspace_id}'].access_token is required"
                    )));
                }
                if credential
                    .refresh_token
                    .as_ref()
                    .is_some_and(|token| !token.trim().is_empty())
                    && credential
                        .oauth_client_id
                        .as_ref()
                        .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(ProfileError::Invalid(format!(
                        "alteryx_one.workspace_credentials['{workspace_id}'].oauth_client_id is required when refresh_token is set"
                    )));
                }
                if credential
                    .client_secret
                    .as_ref()
                    .is_some_and(|token| !token.trim().is_empty())
                    && credential
                        .oauth_client_id
                        .as_ref()
                        .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(ProfileError::Invalid(format!(
                        "alteryx_one.workspace_credentials['{workspace_id}'].oauth_client_id is required when client_secret is set"
                    )));
                }
            }
        }

        if let Some(observability) = &self.observability
            && let Some(api_logging) = &observability.api_logging
            && api_logging.enabled
            && api_logging
                .path
                .as_ref()
                .is_some_and(|path| path.trim().is_empty())
        {
            return Err(ProfileError::Invalid(
                "observability.api_logging.path cannot be empty when enabled".to_string(),
            ));
        }

        if let Some(server) = &self.server {
            if server.webapi_url.trim().is_empty() {
                return Err(ProfileError::Invalid(
                    "server.webapi_url cannot be empty".to_string(),
                ));
            }
            if server.curator_api_key.trim().is_empty() {
                return Err(ProfileError::Invalid(
                    "server.curator_api_key cannot be empty".to_string(),
                ));
            }
            if server.curator_api_secret.trim().is_empty() {
                return Err(ProfileError::Invalid(
                    "server.curator_api_secret cannot be empty".to_string(),
                ));
            }
        }

        if let Some(sql) = &self.sqlserver {
            validate_sql_connection(sql.controller.as_ref(), "sqlserver.controller")?;
            validate_sql_connection(sql.server_ui.as_ref(), "sqlserver.server_ui")?;
        }

        Ok(())
    }
}

pub fn load_workspace_config(path: &Path) -> Result<WorkspaceConfig, ProfileError> {
    let resolved = resolve_profile_or_workspace_path(path)?;
    load_workspace_config_from_resolved(&resolved)
}

fn is_default_auth_mode(mode: &AuthMode) -> bool {
    *mode == AuthMode::default()
}

pub fn normalize_alteryx_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    let stripped = trimmed
        .strip_suffix("/webapi")
        .or_else(|| trimmed.strip_suffix("/gallery"))
        .unwrap_or(trimmed);
    stripped.trim_end_matches('/').to_string()
}

pub fn normalize_alteryx_one_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub fn derive_alteryx_one_token_endpoint(base_url: &str) -> String {
    format!("{}/as/token", base_url.trim().trim_end_matches('/'))
}

pub fn normalize_alteryx_one_token_endpoint(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.ends_with("/as") {
        derive_alteryx_one_token_endpoint(trimmed.trim_end_matches("/as").trim_end_matches('/'))
    } else {
        trimmed.to_string()
    }
}

pub fn infer_alteryx_one_base_url(token_endpoint_url: &str) -> Option<String> {
    let trimmed = token_endpoint_url.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/as/token")
        .or_else(|| trimmed.strip_suffix("/as"))
        .and_then(normalize_alteryx_one_base_url)
}

fn validate_sql_connection(
    conn: Option<&SqlServerConnectionProfile>,
    field: &str,
) -> Result<(), ProfileError> {
    if let Some(conn) = conn {
        if conn
            .connection_string
            .as_ref()
            .is_some_and(|s| s.trim().is_empty())
        {
            return Err(ProfileError::Invalid(format!(
                "{field}.connection_string cannot be empty when set"
            )));
        }
        if conn.host.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err(ProfileError::Invalid(format!(
                "{field}.host cannot be empty when set"
            )));
        }
        if conn.database.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err(ProfileError::Invalid(format!(
                "{field}.database cannot be empty when set"
            )));
        }
        if conn.password.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err(ProfileError::Invalid(format!(
                "{field}.password cannot be empty when set"
            )));
        }
    }
    Ok(())
}

fn read_env_file_if_present(path: &Path) -> std::io::Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    if !path.exists() {
        return Ok(values);
    }

    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let mut parts = trimmed.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if key.is_empty() {
            continue;
        }
        values.insert(
            key.to_string(),
            value.trim_matches('"').trim_matches('\'').to_string(),
        );
    }
    Ok(values)
}

/// The `.env` values the loader would apply for `profile_path`.
///
/// Exposed so commands that resolve secret references *after* load (notably
/// `ayx secret status` / `validate`) see the same environment view the loader
/// used, rather than reporting an `env:` reference as unresolvable because the
/// value lives in a `.env` file instead of a process variable.
///
/// Returns an empty map when the files cannot be read; a missing or unreadable
/// `.env` is not an error.
pub fn env_file_values(profile_path: &Path) -> HashMap<String, String> {
    collect_env_overrides(profile_path).unwrap_or_default()
}

fn collect_env_overrides(profile_path: &Path) -> std::io::Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    // The working-directory `.env` is a developer convenience for running `ayx`
    // out of a project checkout.  It is deliberately skipped when the caller has
    // set AYX_CONFIG_HOME, which is the explicit isolation knob: tests, CI, and
    // scripted runs point it at a scratch directory and must not silently
    // inherit whatever `.env` happens to sit in the current directory.  Real
    // process environment variables still apply in both cases via `env_value`.
    if env::var_os("AYX_CONFIG_HOME").is_none()
        && let Ok(cwd) = env::current_dir()
    {
        values.extend(read_env_file_if_present(&cwd.join(".env"))?);
    }
    if let Some(parent) = profile_path.parent() {
        values.extend(read_env_file_if_present(&parent.join(".env"))?);
    }
    Ok(values)
}

fn expand_env_placeholders(input: &str, env_values: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'{') {
            let _ = chars.next();
            let mut name = String::new();
            while let Some(&c) = chars.peek() {
                chars.next();
                if c == '}' {
                    break;
                }
                name.push(c);
            }
            if let Some(value) = env_values.get(&name) {
                out.push_str(value);
            } else if let Ok(value) = std::env::var(&name) {
                out.push_str(&value);
            } else {
                out.push_str("${");
                out.push_str(&name);
                out.push('}');
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn flatten_alteryx_server_block(value: serde_yaml::Value) -> serde_yaml::Value {
    let Some(root) = value.as_mapping() else {
        return value;
    };

    let alteryx_server_key = serde_yaml::Value::String("alteryx_server".to_string());
    let Some(alteryx_server_value) = root.get(&alteryx_server_key) else {
        return value;
    };
    let Some(alteryx_server_map) = alteryx_server_value.as_mapping() else {
        return value;
    };

    let mut merged = root.clone();
    for key in ["server_api", "mongo"] {
        let key_value = serde_yaml::Value::String(key.to_string());
        if merged.contains_key(&key_value) {
            continue;
        }
        if let Some(child) = alteryx_server_map.get(&key_value) {
            merged.insert(key_value, child.clone());
        }
    }

    serde_yaml::Value::Mapping(merged)
}

fn is_workspace_value(value: &serde_yaml::Value) -> bool {
    value.as_mapping().is_some_and(|map| {
        map.contains_key(serde_yaml::Value::String("workspace_name".to_string()))
            && map.contains_key(serde_yaml::Value::String("active_environment".to_string()))
            && map.contains_key(serde_yaml::Value::String("environments".to_string()))
    })
}

fn env_value(env_values: &HashMap<String, String>, name: &str) -> Option<String> {
    env_values
        .get(name)
        .cloned()
        .or_else(|| env::var(name).ok())
}

/// Resolve the first variable in `names` that has a value, returning the
/// variable *name* that matched rather than the value itself.
///
/// Secret-bearing fields use this instead of [`env_value`] so the profile can
/// record an `env:NAME` reference.  Storing the reference (not the resolved
/// value) is what keeps an env-sourced secret out of the serialized YAML: a
/// bare value has no `_ref`, so `secret status` reports it as `plaintext` and
/// a later save writes it back as `inline:<secret>`.
fn env_secret_name(env_values: &HashMap<String, String>, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find(|name| {
            env_values
                .get(**name)
                .is_some_and(|value| !value.trim().is_empty())
                || env::var(*name).is_ok_and(|value| !value.trim().is_empty())
        })
        .map(|name| (*name).to_string())
}

fn apply_env_fallbacks(mut config: Config, env_values: &HashMap<String, String>) -> Config {
    let account_email = env_value(env_values, "AYX_ACCOUNT_EMAIL");
    let base_url = env_value(env_values, "AYX_ONE_BASE_URL");
    let oauth_client_id = env_value(env_values, "AYX_ONE_OAUTH_CLIENT_ID")
        .or_else(|| env_value(env_values, "AYX_ONE_CLIENT_ID"));
    let token_endpoint_url = env_value(env_values, "AYX_ONE_TOKEN_ENDPOINT_URL");
    // Secret-bearing fields resolve to an `env:NAME` reference, never to the
    // value.  See `env_secret_name`.
    let access_token_ref =
        env_secret_name(env_values, &["AYX_ONE_API_ACCESS_TOKEN"]).map(|n| env_secret_ref(&n));
    let refresh_token_ref =
        env_secret_name(env_values, &["AYX_ONE_API_REFRESH_TOKEN"]).map(|n| env_secret_ref(&n));
    let client_secret_ref =
        env_secret_name(env_values, &["AYX_ONE_CLIENT_SECRET"]).map(|n| env_secret_ref(&n));
    // The SP client secret has its own dedicated field, separate from the user
    // flow's `client_secret`.
    let sp_client_id = env_value(env_values, "AYX_ONE_SP_CLIENT_ID");
    let sp_client_secret_ref =
        env_secret_name(env_values, &["AYX_ONE_SP_CLIENT_SECRET"]).map(|n| env_secret_ref(&n));
    let sp_token_endpoint_url = env_value(env_values, "AYX_ONE_SP_TOKEN_ENDPOINT_URL");
    let workspace_gid = env_value(env_values, "AYX_ONE_WORKSPACE_GID");

    if account_email.is_some()
        || base_url.is_some()
        || oauth_client_id.is_some()
        || token_endpoint_url.is_some()
        || access_token_ref.is_some()
        || refresh_token_ref.is_some()
        || client_secret_ref.is_some()
        || sp_client_id.is_some()
        || sp_client_secret_ref.is_some()
        || sp_token_endpoint_url.is_some()
        || workspace_gid.is_some()
    {
        let mut one = config.alteryx_one.unwrap_or(AlteryxOneProfile {
            schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: account_email.clone().unwrap_or_default(),
            base_url: None,
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials: BTreeMap::new(),
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        });
        // `account_email` is the deliberate exception to the gap-fill rule
        // below: a `.env` value overrides a populated profile so a stale
        // account can be refreshed without hand-editing the YAML. See
        // `env_file_overrides_stale_profile_auth_fields`.
        if let Some(value) = account_email {
            one.account_email = value;
        }
        // Gap-fill rule: for every field after this point, env vars fill only
        // when the profile value is absent or empty. A non-empty profile value
        // always wins, which makes profiles authoritative.
        if one
            .base_url
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.base_url = base_url;
        }
        if one
            .oauth_client_id
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.oauth_client_id = oauth_client_id;
        }
        // Secret fields take the `env:NAME` reference, and only when the
        // profile carries neither a value nor a `_ref` of its own.  A stored
        // `_ref` (keyring or inline) is the authoritative credential; the env
        // var is a last-resort fallback.
        if one
            .client_secret
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
            && one
                .client_secret_ref
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
        {
            one.client_secret_ref = client_secret_ref;
        }
        if one
            .sp_client_secret
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
            && one
                .sp_client_secret_ref
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
        {
            one.sp_client_secret_ref = sp_client_secret_ref;
        }
        if one
            .token_endpoint_url
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.token_endpoint_url = token_endpoint_url;
        }
        // Only apply env-fallback tokens when there is no _ref already in the
        // profile.  A _ref (inline or keyring) is the authoritative stored
        // credential; the env var is a last-resort fallback.
        if one
            .access_token
            .as_ref()
            .is_none_or(|v| v.trim().is_empty())
            && one
                .access_token_ref
                .as_ref()
                .is_none_or(|v| v.trim().is_empty())
        {
            one.access_token_ref = access_token_ref;
        }
        if one
            .refresh_token
            .as_ref()
            .is_none_or(|v| v.trim().is_empty())
            && one
                .refresh_token_ref
                .as_ref()
                .is_none_or(|v| v.trim().is_empty())
        {
            one.refresh_token_ref = refresh_token_ref;
        }
        if one
            .sp_client_id
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.sp_client_id = sp_client_id;
        }
        if one
            .sp_token_endpoint_url
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.sp_token_endpoint_url = sp_token_endpoint_url;
        }
        if one
            .workspace_gid
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.workspace_gid = workspace_gid;
        }
        one.canonicalize();
        config.alteryx_one = Some(one);
    }

    config
}

/// Overlay One credentials from the active central profile, unless the
/// profile currently being loaded is itself the active profile file.
fn overlay_active_profile_one_from_state(mut config: Config, current_path: &Path) -> Config {
    let Some(shared_one) = load_active_profile_one_from_state(current_path) else {
        return config;
    };

    config.alteryx_one = match config.alteryx_one.take() {
        Some(current_one) => Some(merge_one_profiles(current_one, &shared_one)),
        None => Some(shared_one),
    };

    config
}

fn load_active_profile_one_from_state(current_path: &Path) -> Option<AlteryxOneProfile> {
    let state = load_ayx_state().ok()?;
    let profile_name = state.active_profile?;
    let path = profile_storage_path(&profile_name).ok()?;
    if path == current_path {
        return None;
    }
    Config::load_from_path_lenient(&path)
        .ok()?
        .alteryx_one
        .map(|mut one| {
            one.canonicalize();
            one
        })
}

/// Resolve a secret reference during profile load, degrading an unreadable
/// keyring entry to "unresolved" instead of failing the load.
///
/// A profile that names a keyring account the OS cannot read is a condition the
/// operator needs *reported*, not one that should make every command fail —
/// including `ayx secret status` and `ayx doctor`, the diagnostics whose job is
/// to report it. Which failure a host produces is platform-dependent: a machine
/// with no store at all already yielded `Ok(None)` and degraded gracefully,
/// while macOS with no default keychain returns a hard error, so the same
/// profile worked on one and broke every command on the other.
///
/// The credential is left unset and the reference is preserved, so
/// `secret status` reports the slot as unresolved with its remediation. Only
/// keyring read failures degrade; a malformed reference is still an error.
fn resolve_ref_for_load(
    reference: &str,
    env_files: &HashMap<String, String>,
) -> Result<Option<String>, ProfileError> {
    match resolve_secret_ref_with(reference, env_files) {
        Err(err) if crate::secrets::is_keyring_read_error(&err) => {
            eprintln!("[ayx WARN] {err} — continuing without this credential");
            Ok(None)
        }
        other => other,
    }
}

fn merge_one_profiles(
    mut current: AlteryxOneProfile,
    fallback: &AlteryxOneProfile,
) -> AlteryxOneProfile {
    if current.account_email.trim().is_empty() {
        current.account_email = fallback.account_email.clone();
    }
    if current
        .base_url
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.base_url = fallback.base_url.clone();
    }
    if current
        .oauth_client_id
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.oauth_client_id = fallback.oauth_client_id.clone();
    }
    if current
        .client_secret
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.client_secret = fallback.client_secret.clone();
    }
    if current.client_secret_ref.is_none() {
        current.client_secret_ref = fallback.client_secret_ref.clone();
    }
    // Keep the SP client configuration coherent: inheriting the client_id
    // without its matching secret produces a confusing auth failure.
    if current
        .sp_client_secret
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.sp_client_secret = fallback.sp_client_secret.clone();
    }
    if current.sp_client_secret_ref.is_none() {
        current.sp_client_secret_ref = fallback.sp_client_secret_ref.clone();
    }
    if current
        .token_endpoint_url
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.token_endpoint_url = fallback.token_endpoint_url.clone();
    }
    if current
        .access_token
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.access_token = fallback.access_token.clone();
    }
    if current.access_token_ref.is_none() {
        current.access_token_ref = fallback.access_token_ref.clone();
    }
    if current
        .refresh_token
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.refresh_token = fallback.refresh_token.clone();
    }
    if current.refresh_token_ref.is_none() {
        current.refresh_token_ref = fallback.refresh_token_ref.clone();
    }
    // `workspace_password` / `workspace_password_ref` are deliberately NOT overlaid
    // from the fallback profile, unlike the token fields above.
    //
    // A workspace password authenticates against one specific workspace. Overlaying
    // it would mean that loading profile B with `--profile B`, while profile A is
    // active, silently submits A's password to B's workspace login endpoint — a
    // credential sent somewhere it does not belong, and repeated rejections risk
    // locking the account. Tokens are already workspace-bound and merely fail; a
    // password is a reusable secret, so the blast radius differs in kind.
    //
    // `expected_workspace_id` and `auth_mode` are excluded from this overlay for the
    // same class of reason: they express which workspace/identity a profile means,
    // and inheriting them defeats the point of having separate profiles.
    for (workspace_id, credential) in &fallback.workspace_credentials {
        current
            .workspace_credentials
            .entry(workspace_id.clone())
            .or_insert_with(|| credential.clone());
    }
    if current
        .sp_client_id
        .as_ref()
        .is_none_or(|v| v.trim().is_empty())
    {
        current.sp_client_id = fallback.sp_client_id.clone();
    }
    if current
        .sp_token_endpoint_url
        .as_ref()
        .is_none_or(|v| v.trim().is_empty())
    {
        current.sp_token_endpoint_url = fallback.sp_token_endpoint_url.clone();
    }
    if current
        .workspace_gid
        .as_ref()
        .is_none_or(|v| v.trim().is_empty())
    {
        current.workspace_gid = fallback.workspace_gid.clone();
    }
    current.canonicalize();
    current
}

fn normalize_profile_value(value: serde_yaml::Value) -> Result<serde_yaml::Value, ProfileError> {
    let value = normalize_canonical_server_block(value)?;
    let value = flatten_alteryx_server_block(value);
    if let Some(workspace_value) = value.as_mapping()
        && workspace_value.contains_key(serde_yaml::Value::String("environments".to_string()))
    {
        return normalize_workspace_environments(value);
    }
    Ok(value)
}

fn default_mongo_profile() -> MongoProfile {
    MongoProfile {
        mode: MongoMode::Embedded,
        databases: MongoDatabases {
            gallery_name: "AlteryxGallery".to_string(),
            service_name: "AlteryxService".to_string(),
        },
        embedded: Some(MongoEmbedded {
            runtime_settings_path: Some("RuntimeSettings.xml".to_string()),
            alteryx_service_path: None,
            restore_target_path: None,
        }),
        managed: None,
    }
}

fn default_runtime_settings_path() -> Option<String> {
    Some("RuntimeSettings.xml".to_string())
}

fn default_mongo_port() -> u16 {
    27017
}

pub fn profile_shape_label(value: &serde_yaml::Value) -> &'static str {
    let Some(root) = value.as_mapping() else {
        return "unknown";
    };
    if root.contains_key(serde_yaml::Value::String("workspace_name".to_string())) {
        if let Some(environments) = root
            .get(serde_yaml::Value::String("environments".to_string()))
            .and_then(|value| value.as_mapping())
            && environments.values().any(|env| {
                env.as_mapping().is_some_and(|map| {
                    map.contains_key(serde_yaml::Value::String("server".to_string()))
                })
            })
        {
            return "workspace-canonical";
        }
        return "workspace-legacy";
    }
    if let Some(server) = root
        .get(serde_yaml::Value::String("server".to_string()))
        .and_then(|value| value.as_mapping())
    {
        if server.contains_key(serde_yaml::Value::String("api".to_string()))
            || server.contains_key(serde_yaml::Value::String("storage".to_string()))
        {
            return "canonical";
        }
        return "legacy";
    }
    if root.contains_key(serde_yaml::Value::String("alteryx_server".to_string()))
        || root.contains_key(serde_yaml::Value::String("server_api".to_string()))
        || root.contains_key(serde_yaml::Value::String("mongo".to_string()))
        || root.contains_key(serde_yaml::Value::String("sqlserver".to_string()))
    {
        return "legacy";
    }
    "unknown"
}

fn normalize_workspace_environments(
    value: serde_yaml::Value,
) -> Result<serde_yaml::Value, ProfileError> {
    let Some(root) = value.as_mapping() else {
        return Ok(value);
    };
    let mut merged = root.clone();
    let env_key = serde_yaml::Value::String("environments".to_string());
    let Some(envs_value) = merged.get_mut(&env_key) else {
        return Ok(serde_yaml::Value::Mapping(merged));
    };
    let Some(envs_map) = envs_value.as_mapping_mut() else {
        return Err(ProfileError::Invalid(
            "workspace.environments must be a mapping".to_string(),
        ));
    };
    for env_value in envs_map.values_mut() {
        let normalized = normalize_canonical_server_block(env_value.clone())?;
        *env_value = flatten_alteryx_server_block(normalized);
    }
    Ok(serde_yaml::Value::Mapping(merged))
}

fn normalize_canonical_server_block(
    value: serde_yaml::Value,
) -> Result<serde_yaml::Value, ProfileError> {
    let Some(root) = value.as_mapping() else {
        return Ok(value);
    };

    let server_key = serde_yaml::Value::String("server".to_string());
    let Some(server_value) = root.get(&server_key) else {
        return Ok(value);
    };
    let Some(server_map) = server_value.as_mapping() else {
        return Ok(value);
    };

    let api_key = serde_yaml::Value::String("api".to_string());
    let storage_key = serde_yaml::Value::String("storage".to_string());
    if !server_map.contains_key(&api_key) && !server_map.contains_key(&storage_key) {
        return Ok(value);
    }

    let mut merged = root.clone();
    let mut legacy_server_api = None;
    let mut legacy_mongo = None;
    let mut legacy_sqlserver = None;

    if let Some(api_value) = server_map.get(&api_key) {
        legacy_server_api = Some(api_value.clone());
    }

    if let Some(storage_value) = server_map.get(&storage_key) {
        let Some(storage_map) = storage_value.as_mapping() else {
            return Err(ProfileError::Invalid(
                "server.storage must be a mapping".to_string(),
            ));
        };
        let kind_key = serde_yaml::Value::String("kind".to_string());
        let kind = storage_map
            .get(&kind_key)
            .and_then(|value| value.as_str())
            .unwrap_or("embedded-mongo");
        let mongo_key = serde_yaml::Value::String("mongo".to_string());
        let sqlserver_key = serde_yaml::Value::String("sqlserver".to_string());
        if let Some(mongo_value) = storage_map.get(&mongo_key) {
            legacy_mongo = Some(mongo_value.clone());
        }
        if let Some(sql_value) = storage_map.get(&sqlserver_key) {
            legacy_sqlserver = Some(sql_value.clone());
        }
        match kind {
            "embedded-mongo" | "managed-mongo" | "sqlserver" | "sql-server" => {}
            other => {
                return Err(ProfileError::Invalid(format!(
                    "server.storage.kind '{}' is not supported",
                    other
                )));
            }
        }
    }

    merged.remove(&server_key);
    if let Some(value) = legacy_server_api {
        merged.insert(serde_yaml::Value::String("server_api".to_string()), value);
    }
    if let Some(value) = legacy_mongo {
        merged.insert(serde_yaml::Value::String("mongo".to_string()), value);
    }
    if let Some(value) = legacy_sqlserver {
        merged.insert(serde_yaml::Value::String("sqlserver".to_string()), value);
    }
    Ok(serde_yaml::Value::Mapping(merged))
}

/// Drop credential values that a `_ref` already accounts for, before the
/// profile is serialized.
///
/// Loading a profile *hydrates* every `keyring:`/`env:`/`inline:` reference into
/// its plaintext value field so the rest of the process can use the credential
/// (`resolve_secret_refs`). Those value fields are serializable, so any command
/// that loads a profile and writes it back — `ayx secret set`, `ayx secret
/// unset`, and anything else round-tripping through `write_config_exact` — used
/// to persist the resolved plaintext next to the reference it came from. A
/// keyring- or env-backed credential silently became cleartext on disk as a
/// side effect of touching an unrelated slot.
///
/// This is the exact inverse of `resolve_secret_refs`, and it is deliberately
/// narrow: a value is dropped only when its reference **actually produces that
/// same value**. Presence of a reference is not enough.
///
/// The difference is not academic. `resolve_secret_refs` hydrates a reference
/// only when the value is absent, so a profile carrying both a hand-written
/// plaintext value *and* a stale or dead reference reaches serialization with
/// the plaintext as the only working copy of the credential. Clearing on mere
/// presence deleted it — during an unrelated `ayx secret set`, with exit 0 and
/// no warning, and with no undo because `write_config_exact` replaces the file.
/// A mixed-state profile is exactly what a copied profile, a restored backup, a
/// wiped keyring, or the very write-back bug above produces, and the loader
/// tolerates it on purpose (`detect_secret_conflict` is warn-only) so the
/// operator can repair it.
///
/// So: an unresolvable reference, or one resolving to something different,
/// leaves the value alone. Values with no reference at all are genuine
/// plaintext the user put there and are likewise preserved.
fn strip_values_covered_by_refs(config: &Config, env_files: &HashMap<String, String>) -> Config {
    fn covers(
        reference: Option<&String>,
        value: Option<&str>,
        env_files: &HashMap<String, String>,
    ) -> bool {
        let Some(reference) = reference.filter(|r| !r.trim().is_empty()) else {
            return false;
        };
        // A keyring read failure lands in the `Err` arm and resolves to
        // "not covered", so the credential survives. Preserving a secret we
        // cannot re-derive is the only safe direction here.
        match resolve_secret_ref_with(reference, env_files) {
            Ok(Some(resolved)) => Some(resolved.as_str()) == value,
            _ => false,
        }
    }

    let clear = |value: &mut Option<String>, reference: Option<&String>| {
        if covers(reference, value.as_deref(), env_files) {
            *value = None;
        }
    };

    let mut config = config.clone();
    if let Some(one) = config.alteryx_one.as_mut() {
        clear(&mut one.access_token, one.access_token_ref.as_ref());
        clear(&mut one.refresh_token, one.refresh_token_ref.as_ref());
        clear(
            &mut one.workspace_password,
            one.workspace_password_ref.as_ref(),
        );
        clear(&mut one.client_secret, one.client_secret_ref.as_ref());
        clear(&mut one.sp_client_secret, one.sp_client_secret_ref.as_ref());
        for credential in one.workspace_credentials.values_mut() {
            clear(
                &mut credential.access_token,
                credential.access_token_ref.as_ref(),
            );
            clear(
                &mut credential.refresh_token,
                credential.refresh_token_ref.as_ref(),
            );
            clear(
                &mut credential.workspace_password,
                credential.workspace_password_ref.as_ref(),
            );
            clear(
                &mut credential.client_secret,
                credential.client_secret_ref.as_ref(),
            );
            clear(
                &mut credential.sp_client_secret,
                credential.sp_client_secret_ref.as_ref(),
            );
        }
    }
    if let Some(api) = config.api.as_mut() {
        clear(
            &mut api.auth.client_secret,
            api.auth.client_secret_ref.as_ref(),
        );
    }
    if let Some(server) = config.server.as_mut()
        && covers(
            server.curator_api_secret_ref.as_ref(),
            Some(server.curator_api_secret.as_str()),
            env_files,
        )
    {
        server.curator_api_secret = String::new();
    }
    if let Some(server_api) = config.server_api.as_mut()
        && covers(
            server_api.client_secret_ref.as_ref(),
            Some(server_api.client_secret.as_str()),
            env_files,
        )
    {
        server_api.client_secret = String::new();
    }
    if let Some(sqlserver) = config.sqlserver.as_mut() {
        for conn in [sqlserver.controller.as_mut(), sqlserver.server_ui.as_mut()]
            .into_iter()
            .flatten()
        {
            clear(&mut conn.password, conn.password_ref.as_ref());
        }
    }
    if let Some(mongo) = config.mongo.managed.as_mut() {
        clear(&mut mongo.password, mongo.password_ref.as_ref());
    }
    config
}

/// Serialize a profile, dropping values their references demonstrably cover.
///
/// Prefer [`canonical_profile_value_with_env`] wherever the profile path is in
/// hand: without the profile-adjacent `.env`, an `env:NAME` reference supplied
/// through that file cannot be resolved, so its value is treated as uncovered
/// and preserved. That is the safe direction — a credential is never destroyed
/// — but it can leave plaintext beside a reference that did in fact cover it.
pub fn canonical_profile_value(config: &Config) -> Result<serde_yaml::Value, ProfileError> {
    canonical_profile_value_with_env(config, &HashMap::new())
}

/// [`canonical_profile_value`], resolving `env:` references against the same
/// `.env` view the loader used.
pub fn canonical_profile_value_with_env(
    config: &Config,
    env_files: &HashMap<String, String>,
) -> Result<serde_yaml::Value, ProfileError> {
    let config = &strip_values_covered_by_refs(config, env_files);
    let mut root = serde_yaml::Mapping::new();
    root.insert(
        serde_yaml::Value::String("profile_name".to_string()),
        serde_yaml::to_value(&config.profile_name).map_err(|source| ProfileError::Parse {
            path: "profile_name".to_string(),
            source,
        })?,
    );
    if let Some(one) = &config.alteryx_one {
        root.insert(
            serde_yaml::Value::String("alteryx_one".to_string()),
            serde_yaml::to_value(one).map_err(|source| ProfileError::Parse {
                path: "alteryx_one".to_string(),
                source,
            })?,
        );
    }
    if let Some(observability) = &config.observability {
        root.insert(
            serde_yaml::Value::String("observability".to_string()),
            serde_yaml::to_value(observability).map_err(|source| ProfileError::Parse {
                path: "observability".to_string(),
                source,
            })?,
        );
    }
    if let Some(upgrade) = &config.upgrade {
        root.insert(
            serde_yaml::Value::String("upgrade".to_string()),
            serde_yaml::to_value(upgrade).map_err(|source| ProfileError::Parse {
                path: "upgrade".to_string(),
                source,
            })?,
        );
    }
    if let Some(server) = canonical_server_value(config)? {
        root.insert(serde_yaml::Value::String("server".to_string()), server);
    }
    Ok(serde_yaml::Value::Mapping(root))
}

pub fn canonical_workspace_value(
    workspace: &WorkspaceConfig,
) -> Result<serde_yaml::Value, ProfileError> {
    let mut root = serde_yaml::Mapping::new();
    root.insert(
        serde_yaml::Value::String("workspace_name".to_string()),
        serde_yaml::to_value(&workspace.workspace_name).map_err(|source| ProfileError::Parse {
            path: "workspace_name".to_string(),
            source,
        })?,
    );
    root.insert(
        serde_yaml::Value::String("active_environment".to_string()),
        serde_yaml::to_value(&workspace.active_environment).map_err(|source| {
            ProfileError::Parse {
                path: "active_environment".to_string(),
                source,
            }
        })?,
    );
    let mut env_map = serde_yaml::Mapping::new();
    let mut env_names = workspace.environments.keys().cloned().collect::<Vec<_>>();
    env_names.sort();
    for name in env_names {
        let config = workspace.environments.get(&name).ok_or_else(|| {
            ProfileError::Invalid(format!(
                "workspace '{}' is missing environment '{}'",
                workspace.workspace_name, name
            ))
        })?;
        env_map.insert(
            serde_yaml::Value::String(name),
            canonical_profile_value(config)?,
        );
    }
    root.insert(
        serde_yaml::Value::String("environments".to_string()),
        serde_yaml::Value::Mapping(env_map),
    );
    Ok(serde_yaml::Value::Mapping(root))
}

fn canonical_server_value(config: &Config) -> Result<Option<serde_yaml::Value>, ProfileError> {
    let api = config.server_api.clone().or_else(|| {
        config
            .api
            .as_ref()
            .and_then(api_profile_to_server_api)
            .or_else(|| {
                config.server.as_ref().map(|server| ServerApiProfile {
                    base_url: server.webapi_url.clone(),
                    client_id: server.curator_api_key.clone(),
                    client_secret: server.curator_api_secret.clone(),
                    client_secret_ref: server.curator_api_secret_ref.clone(),
                })
            })
    });
    let Some(api) = api else {
        return Ok(None);
    };

    let mut storage = serde_yaml::Mapping::new();
    let kind = if config.sqlserver.is_some() {
        ServerStorageKind::SqlServer
    } else {
        match config.mongo.mode {
            MongoMode::Embedded => ServerStorageKind::EmbeddedMongo,
            MongoMode::Managed => ServerStorageKind::ManagedMongo,
        }
    };
    storage.insert(
        serde_yaml::Value::String("kind".to_string()),
        serde_yaml::to_value(kind).map_err(|source| ProfileError::Parse {
            path: "server.storage.kind".to_string(),
            source,
        })?,
    );
    storage.insert(
        serde_yaml::Value::String("mongo".to_string()),
        serde_yaml::to_value(&config.mongo).map_err(|source| ProfileError::Parse {
            path: "server.storage.mongo".to_string(),
            source,
        })?,
    );
    if let Some(sqlserver) = &config.sqlserver {
        storage.insert(
            serde_yaml::Value::String("sqlserver".to_string()),
            serde_yaml::to_value(sqlserver).map_err(|source| ProfileError::Parse {
                path: "server.storage.sqlserver".to_string(),
                source,
            })?,
        );
    }

    let mut server = serde_yaml::Mapping::new();
    server.insert(
        serde_yaml::Value::String("api".to_string()),
        serde_yaml::to_value(api).map_err(|source| ProfileError::Parse {
            path: "server.api".to_string(),
            source,
        })?,
    );
    server.insert(
        serde_yaml::Value::String("storage".to_string()),
        serde_yaml::Value::Mapping(storage),
    );
    Ok(Some(serde_yaml::Value::Mapping(server)))
}

fn api_profile_to_server_api(api: &ApiProfile) -> Option<ServerApiProfile> {
    let client_id = api
        .auth
        .client_id
        .as_ref()
        .filter(|v| !v.is_empty())?
        .clone();
    // Carry the ref through even when the plaintext is absent — after secretize,
    // `client_secret` is None and the secret lives in `client_secret_ref`, which
    // must reach the canonical `server.api.client_secret_ref` output (not be
    // dropped, which would lose the secret on round-trip).
    Some(ServerApiProfile {
        base_url: api.base_url.clone(),
        client_id,
        client_secret: api.auth.client_secret.clone().unwrap_or_default(),
        client_secret_ref: api.auth.client_secret_ref.clone(),
    })
}

pub fn ayx_config_home() -> Result<PathBuf, ProfileError> {
    if let Some(path) = env::var_os("AYX_CONFIG_HOME") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("ayx"));
    }
    if cfg!(windows)
        && let Some(path) = env::var_os("APPDATA")
    {
        return Ok(PathBuf::from(path).join("ayx"));
    }
    if let Some(path) = env::var_os("HOME") {
        return Ok(PathBuf::from(path).join(".config").join("ayx"));
    }
    if cfg!(windows)
        && let (Some(drive), Some(path)) = (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH"))
    {
        return Ok(PathBuf::from(format!(
            "{}{}",
            PathBuf::from(drive).display(),
            PathBuf::from(path).display()
        ))
        .join(".config")
        .join("ayx"));
    }
    Err(ProfileError::Invalid(
        "unable to resolve ayx config home; set AYX_CONFIG_HOME".to_string(),
    ))
}

pub fn ayx_profiles_dir() -> Result<PathBuf, ProfileError> {
    Ok(ayx_config_home()?.join("profiles"))
}

pub fn ayx_workspaces_dir() -> Result<PathBuf, ProfileError> {
    Ok(ayx_config_home()?.join("workspaces"))
}

pub fn ayx_state_path() -> Result<PathBuf, ProfileError> {
    Ok(ayx_config_home()?.join("state.yaml"))
}

pub fn load_ayx_state() -> Result<AyxState, ProfileError> {
    let path = ayx_state_path()?;
    if !path.exists() {
        return Ok(AyxState::default());
    }
    let content = fs::read_to_string(&path).map_err(|source| ProfileError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let mut state: AyxState =
        serde_yaml::from_str(&content).map_err(|source| ProfileError::Parse {
            path: path.display().to_string(),
            source,
        })?;
    state.active_profile = state
        .active_profile
        .map(|name| normalize_storage_name(&name));
    state.active_workspace = state
        .active_workspace
        .map(|name| normalize_storage_name(&name));
    Ok(state)
}

pub fn save_ayx_state(state: &AyxState) -> Result<(), ProfileError> {
    let path = ayx_state_path()?;
    let body = serde_yaml::to_string(state).map_err(|source| ProfileError::Parse {
        path: path.display().to_string(),
        source,
    })?;
    write_sensitive_file(&path, body.as_bytes()).map_err(|err| match err {
        crate::sensitive::SensitiveIoError::CreateDir { path, source }
        | crate::sensitive::SensitiveIoError::Lock { path, source }
        | crate::sensitive::SensitiveIoError::Write { path, source }
        | crate::sensitive::SensitiveIoError::Append { path, source } => {
            ProfileError::Write { path, source }
        }
    })
}

pub fn profile_storage_path(name: &str) -> Result<PathBuf, ProfileError> {
    Ok(ayx_profiles_dir()?.join(format!("{}.yaml", normalize_storage_name(name))))
}

pub fn workspace_storage_path(name: &str) -> Result<PathBuf, ProfileError> {
    Ok(ayx_workspaces_dir()?.join(format!("{name}.yaml")))
}

pub fn default_profile_storage_path() -> Result<PathBuf, ProfileError> {
    let state = load_ayx_state()?;
    profile_path_for_name(
        state
            .active_profile
            .as_deref()
            .unwrap_or(DEFAULT_ACTIVE_PROFILE_NAME),
    )
}

pub fn default_workspace_storage_path() -> Result<PathBuf, ProfileError> {
    let state = load_ayx_state()?;
    workspace_storage_path(
        state
            .active_workspace
            .as_deref()
            .unwrap_or(DEFAULT_ACTIVE_WORKSPACE_NAME),
    )
}

pub fn resolve_profile_path(path: &Path) -> Result<PathBuf, ProfileError> {
    resolve_path_internal(path, false)
}

pub fn resolve_profile_or_workspace_path(path: &Path) -> Result<PathBuf, ProfileError> {
    resolve_path_internal(path, true)
}

pub fn profile_resolution_detail(path: &Path) -> Result<ResolvedProfilePath, ProfileError> {
    let state = load_ayx_state()?;
    let requested = path.display().to_string();
    let resolved = resolve_profile_or_workspace_path(path)?;
    let source = if resolved == path {
        "explicit".to_string()
    } else if is_default_environments_request(path) {
        "environments-state".to_string()
    } else if is_default_profile_request(path) {
        "profile-state".to_string()
    } else {
        "resolved".to_string()
    };
    Ok(ResolvedProfilePath {
        requested_path: requested,
        resolved_path: resolved.display().to_string(),
        source,
        active_profile: state.active_profile,
        active_workspace: state.active_workspace,
    })
}

pub fn resolve_runtime_profile(
    profile: Option<&str>,
) -> Result<RuntimeProfileResolution, ProfileError> {
    let config_home = ayx_config_home()?.display().to_string();
    let state = load_ayx_state()?;
    let (selected_profile, selection_source) =
        match profile.map(str::trim).filter(|v| !v.is_empty()) {
            Some(name) => (normalize_runtime_profile_name(name)?, "cli".to_string()),
            None => {
                if let Ok(env_profile) = env::var("AYX_PROFILE") {
                    let env_profile = env_profile.trim();
                    if !env_profile.is_empty() {
                        (
                            normalize_runtime_profile_name(env_profile)?,
                            "environment".to_string(),
                        )
                    } else if let Some(active) = state.active_profile.clone() {
                        (active, "state".to_string())
                    } else {
                        (
                            DEFAULT_ACTIVE_PROFILE_NAME.to_string(),
                            "default".to_string(),
                        )
                    }
                } else if let Some(active) = state.active_profile.clone() {
                    (active, "state".to_string())
                } else {
                    (
                        DEFAULT_ACTIVE_PROFILE_NAME.to_string(),
                        "default".to_string(),
                    )
                }
            }
        };
    let resolved_profile_path = profile_path_for_name(&selected_profile)?
        .display()
        .to_string();
    Ok(RuntimeProfileResolution {
        config_home,
        selected_profile,
        selection_source,
        resolved_profile_path,
        active_profile: state.active_profile,
    })
}

pub fn list_central_profiles() -> Result<Vec<String>, ProfileError> {
    let mut names = list_named_yaml_entries(&ayx_profiles_dir()?)?;
    if ayx_config_home()?
        .join(LEGACY_DEFAULT_PROFILE_FILE)
        .exists()
        && !names.iter().any(|name| name == DEFAULT_ACTIVE_PROFILE_NAME)
    {
        names.push(DEFAULT_ACTIVE_PROFILE_NAME.to_string());
        names.sort();
    }
    Ok(names)
}

pub fn list_central_workspaces() -> Result<Vec<String>, ProfileError> {
    list_named_yaml_entries(&ayx_workspaces_dir()?)
}

fn list_named_yaml_entries(dir: &Path) -> Result<Vec<String>, ProfileError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).map_err(|source| ProfileError::Read {
        path: dir.display().to_string(),
        source,
    })? {
        let entry = entry.map_err(|source| ProfileError::Read {
            path: dir.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("yaml") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|v| v.to_str()) {
            names.push(stem.to_string());
        }
    }
    names.sort();
    Ok(names)
}

fn resolve_path_internal(path: &Path, allow_workspace: bool) -> Result<PathBuf, ProfileError> {
    if is_explicit_path(path) {
        return Ok(path.to_path_buf());
    }

    if allow_workspace && is_default_environments_request(path) {
        if let Some(workspace) = env::var_os("AYX_WORKSPACE") {
            return Ok(PathBuf::from(workspace));
        }
        let state = load_ayx_state()?;
        if let Some(name) = state.active_workspace {
            return workspace_storage_path(&name);
        }
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return workspace_storage_path(DEFAULT_ACTIVE_WORKSPACE_NAME);
    }

    if is_default_profile_request(path) {
        if let Some(profile) = env::var_os("AYX_PROFILE") {
            return Ok(PathBuf::from(profile));
        }
        let state = load_ayx_state()?;
        if let Some(name) = state.active_profile {
            return profile_path_for_name(&name);
        }
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return profile_path_for_name(DEFAULT_ACTIVE_PROFILE_NAME);
    }

    Ok(path.to_path_buf())
}

fn normalize_runtime_profile_name(name: &str) -> Result<String, ProfileError> {
    let trimmed = normalize_storage_name(name);
    if trimmed.is_empty() {
        return Err(ProfileError::Invalid(
            "runtime profile name must not be empty".to_string(),
        ));
    }
    let candidate = Path::new(&trimmed);
    if candidate.is_absolute()
        || candidate.components().count() > 1
        || trimmed == DEFAULT_PROFILE_FILE
        || trimmed == DEFAULT_ENVIRONMENTS_FILE
        || trimmed == LEGACY_WORKSPACE_FILE
    {
        return Err(ProfileError::Invalid(format!(
            "runtime profile '{trimmed}' must be a central profile name, not a path or config file"
        )));
    }
    Ok(trimmed)
}

fn normalize_storage_name(name: &str) -> String {
    let trimmed = name.trim();
    if let Some(stripped) = trimmed.strip_suffix(".yaml") {
        return stripped.to_string();
    }
    if let Some(stripped) = trimmed.strip_suffix(".yml") {
        return stripped.to_string();
    }
    trimmed.to_string()
}

fn profile_path_for_name(name: &str) -> Result<PathBuf, ProfileError> {
    let normalized = normalize_storage_name(name);
    let canonical = profile_storage_path(&normalized)?;
    if canonical.exists() {
        return Ok(canonical);
    }
    if normalized == DEFAULT_ACTIVE_PROFILE_NAME {
        let legacy = ayx_config_home()?.join(LEGACY_DEFAULT_PROFILE_FILE);
        if legacy.exists() {
            return Ok(legacy);
        }
    }
    Ok(canonical)
}

fn is_default_profile_request(path: &Path) -> bool {
    is_single_component_file(path, DEFAULT_PROFILE_FILE)
}

fn is_default_environments_request(path: &Path) -> bool {
    is_single_component_file(path, DEFAULT_ENVIRONMENTS_FILE)
        || is_single_component_file(path, LEGACY_WORKSPACE_FILE)
}

fn is_single_component_file(path: &Path, file_name: &str) -> bool {
    path.file_name().and_then(|v| v.to_str()) == Some(file_name) && path.components().count() == 1
}

fn is_explicit_path(path: &Path) -> bool {
    path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::CurDir | Component::RootDir
            )
        })
        || (!is_default_profile_request(path) && !is_default_environments_request(path))
}

fn load_workspace_config_from_resolved(path: &Path) -> Result<WorkspaceConfig, ProfileError> {
    let path_str = path.display().to_string();
    let content = fs::read_to_string(path).map_err(|source| ProfileError::Read {
        path: path_str.clone(),
        source,
    })?;
    let env_path = path
        .parent()
        .map(|parent| parent.join(".env"))
        .unwrap_or_else(|| Path::new(".env").to_path_buf());
    let env_values = read_env_file_if_present(&env_path).map_err(|source| ProfileError::Read {
        path: env_path.display().to_string(),
        source,
    })?;
    let expanded = expand_env_placeholders(&content, &env_values);

    let value: serde_yaml::Value =
        serde_yaml::from_str(&expanded).map_err(|source| ProfileError::Parse {
            path: path_str.clone(),
            source,
        })?;
    let value = normalize_profile_value(value)?;
    let workspace: WorkspaceConfig =
        serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
            path: path_str,
            source,
        })?;
    if !workspace
        .environments
        .contains_key(&workspace.active_environment)
    {
        return Err(ProfileError::Invalid(format!(
            "workspace '{}' does not contain active environment '{}'",
            workspace.workspace_name, workspace.active_environment
        )));
    }
    Ok(workspace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};

    static TEST_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct EnvGuard {
        key: &'static str,
        old: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let old = std::env::var(key).ok();
            // Tests serialize env access with TEST_ENV_LOCK.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, old }
        }

        fn unset(key: &'static str) -> Self {
            let old = std::env::var(key).ok();
            // Tests serialize env access with TEST_ENV_LOCK.
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(old) = &self.old {
                // Tests serialize env access with TEST_ENV_LOCK.
                unsafe {
                    std::env::set_var(self.key, old);
                }
            } else {
                // Tests serialize env access with TEST_ENV_LOCK.
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    struct CurrentDirGuard {
        old: PathBuf,
    }

    impl CurrentDirGuard {
        fn set(dir: &Path) -> Self {
            let old = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir).unwrap();
            Self { old }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.old);
        }
    }

    fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn base_config(profile_name: &str, database: &str) -> Config {
        Config {
            profile_name: profile_name.to_string(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "AlteryxGallery".to_string(),
                    service_name: "AlteryxService".to_string(),
                },
                embedded: Some(MongoEmbedded {
                    runtime_settings_path: Some("RuntimeSettings.xml".to_string()),
                    alteryx_service_path: None,
                    restore_target_path: None,
                }),
                managed: None,
            },
            alteryx_one: Some(AlteryxOneProfile {
                schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
                account_email: "user@example.com".to_string(),
                base_url: Some("https://us1.alteryxcloud.com".to_string()),
                oauth_client_id: None,
                client_secret: None,
                client_secret_ref: None,
                sp_client_secret: None,
                sp_client_secret_ref: None,
                token_endpoint_url: None,
                access_token: None,
                access_token_ref: None,
                refresh_token: None,
                refresh_token_ref: None,
                workspace_password: None,
                workspace_password_ref: None,
                workspace_credentials: Default::default(),
                active_workspace_id: None,
                auth_rollout: None,
                expected_workspace_id: None,
                sp_client_id: None,
                sp_token_endpoint_url: None,
                workspace_gid: None,
                auth_mode: AuthMode::default(),
            }),
            observability: None,
            server_api: Some(ServerApiProfile {
                base_url: "http://localhost/webapi/".to_string(),
                client_id: "client".to_string(),
                client_secret: "secret".to_string(),
                client_secret_ref: None,
            }),
            api: None,
            server: None,
            sqlserver: Some(SqlServerProfile {
                controller: Some(SqlServerConnectionProfile {
                    connection_string: None,
                    host: Some("localhost".to_string()),
                    port: Some(1433),
                    database: Some(database.to_string()),
                    username: Some("sa".to_string()),
                    password: Some("secret".to_string()),
                    password_ref: None,
                    password_env: Some("AYX_SQL_CONTROLLER_PASSWORD".to_string()),
                    integrated_security: Some(false),
                    encrypt: Some(true),
                    trust_server_certificate: Some(false),
                    multi_subnet_failover: Some(false),
                }),
                server_ui: Some(SqlServerConnectionProfile {
                    connection_string: None,
                    host: Some("localhost".to_string()),
                    port: Some(1433),
                    database: Some("AlteryxServerUI".to_string()),
                    username: Some("sa".to_string()),
                    password: Some("secret".to_string()),
                    password_ref: None,
                    password_env: Some("AYX_SQL_SERVER_UI_PASSWORD".to_string()),
                    integrated_security: Some(false),
                    encrypt: Some(true),
                    trust_server_certificate: Some(false),
                    multi_subnet_failover: Some(false),
                }),
                legacy_connection_string: None,
            }),
            upgrade: None,
        }
    }

    #[test]
    fn loads_active_workspace_environment() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let mut file = temp.reopen().unwrap();
        let workspace = serde_yaml::to_string(&serde_yaml::Value::Mapping(
            [("workspace_name", "lab"), ("active_environment", "dev")]
                .into_iter()
                .map(|(k, v)| {
                    (
                        serde_yaml::Value::String(k.to_string()),
                        serde_yaml::Value::String(v.to_string()),
                    )
                })
                .chain(std::iter::once((
                    serde_yaml::Value::String("environments".to_string()),
                    serde_yaml::to_value(serde_yaml::Mapping::from_iter([
                        (
                            serde_yaml::Value::String("dev".to_string()),
                            serde_yaml::to_value(base_config("dev", "AlteryxService")).unwrap(),
                        ),
                        (
                            serde_yaml::Value::String("prod".to_string()),
                            serde_yaml::to_value(base_config("prod", "ProdService")).unwrap(),
                        ),
                    ]))
                    .unwrap(),
                )))
                .collect(),
        ))
        .unwrap();
        file.write_all(workspace.as_bytes()).unwrap();

        let cfg = Config::load_from_path_with_environment(temp.path(), None).unwrap();
        assert_eq!(cfg.profile_name, "dev");
        assert_eq!(
            cfg.sqlserver
                .as_ref()
                .unwrap()
                .controller
                .as_ref()
                .unwrap()
                .database
                .as_deref(),
            Some("AlteryxService")
        );
    }

    #[test]
    fn loads_named_workspace_environment_override() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let mut file = temp.reopen().unwrap();
        let workspace = serde_yaml::to_string(&serde_yaml::Value::Mapping(
            [("workspace_name", "lab"), ("active_environment", "dev")]
                .into_iter()
                .map(|(k, v)| {
                    (
                        serde_yaml::Value::String(k.to_string()),
                        serde_yaml::Value::String(v.to_string()),
                    )
                })
                .chain(std::iter::once((
                    serde_yaml::Value::String("environments".to_string()),
                    serde_yaml::to_value(serde_yaml::Mapping::from_iter([
                        (
                            serde_yaml::Value::String("dev".to_string()),
                            serde_yaml::to_value(base_config("dev", "DevService")).unwrap(),
                        ),
                        (
                            serde_yaml::Value::String("prod".to_string()),
                            serde_yaml::to_value(base_config("prod", "ProdService")).unwrap(),
                        ),
                    ]))
                    .unwrap(),
                )))
                .collect(),
        ))
        .unwrap();
        file.write_all(workspace.as_bytes()).unwrap();

        let cfg = Config::load_from_path_with_environment(temp.path(), Some("prod")).unwrap();
        assert_eq!(cfg.profile_name, "prod");
        assert_eq!(
            cfg.sqlserver
                .as_ref()
                .unwrap()
                .controller
                .as_ref()
                .unwrap()
                .database
                .as_deref(),
            Some("ProdService")
        );
    }

    #[test]
    fn workspace_inherits_active_profile_one_credentials() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        fs::create_dir_all(config_home.join("profiles")).unwrap();
        fs::create_dir_all(config_home.join("workspaces")).unwrap();

        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());
        let state = AyxState {
            active_profile: Some("shared".to_string()),
            active_workspace: Some("lab".to_string()),
        };
        save_ayx_state(&state).unwrap();

        let mut profile = base_config("shared", "SharedService");
        profile.alteryx_one.as_mut().unwrap().account_email = "shared@example.com".to_string();
        let profile_path = profile_storage_path("shared").unwrap();
        fs::write(&profile_path, serde_yaml::to_string(&profile).unwrap()).unwrap();

        let mut workspace_env = base_config("dev", "DevService");
        workspace_env.alteryx_one = None;
        let workspace = WorkspaceConfig {
            workspace_name: "lab".to_string(),
            active_environment: "dev".to_string(),
            environments: HashMap::from([(String::from("dev"), workspace_env)]),
        };
        let workspace_path = workspace_storage_path("lab").unwrap();
        fs::write(&workspace_path, serde_yaml::to_string(&workspace).unwrap()).unwrap();

        let cfg = Config::load_from_path_with_environment(
            std::path::Path::new("environments.yaml"),
            None,
        )
        .unwrap();
        assert_eq!(
            cfg.alteryx_one
                .as_ref()
                .map(|one| one.account_email.as_str()),
            Some("shared@example.com")
        );
        assert_eq!(
            cfg.sqlserver
                .as_ref()
                .unwrap()
                .controller
                .as_ref()
                .unwrap()
                .database
                .as_deref(),
            Some("DevService")
        );
    }

    #[test]
    fn active_profile_one_overlay_does_not_recurse_on_self() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        let profiles_dir = config_home.join("profiles");
        fs::create_dir_all(&profiles_dir).unwrap();

        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());
        save_ayx_state(&AyxState {
            active_profile: Some("default".to_string()),
            active_workspace: None,
        })
        .unwrap();

        let mut profile = base_config("default", "ServiceDb");
        profile.alteryx_one.as_mut().unwrap().account_email = "self@example.com".to_string();
        let profile_path = profile_storage_path("default").unwrap();
        fs::write(&profile_path, serde_yaml::to_string(&profile).unwrap()).unwrap();

        let loaded = Config::load_from_path_lenient(&profile_path).unwrap();
        assert_eq!(
            loaded
                .alteryx_one
                .as_ref()
                .map(|one| one.account_email.as_str()),
            Some("self@example.com")
        );
    }

    #[test]
    fn load_from_path_lenient_without_active_overlay_keeps_source_profile() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        fs::create_dir_all(config_home.join("profiles")).unwrap();

        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());
        save_ayx_state(&AyxState {
            active_profile: Some("shared".to_string()),
            active_workspace: None,
        })
        .unwrap();

        let mut shared = base_config("shared", "SharedService");
        shared.alteryx_one.as_mut().unwrap().account_email = "shared@example.com".to_string();
        let shared_path = profile_storage_path("shared").unwrap();
        fs::write(&shared_path, serde_yaml::to_string(&shared).unwrap()).unwrap();

        let mut local = base_config("local", "LocalService");
        local.alteryx_one.as_mut().unwrap().account_email = "local@example.com".to_string();
        let local_path = profile_storage_path("local").unwrap();
        fs::write(&local_path, serde_yaml::to_string(&local).unwrap()).unwrap();

        let loaded = Config::load_from_path_lenient_without_active_overlay(&local_path).unwrap();
        assert_eq!(
            loaded
                .alteryx_one
                .as_ref()
                .map(|one| one.account_email.as_str()),
            Some("local@example.com")
        );
    }

    #[test]
    fn one_token_endpoint_normalizes_issuer_root() {
        let profile = AlteryxOneProfile {
            schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "user@example.com".to_string(),
            base_url: Some("https://pingauth.alteryxcloud.com".to_string()),
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: Some("https://pingauth.alteryxcloud.com/as".to_string()),
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials: Default::default(),
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(
            profile.effective_token_endpoint_url().as_deref(),
            Some("https://pingauth.alteryxcloud.com/as/token")
        );
        assert_eq!(
            profile.normalized_base_url().as_deref(),
            Some("https://pingauth.alteryxcloud.com")
        );
    }

    #[test]
    fn one_token_endpoint_does_not_infer_api_base_url_from_auth_host() {
        let profile = AlteryxOneProfile {
            schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "user@example.com".to_string(),
            base_url: None,
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: Some("https://pingauth.alteryxcloud.com/as".to_string()),
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials: Default::default(),
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(profile.normalized_base_url(), None);
        assert_eq!(
            profile.effective_token_endpoint_url().as_deref(),
            Some("https://pingauth.alteryxcloud.com/as/token")
        );
    }

    #[test]
    fn one_prefers_expected_workspace_credential_over_legacy_fields() {
        let mut workspace_credentials = BTreeMap::new();
        workspace_credentials.insert(
            "ws-1".to_string(),
            WorkspaceCredential {
                workspace_id: None,
                workspace_name: None,
                credential_health: None,
                access_token: Some("workspace-access".to_string()),
                access_token_ref: None,
                refresh_token: Some("workspace-refresh".to_string()),
                refresh_token_ref: None,
                workspace_password: None,
                workspace_password_ref: None,
                oauth_client_id: Some("workspace-client".to_string()),
                client_secret: None,
                client_secret_ref: None,
                sp_client_secret: None,
                sp_client_secret_ref: None,
                token_endpoint_url: Some("https://pingauth.alteryxcloud.com/as".to_string()),
                sp_client_id: None,
                workspace_gid: None,
                api_base_url: None,
            },
        );

        let profile = AlteryxOneProfile {
            schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "user@example.com".to_string(),
            base_url: Some("https://us1.alteryxcloud.com".to_string()),
            oauth_client_id: Some("legacy-client".to_string()),
            client_secret: None,
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: Some("https://legacy.example/as".to_string()),
            access_token: Some("legacy-access".to_string()),
            access_token_ref: None,
            refresh_token: Some("legacy-refresh".to_string()),
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials,
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: Some("ws-1".to_string()),
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(profile.active_workspace_id(), Some("ws-1"));
        assert_eq!(profile.resolved_access_token(), Some("workspace-access"));
        assert_eq!(profile.resolved_refresh_token(), Some("workspace-refresh"));
        assert_eq!(profile.resolved_oauth_client_id(), Some("workspace-client"));
        assert_eq!(
            profile
                .effective_token_endpoint_url_for_workspace(profile.active_workspace_id())
                .as_deref(),
            Some("https://pingauth.alteryxcloud.com/as/token")
        );
    }

    #[test]
    fn one_uses_single_workspace_credential_without_expected_workspace_id() {
        let mut workspace_credentials = BTreeMap::new();
        workspace_credentials.insert(
            "ws-2".to_string(),
            WorkspaceCredential {
                workspace_id: None,
                workspace_name: None,
                credential_health: None,
                access_token: Some("single-access".to_string()),
                access_token_ref: None,
                refresh_token: Some("single-refresh".to_string()),
                refresh_token_ref: None,
                workspace_password: None,
                workspace_password_ref: None,
                oauth_client_id: Some("single-client".to_string()),
                client_secret: None,
                client_secret_ref: None,
                sp_client_secret: None,
                sp_client_secret_ref: None,
                token_endpoint_url: Some("https://tenant.example/as".to_string()),
                sp_client_id: None,
                workspace_gid: None,
                api_base_url: None,
            },
        );

        let profile = AlteryxOneProfile {
            schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "user@example.com".to_string(),
            base_url: Some("https://us1.alteryxcloud.com".to_string()),
            oauth_client_id: Some("legacy-client".to_string()),
            client_secret: None,
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: None,
            access_token: Some("legacy-access".to_string()),
            access_token_ref: None,
            refresh_token: Some("legacy-refresh".to_string()),
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials,
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(profile.active_workspace_id(), Some("ws-2"));
        assert_eq!(profile.resolved_access_token(), Some("single-access"));
        assert_eq!(profile.resolved_refresh_token(), Some("single-refresh"));
        assert_eq!(profile.resolved_oauth_client_id(), Some("single-client"));
        assert_eq!(
            profile
                .effective_token_endpoint_url_for_workspace(profile.active_workspace_id())
                .as_deref(),
            Some("https://tenant.example/as/token")
        );
    }

    #[test]
    fn workspace_selector_keeps_active_selection_separate_from_mutation_guard() {
        let mut profile = AlteryxOneProfile::default();
        profile.workspace_credentials.insert(
            "42".to_string(),
            WorkspaceCredential {
                workspace_id: Some("42".to_string()),
                workspace_gid: Some("gid-42".to_string()),
                workspace_name: Some("Finance".to_string()),
                credential_health: Some("fresh".to_string()),
                ..WorkspaceCredential::default()
            },
        );
        profile.expected_workspace_id = Some("guard".to_string());
        assert_eq!(profile.resolve_workspace_selector("gid-42").unwrap(), "42");
        assert_eq!(profile.resolve_workspace_selector("Finance").unwrap(), "42");
        profile.active_workspace_id = Some("42".to_string());
        assert_eq!(profile.active_workspace_id(), Some("42"));
        assert_eq!(profile.expected_workspace_id.as_deref(), Some("guard"));
    }

    #[test]
    fn workspace_target_requires_canonical_complete_identity() {
        let complete = WorkspaceCredential {
            workspace_id: Some("42".to_string()),
            workspace_gid: Some("gid-42".to_string()),
            workspace_name: Some("Finance".to_string()),
            ..WorkspaceCredential::default()
        };
        assert!(
            WorkspaceTarget::from_credential(
                "42",
                &complete,
                WorkspaceResolutionSource::SavedCredential,
            )
            .is_some()
        );
        assert!(
            WorkspaceTarget::from_credential(
                "legacy-name",
                &complete,
                WorkspaceResolutionSource::SavedCredential,
            )
            .is_none()
        );
        assert!(
            WorkspaceTarget::from_credential(
                "42",
                &WorkspaceCredential {
                    workspace_id: Some("42".to_string()),
                    workspace_gid: None,
                    workspace_name: Some("Finance".to_string()),
                    ..WorkspaceCredential::default()
                },
                WorkspaceResolutionSource::SavedCredential,
            )
            .is_none()
        );
    }

    #[test]
    fn workspace_identity_validation_rejects_duplicate_metadata() {
        let mut profile = AlteryxOneProfile::default();
        let credential = WorkspaceCredential {
            workspace_id: Some("42".to_string()),
            workspace_gid: Some("gid-42".to_string()),
            workspace_name: Some("Finance".to_string()),
            ..WorkspaceCredential::default()
        };
        profile
            .workspace_credentials
            .insert("42".to_string(), credential.clone());
        profile
            .workspace_credentials
            .insert("43".to_string(), credential);
        let error = profile
            .validate_workspace_identities()
            .expect_err("duplicate identities must fail closed");
        assert!(error.contains("duplicate workspace ID"));
    }

    #[test]
    fn workspace_credential_migration_rekeys_complete_and_stales_legacy() {
        let mut profile = AlteryxOneProfile::default();
        profile.workspace_credentials.insert(
            "legacy-label".to_string(),
            WorkspaceCredential {
                workspace_id: Some("42".to_string()),
                workspace_gid: Some("gid-42".to_string()),
                workspace_name: Some("Finance".to_string()),
                ..WorkspaceCredential::default()
            },
        );
        profile
            .workspace_credentials
            .insert("unknown".to_string(), WorkspaceCredential::default());
        assert_eq!(profile.migrate_workspace_credentials().unwrap(), 1);
        assert!(profile.workspace_credentials.contains_key("42"));
        assert_eq!(
            profile.workspace_credentials["unknown"]
                .credential_health
                .as_deref(),
            Some("stale")
        );
    }

    #[test]
    fn one_workspace_password_prefers_workspace_credential_over_profile_fallback() {
        let mut workspace_credentials = BTreeMap::new();
        workspace_credentials.insert(
            "ws-3".to_string(),
            WorkspaceCredential {
                workspace_id: None,
                workspace_name: None,
                credential_health: None,
                access_token: Some("workspace-access".to_string()),
                access_token_ref: None,
                refresh_token: None,
                refresh_token_ref: None,
                workspace_password: Some("workspace-password".to_string()),
                workspace_password_ref: None,
                oauth_client_id: None,
                client_secret: None,
                client_secret_ref: None,
                sp_client_secret: None,
                sp_client_secret_ref: None,
                token_endpoint_url: None,
                sp_client_id: None,
                workspace_gid: None,
                api_base_url: None,
            },
        );

        let profile = AlteryxOneProfile {
            schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "user@example.com".to_string(),
            base_url: Some("https://us1.alteryxcloud.com".to_string()),
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_password: Some("profile-password".to_string()),
            workspace_password_ref: None,
            workspace_credentials,
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: Some("ws-3".to_string()),
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(
            profile.resolved_workspace_password(),
            Some("workspace-password")
        );
    }

    #[test]
    fn resolved_sp_client_secret_prefers_workspace_credential_over_profile_level() {
        let mut workspace_credentials = BTreeMap::new();
        workspace_credentials.insert(
            "ws-1".to_string(),
            WorkspaceCredential {
                workspace_id: None,
                workspace_name: None,
                credential_health: None,
                access_token: None,
                access_token_ref: None,
                refresh_token: None,
                refresh_token_ref: None,
                workspace_password: None,
                workspace_password_ref: None,
                oauth_client_id: None,
                client_secret: Some("user-client-secret".to_string()),
                client_secret_ref: None,
                sp_client_secret: Some("workspace-sp-secret".to_string()),
                sp_client_secret_ref: None,
                token_endpoint_url: None,
                sp_client_id: None,
                workspace_gid: None,
                api_base_url: None,
            },
        );

        let profile = AlteryxOneProfile {
            schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "user@example.com".to_string(),
            base_url: Some("https://us1.alteryxcloud.com".to_string()),
            oauth_client_id: None,
            client_secret: Some("legacy-client-secret".to_string()),
            client_secret_ref: None,
            sp_client_secret: Some("profile-sp-secret".to_string()),
            sp_client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials,
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: Some("ws-1".to_string()),
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(
            profile.resolved_sp_client_secret(),
            Some("workspace-sp-secret")
        );
    }

    /// A workspace password must NEVER be inherited from the active profile into a
    /// different profile loaded via `--profile`. Overlaying it would submit one
    /// workspace's password to another workspace's login endpoint. The token fields
    /// above this in `merge_one_profiles` are deliberately overlaid; this one is
    /// deliberately not. If someone "restores consistency" by adding it back, this
    /// test is the tripwire.
    #[test]
    fn merge_one_profiles_never_overlays_workspace_password() {
        let current = AlteryxOneProfile {
            account_email: "current@example.com".to_string(),
            workspace_password: None,
            workspace_password_ref: None,
            ..Default::default()
        };
        let fallback = AlteryxOneProfile {
            account_email: "fallback@example.com".to_string(),
            workspace_password: Some("fallback-password".to_string()),
            workspace_password_ref: Some("inline:fallback-password".to_string()),
            ..Default::default()
        };

        let merged = merge_one_profiles(current, &fallback);

        assert_eq!(
            merged.workspace_password, None,
            "workspace_password must not be inherited across profiles"
        );
        assert_eq!(
            merged.workspace_password_ref, None,
            "workspace_password_ref must not be inherited across profiles"
        );
    }

    #[test]
    fn resolved_sp_client_secret_falls_back_to_shared_client_secret_for_compatibility() {
        let profile = AlteryxOneProfile {
            schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "user@example.com".to_string(),
            base_url: Some("https://us1.alteryxcloud.com".to_string()),
            oauth_client_id: None,
            client_secret: Some("legacy-client-secret".to_string()),
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials: Default::default(),
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(
            profile.resolved_sp_client_secret(),
            Some("legacy-client-secret")
        );
    }

    #[test]
    fn resolved_sp_client_secret_prefers_dedicated_field_over_shared_client_secret() {
        let profile = AlteryxOneProfile {
            schema_version: CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "user@example.com".to_string(),
            base_url: Some("https://us1.alteryxcloud.com".to_string()),
            oauth_client_id: None,
            client_secret: Some("legacy-client-secret".to_string()),
            client_secret_ref: None,
            sp_client_secret: Some("sp-test-secret".to_string()),
            sp_client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials: Default::default(),
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(profile.resolved_sp_client_secret(), Some("sp-test-secret"));
    }

    #[test]
    fn debug_output_never_contains_one_secret_values_or_inline_refs() {
        let profile = AlteryxOneProfile {
            account_email: "person@example.com".to_string(),
            access_token: Some("sentinel-access-token".to_string()),
            access_token_ref: Some("inline:sentinel-access-ref".to_string()),
            sp_client_secret: Some("sentinel-sp-secret".to_string()),
            sp_client_secret_ref: Some("inline:sentinel-sp-ref".to_string()),
            ..Default::default()
        };
        let rendered = format!("{profile:?}");
        for secret in [
            "sentinel-access-token",
            "sentinel-access-ref",
            "sentinel-sp-secret",
            "sentinel-sp-ref",
        ] {
            assert!(!rendered.contains(secret), "debug output leaked {secret}");
        }
    }

    #[test]
    fn workspace_password_ref_round_trips_in_workspace_credential_yaml() {
        let mut profile = base_config("roundtrip", "RoundTripDb");
        profile.alteryx_one.as_mut().unwrap().workspace_credentials = BTreeMap::from([(
            "ws-1".to_string(),
            WorkspaceCredential {
                workspace_id: None,
                workspace_name: None,
                credential_health: None,
                access_token: Some("test-access".to_string()),
                access_token_ref: None,
                refresh_token: None,
                refresh_token_ref: None,
                workspace_password: None,
                workspace_password_ref: Some("keyring:test/workspace.password".to_string()),
                oauth_client_id: None,
                client_secret: None,
                client_secret_ref: None,
                sp_client_secret: None,
                sp_client_secret_ref: None,
                token_endpoint_url: None,
                sp_client_id: None,
                workspace_gid: None,
                api_base_url: None,
            },
        )]);

        let yaml = serde_yaml::to_string(&profile).unwrap();
        let round_tripped: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            round_tripped
                .alteryx_one
                .as_ref()
                .unwrap()
                .workspace_credentials
                .get("ws-1")
                .and_then(|credential| credential.workspace_password_ref.as_deref()),
            Some("keyring:test/workspace.password")
        );
    }

    #[test]
    fn workspace_sp_client_secret_ref_round_trips_through_yaml() {
        let mut profile = base_config("roundtrip", "RoundtripDb");
        let one = profile.alteryx_one.as_mut().unwrap();
        one.workspace_credentials.insert(
            "ws-rt".to_string(),
            WorkspaceCredential {
                workspace_id: None,
                workspace_name: None,
                credential_health: None,
                access_token: None,
                access_token_ref: None,
                refresh_token: None,
                refresh_token_ref: None,
                workspace_password: None,
                workspace_password_ref: None,
                oauth_client_id: None,
                client_secret: None,
                client_secret_ref: None,
                sp_client_secret: None,
                sp_client_secret_ref: Some(
                    "keyring:roundtrip/alteryx_one.workspace_credentials.ws-rt.sp_client_secret"
                        .to_string(),
                ),
                token_endpoint_url: None,
                sp_client_id: None,
                workspace_gid: None,
                api_base_url: None,
            },
        );

        let yaml = serde_yaml::to_string(&profile).unwrap();
        let round_tripped: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            round_tripped
                .alteryx_one
                .as_ref()
                .unwrap()
                .workspace_credentials
                .get("ws-rt")
                .and_then(|credential| credential.sp_client_secret_ref.as_deref()),
            Some("keyring:roundtrip/alteryx_one.workspace_credentials.ws-rt.sp_client_secret")
        );
    }

    #[test]
    fn runtime_profile_loader_does_not_recurse_on_self() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        let profiles_dir = config_home.join("profiles");
        fs::create_dir_all(&profiles_dir).unwrap();

        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());
        save_ayx_state(&AyxState {
            active_profile: Some("default".to_string()),
            active_workspace: None,
        })
        .unwrap();

        let mut profile = base_config("default", "ServiceDb");
        profile.alteryx_one.as_mut().unwrap().account_email = "runtime@example.com".to_string();
        let profile_path = profile_storage_path("default").unwrap();
        fs::write(&profile_path, serde_yaml::to_string(&profile).unwrap()).unwrap();

        let loaded = Config::load_runtime_profile_with_environment_lenient(None, None).unwrap();
        assert_eq!(
            loaded
                .alteryx_one
                .as_ref()
                .map(|one| one.account_email.as_str()),
            Some("runtime@example.com")
        );
    }

    #[test]
    fn runtime_profile_loader_supports_legacy_profile_shape_without_top_level_mongo() {
        let _lock = test_env_lock();
        crate::secrets::install_test_keyring_store();
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        let profiles_dir = config_home.join("profiles");
        fs::create_dir_all(&profiles_dir).unwrap();

        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());
        save_ayx_state(&AyxState {
            active_profile: Some("default".to_string()),
            active_workspace: None,
        })
        .unwrap();

        let legacy_profile = r#"
profile_name: local-dev
alteryx_one:
  account_email: user@example.com
  base_url: https://us1.alteryxcloud.com
  oauth_client_id: client-id
  token_endpoint_url: https://pingauth.alteryxcloud.com/as
  access_token_ref: keyring:default/alteryx_one.access_token
  refresh_token_ref: keyring:default/alteryx_one.refresh_token
observability:
  api_logging:
    enabled: false
    path: logs/api-events.jsonl
    redact_bodies: true
    log_requests: false
    log_responses: false
upgrade:
  target_version: "2025.2"
  deployment: embedded-mongo
server:
  api:
    base_url: http://localhost/webapi/
    client_id: client-id
    client_secret: secret
  storage:
    kind: sql-server
    mongo:
      mode: embedded
      databases:
        gallery_name: AlteryxGallery
        service_name: AlteryxService
      embedded:
        runtime_settings_path: null
        alteryx_service_path: null
        restore_target_path: null
      managed:
        url: null
        host: localhost
        port: 27017
        auth_database: admin
        username: user
        password: null
        password_ref: keyring:default/server.storage.mongo.managed.password
        tls:
          enabled: false
          ca_path: null
          cert_path: null
          key_path: null
          allow_invalid_hostnames: false
        timeout_ms: 15000
        retry_count: 2
        max_pool_size: 20
    sqlserver:
      controller:
        connection_string: null
        host: localhost
        port: 1433
        database: AlteryxService
        username: sa
        password: null
        password_ref: keyring:default/server.storage.sqlserver.controller.password
        password_env: AYX_SQL_CONTROLLER_PASSWORD
        integrated_security: false
        encrypt: true
        trust_server_certificate: false
        multi_subnet_failover: false
      server_ui:
        connection_string: null
        host: localhost
        port: 1433
        database: AlteryxServerUI
        username: sa
        password: null
        password_ref: keyring:default/server.storage.sqlserver.server_ui.password
        password_env: AYX_SQL_SERVER_UI_PASSWORD
        integrated_security: false
        encrypt: true
        trust_server_certificate: false
        multi_subnet_failover: false
      legacy_connection_string: null
sqlserver:
  controller:
    connection_string: null
    host: localhost
    port: 1433
    database: AlteryxService
    username: sa
    password: null
    password_ref: keyring:default/server.storage.sqlserver.controller.password
    password_env: AYX_SQL_CONTROLLER_PASSWORD
    integrated_security: false
    encrypt: true
    trust_server_certificate: false
    multi_subnet_failover: false
  server_ui:
    connection_string: null
    host: localhost
    port: 1433
    database: AlteryxServerUI
    username: sa
    password: null
    password_ref: keyring:default/server.storage.sqlserver.server_ui.password
    password_env: AYX_SQL_SERVER_UI_PASSWORD
    integrated_security: false
    encrypt: true
    trust_server_certificate: false
    multi_subnet_failover: false
  legacy_connection_string: null
"#;
        fs::write(profiles_dir.join("default.yaml"), legacy_profile).unwrap();

        let loaded = Config::load_runtime_profile_with_environment_lenient(None, None).unwrap();
        assert_eq!(
            loaded
                .alteryx_one
                .as_ref()
                .map(|one| one.account_email.as_str()),
            Some("user@example.com")
        );
    }

    #[test]
    fn env_file_overrides_stale_profile_auth_fields() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let _cwd = CurrentDirGuard::set(temp.path());

        let env_file = temp.path().join(".env");
        fs::write(
            &env_file,
            "AYX_ACCOUNT_EMAIL=fresh@example.com\nAYX_ONE_API_ACCESS_TOKEN=fresh-access\nAYX_ONE_API_REFRESH_TOKEN=fresh-refresh\nAYX_ONE_TOKEN_ENDPOINT_URL=https://pingauth.example.com/as\n",
        )
        .unwrap();

        let profile_path = temp.path().join("config.yaml");
        let profile = base_config("default", "ServiceDb");
        fs::write(&profile_path, serde_yaml::to_string(&profile).unwrap()).unwrap();

        let loaded = Config::load_from_path_lenient(&profile_path).unwrap();
        let one = loaded.alteryx_one.as_ref().unwrap();
        assert_eq!(one.account_email, "fresh@example.com");
        assert_eq!(one.access_token.as_deref(), Some("fresh-access"));
        assert_eq!(one.refresh_token.as_deref(), Some("fresh-refresh"));
        assert_eq!(
            one.token_endpoint_url.as_deref(),
            Some("https://pingauth.example.com/as")
        );
    }

    #[test]
    fn normalizes_alteryx_base_urls() {
        assert_eq!(
            normalize_alteryx_base_url("http://host/webapi/"),
            "http://host"
        );
        assert_eq!(
            normalize_alteryx_base_url("http://host/gallery"),
            "http://host"
        );
        assert_eq!(normalize_alteryx_base_url("http://host"), "http://host");
    }

    #[test]
    fn loads_canonical_server_shape() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let canonical = r#"
profile_name: canonical
alteryx_one:
  account_email: user@example.com
  base_url: https://us1.alteryxcloud.com
server:
  api:
    base_url: http://localhost/webapi
    client_id: client
    client_secret: secret
  storage:
    kind: embedded-mongo
    mongo:
      mode: embedded
      databases:
        gallery_name: AlteryxGallery
        service_name: AlteryxService
      embedded:
        runtime_settings_path: RuntimeSettings.xml
"#;
        std::fs::write(temp.path(), canonical).unwrap();
        let cfg = Config::load_from_path(temp.path()).unwrap();
        assert_eq!(cfg.profile_name, "canonical");
        assert_eq!(cfg.server.as_ref().unwrap().webapi_url, "http://localhost");
        assert_eq!(
            cfg.server_api.as_ref().unwrap().base_url,
            "http://localhost/webapi"
        );
        assert!(matches!(cfg.mongo.mode, MongoMode::Embedded));
        assert!(cfg.server.is_some());
    }

    #[test]
    fn resolves_default_profile_from_central_state() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("AYX_CONFIG_HOME", &temp.path().display().to_string());
        let profiles_dir = temp.path().join("profiles");
        std::fs::create_dir_all(&profiles_dir).unwrap();
        std::fs::write(temp.path().join("state.yaml"), "active_profile: central\n").unwrap();
        std::fs::write(
            profiles_dir.join("central.yaml"),
            serde_yaml::to_string(&base_config("central", "CentralDb")).unwrap(),
        )
        .unwrap();

        let cfg = Config::load_from_path(Path::new("config.yaml")).unwrap();
        assert_eq!(cfg.profile_name, "central");
    }

    #[test]
    fn resolves_legacy_root_default_profile() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("AYX_CONFIG_HOME", &temp.path().display().to_string());
        std::fs::write(
            temp.path().join("default.yaml"),
            serde_yaml::to_string(&base_config("legacy", "LegacyDb")).unwrap(),
        )
        .unwrap();
        std::fs::write(
            temp.path().join("state.yaml"),
            "active_profile: default.yaml\n",
        )
        .unwrap();

        let path = default_profile_storage_path().unwrap();
        assert_eq!(path, temp.path().join("default.yaml"));

        let cfg = Config::load_from_path(Path::new("config.yaml")).unwrap();
        assert_eq!(cfg.profile_name, "legacy");
    }

    // ---------------------------------------------------------------------------
    // Task-1 helpers: minimal in-memory configs for derived-marker tests.
    // ---------------------------------------------------------------------------

    /// A bare `Config` with only `server_api` populated; `api` and `server` are
    /// `None` so `with_server_api_overrides` will synthesize both from scratch.
    fn config_with_server_api_only(base_url: &str, client_id: &str, client_secret: &str) -> Config {
        Config {
            profile_name: "test".to_string(),
            mongo: MongoProfile::default(),
            alteryx_one: None,
            observability: None,
            server_api: Some(ServerApiProfile {
                base_url: base_url.to_string(),
                client_id: client_id.to_string(),
                client_secret: client_secret.to_string(),
                client_secret_ref: None,
            }),
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    /// A `Config` with an explicit `api` already set by the user.  `server` is
    /// also pre-populated so neither synthesized arm fires.
    fn config_with_explicit_api(base_url: &str, client_id: &str, client_secret: &str) -> Config {
        Config {
            profile_name: "test".to_string(),
            mongo: MongoProfile::default(),
            alteryx_one: None,
            observability: None,
            server_api: Some(ServerApiProfile {
                base_url: base_url.to_string(),
                client_id: client_id.to_string(),
                client_secret: client_secret.to_string(),
                client_secret_ref: None,
            }),
            api: Some(ApiProfile {
                base_url: base_url.to_string(),
                auth: ApiAuth {
                    mode: ApiAuthMode::Oauth2ClientCredentials,
                    pat: None,
                    client_id: Some(client_id.to_string()),
                    client_secret: Some(client_secret.to_string()),
                    client_secret_ref: None,
                    scope: None,
                },
                timeout_ms: None,
                derived: false,
            }),
            server: Some(ServerProfile {
                webapi_url: base_url.to_string(),
                curator_api_key: client_id.to_string(),
                curator_api_secret: client_secret.to_string(),
                curator_api_secret_ref: None,
                verify_tls: None,
                derived: false,
            }),
            sqlserver: None,
            upgrade: None,
        }
    }

    #[test]
    fn synthesized_api_and_server_are_marked_derived() {
        let cfg = config_with_server_api_only("https://x.example", "cid", "shh");
        let finalized = cfg.with_server_api_overrides(&HashMap::new()).unwrap();
        assert!(
            finalized.api.as_ref().unwrap().is_derived(),
            "synthesized api must be derived"
        );
        assert!(
            finalized.server.as_ref().unwrap().is_derived(),
            "synthesized server must be derived"
        );
    }

    #[test]
    fn user_authored_api_is_not_derived() {
        let cfg = config_with_explicit_api("https://x.example", "cid", "shh");
        let finalized = cfg.with_server_api_overrides(&HashMap::new()).unwrap();
        assert!(
            !finalized.api.as_ref().unwrap().is_derived(),
            "explicit api must not be derived"
        );
    }

    #[test]
    fn ref_form_for_redacts_schemeless_ref() {
        // A scheme-less value in a `_ref` field (e.g. written by a future ref scheme
        // or a malformed config) must be redacted, not printed verbatim.
        assert_eq!(
            ref_form_for("", Some("bare-secret-value"), "inline:***"),
            "inline:***",
            "scheme-less ref must be redacted, not printed verbatim"
        );
        // env: and keyring: are the only allowlisted schemes.
        assert_eq!(
            ref_form_for("", Some("env:MY_VAR"), "inline:***"),
            "env:MY_VAR",
            "env: ref must be printed verbatim"
        );
        assert_eq!(
            ref_form_for("", Some("keyring:my/account"), "inline:***"),
            "keyring:my/account",
            "keyring: ref must be printed verbatim"
        );
        // inline: refs must always be redacted (the suffix IS the secret).
        assert_eq!(
            ref_form_for("", Some("inline:actual-secret"), "inline:***"),
            "inline:***",
            "inline: ref suffix must be redacted"
        );
    }

    #[test]
    fn ref_form_for_redacts_inline_workspace_password_ref() {
        assert_eq!(
            ref_form_for("", Some("inline:test-password"), "inline:***"),
            "inline:***",
            "workspace_password_ref must redact inline secrets"
        );
    }

    /// A secret supplied through `.env` must be recorded as an `env:NAME`
    /// reference, never as a bare value.
    ///
    /// A bare value has no `_ref`, so `ayx secret status` classifies it as
    /// `plaintext` and the next profile save serializes it as
    /// `inline:<secret>` — writing a live credential to disk in cleartext for
    /// anyone who happened to run `ayx` next to a `.env`.
    #[test]
    fn env_sourced_one_secrets_become_env_refs_not_plaintext() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        let profiles = config_home.join("profiles");
        fs::create_dir_all(&profiles).unwrap();
        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());

        fs::write(
            profiles.join(".env"),
            "AYX_ONE_API_ACCESS_TOKEN=access-sentinel-a1\n\
             AYX_ONE_API_REFRESH_TOKEN=refresh-sentinel-b2\n\
             AYX_ONE_CLIENT_SECRET=client-sentinel-c3\n\
             AYX_ONE_SP_CLIENT_SECRET=sp-sentinel-d4\n",
        )
        .unwrap();

        let path = profiles.join("envrefs.yaml");
        fs::write(
            &path,
            serde_yaml::to_string(&base_config("envrefs", "Svc")).unwrap(),
        )
        .unwrap();

        let loaded = Config::load_from_path_lenient_without_active_overlay(&path).unwrap();
        let one = loaded.alteryx_one.as_ref().expect("alteryx_one present");

        assert_eq!(
            one.access_token_ref.as_deref(),
            Some("env:AYX_ONE_API_ACCESS_TOKEN")
        );
        assert_eq!(
            one.refresh_token_ref.as_deref(),
            Some("env:AYX_ONE_API_REFRESH_TOKEN")
        );
        assert_eq!(
            one.client_secret_ref.as_deref(),
            Some("env:AYX_ONE_CLIENT_SECRET")
        );
        assert_eq!(
            one.sp_client_secret_ref.as_deref(),
            Some("env:AYX_ONE_SP_CLIENT_SECRET")
        );

        // The reference must still resolve, or the credential is unusable.
        assert_eq!(one.access_token.as_deref(), Some("access-sentinel-a1"));
        assert_eq!(one.sp_client_secret.as_deref(), Some("sp-sentinel-d4"));

        // Nothing on disk may carry the value.
        let on_disk = fs::read_to_string(&path).unwrap();
        for sentinel in [
            "access-sentinel-a1",
            "refresh-sentinel-b2",
            "client-sentinel-c3",
            "sp-sentinel-d4",
        ] {
            assert!(
                !on_disk.contains(sentinel),
                "profile must not contain the resolved secret {sentinel}"
            );
        }
        assert!(
            !on_disk.contains("inline:"),
            "env-sourced secrets must never be persisted as inline: refs"
        );
    }

    /// AYX_CONFIG_HOME is the isolation knob. When it is set, the
    /// working-directory `.env` must not bleed in: tests and CI point it at a
    /// scratch directory and must not inherit whichever repo checkout the
    /// process happens to be standing in.
    #[test]
    fn cwd_env_file_is_ignored_when_config_home_is_set() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let workdir = temp.path().join("checkout");
        let config_home = temp.path().join("ayx-home");
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(config_home.join("profiles")).unwrap();
        fs::write(
            workdir.join(".env"),
            "AYX_ONE_API_ACCESS_TOKEN=cwd-must-not-bleed\n",
        )
        .unwrap();

        let path = config_home.join("profiles").join("isolated.yaml");
        fs::write(
            &path,
            serde_yaml::to_string(&base_config("isolated", "Svc")).unwrap(),
        )
        .unwrap();

        let _cwd = CurrentDirGuard::set(&workdir);
        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());

        let loaded = Config::load_from_path_lenient_without_active_overlay(&path).unwrap();
        let one = loaded.alteryx_one.as_ref().expect("alteryx_one present");
        assert_eq!(one.access_token_ref, None);
        assert_eq!(one.access_token, None);
    }

    /// Without AYX_CONFIG_HOME the working-directory `.env` remains a
    /// developer convenience, so the isolation guard above cannot be mistaken
    /// for removing the feature.
    #[test]
    fn cwd_env_file_still_applies_without_config_home() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let workdir = temp.path().join("checkout");
        fs::create_dir_all(&workdir).unwrap();
        fs::write(
            workdir.join(".env"),
            "AYX_ONE_API_ACCESS_TOKEN=cwd-applies\n",
        )
        .unwrap();

        // The profile is loaded by explicit path, so no real config home is read.
        let path = temp.path().join("plain.yaml");
        fs::write(
            &path,
            serde_yaml::to_string(&base_config("plain", "Svc")).unwrap(),
        )
        .unwrap();

        let _cwd = CurrentDirGuard::set(&workdir);
        let _guard = EnvGuard::unset("AYX_CONFIG_HOME");

        let loaded = Config::load_from_path_lenient_without_active_overlay(&path).unwrap();
        let one = loaded.alteryx_one.as_ref().expect("alteryx_one present");
        assert_eq!(
            one.access_token_ref.as_deref(),
            Some("env:AYX_ONE_API_ACCESS_TOKEN")
        );
        assert_eq!(one.access_token.as_deref(), Some("cwd-applies"));
    }
}
