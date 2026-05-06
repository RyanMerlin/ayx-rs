use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use serde_json::Value;

use ayx_core::profile::{
    default_profile_storage_path, list_central_profiles, list_central_workspaces, load_ayx_state,
    load_workspace_config, profile_resolution_detail, profile_storage_path, save_ayx_state,
    workspace_storage_path, ApiLoggingProfile, AlteryxOneProfile, Config, ObservabilityProfile,
    ServerProfile, WorkspaceConfig,
};

use crate::onboard::{
    default_config, load_existing_config, summarize_config, summarize_onboarding_validation,
    write_config, write_workspace_config,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Profiles,
    Config,
    Credentials,
    Connectivity,
    Inspect,
    One,
    Help,
}

impl Screen {
    pub fn all() -> [Screen; 7] {
        [
            Screen::Profiles,
            Screen::Config,
            Screen::Credentials,
            Screen::Connectivity,
            Screen::Inspect,
            Screen::One,
            Screen::Help,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Screen::Profiles => "Profiles",
            Screen::Config => "Config",
            Screen::Credentials => "One Credentials",
            Screen::Connectivity => "Connectivity",
            Screen::Inspect => "Inspect",
            Screen::One => "One Browser",
            Screen::Help => "Help",
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::all()[index.min(Self::all().len() - 1)]
    }

    pub fn index(self) -> usize {
        match self {
            Screen::Profiles => 0,
            Screen::Config => 1,
            Screen::Credentials => 2,
            Screen::Connectivity => 3,
            Screen::Inspect => 4,
            Screen::One => 5,
            Screen::Help => 6,
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
pub enum TargetKind {
    Profile,
    Workspace,
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
}

#[derive(Debug, Clone)]
pub struct ConfigForm {
    pub fields: Vec<ConfigFieldState>,
    pub cursor: usize,
    pub editing: bool,
    pub edit_buffer: String,
    pub dirty: bool,
}

impl ConfigForm {
    pub fn from_config(config: &Config) -> Self {
        Self {
            fields: vec![
                ConfigFieldState {
                    label: "Profile Name",
                    value: config.profile_name.clone(),
                    placeholder: "local",
                    kind: ConfigFieldKind::Text,
                },
                ConfigFieldState {
                    label: "Server Base URL",
                    value: config
                        .server
                        .as_ref()
                        .map(|server| server.webapi_url.clone())
                        .unwrap_or_default(),
                    placeholder: "https://server.example.com/",
                    kind: ConfigFieldKind::Text,
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
                },
                ConfigFieldState {
                    label: "API Logging",
                    value: config
                        .observability
                        .as_ref()
                        .and_then(|obs| obs.api_logging.as_ref())
                        .map(|logging| logging.enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "true|false",
                    kind: ConfigFieldKind::Bool,
                },
            ],
            cursor: 0,
            editing: false,
            edit_buffer: String::new(),
            dirty: false,
        }
    }

    pub fn active_field(&self) -> &ConfigFieldState {
        &self.fields[self.cursor]
    }

    pub fn active_field_mut(&mut self) -> &mut ConfigFieldState {
        &mut self.fields[self.cursor]
    }

    pub fn display_value(&self, index: usize) -> String {
        self.fields[index].value.clone()
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
                    label: "Account Email",
                    value: one.map(|v| v.account_email.clone()).unwrap_or_default(),
                    placeholder: "operator@example.com",
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
                    label: "Token Endpoint URL",
                    value: one
                        .and_then(|v| v.token_endpoint_url.clone())
                        .unwrap_or_default(),
                    placeholder: "https://.../oauth/token",
                    secret: false,
                },
                FieldState {
                    label: "Access Token",
                    value: one.and_then(|v| v.access_token.clone()).unwrap_or_default(),
                    placeholder: "stored in keyring on save",
                    secret: true,
                },
                FieldState {
                    label: "Refresh Token",
                    value: one.and_then(|v| v.refresh_token.clone()).unwrap_or_default(),
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
    pub screen: Screen,
    pub focus: Focus,
    pub should_quit: bool,
    pub sidebar: ListState,
    pub profiles_state: ListState,
    pub workspaces_state: ListState,
    pub environments_state: ListState,
    pub profiles_pane: ProfilesPane,
    pub profiles: Vec<String>,
    pub workspaces: Vec<WorkspaceEntry>,
    pub active_profile: Option<String>,
    pub active_workspace: Option<String>,
    pub target_kind: TargetKind,
    pub target_path: PathBuf,
    pub target_environment: Option<String>,
    pub resolution_source: String,
    pub current_config: Config,
    pub config_form: ConfigForm,
    pub credentials: CredentialsForm,
    pub connectivity: ConnectivityState,
    pub one_browser: OneBrowserState,
    pub status_message: String,
    pub toast: Option<ToastState>,
}

impl App {
    pub fn new() -> Result<Self> {
        let state = load_ayx_state().map_err(anyhow::Error::from)?;
        let profiles = list_central_profiles().map_err(anyhow::Error::from)?;
        let workspaces = load_workspace_entries()?;
        let target_path = default_profile_storage_path().map_err(anyhow::Error::from)?;
        let current_config = load_existing_config(&target_path, None).unwrap_or_else(|_| default_config());
        let resolution = profile_resolution_detail(Path::new("config.yaml")).map_err(anyhow::Error::from)?;

        let mut sidebar = ListState::default();
        sidebar.select(Some(Screen::Profiles.index()));

        let mut profiles_state = ListState::default();
        profiles_state.select(Some(0));

        let mut workspaces_state = ListState::default();
        workspaces_state.select(Some(0));

        let mut environments_state = ListState::default();
        environments_state.select(Some(0));

        let mut app = Self {
            screen: Screen::Profiles,
            focus: Focus::Sidebar,
            should_quit: false,
            sidebar,
            profiles_state,
            workspaces_state,
            environments_state,
            profiles_pane: ProfilesPane::Profiles,
            profiles,
            workspaces,
            active_profile: state.active_profile,
            active_workspace: state.active_workspace,
            target_kind: TargetKind::Profile,
            target_path,
            target_environment: None,
            resolution_source: resolution.source,
            config_form: ConfigForm::from_config(&current_config),
            credentials: CredentialsForm::from_config(&current_config),
            current_config,
            connectivity: ConnectivityState::default(),
            one_browser: OneBrowserState::default(),
            status_message: "Ready".to_string(),
            toast: None,
        };
        app.sync_selected_entries();
        app.refresh_connectivity();
        app.refresh_one_browser();
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

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.config_form.editing {
            self.handle_config_edit_key(key);
            return;
        }

        if self.credentials.editing {
            self.handle_credentials_edit_key(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab => self.handle_tab(),
            KeyCode::BackTab => self.handle_backtab(),
            KeyCode::Char('1') => self.select_screen(Screen::Profiles),
            KeyCode::Char('2') => self.select_screen(Screen::Config),
            KeyCode::Char('3') => self.select_screen(Screen::Credentials),
            KeyCode::Char('4') => self.select_screen(Screen::Connectivity),
            KeyCode::Char('5') => self.select_screen(Screen::Inspect),
            KeyCode::Char('6') => self.select_screen(Screen::One),
            KeyCode::Char('7') => self.select_screen(Screen::Help),
            _ => match self.focus {
                Focus::Sidebar => self.handle_sidebar_key(key),
                Focus::Content => self.handle_content_key(key),
            },
        }
    }

    fn handle_credentials_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.credentials.cancel_edit(),
            KeyCode::Enter | KeyCode::Char('\r') | KeyCode::Char('\n') => self.credentials.commit_edit(),
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
            KeyCode::Enter | KeyCode::Char('\r') | KeyCode::Char('\n') => self.config_form.commit_edit(),
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

    fn handle_tab(&mut self) {
        match self.focus {
            Focus::Sidebar => self.focus = Focus::Content,
            Focus::Content => {
                if self.screen == Screen::Profiles {
                    self.profiles_pane = self.profiles_pane.next();
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
            KeyCode::Char('r') => {
                if let Err(err) = self.reload_indexes() {
                    self.push_toast(err.to_string(), true);
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
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Sidebar,
            KeyCode::Char('r') => {
                if let Err(err) = self.reload_indexes() {
                    self.push_toast(err.to_string(), true);
                }
            }
            KeyCode::Char('i') | KeyCode::Enter => {
                self.select_screen(Screen::Profiles);
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
            KeyCode::Tab => self.one_browser.pane = match self.one_browser.pane {
                OneBrowserPane::Resources => OneBrowserPane::Items,
                OneBrowserPane::Items => OneBrowserPane::Resources,
            },
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
            KeyCode::Enter => {
                match self.one_browser.pane {
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
                }
            }
            KeyCode::Char('i') => self.select_screen(Screen::Inspect),
            _ => {}
        }
    }

    fn select_screen(&mut self, screen: Screen) {
        self.screen = screen;
        self.sidebar.select(Some(screen.index()));
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
            ProfilesPane::Profiles => move_list_state(&mut self.profiles_state, self.profiles.len(), delta),
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
        self.load_target(workspace.path.clone(), Some(env.clone()), TargetKind::Workspace)?;
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
            TargetKind::Profile => load_existing_config(&path, None).unwrap_or_else(|_| default_config()),
            TargetKind::Workspace => load_existing_config(&path, environment.as_deref())?,
        };
        self.target_kind = kind;
        self.target_path = path;
        self.target_environment = environment;
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
        config.profile_name = self.config_form.fields[0].value.trim().to_string();

        let server_base_url = self.config_form.fields[1].value.trim().to_string();
        let verify_tls = parse_bool_field(&self.config_form.fields[2].value, true)?;
        let api_logging_enabled = parse_bool_field(&self.config_form.fields[3].value, false)?;

        if !server_base_url.is_empty() || config.server.is_some() {
            let mut server = config.server.unwrap_or_else(default_server_profile);
            if !server_base_url.is_empty() {
                server.webapi_url = normalize_server_url(&server_base_url);
            }
            server.verify_tls = Some(verify_tls);
            config.server = Some(server);
        }

        if api_logging_enabled || config.observability.is_some() {
            let mut observability = config.observability.unwrap_or(ObservabilityProfile {
                api_logging: None,
            });
            let mut api_logging = observability
                .api_logging
                .unwrap_or(ApiLoggingProfile {
                    enabled: false,
                    path: None,
                    redact_bodies: None,
                    log_requests: None,
                    log_responses: None,
                });
            api_logging.enabled = api_logging_enabled;
            observability.api_logging = Some(api_logging);
            config.observability = Some(observability);
        }

        self.persist_current_config(config)?;
        self.status_message = "Config saved".to_string();
        self.push_toast("Config saved with canonical profile persistence".to_string(), false);
        Ok(())
    }

    fn save_credentials(&mut self) -> Result<()> {
        let mut config = self.current_config.clone();
        let mut one = config.alteryx_one.unwrap_or(AlteryxOneProfile {
            account_email: String::new(),
            oauth_client_id: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
        });

        one.account_email = self.credentials.fields[0].value.clone();
        one.oauth_client_id = option_string(&self.credentials.fields[1].value);
        one.token_endpoint_url = option_string(&self.credentials.fields[2].value);
        one.access_token = option_string(&self.credentials.fields[3].value);
        one.refresh_token = option_string(&self.credentials.fields[4].value);
        config.alteryx_one = Some(one);

        self.persist_current_config(config)?;
        self.status_message = "One credentials saved".to_string();
        self.push_toast("Credentials saved with canonical profile persistence".to_string(), false);
        Ok(())
    }

    fn persist_current_config(&mut self, config: Config) -> Result<()> {
        match self.target_kind {
            TargetKind::Profile => {
                let secret_refs: BTreeMap<String, String> = BTreeMap::new();
                write_config(&self.target_path, &config, &secret_refs)?;
            }
            TargetKind::Workspace => {
                let mut workspace = load_workspace_config(&self.target_path).map_err(anyhow::Error::from)?;
                let env_name = self
                    .target_environment
                    .clone()
                    .unwrap_or_else(|| workspace.active_environment.clone());
                workspace.environments.insert(env_name, config.clone());
                write_workspace_config(&self.target_path, &workspace)?;
            }
        }
        self.current_config = load_existing_config(&self.target_path, self.target_environment.as_deref())?;
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
        self.profiles = list_central_profiles().map_err(anyhow::Error::from)?;
        self.workspaces = load_workspace_entries()?;
        self.sync_selected_entries();
        self.status_message = "Indexes reloaded".to_string();
        Ok(())
    }

    fn sync_selected_entries(&mut self) {
        if let Some(active_profile) = self.active_profile.as_ref() {
            if let Some(index) = self.profiles.iter().position(|name| name == active_profile) {
                self.profiles_state.select(Some(index));
            }
        }
        if let Some(active_workspace) = self.active_workspace.as_ref() {
            if let Some(index) = self
                .workspaces
                .iter()
                .position(|workspace| &workspace.name == active_workspace)
            {
                self.workspaces_state.select(Some(index));
            }
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
        self.profiles
            .get(self.profiles_state.selected().unwrap_or(0))
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
        let mut panels = Vec::new();

        panels.push(render_envelope_panel(
            "Doctor Config",
            crate::doctor_config_envelope(&self.target_path, false).map(|env| env.data),
        ));
        panels.push(render_envelope_panel(
            "Doctor Auth",
            crate::doctor_auth_envelope(&self.target_path, self.target_environment.as_deref())
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
        let result = self.request_for_one_browser(resource, None);
        self.set_one_browser_panel(resource, result);
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
        let envelope = match resource {
            OneBrowserResource::AuthStatus => crate::one_platform_auth_status_envelope(&self.current_config)?,
            OneBrowserResource::AuthDiagnose => crate::one_platform_auth_diagnose_envelope(&self.current_config)?,
            OneBrowserResource::SurfaceInventory => crate::one_surface_inventory_envelope(&self.current_config)?,
            OneBrowserResource::WorkspaceCurrent => crate::one_api_live_request(
                &self.current_config,
                "platform",
                "tui-workspace-current",
                "GET",
                "/v4/workspaces/current",
                false,
                &[],
            )?,
            OneBrowserResource::WorkspaceCurrentConfiguration => crate::one_api_live_request(
                &self.current_config,
                "platform",
                "tui-workspace-current-configuration",
                "GET",
                "/v4/workspaces/current/configuration",
                false,
                &[],
            )?,
            OneBrowserResource::WorkspaceCurrentConfigurationSchema => crate::one_api_live_request(
                &self.current_config,
                "platform",
                "tui-workspace-current-configuration-schema",
                "GET",
                "/v4/workspaces/current/configuration-schema",
                false,
                &[],
            )?,
            OneBrowserResource::WorkspaceList => crate::one_api_live_request(
                &self.current_config,
                "platform",
                "tui-workspace-list",
                "GET",
                "/v4/workspaces",
                false,
                &[],
            )?,
            OneBrowserResource::WorkspaceDetail => crate::one_api_live_request(
                &self.current_config,
                "platform",
                "tui-workspace-detail",
                "GET",
                "/v4/workspaces/{id}",
                false,
                &[("id", id.ok_or_else(|| anyhow!("workspace id required"))?)],
            )?,
            OneBrowserResource::FlowList => crate::one_api_live_request(
                &self.current_config,
                "flow",
                "tui-flow-list",
                "GET",
                "/v4/flows",
                false,
                &[],
            )?,
            OneBrowserResource::FlowDetail => crate::one_api_live_request(
                &self.current_config,
                "flow",
                "tui-flow-detail",
                "GET",
                "/v4/flows/{id}",
                false,
                &[("id", id.ok_or_else(|| anyhow!("flow id required"))?)],
            )?,
            OneBrowserResource::ConnectionList => crate::one_api_live_request(
                &self.current_config,
                "connection",
                "tui-connection-list",
                "GET",
                "/v4/connections",
                false,
                &[],
            )?,
            OneBrowserResource::ConnectionDetail => crate::one_api_live_request(
                &self.current_config,
                "connection",
                "tui-connection-detail",
                "GET",
                "/v4/connections/{id}",
                false,
                &[("id", id.ok_or_else(|| anyhow!("connection id required"))?)],
            )?,
        };
        Ok(envelope.data)
    }

    fn set_one_browser_panel(&mut self, resource: OneBrowserResource, result: Result<Value>) {
        let title = resource.label();
        let panel = match result {
            Ok(value) => PanelState {
                title: title.to_string(),
                lines: pretty_lines(&value),
                is_error: false,
                raw: Some(value),
            },
            Err(err) => PanelState {
                title: title.to_string(),
                lines: vec![err.to_string()],
                is_error: true,
                raw: None,
            },
        };
        self.one_browser.panels = vec![panel];
        self.one_browser.last_run = Some(format!("{:?}", std::time::SystemTime::now()));
        self.one_browser.item_cursor = 0;
        self.status_message = format!("One browser refreshed: {}", resource.label());
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

fn normalize_server_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{trimmed}/")
    }
}

fn parse_bool_field(value: &str, default: bool) -> Result<bool> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(default);
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "true" | "yes" | "y" | "1" | "on" => Ok(true),
        "false" | "no" | "n" | "0" | "off" => Ok(false),
        _ => Err(anyhow!("expected true/false value")),
    }
}

fn default_server_profile() -> ServerProfile {
    ServerProfile {
        webapi_url: "http://localhost/".to_string(),
        curator_api_key: String::new(),
        curator_api_secret: String::new(),
        curator_api_secret_ref: None,
        verify_tls: Some(true),
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

fn render_envelope_panel(title: &str, value: Result<Value>) -> PanelState {
    match value {
        Ok(value) => PanelState {
            title: title.to_string(),
            lines: pretty_lines(&value),
            is_error: false,
            raw: Some(value),
        },
        Err(err) => PanelState {
            title: title.to_string(),
            lines: vec![err.to_string()],
            is_error: true,
            raw: None,
        },
    }
}

fn pretty_lines(value: &Value) -> Vec<String> {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|_| value.to_string())
        .lines()
        .map(|line| line.to_string())
        .collect()
}

fn extract_one_browser_items(resource: OneBrowserResource, value: &Value) -> Vec<OneBrowserItem> {
    let array = preferred_item_array(resource, value)
        .or_else(|| find_first_object_array(value))
        .or_else(|| value.as_array().map(|v| v.as_slice()));

    let Some(array) = array else {
        return Vec::new();
    };

    array
        .iter()
        .map(|item| {
            let id = item.as_object().and_then(|object| {
                string_field(
                    object,
                    &[
                        "id",
                        "workspace_id",
                        "workspaceId",
                        "flow_id",
                        "flowId",
                        "connection_id",
                        "connectionId",
                        "person_id",
                        "personId",
                        "subjectId",
                        "roleId",
                        "planId",
                    ],
                )
            });
            let id = id.map(str::to_owned);
            let label = item
                .as_object()
                .and_then(|object| {
                    string_field(
                        object,
                        &[
                            "name",
                            "title",
                            "label",
                            "display_name",
                            "displayName",
                            "workspace_name",
                            "workspaceName",
                            "flow_name",
                            "flowName",
                            "connection_name",
                            "connectionName",
                            "email",
                            "status",
                        ],
                    )
                })
                .map(str::to_owned)
                .unwrap_or_else(|| id.clone().unwrap_or_else(|| item.to_string()));
            let summary = item
                .as_object()
                .and_then(|object| {
                    string_field(
                        object,
                        &[
                            "description",
                            "status",
                            "type",
                            "path",
                            "command",
                            "method",
                            "role",
                        ],
                    )
                })
                .map(str::to_owned)
                .unwrap_or_default();
            OneBrowserItem { id, label, summary }
        })
        .collect()
}

fn preferred_item_array<'a>(resource: OneBrowserResource, value: &'a Value) -> Option<&'a [Value]> {
    let object = value.as_object()?;
    let keys = match resource {
        OneBrowserResource::SurfaceInventory => [
            "surfaces",
            "partial_surfaces",
            "documented_only_surfaces",
            "deferred_surfaces",
        ]
        .as_slice(),
        OneBrowserResource::WorkspaceCurrentConfiguration | OneBrowserResource::WorkspaceCurrentConfigurationSchema => &["environments"],
        OneBrowserResource::WorkspaceList
        | OneBrowserResource::WorkspaceDetail
        | OneBrowserResource::ConnectionList
        | OneBrowserResource::ConnectionDetail
        | OneBrowserResource::FlowList
        | OneBrowserResource::FlowDetail => &["items", "data", "results", "rows"],
        _ => &["items", "data", "results", "rows", "surfaces", "endpoints"],
    };

    for key in keys {
        if let Some(array) = object.get(*key).and_then(|value| value.as_array()) {
            return Some(array.as_slice());
        }
    }
    None
}

fn find_first_object_array(value: &Value) -> Option<&[Value]> {
    let object = value.as_object()?;
    for key in [
        "items",
        "data",
        "results",
        "rows",
        "surfaces",
        "endpoints",
        "workspaces",
        "flows",
        "connections",
        "people",
    ] {
        if let Some(array) = object.get(key).and_then(|value| value.as_array()) {
            return Some(array.as_slice());
        }
    }
    None
}

fn string_field<'a>(object: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| object.get(*key)?.as_str())
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
