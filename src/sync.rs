//! Sync profile metadata (profiles.toml only) via a user-supplied Git remote.
//! No credentials are synced; only profile names and account/project identifiers.
//! Uses the system `git` CLI so the user's git auth (SSH or credential helper) is used.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::profile::{Profile, ProfilesFile};
use crate::store::Store;

const SYNC_FILE: &str = "profiles.toml";
const DEFAULT_BRANCH: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// Git remote URL (e.g. https://github.com/user/repo.git or git@github.com:user/repo.git)
    pub remote_url: String,
    /// Branch to push/pull (default main)
    #[serde(default = "default_branch")]
    pub branch: String,
}

fn default_branch() -> String {
    DEFAULT_BRANCH.to_string()
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            remote_url: String::new(),
            branch: DEFAULT_BRANCH.to_string(),
        }
    }
}

/// Load sync config from path. Returns None if file does not exist or is empty.
pub fn load_sync_config(path: &Path) -> Result<Option<SyncConfig>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let content = content.trim();
    if content.is_empty() {
        return Ok(None);
    }
    let config: SyncConfig =
        toml::from_str(content).with_context(|| "Failed to parse sync-config.toml")?;
    if config.remote_url.is_empty() {
        return Ok(None);
    }
    Ok(Some(config))
}

/// Save sync config to path.
pub fn save_sync_config(path: &Path, config: &SyncConfig) -> Result<()> {
    let content = toml::to_string_pretty(config).context("Failed to serialize sync config")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn run_git(repo_path: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("git")
        .current_dir(repo_path)
        .args(args)
        .output()
        .context("Failed to run git")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git failed: {}", stderr);
    }
    Ok(out.stdout)
}

/// Ensure sync repo is cloned. If it doesn't exist, clone the remote (or init + remote if empty).
pub fn ensure_cloned(store: &Store, config: &SyncConfig) -> Result<()> {
    let repo_path = store.sync_repo_path();
    if repo_path.join(".git").exists() {
        return Ok(());
    }
    let parent = repo_path.parent().context("repo path has no parent")?;
    fs::create_dir_all(parent)?;
    let path_str = repo_path.as_os_str().to_str().context("repo path")?;
    let s = Command::new("git")
        .current_dir(parent)
        .args(["clone", "--branch", config.branch.as_str(), config.remote_url.as_str(), path_str])
        .status();
    if s.as_ref().map(|st| !st.success()).unwrap_or(true) {
        let s2 = Command::new("git")
            .current_dir(parent)
            .args(["clone", config.remote_url.as_str(), path_str])
            .status();
        if s2.as_ref().map(|st| !st.success()).unwrap_or(true) {
            // Empty remote: init and add remote; first push will create the branch
            fs::create_dir_all(&repo_path)?;
            Command::new("git").current_dir(&repo_path).args(["init"]).status().context("git init")?;
            Command::new("git")
                .current_dir(&repo_path)
                .args(["remote", "add", "origin", config.remote_url.as_str()])
                .status()
                .context("git remote add")?;
        }
    }
    Ok(())
}

/// Push current profiles.toml to the remote. Clones if needed.
pub fn sync_push(store: &Store, config: &SyncConfig) -> Result<()> {
    ensure_cloned(store, config)?;
    let repo_path = store.sync_repo_path();
    let sync_file_path = repo_path.join(SYNC_FILE);

    let data = store.load_profiles()?;
    let content = toml::to_string_pretty(&data).context("Failed to serialize profiles.toml")?;
    fs::write(&sync_file_path, content)?;

    run_git(&repo_path, &["add", SYNC_FILE])?;
    if run_git(&repo_path, &["commit", "-m", "gcloud-switch sync"]).is_err() {
        // Nothing to commit (working tree clean) is ok
    }
    run_git(
        &repo_path,
        &["push", "-u", "origin", config.branch.as_str()],
    )?;
    Ok(())
}

/// Fetch and merge: get remote profiles.toml, merge by timestamp (newer wins), resolve conflicts by prompting.
pub fn sync_pull(store: &Store, config: &SyncConfig) -> Result<()> {
    ensure_cloned(store, config)?;
    let repo_path = store.sync_repo_path();

    run_git(&repo_path, &["fetch", "origin", config.branch.as_str()])?;

    let remote_ref = format!("origin/{}", config.branch);
    let remote_content = run_git(
        &repo_path,
        &["show", format!("{}:{}", remote_ref, SYNC_FILE).as_str()],
    )
    .unwrap_or_else(|_| Vec::new());

    let remote_content = String::from_utf8_lossy(&remote_content).to_string();
    let local = store.load_profiles()?;
    let remote_profiles: ProfilesFile = toml::from_str(&remote_content)
        .unwrap_or_else(|_| ProfilesFile::default());

    let merged = merge_profiles(&local, &remote_profiles)?;
    store.save_profiles(&merged)?;

    // Update sync repo so next push is clean: checkout branch to remote, replace file with merged, commit
    run_git(&repo_path, &["checkout", "-B", config.branch.as_str(), remote_ref.as_str()])?;
    let content = toml::to_string_pretty(&merged)?;
    fs::write(repo_path.join(SYNC_FILE), content)?;
    run_git(&repo_path, &["add", SYNC_FILE])?;
    if run_git(&repo_path, &["commit", "-m", "gcloud-switch sync merge"]).is_err() {
        // No change after merge is ok
    }

    Ok(())
}

/// Merge local and remote: newer wins per profile; new remote profiles inserted; on conflict prompt which to keep.
fn merge_profiles(local: &ProfilesFile, remote: &ProfilesFile) -> Result<ProfilesFile> {
    let mut out = local.clone();
    for (name, remote_prof) in &remote.profiles {
        match out.profiles.get(name) {
            Some(local_prof) => {
                let local_ts = local_prof.updated_at.unwrap_or(0);
                let remote_ts = remote_prof.updated_at.unwrap_or(0);
                if remote_ts > local_ts {
                    out.profiles.insert(name.clone(), remote_prof.clone());
                } else if remote_ts == local_ts && remote_ts != 0 && *local_prof != *remote_prof {
                    let choice = prompt_which_to_keep(name, local_prof, remote_prof)?;
                    match choice {
                        MergeChoice::Local => {}
                        MergeChoice::Remote => {
                            out.profiles.insert(name.clone(), remote_prof.clone());
                        }
                    }
                }
            }
            None => {
                out.profiles.insert(name.clone(), remote_prof.clone());
            }
        }
    }
    Ok(out)
}

enum MergeChoice {
    Local,
    Remote,
}

fn prompt_which_to_keep(name: &str, local: &Profile, remote: &Profile) -> Result<MergeChoice> {
    eprintln!("Profile '{}' changed on both sides.", name);
    eprintln!(
        "  Local:  {} / {} (adc: {} / {})",
        local.user_account,
        local.user_project,
        local.adc_account,
        local.adc_quota_project
    );
    eprintln!(
        "  Remote: {} / {} (adc: {} / {})",
        remote.user_account,
        remote.user_project,
        remote.adc_account,
        remote.adc_quota_project
    );
    eprint!("Keep (L)ocal or (R)emote? [L/r]: ");
    io::stderr().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let choice = buf.trim().to_lowercase();
    if choice.starts_with('r') {
        Ok(MergeChoice::Remote)
    } else {
        Ok(MergeChoice::Local)
    }
}
