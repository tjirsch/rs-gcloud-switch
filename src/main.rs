mod app;
mod gcloud;
mod profile;
mod store;
mod ui;

use std::io;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::App;
use crate::profile::Profile;
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Add {
            name,
            account,
            project,
            adc_account,
            adc_quota_project,
        }) => {
            let store = Store::new()?;
            let profile = Profile {
                user_account: account.clone(),
                user_project: project.clone(),
                adc_account: adc_account.unwrap_or_else(|| account.clone()),
                adc_quota_project: adc_quota_project.unwrap_or_else(|| project.clone()),
            };
            store.add_profile(&name, profile)?;
            println!("Profile '{}' added.", name);
        }
        Some(Commands::List) => {
            let store = Store::new()?;
            let profiles = store.load_profiles()?;
            let state = store.load_state()?;
            if profiles.profiles.is_empty() {
                println!("No profiles configured. Use 'gcloud-switch add' or press 'a' in the TUI.");
            } else {
                for (name, profile) in &profiles.profiles {
                    let active = if state.active_profile.as_deref() == Some(name.as_str()) {
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
            let profiles = store.load_profiles()?;
            let profile = profiles
                .profiles
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found", name))?;
            gcloud::activate_both(&store, &name, &profile.user_account, &profile.user_project)?;
            store.save_state(&profile::StateFile {
                active_profile: Some(name.clone()),
            })?;
            println!("Switched to profile '{}'.", name);
        }
        Some(Commands::Import) => {
            run_import().await?;
        }
        None => {
            run_tui()?;
        }
    }

    Ok(())
}

async fn run_import() -> Result<()> {
    let store = Store::new()?;
    let configs = gcloud::discover_existing_configs()?;

    if configs.is_empty() {
        println!("No existing gcloud configurations found to import.");
        return Ok(());
    }

    let existing = store.load_profiles()?;

    for (name, account, project) in &configs {
        if existing.profiles.contains_key(name) {
            println!("Skipping '{}' (already exists).", name);
            continue;
        }

        // Check for existing ADC and try to resolve account
        let gcloud_dir = dirs::home_dir()
            .unwrap()
            .join(".config/gcloud/application_default_credentials.json");
        let mut adc_account = account.clone();
        if gcloud_dir.exists() {
            if let Ok(content) = std::fs::read_to_string(&gcloud_dir) {
                if let Ok(adc_json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Ok(Some(email)) = gcloud::resolve_adc_account(&adc_json).await {
                        adc_account = email;
                    }
                }
            }
        }

        let profile = Profile {
            user_account: account.clone(),
            user_project: project.clone(),
            adc_account,
            adc_quota_project: project.clone(),
        };
        store.add_profile(name, profile)?;
        println!("Imported '{}'.", name);
    }

    Ok(())
}

fn run_tui() -> Result<()> {
    // Check for first run and offer import
    let store = Store::new()?;
    let profiles = store.load_profiles()?;
    if profiles.profiles.is_empty() {
        let configs = gcloud::discover_existing_configs()?;
        if !configs.is_empty() {
            eprintln!(
                "Found {} existing gcloud configuration(s). Run 'gcloud-switch import' to import them.",
                configs.len()
            );
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new()?;

    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;

        if app.handle_event()? {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        crossterm::style::ResetColor,
        crossterm::cursor::MoveToColumn(0)
    )?;
    terminal.show_cursor()?;
    // Flush to ensure all escape sequences are written before println
    use std::io::Write;
    io::stdout().flush()?;

    // Print final status message if any
    if let Some(msg) = &app.status_message {
        print!("\r\n{}\r\n", msg);
        io::stdout().flush()?;
    }

    Ok(())
}
