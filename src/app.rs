use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::gcloud;
use crate::profile::{Profile, SyncMode};
use crate::store::Store;


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Both,
    User,
    Adc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    AddProfileName,
    AddProfileUserAccount,
    AddProfileUserProject,
    AddProfileAdcAccount,
    AddProfileAdcQuotaProject,
    ConfirmDelete,
    EditAccount,
    EditProject,
}

/// A shell command that requires TUI suspension (e.g. interactive gcloud auth).
pub enum PendingAction {
    None,
    Reauth,
    ReauthAndActivate,
}

/// Result from a background auth check thread.
struct AuthResult {
    generation: u64,
    profile_index: usize,
    is_user: bool,
    valid: bool,
}

pub struct App {
    pub store: Store,
    pub profile_names: Vec<String>,
    pub profiles: Vec<Profile>,
    pub active_profile: Option<String>,
    pub user_auth_valid: Vec<Option<bool>>,
    pub adc_auth_valid: Vec<Option<bool>>,
    pub selected_row: usize,
    pub selected_col: Column,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub input_mode: InputMode,
    pub input_buffer: String,
    // Temporary storage for profile being added
    pub new_profile_name: String,
    pub new_profile: Profile,
    // In-place editing state
    pub edit_col: Column,
    pub edit_account_buffer: String,
    pub edit_project_buffer: String,
    pub suggestions: Vec<String>,
    pub suggestion_index: Option<usize>,
    // Pending action that needs TUI suspended
    pub pending_action: PendingAction,
    pub quit_after_activate: bool,
    // Async auth check state
    auth_tx: mpsc::Sender<AuthResult>,
    auth_rx: mpsc::Receiver<AuthResult>,
    auth_generation: u64,
    // Async project list fetch state
    project_tx: mpsc::Sender<Vec<String>>,
    project_rx: mpsc::Receiver<Vec<String>>,
    pub fetched_projects: Vec<String>,
    pub fetching_projects: bool,
    pub sync_mode: SyncMode,
}

impl App {
    pub fn new() -> Result<Self> {
        let store = Store::new()?;
        let data = store.load_profiles()?;

        let profile_names: Vec<String> = data.profiles.keys().cloned().collect();
        let profiles: Vec<Profile> = data.profiles.values().cloned().collect();
        let active_profile = data.active_profile;
        let sync_mode = data.sync_mode;

        let selected_row = if let Some(ref active) = active_profile {
            profile_names
                .iter()
                .position(|n| n == active)
                .unwrap_or(0)
        } else {
            0
        };

        let (auth_tx, auth_rx) = mpsc::channel();
        let (project_tx, project_rx) = mpsc::channel();

        let mut app = Self {
            store,
            profile_names,
            profiles,
            active_profile,
            user_auth_valid: Vec::new(),
            adc_auth_valid: Vec::new(),
            selected_row,
            selected_col: Column::Both,
            should_quit: false,
            status_message: None,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            new_profile_name: String::new(),
            new_profile: Profile {
                user_account: String::new(),
                user_project: String::new(),
                adc_account: String::new(),
                adc_quota_project: String::new(),
            },
            edit_col: Column::User,
            edit_account_buffer: String::new(),
            edit_project_buffer: String::new(),
            suggestions: Vec::new(),
            suggestion_index: None,
            pending_action: PendingAction::None,
            quit_after_activate: false,
            auth_tx,
            auth_rx,
            auth_generation: 0,
            project_tx,
            project_rx,
            fetched_projects: Vec::new(),
            fetching_projects: false,
            sync_mode,
        };

        app.start_auth_checks();
        Ok(app)
    }

    /// Spawn background threads to check auth for all unique accounts.
    fn start_auth_checks(&mut self) {
        self.auth_generation += 1;
        let gen = self.auth_generation;
        self.user_auth_valid = vec![None; self.profiles.len()];
        self.adc_auth_valid = vec![None; self.profiles.len()];

        // Deduplicate: group (profile_index, is_user) by account email
        let mut account_targets: std::collections::HashMap<String, Vec<(usize, bool)>> =
            std::collections::HashMap::new();
        for (i, profile) in self.profiles.iter().enumerate() {
            if !profile.user_account.is_empty() {
                account_targets
                    .entry(profile.user_account.clone())
                    .or_default()
                    .push((i, true));
            }
            if !profile.adc_account.is_empty() {
                account_targets
                    .entry(profile.adc_account.clone())
                    .or_default()
                    .push((i, false));
            }
        }

        for (account, targets) in account_targets {
            let tx = self.auth_tx.clone();
            std::thread::spawn(move || {
                let valid = gcloud::check_account_auth(&account);
                for (idx, is_user) in targets {
                    let _ = tx.send(AuthResult {
                        generation: gen,
                        profile_index: idx,
                        is_user,
                        valid,
                    });
                }
            });
        }
    }

