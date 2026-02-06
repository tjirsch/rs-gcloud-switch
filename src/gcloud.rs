use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::store::Store;

fn gcloud_config_dir() -> Result<PathBuf> {
    // gcloud always uses ~/.config/gcloud on all platforms, ignoring XDG/macOS conventions,
    // unless CLOUDSDK_CONFIG is set.
    if let Ok(custom) = std::env::var("CLOUDSDK_CONFIG") {
        return Ok(PathBuf::from(custom));
    }
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".config").join("gcloud"))
}

fn configurations_dir() -> Result<PathBuf> {
    let dir = gcloud_config_dir()?.join("configurations");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Write a gcloud configuration file for the given profile.
fn write_gcloud_configuration(profile_name: &str, account: &str, project: &str) -> Result<()> {
    let dir = configurations_dir()?;
    let config_path = dir.join(format!("config_{}", profile_name));

    let content = format!(
        "[core]\naccount = {}\nproject = {}\n",
        account, project
    );
    fs::write(&config_path, content)
        .with_context(|| format!("Failed to write gcloud config at {}", config_path.display()))?;
    Ok(())
}

/// Set the active gcloud configuration.
fn set_active_config(profile_name: &str) -> Result<()> {
    let config_dir = gcloud_config_dir()?;
    let active_config_path = config_dir.join("active_config");
    fs::write(&active_config_path, profile_name).with_context(|| {
        format!(
            "Failed to write active_config at {}",
            active_config_path.display()
        )
    })?;
    Ok(())
}

/// Copy a stored ADC JSON file to gcloud's application_default_credentials.json.
fn copy_adc_to_gcloud(store: &Store, profile_name: &str) -> Result<()> {
    let src = store.adc_path(profile_name);
    if !src.exists() {
        anyhow::bail!(
            "No ADC credentials stored for profile '{}'. Run re-auth (r) first.",
            profile_name
        );
    }
    let config_dir = gcloud_config_dir()?;
    let dest = config_dir.join("application_default_credentials.json");
    fs::copy(&src, &dest).with_context(|| {
        format!(
            "Failed to copy ADC from {} to {}",
            src.display(),
            dest.display()
        )
    })?;
    Ok(())
}

/// Activate a profile's user credentials (gcloud config).
pub fn activate_user(profile_name: &str, account: &str, project: &str) -> Result<()> {
    write_gcloud_configuration(profile_name, account, project)?;
    set_active_config(profile_name)?;
    Ok(())
}

/// Activate a profile's ADC credentials.
pub fn activate_adc(store: &Store, profile_name: &str) -> Result<()> {
    copy_adc_to_gcloud(store, profile_name)
}

/// Activate both user and ADC credentials for a profile.
pub fn activate_both(
    store: &Store,
    profile_name: &str,
    account: &str,
    project: &str,
) -> Result<()> {
    activate_user(profile_name, account, project)?;
    // ADC activation is best-effort if no ADC file exists yet
    if store.has_adc(profile_name) {
        activate_adc(store, profile_name)?;
    }
    Ok(())
}

/// Re-authenticate user credentials via `gcloud auth login`.
pub fn reauth_user(account: &str) -> Result<()> {
    let status = Command::new("gcloud")
        .args(["auth", "login", &format!("--account={}", account)])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("Failed to run gcloud auth login")?;
    if !status.success() {
        anyhow::bail!("gcloud auth login failed with status {}", status);
    }
    Ok(())
}

/// Re-authenticate ADC via `gcloud auth application-default login`, then store the result.
pub fn reauth_adc(store: &Store, profile_name: &str, quota_project: &str) -> Result<()> {
    let status = Command::new("gcloud")
        .args([
            "auth",
            "application-default",
            "login",
            "--quiet",
        ])
        .status()
        .context("Failed to run gcloud auth application-default login")?;
    if !status.success() {
        anyhow::bail!(
            "gcloud auth application-default login failed with status {}",
            status
        );
    }

    // Set quota project
    let _ = Command::new("gcloud")
        .args([
            "auth",
            "application-default",
            "set-quota-project",
            quota_project,
        ])
        .status();

    // Copy the newly created ADC to our store
    let config_dir = gcloud_config_dir()?;
    let adc_src = config_dir.join("application_default_credentials.json");
    if adc_src.exists() {
        let content = fs::read_to_string(&adc_src)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;
        store.save_adc_json(profile_name, &value)?;
    }

    Ok(())
}

/// Try to resolve the account email from an ADC JSON file by calling Google's userinfo endpoint.
pub async fn resolve_adc_account(adc_json: &serde_json::Value) -> Result<Option<String>> {
    // Extract the client_id, client_secret, and refresh_token
    let client_id = adc_json
        .get("client_id")
        .and_then(|v| v.as_str())
        .context("ADC JSON missing client_id")?;
    let client_secret = adc_json
        .get("client_secret")
        .and_then(|v| v.as_str())
        .context("ADC JSON missing client_secret")?;
    let refresh_token = adc_json
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .context("ADC JSON missing refresh_token")?;

    // Exchange refresh token for access token
    let client = reqwest::Client::new();
    let token_resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await?;

    if !token_resp.status().is_success() {
        return Ok(None);
    }

    let token_json: serde_json::Value = token_resp.json().await?;
    let access_token = match token_json.get("access_token").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return Ok(None),
    };

    // Call userinfo endpoint
    let userinfo_resp = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(&access_token)
        .send()
        .await?;

    if !userinfo_resp.status().is_success() {
        return Ok(None);
    }

    let userinfo: serde_json::Value = userinfo_resp.json().await?;
    Ok(userinfo.get("email").and_then(|v| v.as_str()).map(String::from))
}

