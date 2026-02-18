mod app;
mod gcloud;
mod profile;
mod store;
mod sync;
mod ui;

use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, PendingAction};
use crate::profile::{Profile, SyncMode};
use crate::store::Store;

#[derive(Parser)]
#[command(name = "gcloud-switch", version, about = "TUI Google Cloud profile switcher")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new profile
    Add {
        /// Profile name
        name: String,
        /// User account email
        #[arg(long)]
        account: String,
        /// User project
        #[arg(long)]
        project: String,
        /// ADC account email (defaults to user account)
        #[arg(long)]
        adc_account: Option<String>,
        /// ADC quota project (defaults to user project)
        #[arg(long)]
        adc_quota_project: Option<String>,
    },
    /// List all profiles
    List,
    /// Switch to a profile
    Switch {
        /// Profile name
        name: String,
    },
    /// Import existing gcloud configurations
    Import,
    /// Check for and install new releases from GitHub
    SelfUpdate {
        /// Do not download README.md after installing
        #[arg(long)]
        no_download_readme: bool,
        /// Do not open README.md after downloading (only applies if download runs)
        #[arg(long)]
        no_open_readme: bool,
        /// Only check if an update is available; do not install or download README
        #[arg(long)]
        check_only: bool,
    },
    /// Sync profile metadata (profiles.toml only) via a Git remote
    Sync {
        #[command(subcommand)]
        sub: SyncSub,
    },
}

#[derive(Subcommand)]
enum SyncSub {
    /// Set remote URL and optionally clone (run first before push/pull)
    Init {
        /// Git remote URL (e.g. https://github.com/user/repo.git or git@github.com:user/repo.git)
        remote_url: String,
        /// Branch name (default: main)
        #[arg(long, default_value = "main")]
        branch: String,
    },
    /// Push current profiles to the remote
    Push,
    /// Pull and merge profiles from the remote (newer wins per profile)
    Pull,
}

/// User-level parameters in ~/.config/gcloud-switch/gcloud-switch.toml. Profile data stays in profiles.toml.
/// This file is created on first run when the program needs to persist settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GlobalSettings {
    /// When to check for updates: "never", "always", "daily". Default "always".
    #[serde(default = "default_self_update_frequency")]
    self_update_frequency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_update_check: Option<String>,
}

fn default_self_update_frequency() -> String {
    "always".to_string()
}

fn global_settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("gcloud-switch").join("gcloud-switch.toml"))
}

fn load_global_settings() -> GlobalSettings {
    let path = match global_settings_path() {
        Some(p) => p,
        None => return GlobalSettings::default(),
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return GlobalSettings::default(),
    };
    toml::from_str(&content).unwrap_or_default()
}

fn save_global_settings(settings: &GlobalSettings) -> Result<()> {
    let path = global_settings_path().context("Could not determine config directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let toml = toml::to_string_pretty(settings).context("Serialize global settings")?;
    std::fs::write(&path, toml)?;
    Ok(())
}

fn check_update_available(client: &reqwest::blocking::Client) -> Result<Option<(String, String)>> {
    let url = format!("{}/{}/releases/latest", API_URL, REPO);
    let response = client.get(&url).send()?;
    if !response.status().is_success() {
        return Ok(None);
    }
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        html_url: String,
    }
    let release: Release = response.json()?;
    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    let current = env!("CARGO_PKG_VERSION");
    if compare_versions(current, &latest_version) < 0 {
        Ok(Some((latest_version, release.html_url)))
    } else {
        Ok(None)
    }
}

