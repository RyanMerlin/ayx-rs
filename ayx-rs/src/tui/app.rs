// TUI module pre-dates the structural refactor planned in audit Stage 3.
// These allows scope known stylistic lints to this file only; the next
// pass should split state/update/effects and remove them.
#![allow(
    clippy::derivable_impls,
    clippy::vec_init_then_push,
    clippy::type_complexity,
    clippy::needless_lifetimes,
    clippy::collapsible_match
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use serde_json::Value;

use super::forms::{
    api_profile_to_server_api_ref, default_mongo_embedded, default_mongo_managed,
    default_server_profile, default_sqlserver_profile, field_value, mongo_values,
    normalize_server_url, observability_fields, parse_bool_field, parse_optional_text_field,
    parse_u16_field, parse_u32_field, parse_u64_field, server_profile_to_server_api_ref,
    sqlserver_fields, update_sql_connection,
};
use super::render_helpers::{extract_one_browser_items, pretty_yaml_lines, render_envelope_panel};

use ayx_core::profile::{
    AlteryxOneProfile, ApiAuth, ApiAuthMode, ApiLoggingProfile, ApiProfile, Config, MongoMode,
    ObservabilityProfile, WorkspaceConfig, ayx_config_home, default_profile_storage_path,
    list_central_workspaces, load_ayx_state, load_workspace_config, normalize_alteryx_base_url,
    normalize_alteryx_one_base_url, profile_resolution_detail, profile_storage_path,
    resolve_runtime_profile, save_ayx_state, workspace_storage_path,
};

use crate::onboard::{
    default_config, summarize_config, summarize_onboarding_validation, write_config,
    write_workspace_config,
};

use super::store::{
    ProfileRecord, ProfileScope, create_profile_from_default_scope_at, delete_profile_at,
    duplicate_profile_at, list_profile_records_at, rename_profile_at,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Profiles,
    Credentials,
    Config,
    Connectivity,
    Inspect,
    One,
    Help,
}