/// Read credentials for an account from gcloud's credentials.db.
pub fn read_gcloud_credentials(account: &str) -> Result<Option<serde_json::Value>> {
    let db_path = gcloud_config_dir()?.join("credentials.db");
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Failed to open credentials.db at {}", db_path.display()))?;
    let mut stmt = conn.prepare("SELECT value FROM credentials WHERE account_id = ?1")?;
    let result: Option<String> = stmt
        .query_row(rusqlite::params![account], |row| row.get(0))
        .ok();
    match result {
        Some(blob) => {
            let value: serde_json::Value = serde_json::from_str(&blob)
                .context("Failed to parse credentials blob as JSON")?;
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

/// Validate a refresh token by attempting a token exchange.
pub fn validate_token_blocking(credentials: &serde_json::Value) -> Result<bool> {
    let client_id = credentials
        .get("client_id")
        .and_then(|v| v.as_str())
        .context("credentials missing client_id")?;
    let client_secret = credentials
        .get("client_secret")
        .and_then(|v| v.as_str())
        .context("credentials missing client_secret")?;
    let refresh_token = credentials
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .context("credentials missing refresh_token")?;
    let token_uri = credentials
        .get("token_uri")
        .and_then(|v| v.as_str())
        .unwrap_or("https://oauth2.googleapis.com/token");

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(token_uri)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()?;

    Ok(resp.status().is_success())
}

/// Check whether an account's gcloud credentials are valid.
/// Returns false on any error (missing from DB, invalid token, network issue).
/// Runs the blocking HTTP call on a dedicated thread to avoid panicking
/// when called from within a tokio runtime.
pub fn check_account_auth(account: &str) -> bool {
    let creds = match read_gcloud_credentials(account) {
        Ok(Some(c)) => c,
        _ => return false,
    };
    std::thread::spawn(move || validate_token_blocking(&creds).unwrap_or(false))
        .join()
        .unwrap_or(false)
}

/// List all account emails that have stored credentials in credentials.db.
pub fn list_authenticated_accounts() -> Result<Vec<String>> {
    let db_path = gcloud_config_dir()?.join("credentials.db");
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .with_context(|| format!("Failed to open credentials.db at {}", db_path.display()))?;
    let mut stmt = conn.prepare("SELECT account_id FROM credentials")?;
    let accounts: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(accounts)
}

/// Import existing gcloud configurations as profiles.
pub fn discover_existing_configs() -> Result<Vec<(String, String, String)>> {
    let dir = match configurations_dir() {
        Ok(d) => d,
        Err(_) => return Ok(vec![]),
    };

    let mut results = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if let Some(name) = file_name.strip_prefix("config_") {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    let mut account = String::new();
                    let mut project = String::new();
                    for line in content.lines() {
                        let line = line.trim();
                        if let Some(val) = line.strip_prefix("account = ") {
                            account = val.trim().to_string();
                        }
                        if let Some(val) = line.strip_prefix("project = ") {
                            project = val.trim().to_string();
                        }
                    }
                    if !account.is_empty() {
                        results.push((name.to_string(), account, project));
                    }
                }
            }
        }
    }
    Ok(results)
}
