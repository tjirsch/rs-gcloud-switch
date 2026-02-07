use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    #[default]
    Strict,
    Add,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub user_account: String,
    pub user_project: String,
    pub adc_account: String,
    pub adc_quota_project: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfilesFile {
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub sync_mode: SyncMode,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}
