use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::profile::{Profile, ProfilesFile};

pub struct Store {
    base_dir: PathBuf,
}

impl Store {
    pub fn new() -> Result<Self> {
        // Store inside gcloud's config directory so everything lives together.
        // Respects CLOUDSDK_CONFIG, just like gcloud itself.
        let gcloud_dir = if let Ok(custom) = std::env::var("CLOUDSDK_CONFIG") {
            PathBuf::from(custom)
        } else {
            dirs::home_dir()
                .context("Could not determine home directory")?
                .join(".config")
                .join("gcloud")
        };
        let base_dir = gcloud_dir.join("gcloud-switch");
        fs::create_dir_all(&base_dir)?;
        fs::create_dir_all(base_dir.join("adc"))?;
        Ok(Self { base_dir })
    }

    fn profiles_path(&self) -> PathBuf {
        self.base_dir.join("profiles.toml")
    }

    fn adc_dir(&self) -> PathBuf {
        self.base_dir.join("adc")
    }

    pub fn adc_path(&self, profile_name: &str) -> PathBuf {
        self.adc_dir().join(format!("{}.json", profile_name))
    }

    pub fn load_profiles(&self) -> Result<ProfilesFile> {
        let path = self.profiles_path();
        if !path.exists() {
            return Ok(ProfilesFile::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let profiles: ProfilesFile =
            toml::from_str(&content).with_context(|| "Failed to parse profiles.toml")?;
        Ok(profiles)
    }

    pub fn save_profiles(&self, profiles: &ProfilesFile) -> Result<()> {
        let content =
            toml::to_string_pretty(profiles).context("Failed to serialize profiles.toml")?;
        fs::write(self.profiles_path(), content)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn load_adc_json(&self, profile_name: &str) -> Result<Option<serde_json::Value>> {
        let path = self.adc_path(profile_name);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;
        Ok(Some(value))
    }

    pub fn save_adc_json(&self, profile_name: &str, value: &serde_json::Value) -> Result<()> {
        let path = self.adc_path(profile_name);
        let content = serde_json::to_string_pretty(value)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn has_adc(&self, profile_name: &str) -> bool {
        self.adc_path(profile_name).exists()
    }

    pub fn add_profile(&self, name: &str, profile: Profile) -> Result<()> {
        let mut data = self.load_profiles()?;
        data.profiles.insert(name.to_string(), profile);
        self.save_profiles(&data)
    }

    pub fn delete_profile(&self, name: &str) -> Result<()> {
        let mut data = self.load_profiles()?;
        data.profiles.remove(name);

        // Clear active state if this was the active profile
        if data.active_profile.as_deref() == Some(name) {
            data.active_profile = None;
        }

        self.save_profiles(&data)?;

        // Also remove ADC file if it exists
        let adc_path = self.adc_path(name);
        if adc_path.exists() {
            fs::remove_file(adc_path)?;
        }

        Ok(())
    }
}