fn maybe_check_for_updates(settings: &mut GlobalSettings) -> Result<()> {
    let freq = settings.self_update_frequency.as_str();
    if freq == "never" {
        return Ok(());
    }
    if freq == "daily" {
        if let Some(ref last) = settings.last_update_check {
            let last_ts: u64 = last.parse().unwrap_or(0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if now.saturating_sub(last_ts) < 86400 {
                return Ok(());
            }
        }
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent("gcloud-switch-update-checker")
        .build()?;
    let update = check_update_available(&client)?;
    if freq == "daily" {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        settings.last_update_check = Some(now.to_string());
        let _ = save_global_settings(settings);
    }
    if let Some((version, url)) = update {
        println!(
            "⚠️  Update available: {} (current: {}). Run `gcloud-switch self-update` to install. {}",
            version,
            env!("CARGO_PKG_VERSION"),
            url
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Optional: check for updates per global settings (~/.config/gcloud-switch/gcloud-switch.toml)
    let mut global_settings = load_global_settings();
    if !matches!(cli.command, Some(Commands::SelfUpdate { .. })) {
        let _ = maybe_check_for_updates(&mut global_settings);
    }

    match cli.command {
        Some(Commands::Add {
            name,
            account,
            project,
            adc_account,
            adc_quota_project,
        }) => {
            let store = Store::new()?;
            let data = store.load_profiles()?;
            let profile = Profile {
                user_account: account.clone(),
                user_project: project.clone(),
                adc_account: adc_account.unwrap_or_else(|| account.clone()),
                adc_quota_project: adc_quota_project.unwrap_or_else(|| project.clone()),
                updated_at: None,
            };
            // Create gcloud configuration first so the profile won't be orphaned
            if matches!(data.sync_mode, SyncMode::Strict | SyncMode::Add) {
                gcloud::create_configuration(&name, &profile.user_account, &profile.user_project)?;
            }
            store.add_profile(&name, profile.clone())?;
            println!("Profile '{}' added.", name);
        }
        Some(Commands::List) => {
            let store = Store::new()?;
            let data = store.load_profiles()?;
            if data.profiles.is_empty() {
                println!("No profiles configured. Use 'gcloud-switch add' or press 'a' in the TUI.");
            } else {
                for (name, profile) in &data.profiles {
                    let active = if data.active_profile.as_deref() == Some(name.as_str()) {
                        " (active)"
                    } else {
                        ""
                    };
                    println!(
                        "{}{}: user={}@{} adc={}@{}",
                        name,
                        active,
                        profile.user_account,
                        profile.user_project,
                        profile.adc_account,
                        profile.adc_quota_project,
                    );
                }
            }
        }
        Some(Commands::Switch { name }) => {
            let store = Store::new()?;
            let mut data = store.load_profiles()?;
            let profile = data
                .profiles
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found", name))?;
            gcloud::activate_both(&store, &name, &profile.user_account, &profile.user_project)?;
            data.active_profile = Some(name.clone());
            store.save_profiles(&data)?;
            println!("Switched to profile '{}'.", name);
        }
        Some(Commands::Import) => {
            let store = Store::new()?;
            let count = import_profiles(&store)?;
            if count == 0 {
                println!("No new gcloud configurations found to import.");
            }
        }
        Some(Commands::SelfUpdate {
            no_download_readme,
            no_open_readme,
            check_only,
        }) => {
            run_self_update(!no_download_readme, !no_open_readme, check_only)?;
        }
        Some(Commands::Sync { sub }) => {
            let store = Store::new()?;
            let config_path = store.sync_config_path();
            match sub {
                SyncSub::Init { remote_url, branch } => {
                    let config = sync::SyncConfig {
                        remote_url,
                        branch,
                    };
                    sync::save_sync_config(&config_path, &config)?;
                    println!("Sync config saved. Run 'gcloud-switch sync push' to push, or 'sync pull' to pull.");
                    if let Some(ref cfg) = sync::load_sync_config(&config_path)? {
                        sync::ensure_cloned(&store, cfg)?;
                        println!("Remote cloned to {}.", store.sync_repo_path().display());
                    }
                }
                SyncSub::Push => {
                    let config = sync::load_sync_config(&config_path)?
                        .ok_or_else(|| anyhow::anyhow!("Sync not configured. Run 'gcloud-switch sync init <remote_url>' first."))?;
                    sync::sync_push(&store, &config)?;
                    println!("Pushed profiles to remote.");
                }
                SyncSub::Pull => {
                    let config = sync::load_sync_config(&config_path)?
                        .ok_or_else(|| anyhow::anyhow!("Sync not configured. Run 'gcloud-switch sync init <remote_url>' first."))?;
                    sync::sync_pull(&store, &config)?;
                    println!("Pulled and merged profiles from remote.");
                }
            }
        }
        None => {
            run_tui()?;
        }
    }

    Ok(())
}

fn import_profiles(store: &Store) -> Result<usize> {
    let configs = gcloud::discover_existing_configs()?;
    if configs.is_empty() {
        return Ok(0);
    }

    let mut data = store.load_profiles()?;
    let mut count = 0;

    for (name, account, project) in &configs {
        if data.profiles.contains_key(name) {
            println!("Skipping '{}' (already exists).", name);
            continue;
        }

        let mut profile = Profile {
            user_account: account.clone(),
            user_project: project.clone(),
            adc_account: account.clone(),
            adc_quota_project: project.clone(),
            updated_at: None,
        };
        profile.touch();
        data.profiles.insert(name.clone(), profile);
        println!("Imported '{}'.", name);
        count += 1;
    }

    // Set active profile from gcloud's active configuration
    if count > 0 {
        if let Ok(Some(active)) = gcloud::read_active_config() {
            if data.profiles.contains_key(&active) {
                data.active_profile = Some(active.clone());
                println!("Active profile set to '{}'.", active);
            }
        }
        store.save_profiles(&data)?;
    }

    Ok(count)
}

fn sync_on_startup(store: &Store) -> Result<()> {
    let mut data = store.load_profiles()?;

    // First run: import if no profiles exist
    if data.profiles.is_empty() {
        import_profiles(store)?;
        return Ok(());
    }

    let mut changed = false;

    match data.sync_mode {
        SyncMode::Off => {}
        SyncMode::Add | SyncMode::Strict => {
            let configs = gcloud::discover_existing_configs()?;
            let config_names: std::collections::HashSet<String> =
                configs.iter().map(|(n, _, _)| n.clone()).collect();

            // Add new gcloud configs as profiles
            for (name, account, project) in &configs {
                if !data.profiles.contains_key(name) {
                    let mut profile = Profile {
                        user_account: account.clone(),
                        user_project: project.clone(),
                        adc_account: account.clone(),
                        adc_quota_project: project.clone(),
                        updated_at: None,
                    };
                    profile.touch();
                    data.profiles.insert(name.clone(), profile);
                    changed = true;
                }
            }

            // In strict mode, delete profiles whose gcloud configs no longer exist
            if data.sync_mode == SyncMode::Strict {
                let to_delete: Vec<String> = data
                    .profiles
                    .keys()
                    .filter(|name| !config_names.contains(*name))
                    .cloned()
                    .collect();
                for name in &to_delete {
                    data.profiles.remove(name);
                    if data.active_profile.as_deref() == Some(name) {
                        data.active_profile = None;
                    }
                    // Remove ADC file if it exists
                    let adc_path = store.adc_path(name);
                    if adc_path.exists() {
                        let _ = std::fs::remove_file(adc_path);
                    }
                    changed = true;
                }
            }
        }
    }

    // Always sync active config from gcloud
    if let Ok(Some(active)) = gcloud::read_active_config() {
        if data.profiles.contains_key(&active) && data.active_profile.as_deref() != Some(&active) {
            data.active_profile = Some(active);
            changed = true;
        }
    }

    if changed {
        store.save_profiles(&data)?;
    }

    Ok(())
}

fn run_tui() -> Result<()> {
    let store = Store::new()?;
    sync_on_startup(&store)?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new()?;

    let loop_result: Result<()> = (|| {
        loop {
            app.check_auth_results();
            app.check_project_results();
            terminal.draw(|frame| ui::draw(frame, &mut app))?;

            if app.handle_event()? {
                break;
            }

            // Handle pending actions that need TUI suspended (interactive gcloud commands)
            if !matches!(app.pending_action, PendingAction::None) {
                let is_activate = matches!(app.pending_action, PendingAction::ReauthAndActivate);
                app.pending_action = PendingAction::None;

                // Suspend TUI: leave alternate screen and restore normal terminal mode
                disable_raw_mode()?;
                execute!(
                    io::stdout(),
                    LeaveAlternateScreen,
                    DisableMouseCapture,
                    crossterm::cursor::Show
                )?;
                {
                    use std::io::Write;
                    io::stdout().flush()?;
                }

                // Run interactive gcloud commands
                let reauth_result = app.execute_reauth();

                // If reauth succeeded and this was an activate flow, do the activation
                if is_activate && reauth_result.is_ok() {
                    let _ = app.do_activate();
                    if app.quit_after_activate {
                        if let Some(msg) = &app.status_message {
                            use std::io::Write;
                            print!("\r\n{}\r\n", msg);
                            io::stdout().flush()?;
                        }
                        return Ok(());
                    }
                }

                // Resume TUI
                enable_raw_mode()?;
                execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
                // Force ratatui to do a full redraw since the screen was cleared
                terminal.clear()?;
            }
        }
        Ok(())
    })();

    // Always restore terminal, even if the loop returned an error
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        crossterm::style::ResetColor,
        crossterm::cursor::MoveToColumn(0)
    );
    let _ = terminal.show_cursor();
    use std::io::Write;
    let _ = io::stdout().flush();

    // Print final status message if any
    if let Some(msg) = &app.status_message {
        print!("\r\n{}\r\n", msg);
        let _ = io::stdout().flush();
    }

    loop_result
}

const REPO: &str = "tjirsch/rs-gcloud-switch";
const API_URL: &str = "https://api.github.com/repos";

fn run_self_update(download_readme: bool, open_readme: bool, check_only: bool) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    println!("Current version: {}", current_version);

    let client = reqwest::blocking::Client::builder()
        .user_agent("gcloud-switch-update-checker")
        .build()?;

    let url = format!("{}/{}/releases/latest", API_URL, REPO);
    let response = client.get(&url).send()?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to fetch release info: {}", response.status());
    }

    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        html_url: String,
    }

    let release: Release = response.json()?;
    let latest_version = release.tag_name.trim_start_matches('v');
    println!("Latest version: {}", latest_version);

    if compare_versions(current_version, latest_version) < 0 {
        println!("\n⚠️  A new version is available!");
        println!("   Current: {}", current_version);
        println!("   Latest:  {}", latest_version);
        println!("   Release: {}", release.html_url);
        if check_only {
            println!("\nRun `gcloud-switch self-update` to install.");
            return Ok(());
        }
        println!("\n📥 Installing update...");

        let installer_url = format!(
            "https://github.com/{}/releases/latest/download/gcloud-switch-installer.sh",
            REPO
        );
        let installer_script = client.get(&installer_url).send()?.text()?;
        let temp_file = std::env::temp_dir()
            .join(format!("gcloud-switch-installer-{}.sh", std::process::id()));
        std::fs::write(&temp_file, installer_script)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp_file, std::fs::Permissions::from_mode(0o755))?;

            let status = std::process::Command::new("sh").arg(&temp_file).status()?;
            let _ = std::fs::remove_file(&temp_file);

            if status.success() {
                println!("✅ Update installed successfully!");
                println!("   Please restart your terminal or run: source ~/.profile");
                if download_readme {
                    match download_and_open_readme(&client, REPO, latest_version, open_readme) {
                        Ok(Some(path)) => println!("README: {}", path.display()),
                        Ok(None) => {}
                        Err(e) => eprintln!("⚠️  Warning: Could not download README: {}", e),
                    }
                }
            } else {
                anyhow::bail!("Failed to run installer script");
            }
        }

        #[cfg(windows)]
        {
            anyhow::bail!(
                "Automatic installation on Windows is not yet supported. Please download and run the installer manually."
            );
        }
    } else {
        println!("✅ You are running the latest version!");
    }

    Ok(())
}