    /// Drain completed auth results from background threads.
    pub fn check_auth_results(&mut self) {
        while let Ok(result) = self.auth_rx.try_recv() {
            if result.generation != self.auth_generation {
                continue;
            }
            if result.profile_index >= self.profiles.len() {
                continue;
            }
            if result.is_user {
                self.user_auth_valid[result.profile_index] = Some(result.valid);
            } else {
                self.adc_auth_valid[result.profile_index] = Some(result.valid);
            }
        }
    }

    /// Drain completed project list results from background thread.
    pub fn check_project_results(&mut self) {
        while let Ok(projects) = self.project_rx.try_recv() {
            self.fetched_projects = projects;
            self.fetching_projects = false;
        }
    }

    /// Spawn a background thread to fetch projects for the given account.
    fn start_project_fetch(&mut self, account: &str) {
        if account.is_empty() {
            self.fetched_projects.clear();
            return;
        }
        self.fetching_projects = true;
        self.fetched_projects.clear();
        let account = account.to_string();
        let tx = self.project_tx.clone();
        std::thread::spawn(move || {
            let projects = gcloud::list_projects_for_account(&account).unwrap_or_default();
            let _ = tx.send(projects);
        });
    }

    pub fn reload(&mut self) -> Result<()> {
        let data = self.store.load_profiles()?;
        self.profile_names = data.profiles.keys().cloned().collect();
        self.profiles = data.profiles.values().cloned().collect();
        self.active_profile = data.active_profile;
        if self.selected_row >= self.profile_names.len() {
            self.selected_row = self.profile_names.len().saturating_sub(1);
        }
        self.start_auth_checks();
        Ok(())
    }