impl Screen {
    pub fn all() -> [Screen; 6] {
        [
            Screen::Profiles,
            Screen::Credentials,
            Screen::Config,
            Screen::Connectivity,
            Screen::One,
            Screen::Help,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Screen::Profiles => "Profiles",
            Screen::Credentials => "Alteryx One",
            Screen::Config => "Alteryx Server",
            Screen::Connectivity => "Connectivity",
            Screen::Inspect => "Inspect",
            Screen::One => "One Browser",
            Screen::Help => "Help",
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::all()[index.min(Self::all().len() - 1)]
    }

    /// Index in the sidebar list. `Inspect` is a modal overlay and has no
    /// stable sidebar position — callers must not call this on Inspect; use
    /// `sidebar_index` instead, which returns `None` for Inspect so the
    /// current sidebar selection is left alone.
    pub fn index(self) -> usize {
        self.sidebar_index().unwrap_or(0)
    }

    pub fn sidebar_index(self) -> Option<usize> {
        match self {
            Screen::Profiles => Some(0),
            Screen::Credentials => Some(1),
            Screen::Config => Some(2),
            Screen::Connectivity => Some(3),
            Screen::One => Some(4),
            Screen::Help => Some(5),
            // Inspect is a modal overlay rendered on top of whatever screen
            // was active. It must not move the sidebar cursor.
            Screen::Inspect => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfilesPane {
    Profiles,
    Workspaces,
    Environments,
}

impl ProfilesPane {
    pub fn next(self) -> Self {
        match self {
            ProfilesPane::Profiles => ProfilesPane::Workspaces,
            ProfilesPane::Workspaces => ProfilesPane::Environments,
            ProfilesPane::Environments => ProfilesPane::Profiles,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileView {
    All,
    One,
    Server,
}

impl ProfileView {
    pub fn label(self) -> &'static str {
        match self {
            ProfileView::All => "All Profiles",
            ProfileView::One => "One Profiles",
            ProfileView::Server => "Server Profiles",
        }
    }

    // Cycles All → One → Server. Views currently switch via direct assignment, so
    // this is unused for now; kept for a future "cycle profile views" keybinding.
    #[allow(dead_code)]
    pub fn next(self) -> Self {
        match self {
            ProfileView::All => ProfileView::One,
            ProfileView::One => ProfileView::Server,
            ProfileView::Server => ProfileView::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Profile,
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileCrudAction {
    CreateDefault,
    DuplicateSelected,
    RenameSelected,
    DeleteSelected,
}

#[derive(Debug, Clone)]
pub enum CrudPrompt {
    Text {
        title: String,
        message: String,
        buffer: String,
        action: ProfileCrudAction,
    },
    Confirm {
        title: String,
        message: String,
        action: ProfileCrudAction,
    },
}

impl CrudPrompt {
    pub fn title(&self) -> &str {
        match self {
            CrudPrompt::Text { title, .. } | CrudPrompt::Confirm { title, .. } => title,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            CrudPrompt::Text { message, .. } | CrudPrompt::Confirm { message, .. } => message,
        }
    }

    pub fn buffer(&self) -> Option<&str> {
        match self {
            CrudPrompt::Text { buffer, .. } => Some(buffer.as_str()),
            CrudPrompt::Confirm { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceEntry {
    pub name: String,
    pub path: PathBuf,
    pub active_environment: String,
    pub environments: Vec<String>,
    pub config: WorkspaceConfig,
}

#[derive(Debug, Clone)]
pub struct FieldState {
    pub label: &'static str,
    pub value: String,
    pub placeholder: &'static str,
    pub secret: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSection {
    Overview,
    ServerApi,
    Mongo,
    SqlServer,
    Observability,
}

impl ConfigSection {
    pub fn all() -> [ConfigSection; 5] {
        [
            ConfigSection::Overview,
            ConfigSection::ServerApi,
            ConfigSection::Mongo,
            ConfigSection::SqlServer,
            ConfigSection::Observability,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            ConfigSection::Overview => "Overview",
            ConfigSection::ServerApi => "Server API",
            ConfigSection::Mongo => "Mongo",
            ConfigSection::SqlServer => "SQL Server",
            ConfigSection::Observability => "Observability",
        }
    }

    pub fn next(self) -> Self {
        let all = Self::all();
        let index = all.iter().position(|section| *section == self).unwrap_or(0);
        all[(index + 1) % all.len()]
    }

    pub fn prev(self) -> Self {
        let all = Self::all();
        let index = all.iter().position(|section| *section == self).unwrap_or(0);
        all[(index + all.len() - 1) % all.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFieldKind {
    Text,
    Bool,
}

#[derive(Debug, Clone)]
pub struct ConfigFieldState {
    pub label: &'static str,
    pub value: String,
    pub placeholder: &'static str,
    pub kind: ConfigFieldKind,
    pub secret: bool,
}

impl ConfigFieldState {
    pub fn visible_value(&self) -> String {
        if self.secret {
            if self.value.trim().is_empty() {
                String::new()
            } else {
                "stored".to_string()
            }
        } else {
            self.value.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigForm {
    pub section: ConfigSection,
    pub overview_fields: Vec<ConfigFieldState>,
    pub server_api_fields: Vec<ConfigFieldState>,
    pub mongo_fields: Vec<ConfigFieldState>,
    pub sqlserver_fields: Vec<ConfigFieldState>,
    pub observability_fields: Vec<ConfigFieldState>,
    pub cursor: usize,
    pub editing: bool,
    pub edit_buffer: String,
    pub dirty: bool,
}

impl ConfigForm {
    pub fn from_config(config: &Config) -> Self {
        let overview_fields = vec![
            ConfigFieldState {
                label: "Profile Name",
                value: config.profile_name.clone(),
                placeholder: "local",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Alteryx Server Base URL",
                value: config
                    .server
                    .as_ref()
                    .map(|server| server.webapi_url.clone())
                    .unwrap_or_default(),
                placeholder: "https://server.example.com/",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Verify TLS",
                value: config
                    .server
                    .as_ref()
                    .map(|server| server.verify_tls().to_string())
                    .unwrap_or_else(|| "true".to_string()),
                placeholder: "true|false",
                kind: ConfigFieldKind::Bool,
                secret: false,
            },
        ];

        let server_api_source = config
            .server_api
            .clone()
            .or_else(|| config.api.as_ref().and_then(api_profile_to_server_api_ref))
            .or_else(|| {
                config
                    .server
                    .as_ref()
                    .and_then(server_profile_to_server_api_ref)
            });
        let server_api_fields = vec![
            ConfigFieldState {
                label: "Base URL",
                value: server_api_source
                    .as_ref()
                    .map(|api| api.base_url.clone())
                    .unwrap_or_default(),
                placeholder: "https://server.example.com/",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Client ID",
                value: server_api_source
                    .as_ref()
                    .map(|api| api.client_id.clone())
                    .unwrap_or_default(),
                placeholder: "client-id",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Client Secret",
                value: server_api_source
                    .as_ref()
                    .map(|api| api.client_secret.clone())
                    .unwrap_or_default(),
                placeholder: "stored in keyring on save",
                kind: ConfigFieldKind::Text,
                secret: true,
            },
        ];

        let mongo = &config.mongo;
        let (
            embedded_runtime,
            embedded_service,
            embedded_restore,
            managed_url,
            managed_host,
            managed_port,
            managed_auth_db,
            managed_username,
            managed_password,
            managed_tls_enabled,
            managed_tls_ca,
            managed_tls_cert,
            managed_tls_key,
            managed_tls_invalid_hostnames,
            managed_timeout,
            managed_retry,
            managed_pool,
        ) = mongo_values(mongo);
        let mongo_fields = vec![
            ConfigFieldState {
                label: "Mode",
                value: match mongo.mode {
                    ayx_core::profile::MongoMode::Embedded => "embedded",
                    ayx_core::profile::MongoMode::Managed => "managed",
                }
                .to_string(),
                placeholder: "embedded|managed",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Gallery DB",
                value: mongo.databases.gallery_name.clone(),
                placeholder: "AlteryxGallery",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Service DB",
                value: mongo.databases.service_name.clone(),
                placeholder: "AlteryxService",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Embedded RuntimeSettings",
                value: embedded_runtime,
                placeholder: "/path/to/RuntimeSettings.xml",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Embedded Service Path",
                value: embedded_service,
                placeholder: "/path/to/AlteryxService.exe",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Embedded Restore Path",
                value: embedded_restore,
                placeholder: "/path/to/restore",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Managed URL",
                value: managed_url,
                placeholder: "mongodb://...",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Managed Host",
                value: managed_host,
                placeholder: "mongo.example.com",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Managed Port",
                value: managed_port,
                placeholder: "27017",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Auth DB",
                value: managed_auth_db,
                placeholder: "admin",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Managed Username",
                value: managed_username,
                placeholder: "username",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Managed Password",
                value: managed_password,
                placeholder: "stored in keyring on save",
                kind: ConfigFieldKind::Text,
                secret: true,
            },
            ConfigFieldState {
                label: "TLS Enabled",
                value: managed_tls_enabled,
                placeholder: "true|false",
                kind: ConfigFieldKind::Bool,
                secret: false,
            },
            ConfigFieldState {
                label: "TLS CA Path",
                value: managed_tls_ca,
                placeholder: "/path/to/ca.pem",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "TLS Cert Path",
                value: managed_tls_cert,
                placeholder: "/path/to/cert.pem",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "TLS Key Path",
                value: managed_tls_key,
                placeholder: "/path/to/key.pem",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Allow Invalid Hostnames",
                value: managed_tls_invalid_hostnames,
                placeholder: "true|false",
                kind: ConfigFieldKind::Bool,
                secret: false,
            },
            ConfigFieldState {
                label: "Timeout ms",
                value: managed_timeout,
                placeholder: "5000",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Retry Count",
                value: managed_retry,
                placeholder: "3",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
            ConfigFieldState {
                label: "Pool Size",
                value: managed_pool,
                placeholder: "10",
                kind: ConfigFieldKind::Text,
                secret: false,
            },
        ];

        let sqlserver_fields = sqlserver_fields(config);
        let observability_fields = observability_fields(config);

        Self {
            section: ConfigSection::Overview,
            overview_fields,
            server_api_fields,
            mongo_fields,
            sqlserver_fields,
            observability_fields,
            cursor: 0,
            editing: false,
            edit_buffer: String::new(),
            dirty: false,
        }
    }

    pub fn active_section(&self) -> ConfigSection {
        self.section
    }

    pub fn active_field(&self) -> &ConfigFieldState {
        &self.active_fields()[self.cursor]
    }

    pub fn active_field_mut(&mut self) -> &mut ConfigFieldState {
        let cursor = self.cursor;
        &mut self.active_fields_mut()[cursor]
    }

    pub fn display_value(&self, index: usize) -> String {
        self.active_fields()[index].visible_value()
    }

    pub fn active_fields(&self) -> &[ConfigFieldState] {
        match self.section {
            ConfigSection::Overview => &self.overview_fields,
            ConfigSection::ServerApi => &self.server_api_fields,
            ConfigSection::Mongo => &self.mongo_fields,
            ConfigSection::SqlServer => &self.sqlserver_fields,
            ConfigSection::Observability => &self.observability_fields,
        }
    }

    pub fn active_fields_mut(&mut self) -> &mut Vec<ConfigFieldState> {
        match self.section {
            ConfigSection::Overview => &mut self.overview_fields,
            ConfigSection::ServerApi => &mut self.server_api_fields,
            ConfigSection::Mongo => &mut self.mongo_fields,
            ConfigSection::SqlServer => &mut self.sqlserver_fields,
            ConfigSection::Observability => &mut self.observability_fields,
        }
    }

    pub fn begin_edit(&mut self) {
        self.editing = true;
        self.edit_buffer = self.active_field().value.clone();
    }

    pub fn cancel_edit(&mut self) {
        self.editing = false;
        self.edit_buffer.clear();
    }

    pub fn commit_edit(&mut self) {
        let next = self.edit_buffer.trim().to_string();
        self.active_field_mut().value = next;
        self.dirty = true;
        self.editing = false;
        self.edit_buffer.clear();
    }

    pub fn clear_active(&mut self) {
        self.active_field_mut().value.clear();
        self.dirty = true;
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let len = self.active_fields().len() as isize;
        let next = (self.cursor as isize + delta).clamp(0, len - 1);
        self.cursor = next as usize;
    }

    pub fn move_section(&mut self, delta: isize) {
        if delta > 0 {
            for _ in 0..delta as usize {
                self.section = self.section.next();
            }
        } else if delta < 0 {
            for _ in 0..(-delta) as usize {
                self.section = self.section.prev();
            }
        }
        self.cursor = 0;
    }
}

#[derive(Debug, Clone)]
pub struct CredentialsForm {
    pub fields: Vec<FieldState>,
    pub cursor: usize,
    pub editing: bool,
    pub edit_buffer: String,
    pub dirty: bool,
}

impl CredentialsForm {
    pub fn from_config(config: &Config) -> Self {
        let one = config.alteryx_one.as_ref();
        Self {
            fields: vec![
                FieldState {
                    label: "Alteryx One Account Email",
                    value: one.map(|v| v.account_email.clone()).unwrap_or_default(),
                    placeholder: "operator@example.com",
                    secret: false,
                },
                FieldState {
                    label: "Alteryx One Base URL",
                    value: one
                        .and_then(|v| v.normalized_base_url())
                        .unwrap_or_default(),
                    placeholder: "https://us1.alteryxcloud.com",
                    secret: false,
                },
                FieldState {
                    label: "OAuth Client ID",
                    value: one
                        .and_then(|v| v.oauth_client_id.clone())
                        .unwrap_or_default(),
                    placeholder: "client-id",
                    secret: false,
                },
                FieldState {
                    label: "Alteryx One Access Token",
                    value: one.and_then(|v| v.access_token.clone()).unwrap_or_default(),
                    placeholder: "stored in keyring on save",
                    secret: true,
                },
                FieldState {
                    label: "Alteryx One Refresh Token",
                    value: one
                        .and_then(|v| v.refresh_token.clone())
                        .unwrap_or_default(),
                    placeholder: "stored in keyring on save",
                    secret: true,
                },
            ],
            cursor: 0,
            editing: false,
            edit_buffer: String::new(),
            dirty: false,
        }
    }

    pub fn active_field(&self) -> &FieldState {
        &self.fields[self.cursor]
    }

    pub fn active_field_mut(&mut self) -> &mut FieldState {
        &mut self.fields[self.cursor]
    }

    pub fn visible_value(&self, index: usize) -> String {
        let field = &self.fields[index];
        if field.secret {
            if field.value.trim().is_empty() {
                String::new()
            } else {
                "stored".to_string()
            }
        } else {
            field.value.clone()
        }
    }

    pub fn begin_edit(&mut self) {
        self.editing = true;
        self.edit_buffer = self.active_field().value.clone();
    }

    pub fn cancel_edit(&mut self) {
        self.editing = false;
        self.edit_buffer.clear();
    }

    pub fn commit_edit(&mut self) {
        let next = self.edit_buffer.trim().to_string();
        self.active_field_mut().value = next;
        self.dirty = true;
        self.editing = false;
        self.edit_buffer.clear();
    }

    pub fn clear_active(&mut self) {
        self.active_field_mut().value.clear();
        self.dirty = true;
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let len = self.fields.len() as isize;
        let next = (self.cursor as isize + delta).clamp(0, len - 1);
        self.cursor = next as usize;
    }
}

#[derive(Debug, Clone)]
pub struct PanelState {
    pub title: String,
    pub lines: Vec<String>,
    pub is_error: bool,
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneBrowserResource {
    AuthStatus,
    AuthDiagnose,
    SurfaceInventory,
    WorkspaceCurrent,
    WorkspaceCurrentConfiguration,
    WorkspaceCurrentConfigurationSchema,
    WorkspaceList,
    WorkspaceDetail,
    FlowList,
    FlowDetail,
    ConnectionList,
    ConnectionDetail,
}

impl OneBrowserResource {
    pub fn all() -> [OneBrowserResource; 12] {
        [
            OneBrowserResource::AuthStatus,
            OneBrowserResource::AuthDiagnose,
            OneBrowserResource::SurfaceInventory,
            OneBrowserResource::WorkspaceCurrent,
            OneBrowserResource::WorkspaceCurrentConfiguration,
            OneBrowserResource::WorkspaceCurrentConfigurationSchema,
            OneBrowserResource::WorkspaceList,
            OneBrowserResource::WorkspaceDetail,
            OneBrowserResource::FlowList,
            OneBrowserResource::FlowDetail,
            OneBrowserResource::ConnectionList,
            OneBrowserResource::ConnectionDetail,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            OneBrowserResource::AuthStatus => "Auth Status",
            OneBrowserResource::AuthDiagnose => "Auth Diagnose",
            OneBrowserResource::SurfaceInventory => "Surface Inventory",
            OneBrowserResource::WorkspaceCurrent => "Workspace Current",
            OneBrowserResource::WorkspaceCurrentConfiguration => "Workspace Config",
            OneBrowserResource::WorkspaceCurrentConfigurationSchema => "Workspace Schema",
            OneBrowserResource::WorkspaceList => "Workspace List",
            OneBrowserResource::WorkspaceDetail => "Workspace Detail",
            OneBrowserResource::FlowList => "Flows List",
            OneBrowserResource::FlowDetail => "Flow Detail",
            OneBrowserResource::ConnectionList => "Connections List",
            OneBrowserResource::ConnectionDetail => "Connection Detail",
        }
    }

    pub fn needs_id(self) -> bool {
        matches!(
            self,
            OneBrowserResource::WorkspaceDetail
                | OneBrowserResource::FlowDetail
                | OneBrowserResource::ConnectionDetail
        )
    }

    pub fn list_drilldown(self) -> Option<OneBrowserResource> {
        match self {
            OneBrowserResource::WorkspaceList => Some(OneBrowserResource::WorkspaceDetail),
            OneBrowserResource::FlowList => Some(OneBrowserResource::FlowDetail),
            OneBrowserResource::ConnectionList => Some(OneBrowserResource::ConnectionDetail),
            _ => None,
        }
    }

    pub fn accepts_selected_item(self) -> bool {
        matches!(
            self,
            OneBrowserResource::WorkspaceList
                | OneBrowserResource::FlowList
                | OneBrowserResource::ConnectionList
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConnectivityState {
    pub panels: Vec<PanelState>,
    pub last_run: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct OneBrowserState {
    pub cursor: usize,
    pub item_cursor: usize,
    pub pane: OneBrowserPane,
    pub panels: Vec<PanelState>,
    pub last_run: Option<String>,
    pub prompt: Option<OneBrowserPrompt>,
    pub history: Vec<OneBrowserResource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneBrowserPane {
    Resources,
    Items,
}

impl Default for OneBrowserPane {
    fn default() -> Self {
        Self::Resources
    }
}

#[derive(Debug, Clone)]
pub struct OneBrowserPrompt {
    pub resource: OneBrowserResource,
    pub buffer: String,
}

#[derive(Debug, Clone)]
pub struct OneBrowserItem {
    pub id: Option<String>,
    pub label: String,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct ToastState {
    pub message: String,
    pub is_error: bool,
    pub expires_at: Instant,
}

pub struct App {
    pub config_home: PathBuf,
    pub screen: Screen,
    pub focus: Focus,
    pub should_quit: bool,
    pub sidebar: ListState,
    pub profiles_state: ListState,
    pub workspaces_state: ListState,
    pub environments_state: ListState,
    pub profiles_pane: ProfilesPane,
    pub profiles: Vec<ProfileRecord>,
    pub profile_view: ProfileView,
    pub workspaces: Vec<WorkspaceEntry>,
    pub active_profile: Option<String>,
    pub active_workspace: Option<String>,
    pub target_kind: TargetKind,
    pub target_path: PathBuf,
    pub target_environment: Option<String>,
    pub resolution_source: String,
    /// (screen, focus) captured when entering Inspect so closing the modal
    /// restores both. Carrying focus prevents `Focus::Content` from leaking
    /// onto a destination screen that doesn't have a meaningful content pane.
    pub inspect_return: Option<(Screen, Focus)>,
    pub current_config: Config,
    pub config_form: ConfigForm,
    pub credentials: CredentialsForm,
    pub connectivity: ConnectivityState,
    pub one_browser: OneBrowserState,
    pub status_message: String,
    pub toast: Option<ToastState>,
    pub crud_prompt: Option<CrudPrompt>,
    // Background worker for off-UI-thread network/disk work. Held in an
    // Option so headless smoke tests (if any) can construct an App without
    // spawning a real thread.
    pub(super) worker: Option<super::worker::BackgroundWorker>,
    pub(super) latest_connectivity_request: Option<super::worker::RequestId>,
    pub(super) latest_one_browser_request: Option<super::worker::RequestId>,
}

impl App {
    pub(crate) fn visible_profiles(&self) -> Vec<&ProfileRecord> {
        self.profiles
            .iter()
            .filter(|record| match self.profile_view {
                ProfileView::All => true,
                ProfileView::One => {
                    matches!(record.scope, ProfileScope::One | ProfileScope::Combined)
                }
                ProfileView::Server => {
                    matches!(record.scope, ProfileScope::Server | ProfileScope::Combined)
                }
            })
            .collect()
    }

    pub(crate) fn visible_profiles_len(&self) -> usize {
        self.visible_profiles().len()
    }

    fn selected_profile_record(&self) -> Option<&ProfileRecord> {
        let target_index = self.profiles_state.selected().unwrap_or(0);
        let mut visible_index = 0usize;
        for record in &self.profiles {
            let matches_view = match self.profile_view {
                ProfileView::All => true,
                ProfileView::One => {
                    matches!(record.scope, ProfileScope::One | ProfileScope::Combined)
                }
                ProfileView::Server => {
                    matches!(record.scope, ProfileScope::Server | ProfileScope::Combined)
                }
            };
            if !matches_view {
                continue;
            }
            if visible_index == target_index {
                return Some(record);
            }
            visible_index += 1;
        }
        None
    }

    pub(crate) fn selected_profile_scope(&self) -> Option<ProfileScope> {
        self.selected_profile_record().map(|record| record.scope)
    }

    pub fn new() -> Result<Self> {
        Self::new_with_runtime(true, true)
    }

    #[cfg(test)]
    pub(crate) fn new_without_worker() -> Result<Self> {
        Self::new_with_runtime(false, false)
    }

    fn new_with_runtime(spawn_worker: bool, prime_refreshes: bool) -> Result<Self> {
        let state = load_ayx_state().map_err(anyhow::Error::from)?;
        let config_home = ayx_config_home().map_err(anyhow::Error::from)?;
        let profiles = list_profile_records_at(&config_home)?;
        let workspaces = load_workspace_entries()?;
        let target_path = default_profile_storage_path().map_err(anyhow::Error::from)?;
        let current_config = Config::load_from_path_lenient_without_active_overlay(&target_path)
            .map_err(anyhow::Error::from)?;
        let runtime_resolution = resolve_runtime_profile(None).ok();

        let mut sidebar = ListState::default();
        sidebar.select(Some(Screen::Profiles.index()));

        let mut profiles_state = ListState::default();
        profiles_state.select(Some(0));

        let mut workspaces_state = ListState::default();
        workspaces_state.select(Some(0));

        let mut environments_state = ListState::default();
        environments_state.select(Some(0));

        let mut app = Self {
            config_home,
            screen: Screen::Profiles,
            focus: Focus::Sidebar,
            should_quit: false,
            sidebar,
            profiles_state,
            workspaces_state,
            environments_state,
            profiles_pane: ProfilesPane::Profiles,
            profile_view: ProfileView::All,
            profiles,
            workspaces,
            active_profile: state.active_profile,
            active_workspace: state.active_workspace,
            target_kind: TargetKind::Profile,
            target_path,
            target_environment: None,
            resolution_source: runtime_resolution
                .as_ref()
                .map(|resolution| resolution.selection_source.clone())
                .unwrap_or_else(|| "runtime-unavailable".to_string()),
            inspect_return: None,
            config_form: ConfigForm::from_config(&current_config),
            credentials: CredentialsForm::from_config(&current_config),
            current_config,
            connectivity: ConnectivityState::default(),
            one_browser: OneBrowserState::default(),
            status_message: "Ready".to_string(),
            toast: None,
            crud_prompt: None,
            worker: if spawn_worker {
                Some(super::worker::BackgroundWorker::spawn())
            } else {
                None
            },
            latest_connectivity_request: None,
            latest_one_browser_request: None,
        };
        app.sync_selected_entries();
        if prime_refreshes {
            app.refresh_connectivity();
            app.refresh_one_browser();
        }
        Ok(app)
    }

    pub fn tick(&mut self) {
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| Instant::now() >= toast.expires_at)
        {
            self.toast = None;
        }
        self.drain_worker_results();
    }

    /// Pull every available result off the worker channel and apply it.
    /// Results stamped with stale request ids (older than the latest issued
    /// for that lane) are dropped — the user moved on.
    fn drain_worker_results(&mut self) {
        // Collect available results into a local vec first, then process —
        // this releases the immutable borrow on `self.worker` before we
        // mutate `self` to apply each result.
        let mut batch: Vec<super::worker::TaskResult> = Vec::new();
        let mut disconnected = false;
        if let Some(worker) = self.worker.as_ref() {
            loop {
                match worker.try_recv() {
                    Ok(result) => batch.push(result),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if disconnected {
            self.worker = None;
        }
        for result in batch {
            match result {
                super::worker::TaskResult::Connectivity { id, panels } => {
                    if self.latest_connectivity_request == Some(id) {
                        self.connectivity.panels = panels;
                        self.connectivity.last_run =
                            Some(format!("{:?}", std::time::SystemTime::now()));
                        self.status_message = "Connectivity checks refreshed".to_string();
                    }
                }
                super::worker::TaskResult::OneBrowser {
                    id,
                    resource,
                    resource_id: _,
                    result,
                } => {
                    if self.latest_one_browser_request == Some(id) {
                        let typed = result.map_err(|e| anyhow!(e));
                        self.set_one_browser_panel(resource, typed);
                    }
                }
            }
        }
    }

    pub fn is_workspace_target(&self) -> bool {
        self.target_kind == TargetKind::Workspace
    }

    pub fn current_target_label(&self) -> String {
        match self.target_kind {
            TargetKind::Profile => self
                .target_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("profile")
                .to_string(),
            TargetKind::Workspace => {
                let env = self.target_environment.as_deref().unwrap_or("default");
                format!(
                    "{} ({env})",
                    self.target_path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("workspace")
                )
            }
        }
    }

    pub fn current_validation(&self) -> Value {
        summarize_onboarding_validation(&self.current_config)
    }

    pub fn current_summary(&self) -> Value {
        summarize_config(&self.current_config)
    }

    pub fn config_storage_target_label(&self) -> String {
        match self.target_kind {
            TargetKind::Profile => {
                let target = self
                    .target_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("profile");
                format!("profile {target}")
            }
            TargetKind::Workspace => {
                let workspace = self
                    .selected_workspace()
                    .map(|entry| entry.name.clone())
                    .unwrap_or_else(|| "workspace".to_string());
                let env = self.target_environment.as_deref().unwrap_or("active");
                format!("workspace {workspace} env {env}")
            }
        }
    }

    pub fn credentials_storage_target_label(&self) -> String {
        if matches!(self.target_kind, TargetKind::Workspace)
            && let Some(profile_name) = self.active_profile.as_deref()
        {
            return format!("active profile {profile_name}");
        }
        let target = self
            .target_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("profile");
        format!("profile {target}")
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.crud_prompt.is_some() {
            self.handle_crud_prompt_key(key);
            return;
        }

        if self.config_form.editing {
            self.handle_config_edit_key(key);
            return;
        }

        if self.credentials.editing {
            self.handle_credentials_edit_key(key);
            return;
        }

        if key.code == KeyCode::Esc {
            if self.screen == Screen::Inspect {
                self.close_inspect();
            } else if self.focus == Focus::Content {
                self.focus = Focus::Sidebar;
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab => self.handle_tab(),
            KeyCode::BackTab => self.handle_backtab(),
            KeyCode::Char('1') => self.select_screen(Screen::Profiles),
            KeyCode::Char('2') => self.select_screen(Screen::Credentials),
            KeyCode::Char('3') => self.select_screen(Screen::Config),
            KeyCode::Char('4') => self.select_screen(Screen::Connectivity),
            KeyCode::Char('5') => self.select_screen(Screen::One),
            KeyCode::Char('6') => self.select_screen(Screen::Help),
            _ => match self.focus {
                Focus::Sidebar => self.handle_sidebar_key(key),
                Focus::Content => self.handle_content_key(key),
            },
        }
    }

    fn handle_credentials_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.credentials.cancel_edit(),
            KeyCode::Enter | KeyCode::Char('\r') | KeyCode::Char('\n') => {
                self.credentials.commit_edit()
            }
            KeyCode::Backspace => {
                self.credentials.edit_buffer.pop();
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.credentials.edit_buffer.push(ch);
                }
            }
            KeyCode::Delete => self.credentials.edit_buffer.clear(),
            _ => {}
        }
    }

    fn handle_config_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.config_form.cancel_edit(),
            KeyCode::Enter | KeyCode::Char('\r') | KeyCode::Char('\n') => {
                self.config_form.commit_edit()
            }
            KeyCode::Backspace => {
                self.config_form.edit_buffer.pop();
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.config_form.edit_buffer.push(ch);
                }
            }
            KeyCode::Delete => self.config_form.edit_buffer.clear(),
            _ => {}
        }
    }

    fn handle_crud_prompt_key(&mut self, key: KeyEvent) {
        let Some(prompt) = self.crud_prompt.clone() else {
            return;
        };
        match prompt {
            CrudPrompt::Text {
                title,
                message,
                mut buffer,
                action,
            } => match key.code {
                KeyCode::Esc => self.crud_prompt = None,
                KeyCode::Enter | KeyCode::Char('\r') | KeyCode::Char('\n') => {
                    let value = buffer.trim().to_string();
                    if value.is_empty() {
                        self.push_toast("name must not be empty".to_string(), true);
                        return;
                    }
                    self.crud_prompt = None;
                    if let Err(err) = self.apply_profile_crud_action(action, Some(value)) {
                        self.push_toast(err.to_string(), true);
                    }
                }
                KeyCode::Backspace => {
                    buffer.pop();
                    self.crud_prompt = Some(CrudPrompt::Text {
                        title,
                        message,
                        buffer,
                        action,
                    });
                }
                KeyCode::Delete => {
                    buffer.clear();
                    self.crud_prompt = Some(CrudPrompt::Text {
                        title,
                        message,
                        buffer,
                        action,
                    });
                }
                KeyCode::Char(ch) => {
                    if !key.modifiers.contains(KeyModifiers::CONTROL) {
                        buffer.push(ch);
                        self.crud_prompt = Some(CrudPrompt::Text {
                            title,
                            message,
                            buffer,
                            action,
                        });
                    }
                }
                _ => {}
            },
            CrudPrompt::Confirm {
                title: _,
                message: _,
                action,
            } => match key.code {
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.crud_prompt = None;
                }
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.crud_prompt = None;
                    if let Err(err) = self.apply_profile_crud_action(action, None) {
                        self.push_toast(err.to_string(), true);
                    }
                }
                _ => {}
            },
        }
    }

    fn apply_profile_crud_action(
        &mut self,
        action: ProfileCrudAction,
        value: Option<String>,
    ) -> Result<()> {
        let config_home = self.config_home.clone();
        match action {
            ProfileCrudAction::CreateDefault => {
                let name = value.ok_or_else(|| anyhow!("profile name is required"))?;
                let path = create_profile_from_default_scope_at(
                    &config_home,
                    &name,
                    self.selected_profile_scope().unwrap_or(ProfileScope::One),
                )?;
                let mut state = load_ayx_state().map_err(anyhow::Error::from)?;
                state.active_profile = Some(name.clone());
                save_ayx_state(&state).map_err(anyhow::Error::from)?;
                self.active_profile = Some(name.clone());
                let _ = self.reload_indexes();
                self.load_target(path, None, TargetKind::Profile)?;
                self.status_message = format!("Created profile {name}");
            }
            ProfileCrudAction::DuplicateSelected => {
                let Some(source_name) = self.selected_profile_name().cloned() else {
                    return Err(anyhow!("no profile selected"));
                };
                let new_name = value.ok_or_else(|| anyhow!("profile name is required"))?;
                let path = duplicate_profile_at(&config_home, &source_name, &new_name)?;
                let mut state = load_ayx_state().map_err(anyhow::Error::from)?;
                state.active_profile = Some(new_name.clone());
                save_ayx_state(&state).map_err(anyhow::Error::from)?;
                self.active_profile = Some(new_name.clone());
                let _ = self.reload_indexes();
                self.load_target(path, None, TargetKind::Profile)?;
                self.status_message = format!("Duplicated profile {source_name} to {new_name}");
            }
            ProfileCrudAction::RenameSelected => {
                let Some(source_name) = self.selected_profile_name().cloned() else {
                    return Err(anyhow!("no profile selected"));
                };
                let new_name = value.ok_or_else(|| anyhow!("profile name is required"))?;
                let path = rename_profile_at(&config_home, &source_name, &new_name)?;
                let mut state = load_ayx_state().map_err(anyhow::Error::from)?;
                if state.active_profile.as_deref() == Some(source_name.as_str()) {
                    state.active_profile = Some(new_name.clone());
                    save_ayx_state(&state).map_err(anyhow::Error::from)?;
                    self.active_profile = Some(new_name.clone());
                    self.load_target(path, None, TargetKind::Profile)?;
                } else {
                    save_ayx_state(&state).map_err(anyhow::Error::from)?;
                    let _ = self.reload_indexes();
                }
                let _ = self.reload_indexes();
                self.status_message = format!("Renamed profile {source_name} to {new_name}");
            }
            ProfileCrudAction::DeleteSelected => {
                let Some(source_name) = self.selected_profile_name().cloned() else {
                    return Err(anyhow!("no profile selected"));
                };
                let was_active = self.active_profile.as_deref() == Some(source_name.as_str());
                let _deleted = delete_profile_at(&config_home, &source_name)?;
                let remaining = list_profile_records_at(&config_home)?
                    .into_iter()
                    .filter(|record| match self.profile_view {
                        ProfileView::All => true,
                        ProfileView::One => {
                            matches!(record.scope, ProfileScope::One | ProfileScope::Combined)
                        }
                        ProfileView::Server => {
                            matches!(record.scope, ProfileScope::Server | ProfileScope::Combined)
                        }
                    })
                    .map(|record| record.name)
                    .collect::<Vec<_>>();
                let mut state = load_ayx_state().map_err(anyhow::Error::from)?;
                if was_active {
                    state.active_profile = remaining.first().cloned();
                    save_ayx_state(&state).map_err(anyhow::Error::from)?;
                    self.active_profile = state.active_profile.clone();
                    if let Some(next_name) = self.active_profile.as_deref() {
                        let path = profile_storage_path(next_name).map_err(anyhow::Error::from)?;
                        self.load_target(path, None, TargetKind::Profile)?;
                    } else {
                        self.current_config = default_config();
                        self.target_kind = TargetKind::Profile;
                        self.target_environment = None;
                        self.target_path =
                            default_profile_storage_path().map_err(anyhow::Error::from)?;
                        self.config_form = ConfigForm::from_config(&self.current_config);
                        self.credentials = CredentialsForm::from_config(&self.current_config);
                    }
                } else {
                    save_ayx_state(&state).map_err(anyhow::Error::from)?;
                }
                let _ = self.reload_indexes();
                self.status_message = format!("Deleted profile {source_name}");
            }
        }
        Ok(())
    }

    fn handle_tab(&mut self) {
        match self.focus {
            Focus::Sidebar => self.focus = Focus::Content,
            Focus::Content => {
                if self.screen == Screen::Profiles {
                    self.profiles_pane = self.profiles_pane.next();
                } else if self.screen == Screen::Config {
                    self.config_form.move_section(1);
                } else {
                    self.focus = Focus::Sidebar;
                }
            }
        }
    }

    fn handle_backtab(&mut self) {
        match self.focus {
            Focus::Content if self.screen == Screen::Profiles => {
                self.profiles_pane = match self.profiles_pane {
                    ProfilesPane::Profiles => ProfilesPane::Environments,
                    ProfilesPane::Workspaces => ProfilesPane::Profiles,
                    ProfilesPane::Environments => ProfilesPane::Workspaces,
                };
            }
            Focus::Content if self.screen == Screen::Config => {
                self.config_form.move_section(-1);
            }
            Focus::Content => self.focus = Focus::Sidebar,
            Focus::Sidebar => self.focus = Focus::Content,
        }
    }

    fn handle_sidebar_key(&mut self, key: KeyEvent) {
        let current = self.sidebar.selected().unwrap_or(0) as isize;
        let max = Screen::all().len() as isize - 1;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let next = (current - 1).clamp(0, max) as usize;
                self.sidebar.select(Some(next));
                self.screen = Screen::from_index(next);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let next = (current + 1).clamp(0, max) as usize;
                self.sidebar.select(Some(next));
                self.screen = Screen::from_index(next);
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.focus = Focus::Content,
            _ => {}
        }
    }

    fn handle_content_key(&mut self, key: KeyEvent) {
        match self.screen {
            Screen::Profiles => self.handle_profiles_key(key),
            Screen::Config => self.handle_config_key(key),
            Screen::Credentials => self.handle_credentials_key(key),
            Screen::Connectivity => self.handle_connectivity_key(key),
            Screen::Inspect => self.handle_inspect_key(key),
            Screen::One => self.handle_one_browser_key(key),
            Screen::Help => {
                if matches!(key.code, KeyCode::Left | KeyCode::Char('h')) {
                    self.focus = Focus::Sidebar;
                }
            }
        }
    }

    fn handle_profiles_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Sidebar,
            KeyCode::Char('o') => {
                self.profile_view = ProfileView::One;
                self.profiles_state.select(Some(0));
                self.sync_selected_entries();
            }
            KeyCode::Char('s') => {
                self.profile_view = ProfileView::Server;
                self.profiles_state.select(Some(0));
                self.sync_selected_entries();
            }
            KeyCode::Char('a') => {
                self.profile_view = ProfileView::All;
                self.profiles_state.select(Some(0));
                self.sync_selected_entries();
            }
            KeyCode::Char('r') => {
                if let Err(err) = self.reload_indexes() {
                    self.push_toast(err.to_string(), true);
                }
            }
            KeyCode::Char('n') if self.profiles_pane == ProfilesPane::Profiles => {
                let default_name = "new-profile".to_string();
                self.crud_prompt = Some(CrudPrompt::Text {
                    title: "Create Profile".to_string(),
                    message: format!(
                        "Create a new {} profile from defaults:",
                        self.profile_view.label().to_lowercase()
                    ),
                    buffer: default_name,
                    action: ProfileCrudAction::CreateDefault,
                });
            }
            KeyCode::Char('d') if self.profiles_pane == ProfilesPane::Profiles => {
                if let Some(name) = self.selected_profile_name().cloned() {
                    self.crud_prompt = Some(CrudPrompt::Text {
                        title: "Duplicate Profile".to_string(),
                        message: format!("Duplicate '{name}' as:"),
                        buffer: format!("{name}-copy"),
                        action: ProfileCrudAction::DuplicateSelected,
                    });
                }
            }
            KeyCode::Char('R') if self.profiles_pane == ProfilesPane::Profiles => {
                if let Some(name) = self.selected_profile_name().cloned() {
                    self.crud_prompt = Some(CrudPrompt::Text {
                        title: "Rename Profile".to_string(),
                        message: format!("Rename '{name}' to:"),
                        buffer: name,
                        action: ProfileCrudAction::RenameSelected,
                    });
                }
            }
            KeyCode::Char('x') if self.profiles_pane == ProfilesPane::Profiles => {
                if let Some(name) = self.selected_profile_name().cloned() {
                    self.crud_prompt = Some(CrudPrompt::Confirm {
                        title: "Delete Profile".to_string(),
                        message: format!("Delete profile '{name}'?"),
                        action: ProfileCrudAction::DeleteSelected,
                    });
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_profiles_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_profiles_cursor(1),
            KeyCode::Enter | KeyCode::Char('u') => {
                if let Err(err) = self.activate_profiles_selection() {
                    self.push_toast(err.to_string(), true);
                }
            }
            KeyCode::Char('i') => self.select_screen(Screen::Inspect),
            _ => {}
        }
    }

    fn handle_config_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Sidebar,
            KeyCode::Up | KeyCode::Char('k') => self.config_form.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.config_form.move_cursor(1),
            KeyCode::Enter | KeyCode::Char('e') => {
                self.config_form.begin_edit();
            }
            KeyCode::Char('c') => {
                self.config_form.clear_active();
            }
            KeyCode::Char('r') => {
                if let Err(err) = self.reload_target() {
                    self.push_toast(err.to_string(), true);
                }
            }
            KeyCode::Char('s') => {
                if let Err(err) = self.save_config() {
                    self.push_toast(err.to_string(), true);
                }
            }
            _ => {}
        }
    }

    fn handle_credentials_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Sidebar,
            KeyCode::Up | KeyCode::Char('k') => self.credentials.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.credentials.move_cursor(1),
            KeyCode::Enter | KeyCode::Char('e') => {
                self.credentials.begin_edit();
            }
            KeyCode::Char('c') => {
                self.credentials.clear_active();
            }
            KeyCode::Char('r') => {
                if let Err(err) = self.reload_target() {
                    self.push_toast(err.to_string(), true);
                }
            }
            KeyCode::Char('s') => {
                if let Err(err) = self.save_credentials() {
                    self.push_toast(err.to_string(), true);
                }
            }
            _ => {}
        }
    }

    fn handle_connectivity_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Sidebar,
            KeyCode::Char('r') | KeyCode::Char('t') | KeyCode::Enter => self.refresh_connectivity(),
            _ => {}
        }
    }

    fn handle_inspect_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc
            | KeyCode::Left
            | KeyCode::Char('h')
            | KeyCode::Enter
            | KeyCode::Char('i') => self.close_inspect(),
            KeyCode::Char('r') => {
                if let Err(err) = self.reload_indexes() {
                    self.push_toast(err.to_string(), true);
                }
            }
            _ => {}
        }
    }

    fn handle_one_browser_key(&mut self, key: KeyEvent) {
        if let Some(prompt) = self.one_browser.prompt.as_mut() {
            match key.code {
                KeyCode::Esc => self.one_browser.prompt = None,
                KeyCode::Enter | KeyCode::Char('\r') | KeyCode::Char('\n') => {
                    let resource = prompt.resource;
                    let id = prompt.buffer.trim().to_string();
                    self.one_browser.prompt = None;
                    if id.is_empty() {
                        self.push_toast("an id is required".to_string(), true);
                        return;
                    }
                    if let Err(err) = self.refresh_one_browser_detail(resource, Some(id)) {
                        self.push_toast(err.to_string(), true);
                    }
                }
                KeyCode::Backspace => {
                    prompt.buffer.pop();
                }
                KeyCode::Char(ch) => {
                    if !key.modifiers.contains(KeyModifiers::CONTROL) {
                        prompt.buffer.push(ch);
                    }
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Sidebar,
            KeyCode::Backspace | KeyCode::Char('b') => {
                if let Err(err) = self.go_back_one_browser() {
                    self.push_toast(err.to_string(), true);
                }
            }
            KeyCode::Tab => {
                self.one_browser.pane = match self.one_browser.pane {
                    OneBrowserPane::Resources => OneBrowserPane::Items,
                    OneBrowserPane::Items => OneBrowserPane::Resources,
                }
            }
            KeyCode::Up | KeyCode::Char('k') => match self.one_browser.pane {
                OneBrowserPane::Resources => self.move_one_browser_cursor(-1),
                OneBrowserPane::Items => self.move_one_browser_item_cursor(-1),
            },
            KeyCode::Down | KeyCode::Char('j') => match self.one_browser.pane {
                OneBrowserPane::Resources => self.move_one_browser_cursor(1),
                OneBrowserPane::Items => self.move_one_browser_item_cursor(1),
            },
            KeyCode::Char('r') => {
                self.refresh_one_browser();
            }
            KeyCode::Enter => match self.one_browser.pane {
                OneBrowserPane::Resources => {
                    let resource = OneBrowserResource::all()[self.one_browser.cursor];
                    if resource.accepts_selected_item() {
                        if let Err(err) = self.open_selected_one_browser_item() {
                            self.push_toast(err.to_string(), true);
                        }
                    } else if resource.needs_id() {
                        self.one_browser.prompt = Some(OneBrowserPrompt {
                            resource,
                            buffer: String::new(),
                        });
                    } else {
                        self.refresh_one_browser();
                    }
                }
                OneBrowserPane::Items => {
                    if let Err(err) = self.open_selected_one_browser_item() {
                        self.push_toast(err.to_string(), true);
                    }
                }
            },
            _ => {}
        }
    }

    fn select_screen(&mut self, screen: Screen) {
        if screen == Screen::Inspect && self.screen != Screen::Inspect {
            // Capture both the underlying screen and the focus so close_inspect
            // restores the full state.
            self.inspect_return = Some((self.screen, self.focus));
            self.screen = Screen::Inspect;
            self.focus = Focus::Content;
            return;
        } else if screen != Screen::Inspect {
            self.inspect_return = None;
        }
        self.screen = screen;
        // Inspect is modal — leave the sidebar selection alone for it.
        if let Some(idx) = screen.sidebar_index() {
            self.sidebar.select(Some(idx));
        }
    }

    fn close_inspect(&mut self) {
        if self.screen == Screen::Inspect {
            let (next, focus) = self
                .inspect_return
                .take()
                .unwrap_or((Screen::Profiles, Focus::Sidebar));
            self.screen = next;
            self.focus = focus;
            if let Some(idx) = next.sidebar_index() {
                self.sidebar.select(Some(idx));
            }
        }
    }

    fn move_one_browser_cursor(&mut self, delta: isize) {
        let len = OneBrowserResource::all().len() as isize;
        let next = (self.one_browser.cursor as isize + delta).clamp(0, len - 1);
        self.one_browser.cursor = next as usize;
        self.refresh_one_browser();
    }

    fn move_one_browser_item_cursor(&mut self, delta: isize) {
        let len = self.active_one_browser_items().len() as isize;
        if len == 0 {
            self.one_browser.item_cursor = 0;
            return;
        }
        let next = (self.one_browser.item_cursor as isize + delta).clamp(0, len - 1);
        self.one_browser.item_cursor = next as usize;
    }

    fn go_back_one_browser(&mut self) -> Result<()> {
        if let Some(resource) = self.one_browser.history.pop() {
            self.one_browser.cursor = resource.all_index();
            self.one_browser.pane = OneBrowserPane::Resources;
            self.one_browser.item_cursor = 0;
            self.one_browser.prompt = None;
            self.refresh_one_browser();
            return Ok(());
        }
        self.select_screen(Screen::Inspect);
        Ok(())
    }

    fn open_selected_one_browser_item(&mut self) -> Result<()> {
        let items = self.active_one_browser_items();
        let Some(item) = items.get(self.one_browser.item_cursor).cloned() else {
            return Err(anyhow!("no item selected"));
        };
        let resource = OneBrowserResource::all()[self.one_browser.cursor];
        let Some(next_resource) = resource.list_drilldown() else {
            return Err(anyhow!("selected resource does not support drill-down"));
        };
        let Some(id) = item.id else {
            return Err(anyhow!("selected item does not have an id"));
        };
        self.open_one_browser_resource(next_resource, Some(id), true)
    }

    fn open_one_browser_resource(
        &mut self,
        resource: OneBrowserResource,
        id: Option<String>,
        push_history: bool,
    ) -> Result<()> {
        if push_history {
            let current = OneBrowserResource::all()[self.one_browser.cursor];
            if current != resource {
                self.one_browser.history.push(current);
            }
        }
        self.one_browser.cursor = resource.all_index();
        self.one_browser.pane = OneBrowserPane::Resources;
        self.one_browser.item_cursor = 0;
        self.one_browser.prompt = None;
        let result = self.request_for_one_browser(resource, id.as_deref());
        self.set_one_browser_panel(resource, result);
        Ok(())
    }

    fn move_profiles_cursor(&mut self, delta: isize) {
        match self.profiles_pane {
            ProfilesPane::Profiles => {
                let len = self.visible_profiles_len();
                move_list_state(&mut self.profiles_state, len, delta)
            }
            ProfilesPane::Workspaces => {
                move_list_state(&mut self.workspaces_state, self.workspaces.len(), delta);
                self.sync_workspace_env_cursor();
            }
            ProfilesPane::Environments => {
                let len = self
                    .selected_workspace()
                    .map(|workspace| workspace.environments.len())
                    .unwrap_or(0);
                move_list_state(&mut self.environments_state, len, delta);
            }
        }
    }

    fn activate_profiles_selection(&mut self) -> Result<()> {
        match self.profiles_pane {
            ProfilesPane::Profiles => self.activate_profile(),
            ProfilesPane::Workspaces => self.activate_workspace(),
            ProfilesPane::Environments => self.activate_workspace_environment(),
        }
    }

    fn activate_profile(&mut self) -> Result<()> {
        let Some(name) = self.selected_profile_name().cloned() else {
            return Ok(());
        };
        let mut state = load_ayx_state().map_err(anyhow::Error::from)?;
        state.active_profile = Some(name.clone());
        save_ayx_state(&state).map_err(anyhow::Error::from)?;
        self.active_profile = state.active_profile.clone();
        let path = profile_storage_path(&name).map_err(anyhow::Error::from)?;
        self.load_target(path, None, TargetKind::Profile)?;
        self.status_message = format!("Active profile set to {name}");
        Ok(())
    }

    fn activate_workspace(&mut self) -> Result<()> {
        let Some(workspace) = self.selected_workspace().cloned() else {
            return Ok(());
        };
        let mut state = load_ayx_state().map_err(anyhow::Error::from)?;
        state.active_workspace = Some(workspace.name.clone());
        save_ayx_state(&state).map_err(anyhow::Error::from)?;
        self.active_workspace = state.active_workspace.clone();
        let env_index = self.environments_state.selected().unwrap_or(0);
        let env = workspace
            .environments
            .get(env_index)
            .cloned()
            .unwrap_or_else(|| workspace.active_environment.clone());
        self.load_target(workspace.path.clone(), Some(env), TargetKind::Workspace)?;
        self.status_message = format!("Workspace {} selected", workspace.name);
        Ok(())
    }

    fn activate_workspace_environment(&mut self) -> Result<()> {
        let Some(workspace) = self.selected_workspace().cloned() else {
            return Ok(());
        };
        let env_index = self.environments_state.selected().unwrap_or(0);
        let Some(env) = workspace.environments.get(env_index).cloned() else {
            return Ok(());
        };
        self.load_target(
            workspace.path.clone(),
            Some(env.clone()),
            TargetKind::Workspace,
        )?;
        self.status_message = format!("Workspace {} environment set to {env}", workspace.name);
        Ok(())
    }

    fn load_target(
        &mut self,
        path: PathBuf,
        environment: Option<String>,
        kind: TargetKind,
    ) -> Result<()> {
        let config = match kind {
            TargetKind::Profile => Config::load_from_path_lenient_without_active_overlay(&path)?,
            TargetKind::Workspace => {
                Config::load_from_path_with_environment_lenient(&path, environment.as_deref())?
            }
        };
        self.target_kind = kind;
        self.target_path = path;
        self.target_environment = environment;
        self.resolution_source = profile_resolution_detail(&self.target_path)
            .map(|resolution| resolution.source)
            .unwrap_or_else(|_| "editor".to_string());
        self.current_config = config;
        self.config_form = ConfigForm::from_config(&self.current_config);
        self.credentials = CredentialsForm::from_config(&self.current_config);
        self.connectivity = ConnectivityState::default();
        self.refresh_connectivity();
        self.refresh_one_browser();
        Ok(())
    }

    fn save_config(&mut self) -> Result<()> {
        let mut config = self.current_config.clone();
        config.profile_name = field_value(&self.config_form.overview_fields, "Profile Name")
            .trim()
            .to_string();

        let server_base_url =
            field_value(&self.config_form.overview_fields, "Alteryx Server Base URL")
                .trim()
                .to_string();
        let verify_tls = parse_bool_field(
            field_value(&self.config_form.overview_fields, "Verify TLS"),
            true,
        )?;

        if !server_base_url.is_empty() || config.server.is_some() {
            let mut server = config.server.unwrap_or_else(default_server_profile);
            if !server_base_url.is_empty() {
                server.webapi_url = normalize_server_url(&server_base_url);
            }
            server.verify_tls = Some(verify_tls);
            config.server = Some(server);
        } else {
            config.server = None;
        }

        let server_api_base = field_value(&self.config_form.server_api_fields, "Base URL")
            .trim()
            .to_string();
        let server_api_client_id = field_value(&self.config_form.server_api_fields, "Client ID")
            .trim()
            .to_string();
        let server_api_client_secret =
            field_value(&self.config_form.server_api_fields, "Client Secret")
                .trim()
                .to_string();
        if !server_api_base.is_empty()
            || !server_api_client_id.is_empty()
            || !server_api_client_secret.is_empty()
            || config.api.is_some()
        {
            let mut api = config.api.unwrap_or(ApiProfile {
                base_url: String::new(),
                auth: ApiAuth {
                    mode: ApiAuthMode::Oauth2ClientCredentials,
                    pat: None,
                    client_id: None,
                    client_secret: None,
                    client_secret_ref: None,
                    scope: Some(String::new()),
                },
                timeout_ms: None,
                derived: false,
            });
            if !server_api_base.is_empty() {
                api.base_url = normalize_alteryx_base_url(&server_api_base);
            }
            api.auth.mode = ApiAuthMode::Oauth2ClientCredentials;
            api.auth.client_id = option_string(&server_api_client_id);
            api.auth.client_secret = option_string(&server_api_client_secret);
            api.auth.client_secret_ref = None;
            api.auth.pat = None;
            config.api = Some(api);
        } else {
            config.api = None;
        }
        config.server_api = None;

        let mongo_mode = field_value(&self.config_form.mongo_fields, "Mode")
            .trim()
            .to_ascii_lowercase();
        config.mongo.databases.gallery_name =
            field_value(&self.config_form.mongo_fields, "Gallery DB")
                .trim()
                .to_string();
        config.mongo.databases.service_name =
            field_value(&self.config_form.mongo_fields, "Service DB")
                .trim()
                .to_string();

        match mongo_mode.as_str() {
            "embedded" => {
                config.mongo.mode = MongoMode::Embedded;
                let mut embedded = config.mongo.embedded.unwrap_or_else(default_mongo_embedded);
                embedded.runtime_settings_path = parse_optional_text_field(
                    &self.config_form.mongo_fields,
                    "Embedded RuntimeSettings",
                );
                embedded.alteryx_service_path = parse_optional_text_field(
                    &self.config_form.mongo_fields,
                    "Embedded Service Path",
                );
                embedded.restore_target_path = parse_optional_text_field(
                    &self.config_form.mongo_fields,
                    "Embedded Restore Path",
                );
                config.mongo.embedded = Some(embedded);
                config.mongo.managed = None;
            }
            "managed" => {
                config.mongo.mode = MongoMode::Managed;
                let mut managed = config.mongo.managed.unwrap_or_else(default_mongo_managed);
                let managed_url =
                    parse_optional_text_field(&self.config_form.mongo_fields, "Managed URL");
                let managed_host =
                    parse_optional_text_field(&self.config_form.mongo_fields, "Managed Host");
                if managed_url.is_some() {
                    managed.url = managed_url;
                    managed.host = None;
                } else {
                    managed.url = None;
                    managed.host = managed_host;
                }
                managed.port =
                    parse_u16_field(&self.config_form.mongo_fields, "Managed Port", managed.port)?;
                managed.auth_database =
                    parse_optional_text_field(&self.config_form.mongo_fields, "Auth DB");
                managed.username =
                    parse_optional_text_field(&self.config_form.mongo_fields, "Managed Username");
                managed.password =
                    parse_optional_text_field(&self.config_form.mongo_fields, "Managed Password");
                managed.tls.enabled = parse_bool_field(
                    field_value(&self.config_form.mongo_fields, "TLS Enabled"),
                    false,
                )?;
                if managed.tls.enabled {
                    managed.tls.ca_path =
                        parse_optional_text_field(&self.config_form.mongo_fields, "TLS CA Path");
                    managed.tls.cert_path =
                        parse_optional_text_field(&self.config_form.mongo_fields, "TLS Cert Path");
                    managed.tls.key_path =
                        parse_optional_text_field(&self.config_form.mongo_fields, "TLS Key Path");
                    managed.tls.allow_invalid_hostnames = Some(parse_bool_field(
                        field_value(&self.config_form.mongo_fields, "Allow Invalid Hostnames"),
                        false,
                    )?);
                } else {
                    managed.tls.ca_path = None;
                    managed.tls.cert_path = None;
                    managed.tls.key_path = None;
                    managed.tls.allow_invalid_hostnames = None;
                }
                managed.timeout_ms = parse_u64_field(&self.config_form.mongo_fields, "Timeout ms")?;
                managed.retry_count =
                    parse_u32_field(&self.config_form.mongo_fields, "Retry Count")?;
                managed.max_pool_size =
                    parse_u32_field(&self.config_form.mongo_fields, "Pool Size")?;
                config.mongo.managed = Some(managed);
                config.mongo.embedded = None;
            }
            "" => {}
            other => {
                return Err(anyhow!(
                    "expected mongo mode to be embedded or managed, got '{other}'"
                ));
            }
        }

        let sqlserver_has_input = self.config_form.sqlserver_fields.iter().any(|field| {
            matches!(
                field.label,
                "Controller Host"
                    | "Controller Port"
                    | "Controller Database"
                    | "Controller Username"
                    | "Controller Password"
                    | "Controller Conn Str"
                    | "Server UI Host"
                    | "Server UI Port"
                    | "Server UI Database"
                    | "Server UI Username"
                    | "Server UI Password"
                    | "Server UI Conn Str"
                    | "Legacy Conn Str"
            ) && !field.value.trim().is_empty()
        }) || parse_bool_field(
            field_value(&self.config_form.sqlserver_fields, "Controller Integrated"),
            false,
        )? || parse_bool_field(
            field_value(&self.config_form.sqlserver_fields, "Controller Encrypt"),
            false,
        )? || parse_bool_field(
            field_value(&self.config_form.sqlserver_fields, "Controller Trust Cert"),
            false,
        )? || parse_bool_field(
            field_value(&self.config_form.sqlserver_fields, "Controller MultiSubnet"),
            false,
        )? || parse_bool_field(
            field_value(&self.config_form.sqlserver_fields, "Server UI Integrated"),
            false,
        )? || parse_bool_field(
            field_value(&self.config_form.sqlserver_fields, "Server UI Encrypt"),
            false,
        )? || parse_bool_field(
            field_value(&self.config_form.sqlserver_fields, "Server UI Trust Cert"),
            false,
        )? || parse_bool_field(
            field_value(&self.config_form.sqlserver_fields, "Server UI MultiSubnet"),
            false,
        )?;
        if config.sqlserver.is_some() || sqlserver_has_input {
            let mut sqlserver = config.sqlserver.unwrap_or_else(default_sqlserver_profile);
            sqlserver.controller = Some(update_sql_connection(
                sqlserver.controller.take(),
                "Controller",
                "AYX_SQL_CONTROLLER_PASSWORD",
                &self.config_form.sqlserver_fields,
            )?);
            sqlserver.server_ui = Some(update_sql_connection(
                sqlserver.server_ui.take(),
                "Server UI",
                "AYX_SQL_SERVER_UI_PASSWORD",
                &self.config_form.sqlserver_fields,
            )?);
            sqlserver.legacy_connection_string =
                parse_optional_text_field(&self.config_form.sqlserver_fields, "Legacy Conn Str");
            config.sqlserver = Some(sqlserver);
        } else {
            config.sqlserver = None;
        }

        let observability_has_input = config.observability.is_some()
            || parse_bool_field(
                field_value(
                    &self.config_form.observability_fields,
                    "API Logging Enabled",
                ),
                false,
            )?
            || !field_value(&self.config_form.observability_fields, "API Logging Path")
                .trim()
                .is_empty()
            || parse_bool_field(
                field_value(&self.config_form.observability_fields, "Redact Bodies"),
                false,
            )?
            || parse_bool_field(
                field_value(&self.config_form.observability_fields, "Log Requests"),
                false,
            )?
            || parse_bool_field(
                field_value(&self.config_form.observability_fields, "Log Responses"),
                false,
            )?;
        if observability_has_input {
            let api_logging_enabled = parse_bool_field(
                field_value(
                    &self.config_form.observability_fields,
                    "API Logging Enabled",
                ),
                false,
            )?;
            let mut observability = config
                .observability
                .unwrap_or(ObservabilityProfile { api_logging: None });
            let mut api_logging = observability.api_logging.unwrap_or(ApiLoggingProfile {
                enabled: false,
                path: None,
                redact_bodies: None,
                log_requests: None,
                log_responses: None,
            });
            api_logging.enabled = api_logging_enabled;
            api_logging.path = parse_optional_text_field(
                &self.config_form.observability_fields,
                "API Logging Path",
            );
            api_logging.redact_bodies = Some(parse_bool_field(
                field_value(&self.config_form.observability_fields, "Redact Bodies"),
                false,
            )?);
            api_logging.log_requests = Some(parse_bool_field(
                field_value(&self.config_form.observability_fields, "Log Requests"),
                false,
            )?);
            api_logging.log_responses = Some(parse_bool_field(
                field_value(&self.config_form.observability_fields, "Log Responses"),
                false,
            )?);
            observability.api_logging = Some(api_logging);
            config.observability = Some(observability);
        } else {
            config.observability = None;
        }

        self.persist_current_config(config)?;
        self.status_message = "Alteryx Server config saved".to_string();
        self.push_toast(
            "Alteryx Server config saved with canonical profile persistence".to_string(),
            false,
        );
        Ok(())
    }

    fn save_credentials(&mut self) -> Result<()> {
        let mut config = match self.target_kind {
            TargetKind::Workspace => {
                if let Some(profile_name) = self.active_profile.as_ref() {
                    let path = profile_storage_path(profile_name).map_err(anyhow::Error::from)?;
                    let mut config = Config::load_from_path_lenient_without_active_overlay(&path)
                        .map_err(anyhow::Error::from)?;
                    config.profile_name = profile_name.clone();
                    config
                } else {
                    self.current_config.clone()
                }
            }
            TargetKind::Profile => self.current_config.clone(),
        };
        let mut one = config.alteryx_one.unwrap_or(AlteryxOneProfile {
            account_email: String::new(),
            base_url: None,
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_credentials: Default::default(),
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: Default::default(),
        });

        one.account_email = self.credentials.fields[0].value.clone();
        one.base_url = normalize_alteryx_one_base_url(&self.credentials.fields[1].value);
        one.oauth_client_id = option_string(&self.credentials.fields[2].value);
        one.token_endpoint_url = None;
        one.access_token = option_string(&self.credentials.fields[3].value);
        one.refresh_token = option_string(&self.credentials.fields[4].value);
        config.alteryx_one = Some(one);

        let saved_to_profile =
            matches!(self.target_kind, TargetKind::Workspace) && self.active_profile.is_some();
        self.persist_one_config(config)?;
        self.status_message = if saved_to_profile {
            "Alteryx One credentials saved to active profile".to_string()
        } else {
            "Alteryx One credentials saved".to_string()
        };
        self.push_toast(
            if saved_to_profile {
                "Credentials saved to the active profile; workspace config keeps server-specific settings only".to_string()
            } else {
                "Credentials saved with canonical profile persistence".to_string()
            },
            false,
        );
        Ok(())
    }

    fn persist_one_config(&mut self, config: Config) -> Result<()> {
        let mut secret_refs: BTreeMap<String, String> = BTreeMap::new();
        let reload_path = match self.target_kind {
            TargetKind::Profile => self.target_path.clone(),
            TargetKind::Workspace => {
                if let Some(profile_name) = self.active_profile.as_ref() {
                    profile_storage_path(profile_name).map_err(anyhow::Error::from)?
                } else {
                    self.target_path.clone()
                }
            }
        };
        match self.target_kind {
            TargetKind::Profile => {
                secret_refs = write_config(&self.target_path, &config, &secret_refs)?;
            }
            TargetKind::Workspace => {
                if let Some(profile_name) = self.active_profile.as_ref() {
                    let profile_path =
                        profile_storage_path(profile_name).map_err(anyhow::Error::from)?;
                    secret_refs = write_config(&profile_path, &config, &secret_refs)?;
                } else {
                    secret_refs = write_config(&self.target_path, &config, &secret_refs)?;
                }
            }
        }
        self.current_config =
            if matches!(self.target_kind, TargetKind::Profile) || self.active_profile.is_some() {
                Config::load_from_path_lenient_without_active_overlay(&reload_path)
                    .map_err(anyhow::Error::from)?
            } else {
                Config::load_from_path_with_environment_lenient(
                    &reload_path,
                    self.target_environment.as_deref(),
                )
                .map_err(anyhow::Error::from)?
            };
        self.config_form = ConfigForm::from_config(&self.current_config);
        self.credentials = CredentialsForm::from_config(&self.current_config);
        self.refresh_connectivity();
        self.refresh_one_browser();
        if secret_refs
            .values()
            .any(|reference| reference.starts_with("inline:"))
        {
            self.push_toast(
                "Saved with inline secret refs because the keyring backend was unavailable."
                    .to_string(),
                false,
            );
        }
        Ok(())
    }

    fn persist_current_config(&mut self, config: Config) -> Result<()> {
        match self.target_kind {
            TargetKind::Profile => {
                let desired_name = config.profile_name.trim();
                if desired_name.is_empty() {
                    return Err(anyhow!("Profile Name must not be empty"));
                }
                let current_name = self
                    .target_path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_string();
                if desired_name != current_name {
                    let new_path =
                        profile_storage_path(desired_name).map_err(anyhow::Error::from)?;
                    if new_path.exists() {
                        return Err(anyhow!("profile '{desired_name}' already exists"));
                    }
                    if let Some(parent) = new_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::rename(&self.target_path, &new_path)?;
                    self.target_path = new_path;
                    let mut state = load_ayx_state().map_err(anyhow::Error::from)?;
                    if state.active_profile.as_deref() == Some(current_name.as_str()) {
                        state.active_profile = Some(desired_name.to_string());
                        save_ayx_state(&state).map_err(anyhow::Error::from)?;
                        self.active_profile = state.active_profile.clone();
                    }
                }
                let secret_refs: BTreeMap<String, String> = BTreeMap::new();
                write_config(&self.target_path, &config, &secret_refs)?;
            }
            TargetKind::Workspace => {
                let mut workspace =
                    load_workspace_config(&self.target_path).map_err(anyhow::Error::from)?;
                let env_name = self
                    .target_environment
                    .clone()
                    .unwrap_or_else(|| workspace.active_environment.clone());
                let mut persisted = config.clone();
                if self.active_profile.is_some() {
                    persisted.alteryx_one = None;
                }
                workspace.environments.insert(env_name, persisted);
                write_workspace_config(&self.target_path, &workspace)?;
            }
        }
        self.current_config =
            if matches!(self.target_kind, TargetKind::Profile) || self.active_profile.is_some() {
                Config::load_from_path_lenient_without_active_overlay(&self.target_path)
                    .map_err(anyhow::Error::from)?
            } else {
                Config::load_from_path_with_environment_lenient(
                    &self.target_path,
                    self.target_environment.as_deref(),
                )
                .map_err(anyhow::Error::from)?
            };
        self.config_form = ConfigForm::from_config(&self.current_config);
        self.credentials = CredentialsForm::from_config(&self.current_config);
        self.refresh_connectivity();
        self.refresh_one_browser();
        Ok(())
    }

    fn reload_target(&mut self) -> Result<()> {
        let target_path = self.target_path.clone();
        let target_environment = self.target_environment.clone();
        let target_kind = self.target_kind;
        self.load_target(target_path, target_environment, target_kind)?;
        self.status_message = "Target reloaded".to_string();
        Ok(())
    }

    fn reload_indexes(&mut self) -> Result<()> {
        self.profiles = list_profile_records_at(&self.config_home)?;
        self.workspaces = load_workspace_entries()?;
        self.sync_selected_entries();
        self.status_message = "Indexes reloaded".to_string();
        Ok(())
    }

    fn sync_selected_entries(&mut self) {
        if let Some(active_profile) = self.active_profile.as_ref()
            && let Some(index) = self
                .visible_profiles()
                .iter()
                .position(|record| &record.name == active_profile)
        {
            self.profiles_state.select(Some(index));
        }
        if let Some(active_workspace) = self.active_workspace.as_ref()
            && let Some(index) = self
                .workspaces
                .iter()
                .position(|workspace| &workspace.name == active_workspace)
        {
            self.workspaces_state.select(Some(index));
        }
        self.sync_workspace_env_cursor();
    }

    fn sync_workspace_env_cursor(&mut self) {
        let Some(workspace) = self.selected_workspace() else {
            self.environments_state.select(Some(0));
            return;
        };
        let index = workspace
            .environments
            .iter()
            .position(|env| env == &workspace.active_environment)
            .unwrap_or(0);
        self.environments_state.select(Some(index));
    }

    pub fn selected_profile_name(&self) -> Option<&String> {
        self.selected_profile_record().map(|record| &record.name)
    }

    pub fn selected_workspace(&self) -> Option<&WorkspaceEntry> {
        self.workspaces
            .get(self.workspaces_state.selected().unwrap_or(0))
    }

    pub fn push_toast(&mut self, message: String, is_error: bool) {
        self.toast = Some(ToastState {
            message,
            is_error,
            expires_at: Instant::now() + Duration::from_secs(4),
        });
    }

    pub fn refresh_connectivity(&mut self) {
        // If a worker is available, dispatch off-thread. Otherwise fall back
        // to the blocking path so headless tests / unusual environments
        // still work.
        let Some(worker) = self.worker.as_ref() else {
            self.refresh_connectivity_blocking();
            return;
        };
        let id = super::worker::BackgroundWorker::new_request_id();
        self.latest_connectivity_request = Some(id);
        self.status_message = "Connectivity checks in progress…".to_string();
        if let Err(err) = worker.submit(super::worker::BackgroundTask::Connectivity {
            id,
            target_path: self.target_path.clone(),
            target_environment: self.target_environment.clone(),
            config: self.current_config.clone(),
        }) {
            self.push_toast(err.to_string(), true);
            self.refresh_connectivity_blocking();
        }
    }

    fn refresh_connectivity_blocking(&mut self) {
        let mut panels = Vec::new();
        panels.push(render_envelope_panel(
            "Doctor Config",
            crate::doctor_config_envelope_from_path(&self.target_path, false).map(|env| env.data),
        ));
        panels.push(render_envelope_panel(
            "Doctor Auth",
            crate::doctor_auth_envelope_from_path(
                &self.target_path,
                self.target_environment.as_deref(),
            )
            .map(|env| env.data),
        ));
        panels.push(render_envelope_panel(
            "One Auth Status",
            crate::one_platform_auth_status_envelope(&self.current_config).map(|env| env.data),
        ));
        panels.push(render_envelope_panel(
            "One Auth Diagnose",
            crate::one_platform_auth_diagnose_envelope(&self.current_config).map(|env| env.data),
        ));
        self.connectivity.panels = panels;
        self.connectivity.last_run = Some(format!("{:?}", std::time::SystemTime::now()));
        self.status_message = "Connectivity checks refreshed".to_string();
    }

    pub fn refresh_one_browser(&mut self) {
        let resource = OneBrowserResource::all()[self.one_browser.cursor];
        let Some(worker) = self.worker.as_ref() else {
            let result = self.request_for_one_browser(resource, None);
            self.set_one_browser_panel(resource, result);
            return;
        };
        let id = super::worker::BackgroundWorker::new_request_id();
        self.latest_one_browser_request = Some(id);
        self.status_message = format!("Loading {}…", resource.label());
        if let Err(err) = worker.submit(super::worker::BackgroundTask::OneBrowser {
            id,
            config: self.current_config.clone(),
            resource,
            resource_id: None,
        }) {
            self.push_toast(err.to_string(), true);
            let result = self.request_for_one_browser(resource, None);
            self.set_one_browser_panel(resource, result);
        }
    }

    fn refresh_one_browser_detail(
        &mut self,
        resource: OneBrowserResource,
        id: Option<String>,
    ) -> Result<()> {
        self.open_one_browser_resource(resource, id, true)
    }

    fn request_for_one_browser(
        &self,
        resource: OneBrowserResource,
        id: Option<&str>,
    ) -> Result<Value> {
        super::one_browser::request_for_one_browser_blocking(&self.current_config, resource, id)
    }

    fn set_one_browser_panel(&mut self, resource: OneBrowserResource, result: Result<Value>) {
        let title = resource.label();
        let panel = match result {
            Ok(value) => PanelState {
                title: title.to_string(),
                lines: pretty_yaml_lines(&value),
                is_error: false,
                raw: Some(value),
            },
            Err(err) => PanelState {
                title: title.to_string(),
                lines: one_browser_error_lines(resource, &err),
                is_error: true,
                raw: None,
            },
        };
        self.one_browser.panels = vec![panel];
        self.one_browser.last_run = Some(format!("{:?}", std::time::SystemTime::now()));
        self.one_browser.item_cursor = 0;
        self.status_message = if self
            .one_browser
            .panels
            .first()
            .is_some_and(|panel| panel.is_error)
        {
            format!("One browser error: {}", resource.label())
        } else {
            format!("One browser refreshed: {}", resource.label())
        };
    }

    pub fn active_one_browser_items(&self) -> Vec<OneBrowserItem> {
        let resource = OneBrowserResource::all()[self.one_browser.cursor];
        self.one_browser
            .panels
            .first()
            .and_then(|panel| panel.raw.as_ref())
            .map(|value| extract_one_browser_items(resource, value))
            .unwrap_or_default()
    }
}

fn move_list_state(state: &mut ListState, len: usize, delta: isize) {
    if len == 0 {
        state.select(None);
        return;
    }
    let current = state.selected().unwrap_or(0) as isize;
    let next = (current + delta).clamp(0, len as isize - 1) as usize;
    state.select(Some(next));
}

fn option_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn one_browser_error_lines(resource: OneBrowserResource, err: &anyhow::Error) -> Vec<String> {
    let mut lines = vec![err.to_string()];
    let err_text = err.to_string();
    if err_text.contains("refresh token request") {
        lines.push(String::new());
        lines.push("Auth hint: the refresh token could not mint an access token.".to_string());
        lines.push(
            "Re-authenticate the active One profile, or switch to a profile with valid client credentials."
                .to_string(),
        );
    } else if err_text.contains("access_token is required") {
        lines.push(String::new());
        lines.push(
            "Auth hint: configure access_token, or set oauth_client_id + client_secret."
                .to_string(),
        );
    } else if matches!(resource, OneBrowserResource::WorkspaceList) {
        lines.push(String::new());
        lines.push(
            "Auth hint: workspace list requires a valid One bearer token for the active profile."
                .to_string(),
        );
    }
    lines
}

impl OneBrowserResource {
    fn all_index(self) -> usize {
        OneBrowserResource::all()
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }
}

fn load_workspace_entries() -> Result<Vec<WorkspaceEntry>> {
    let mut entries = Vec::new();
    for name in list_central_workspaces().map_err(anyhow::Error::from)? {
        let path = workspace_storage_path(&name).map_err(anyhow::Error::from)?;
        let workspace = load_workspace_config(&path).map_err(anyhow::Error::from)?;
        let mut environments = workspace.environments.keys().cloned().collect::<Vec<_>>();
        environments.sort();
        entries.push(WorkspaceEntry {
            name,
            path,
            active_environment: workspace.active_environment.clone(),
            environments,
            config: workspace,
        });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_smoke_enabled() -> bool {
        matches!(
            std::env::var("AYX_ONE_LIVE_SMOKE").ok().as_deref(),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        )
    }

    #[test]
    fn one_browser_workspace_list_shows_live_items() {
        if !live_smoke_enabled() {
            return;
        }

        let mut app = App::new_without_worker().expect("app should load the active config");
        app.select_screen(Screen::One);
        app.open_one_browser_resource(OneBrowserResource::WorkspaceList, None, false)
            .expect("workspace list should load");

        let panel = app
            .one_browser
            .panels
            .first()
            .expect("workspace list panel should exist");
        if panel.is_error {
            panic!(
                "workspace list failed in live TUI smoke:\n{}",
                panel.lines.join("\n")
            );
        }

        let items = app.active_one_browser_items();
        assert!(
            !items.is_empty(),
            "expected live workspace items in One Browser panel\npanel: {:?}\nlines: {:?}",
            panel.title,
            panel.lines
        );
        assert_eq!(panel.title, "Workspace List");
    }
}