fn download_and_open_readme(
    client: &reqwest::blocking::Client,
    repo: &str,
    version: &str,
    open_after_download: bool,
) -> Result<Option<PathBuf>> {
    let download_dir = get_download_dir()?;
    let readme_path = download_dir.join(format!("gcloud-switch-{}-README.md", version));
    let readme_url = format!("https://raw.githubusercontent.com/{}/main/README.md", repo);
    println!("\n📄 Downloading README...");
    let readme_content = client.get(&readme_url).send()?.text()?;
    std::fs::write(&readme_path, readme_content)?;
    if open_after_download {
        println!("   Opening README...");
        open_file(&readme_path)?;
    }
    Ok(Some(readme_path))
}

fn get_download_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME")?;
        Ok(PathBuf::from(home).join("Downloads"))
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(dir) = std::env::var("XDG_DOWNLOAD_DIR") {
            Ok(PathBuf::from(dir))
        } else {
            let home = std::env::var("HOME")?;
            Ok(PathBuf::from(home).join("Downloads"))
        }
    }

    #[cfg(target_os = "windows")]
    {
        let user_profile = std::env::var("USERPROFILE")?;
        Ok(PathBuf::from(user_profile).join("Downloads"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        anyhow::bail!("Unsupported platform for download directory");
    }
}

fn open_file(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).status()?;
    }

    #[cfg(target_os = "linux")]
    {
        if std::process::Command::new("xdg-open")
            .arg(path)
            .status()
            .is_err()
        {
            if let Ok(editor) = std::env::var("EDITOR") {
                std::process::Command::new(editor).arg(path).status()?;
            } else {
                anyhow::bail!("Could not open file: xdg-open not available and EDITOR not set");
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let path_str = path
            .to_str()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "path is not valid UTF-8"))?;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", path_str])
            .status()?;
    }

    Ok(())
}

fn compare_versions(v1: &str, v2: &str) -> i32 {
    let parse_version = |v: &str| -> Vec<u32> { v.split('.').map(|s| s.parse::<u32>().unwrap_or(0)).collect() };
    let v1_parts = parse_version(v1);
    let v2_parts = parse_version(v2);
    let max_len = v1_parts.len().max(v2_parts.len());
    for i in 0..max_len {
        let a = v1_parts.get(i).copied().unwrap_or(0);
        let b = v2_parts.get(i).copied().unwrap_or(0);
        if a < b {
            return -1;
        }
        if a > b {
            return 1;
        }
    }
    0
}