    pub fn handle_event(&mut self) -> Result<bool> {
        // Use poll with timeout so the UI can refresh for async auth results
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match self.input_mode {
                    InputMode::Normal => self.handle_normal_key(key)?,
                    InputMode::ConfirmDelete => self.handle_confirm_delete(key)?,
                    InputMode::EditAccount | InputMode::EditProject => {
                        self.handle_edit_key(key)?
                    }
                    _ => self.handle_input_key(key)?,
                }
            }
        }
        Ok(self.should_quit)
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Up => {
                if !self.profile_names.is_empty() && self.selected_row > 0 {
                    self.selected_row -= 1;
                }
                self.status_message = None;
            }
            KeyCode::Down => {
                if !self.profile_names.is_empty()
                    && self.selected_row < self.profile_names.len() - 1
                {
                    self.selected_row += 1;
                }
                self.status_message = None;
            }
            KeyCode::Left => {
                self.selected_col = match self.selected_col {
                    Column::Both => Column::Both,
                    Column::User => Column::Both,
                    Column::Adc => Column::User,
                };
                self.status_message = None;
            }
            KeyCode::Right => {
                self.selected_col = match self.selected_col {
                    Column::Both => Column::User,
                    Column::User => Column::Adc,
                    Column::Adc => Column::Adc,
                };
                self.status_message = None;
            }
            KeyCode::Enter => {
                if !self.profile_names.is_empty() {
                    self.quit_after_activate = !key.modifiers.contains(KeyModifiers::ALT);
                    self.activate_selected()?;
                    // Only quit now if no pending reauth (otherwise quit after reauth completes)
                    if self.quit_after_activate
                        && matches!(self.pending_action, PendingAction::None)
                    {
                        self.should_quit = true;
                    }
                }
            }
            KeyCode::Char('a') => {
                if !self.profile_names.is_empty() {
                    self.pending_action = PendingAction::Reauth;
                }
            }
            KeyCode::Char('n') => {
                self.input_mode = InputMode::AddProfileName;
                self.input_buffer.clear();
                self.status_message = Some("Enter profile name:".to_string());
            }
            KeyCode::Char('e') => {
                if !self.profile_names.is_empty() {
                    let edit_col = match self.selected_col {
                        Column::Both => Column::User,
                        col => col,
                    };
                    let profile = &self.profiles[self.selected_row];
                    self.edit_col = edit_col;
                    self.edit_account_buffer = match edit_col {
                        Column::User => profile.user_account.clone(),
                        Column::Adc => profile.adc_account.clone(),
                        _ => unreachable!(),
                    };
                    self.edit_project_buffer = match edit_col {
                        Column::User => profile.user_project.clone(),
                        Column::Adc => profile.adc_quota_project.clone(),
                        _ => unreachable!(),
                    };
                    self.input_mode = InputMode::EditAccount;
                    self.suggestions.clear();
                    self.suggestion_index = None;
                    self.status_message = None;
                }
            }
            KeyCode::Char('d') => {
                if !self.profile_names.is_empty() {
                    let name = &self.profile_names[self.selected_row];
                    self.status_message =
                        Some(format!("Delete profile '{}'? (y/n)", name));
                    self.input_mode = InputMode::ConfirmDelete;
                }
            }
            KeyCode::Char('s') => {
                self.sync_mode = match self.sync_mode {
                    SyncMode::Strict => SyncMode::Add,
                    SyncMode::Add => SyncMode::Off,
                    SyncMode::Off => SyncMode::Strict,
                };
                let mut data = self.store.load_profiles()?;
                data.sync_mode = self.sync_mode;
                self.store.save_profiles(&data)?;
                let label = match self.sync_mode {
                    SyncMode::Strict => "strict",
                    SyncMode::Add => "add",
                    SyncMode::Off => "off",
                };
                self.status_message = Some(format!("Sync mode: {}", label));
            }
            KeyCode::Char('i') => {
                let configs = gcloud::discover_existing_configs()?;
                if configs.is_empty() {
                    self.status_message = Some("No gcloud configurations found.".to_string());
                } else {
                    let mut data = self.store.load_profiles()?;
                    let mut count = 0;
                    for (name, account, project) in &configs {
                        if !data.profiles.contains_key(name) {
                            let profile = Profile {
                                user_account: account.clone(),
                                user_project: project.clone(),
                                adc_account: account.clone(),
                                adc_quota_project: project.clone(),
                            };
                            data.profiles.insert(name.clone(), profile);
                            count += 1;
                        }
                    }
                    if count > 0 {
                        if let Ok(Some(active)) = gcloud::read_active_config() {
                            if data.profiles.contains_key(&active) {
                                data.active_profile = Some(active);
                            }
                        }
                        self.store.save_profiles(&data)?;
                        self.reload()?;
                        self.status_message =
                            Some(format!("Imported {} profile(s).", count));
                    } else {
                        self.status_message =
                            Some("No new configurations to import.".to_string());
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_input_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                self.status_message = None;
            }
            KeyCode::Enter => {
                let value = self.input_buffer.trim().to_string();
                if value.is_empty() {
                    return Ok(());
                }
                match self.input_mode {
                    InputMode::AddProfileName => {
                        self.new_profile_name = value;
                        self.input_buffer.clear();
                        self.input_mode = InputMode::AddProfileUserAccount;
                        self.status_message = Some("Enter user account (email):".to_string());
                    }
                    InputMode::AddProfileUserAccount => {
                        self.new_profile.user_account = value.clone();
                        self.new_profile.adc_account = value; // default
                        self.input_buffer.clear();
                        self.input_mode = InputMode::AddProfileUserProject;
                        self.status_message = Some("Enter user project:".to_string());
                    }
                    InputMode::AddProfileUserProject => {
                        self.new_profile.user_project = value.clone();
                        self.new_profile.adc_quota_project = value; // default
                        self.input_buffer.clear();
                        self.input_mode = InputMode::AddProfileAdcAccount;
                        self.status_message = Some(format!(
                            "Enter ADC account [{}]:",
                            self.new_profile.adc_account
                        ));
                    }
                    InputMode::AddProfileAdcAccount => {
                        self.new_profile.adc_account = value;
                        self.input_buffer.clear();
                        self.input_mode = InputMode::AddProfileAdcQuotaProject;
                        self.status_message = Some(format!(
                            "Enter ADC quota project [{}]:",
                            self.new_profile.adc_quota_project
                        ));
                    }
                    InputMode::AddProfileAdcQuotaProject => {
                        self.new_profile.adc_quota_project = value;
                        // Save the profile
                        self.store
                            .add_profile(&self.new_profile_name, self.new_profile.clone())?;
                        if matches!(self.sync_mode, SyncMode::Strict | SyncMode::Add) {
                            let _ = gcloud::create_configuration(
                                &self.new_profile_name,
                                &self.new_profile.user_account,
                                &self.new_profile.user_project,
                            );
                        }
                        self.status_message = Some(format!(
                            "Profile '{}' added.",
                            self.new_profile_name
                        ));
                        self.reload()?;
                        self.input_mode = InputMode::Normal;
                        self.input_buffer.clear();
                    }
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_confirm_delete(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let name = self.profile_names[self.selected_row].clone();
                self.store.delete_profile(&name)?;
                if self.sync_mode == SyncMode::Strict {
                    let _ = gcloud::delete_configuration(&name);
                }
                self.status_message = Some(format!("Deleted profile '{}'.", name));
                self.reload()?;
                self.input_mode = InputMode::Normal;
            }
            _ => {
                self.status_message = None;
                self.input_mode = InputMode::Normal;
            }
        }
        Ok(())
    }

    fn handle_edit_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.suggestion_index = None;
                self.status_message = Some("Edit cancelled.".to_string());
            }
            KeyCode::Down => {
                if self.suggestion_index.is_none() {
                    self.suggestions = if self.input_mode == InputMode::EditAccount {
                        self.build_account_suggestions()
                    } else {
                        self.build_project_suggestions()
                    };
                    if !self.suggestions.is_empty() {
                        self.suggestion_index = Some(0);
                    }
                } else if !self.suggestions.is_empty() {
                    let idx = self.suggestion_index.unwrap_or(0);
                    self.suggestion_index = Some((idx + 1) % self.suggestions.len());
                }
            }
            KeyCode::Up => {
                if let Some(idx) = self.suggestion_index {
                    if !self.suggestions.is_empty() {
                        self.suggestion_index = Some(if idx == 0 {
                            self.suggestions.len() - 1
                        } else {
                            idx - 1
                        });
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(idx) = self.suggestion_index {
                    // Pick suggestion into buffer
                    if let Some(suggestion) = self.suggestions.get(idx) {
                        let suggestion = suggestion.clone();
                        if self.input_mode == InputMode::EditAccount {
                            self.edit_account_buffer = suggestion;
                        } else {
                            self.edit_project_buffer = suggestion;
                        }
                    }
                    self.suggestion_index = None;
                } else {
                    // Save and exit edit mode
                    self.save_edit()?;
                }
            }
            KeyCode::Tab => {
                if self.input_mode == InputMode::EditAccount {
                    self.input_mode = InputMode::EditProject;
                    self.suggestion_index = None;
                    let account = self.edit_account_buffer.trim().to_string();
                    self.start_project_fetch(&account);
                } else {
                    self.save_edit()?;
                }
            }
            KeyCode::Backspace => {
                if self.input_mode == InputMode::EditAccount {
                    self.edit_account_buffer.pop();
                } else {
                    self.edit_project_buffer.pop();
                }
                self.suggestion_index = None;
            }
            KeyCode::Char(c) => {
                if self.input_mode == InputMode::EditAccount {
                    self.edit_account_buffer.push(c);
                } else {
                    self.edit_project_buffer.push(c);
                }
                self.suggestion_index = None;
            }
            _ => {}
        }
        Ok(())
    }

    fn build_account_suggestions(&self) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        for profile in &self.profiles {
            if !profile.user_account.is_empty() {
                seen.insert(profile.user_account.clone());
            }
            if !profile.adc_account.is_empty() {
                seen.insert(profile.adc_account.clone());
            }
        }
        if let Ok(auth_accounts) = gcloud::list_authenticated_accounts() {
            for account in auth_accounts {
                seen.insert(account);
            }
        }
        seen.into_iter().collect()
    }

    fn build_project_suggestions(&self) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        for project in &self.fetched_projects {
            seen.insert(project.clone());
        }
        for profile in &self.profiles {
            if !profile.user_project.is_empty() {
                seen.insert(profile.user_project.clone());
            }
            if !profile.adc_quota_project.is_empty() {
                seen.insert(profile.adc_quota_project.clone());
            }
        }
        seen.into_iter().collect()
    }

    fn save_edit(&mut self) -> Result<()> {
        let name = self.profile_names[self.selected_row].clone();
        let mut profile = self.profiles[self.selected_row].clone();
        match self.edit_col {
            Column::User => {
                profile.user_account = self.edit_account_buffer.trim().to_string();
                profile.user_project = self.edit_project_buffer.trim().to_string();
            }
            Column::Adc => {
                profile.adc_account = self.edit_account_buffer.trim().to_string();
                profile.adc_quota_project = self.edit_project_buffer.trim().to_string();
            }
            _ => {}
        }
        self.store.add_profile(&name, profile)?;
        self.reload()?;
        self.input_mode = InputMode::Normal;
        self.suggestion_index = None;
        self.status_message = Some(format!("Profile '{}' updated.", name));
        Ok(())
    }

    fn activate_selected(&mut self) -> Result<()> {
        // If auth check is still pending, do a synchronous check now
        let user_valid = match self.user_auth_valid.get(self.selected_row).copied() {
            Some(Some(v)) => v,
            _ => {
                let account = &self.profiles[self.selected_row].user_account;
                gcloud::check_account_auth(account)
            }
        };
        let adc_valid = match self.adc_auth_valid.get(self.selected_row).copied() {
            Some(Some(v)) => v,
            _ => {
                let account = &self.profiles[self.selected_row].adc_account;
                gcloud::check_account_auth(account)
            }
        };

        // Defer to main loop if interactive reauth is needed
        let needs_reauth = match self.selected_col {
            Column::Both => !user_valid || !adc_valid,
            Column::User => !user_valid,
            Column::Adc => !adc_valid,
        };
        if needs_reauth {
            self.pending_action = PendingAction::ReauthAndActivate;
            return Ok(());
        }

        self.do_activate()?;
        Ok(())
    }

    /// Execute activation (called directly or after reauth completes).
    pub fn do_activate(&mut self) -> Result<()> {
        let name = self.profile_names[self.selected_row].clone();
        let profile = self.profiles[self.selected_row].clone();

        match self.selected_col {
            Column::Both => {
                gcloud::activate_both(
                    &self.store,
                    &name,
                    &profile.user_account,
                    &profile.user_project,
                )?;
                self.status_message = Some(format!("Activated profile '{}'.", name));
            }
            Column::User => {
                gcloud::activate_user(&name, &profile.user_account, &profile.user_project)?;
                self.status_message = Some(format!("Activated user config for '{}'.", name));
            }
            Column::Adc => {
                gcloud::activate_adc(&self.store, &name)?;
                self.status_message = Some(format!("Activated ADC for '{}'.", name));
            }
        }

        self.active_profile = Some(name.clone());
        let mut data = self.store.load_profiles()?;
        data.active_profile = Some(name.clone());
        self.store.save_profiles(&data)?;

        Ok(())
    }

    /// Execute a reauth that was deferred for TUI suspension.
    pub fn execute_reauth(&mut self) -> Result<()> {
        let name = self.profile_names[self.selected_row].clone();
        let profile = self.profiles[self.selected_row].clone();

        match self.selected_col {
            Column::Both => {
                gcloud::reauth_user(&profile.user_account)?;
                gcloud::activate_user(&name, &profile.user_account, &profile.user_project)?;
                gcloud::reauth_adc(&self.store, &name, &profile.adc_quota_project)?;
                self.status_message =
                    Some(format!("Re-authenticated user and ADC for '{}'.", name));
            }
            Column::User => {
                gcloud::reauth_user(&profile.user_account)?;
                gcloud::activate_user(&name, &profile.user_account, &profile.user_project)?;
                self.status_message =
                    Some(format!("User re-authenticated for '{}'.", name));
            }
            Column::Adc => {
                gcloud::reauth_adc(&self.store, &name, &profile.adc_quota_project)?;
                self.status_message = Some(format!("ADC re-authenticated for '{}'.", name));
            }
        }

        self.reload()?;
        Ok(())
    }
}
