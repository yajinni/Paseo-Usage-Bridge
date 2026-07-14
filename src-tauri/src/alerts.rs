use crate::model::{Account, UsageWindow};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const ALERTS_FILE_NAME: &str = "usage-alerts.json";
const ALERT_WINDOW_IDS: [&str; 3] = ["five_hour", "weekly", "monthly"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageAlertSetting {
    pub window_id: String,
    pub enabled: bool,
    pub threshold_percent: u8,
}

#[derive(Clone, Debug)]
pub struct AlertNotification {
    pub window_label: String,
    pub remaining_percent: u8,
    pub threshold_percent: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredUsageAlertSetting {
    window_id: String,
    enabled: bool,
    threshold_percent: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_notified_key: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlertFile {
    version: u32,
    accounts: HashMap<String, Vec<StoredUsageAlertSetting>>,
}

pub struct AlertStore {
    path: PathBuf,
    accounts: RwLock<HashMap<String, Vec<StoredUsageAlertSetting>>>,
}

impl AlertStore {
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
        let path = data_dir.join(ALERTS_FILE_NAME);
        let accounts = if path.exists() {
            let payload = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            serde_json::from_str::<AlertFile>(&payload)
                .map_err(|error| format!("Unable to read usage alert settings: {error}"))?
                .accounts
        } else {
            HashMap::new()
        };
        Ok(Self {
            path,
            accounts: RwLock::new(accounts),
        })
    }

    pub fn get(&self, account_id: &str) -> Vec<UsageAlertSetting> {
        let mut settings = self
            .accounts
            .read()
            .get(account_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|setting| UsageAlertSetting {
                window_id: setting.window_id,
                enabled: setting.enabled,
                threshold_percent: setting.threshold_percent,
            })
            .collect::<Vec<_>>();
        sort_settings(&mut settings, |setting| setting.window_id.as_str());
        settings
    }

    pub fn save(
        &self,
        account_id: &str,
        settings: Vec<UsageAlertSetting>,
    ) -> Result<Vec<UsageAlertSetting>, String> {
        if settings.len() > ALERT_WINDOW_IDS.len() {
            return Err("Only 5 hour, weekly, and monthly alerts are supported.".into());
        }

        let mut seen = HashSet::new();
        for setting in &settings {
            if !ALERT_WINDOW_IDS.contains(&setting.window_id.as_str()) {
                return Err(format!("Unsupported usage alert window: {}", setting.window_id));
            }
            if !seen.insert(setting.window_id.as_str()) {
                return Err(format!("Duplicate usage alert window: {}", setting.window_id));
            }
            if !(1..=100).contains(&setting.threshold_percent) {
                return Err("Alert thresholds must be between 1% and 100%.".into());
            }
        }

        let existing = self
            .accounts
            .read()
            .get(account_id)
            .cloned()
            .unwrap_or_default();
        let mut stored = settings
            .iter()
            .map(|setting| {
                let previous = existing.iter().find(|candidate| {
                    candidate.window_id == setting.window_id
                        && candidate.enabled == setting.enabled
                        && candidate.threshold_percent == setting.threshold_percent
                });
                StoredUsageAlertSetting {
                    window_id: setting.window_id.clone(),
                    enabled: setting.enabled,
                    threshold_percent: setting.threshold_percent,
                    last_notified_key: previous.and_then(|candidate| candidate.last_notified_key.clone()),
                }
            })
            .collect::<Vec<_>>();
        sort_settings(&mut stored, |setting| setting.window_id.as_str());

        let mut accounts = self.accounts.write();
        if stored.is_empty() {
            accounts.remove(account_id);
        } else {
            accounts.insert(account_id.to_string(), stored);
        }
        self.persist_locked(&accounts)?;
        Ok(settings)
    }

    pub fn evaluate(&self, account: &Account) -> Result<Vec<AlertNotification>, String> {
        let Some(usage) = account.last_usage.as_ref() else {
            return Ok(Vec::new());
        };
        let mut accounts = self.accounts.write();
        let Some(settings) = accounts.get_mut(&account.id) else {
            return Ok(Vec::new());
        };

        let mut notifications = Vec::new();
        let mut changed = false;
        for setting in settings.iter_mut().filter(|setting| setting.enabled) {
            let Some(window) = usage
                .windows
                .iter()
                .find(|window| canonical_window_id(window) == Some(setting.window_id.as_str()))
            else {
                continue;
            };
            let Some(remaining) = window.remaining_percent.filter(|value| value.is_finite()) else {
                continue;
            };
            let remaining = remaining.clamp(0.0, 100.0).round() as u8;
            if remaining <= setting.threshold_percent {
                let period = window.resets_at.as_deref().unwrap_or(&usage.fetched_at);
                let notification_key = format!("{period}:{}", setting.threshold_percent);
                if setting.last_notified_key.as_deref() != Some(notification_key.as_str()) {
                    setting.last_notified_key = Some(notification_key);
                    changed = true;
                    notifications.push(AlertNotification {
                        window_label: display_window_label(&setting.window_id).into(),
                        remaining_percent: remaining,
                        threshold_percent: setting.threshold_percent,
                    });
                }
            } else if setting.last_notified_key.take().is_some() {
                changed = true;
            }
        }

        if changed {
            self.persist_locked(&accounts)?;
        }
        Ok(notifications)
    }

    pub fn remove(&self, account_id: &str) -> Result<(), String> {
        let mut accounts = self.accounts.write();
        if accounts.remove(account_id).is_some() {
            self.persist_locked(&accounts)?;
        }
        Ok(())
    }

    fn persist_locked(
        &self,
        accounts: &HashMap<String, Vec<StoredUsageAlertSetting>>,
    ) -> Result<(), String> {
        let file = AlertFile {
            version: 1,
            accounts: accounts.clone(),
        };
        let payload = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
        atomic_write(&self.path, &payload)
    }
}

pub fn canonical_window_id(window: &UsageWindow) -> Option<&'static str> {
    let id = window.id.to_ascii_lowercase().replace('-', "_");
    let label = window.label.to_ascii_lowercase();
    if id == "five_hour"
        || id == "rolling"
        || window.window_seconds == Some(18_000)
        || label.contains("5 hour")
        || label.contains("five hour")
    {
        Some("five_hour")
    } else if id == "weekly"
        || window.window_seconds == Some(604_800)
        || label.contains("weekly")
    {
        Some("weekly")
    } else if id == "monthly" || label.contains("monthly") {
        Some("monthly")
    } else {
        None
    }
}

fn display_window_label(window_id: &str) -> &'static str {
    match window_id {
        "five_hour" => "5 hour",
        "weekly" => "Weekly",
        "monthly" => "Monthly",
        _ => "Usage",
    }
}

fn sort_settings<T, F>(settings: &mut [T], window_id: F)
where
    F: Fn(&T) -> &str,
{
    settings.sort_by_key(|setting| {
        ALERT_WINDOW_IDS
            .iter()
            .position(|candidate| candidate == &window_id(setting))
            .unwrap_or(ALERT_WINDOW_IDS.len())
    });
}

fn atomic_write(path: &Path, payload: &[u8]) -> Result<(), String> {
    let temporary_path = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temporary_path).map_err(|error| error.to_string())?;
    file.write_all(payload).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    fs::rename(&temporary_path, path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{now_rfc3339, Provider, UsageFreshness, UsageSnapshot};

    fn account_with_weekly(remaining: f64) -> Account {
        let now = now_rfc3339();
        Account {
            id: "account-1".into(),
            label: "Test account".into(),
            provider: Provider::Openai,
            email: None,
            provider_account_id: None,
            chatgpt_account_id: None,
            plan: None,
            created_at: now.clone(),
            updated_at: now.clone(),
            last_usage: Some(UsageSnapshot {
                plan: None,
                email: None,
                windows: vec![UsageWindow {
                    id: "weekly".into(),
                    label: "Weekly".into(),
                    used_percent: Some(100.0 - remaining),
                    remaining_percent: Some(remaining),
                    resets_at: Some("2026-07-20T00:00:00Z".into()),
                    window_seconds: Some(604_800),
                }],
                credits_usd: None,
                unlimited_credits: false,
                fetched_at: now,
                freshness: UsageFreshness::Live,
                source: "test".into(),
            }),
            last_error: None,
            auth_required: false,
        }
    }

    #[test]
    fn notifies_once_per_period_below_threshold() {
        let directory = tempfile::tempdir().unwrap();
        let store = AlertStore::load(directory.path()).unwrap();
        store
            .save(
                "account-1",
                vec![UsageAlertSetting {
                    window_id: "weekly".into(),
                    enabled: true,
                    threshold_percent: 20,
                }],
            )
            .unwrap();
        let account = account_with_weekly(18.0);
        assert_eq!(store.evaluate(&account).unwrap().len(), 1);
        assert!(store.evaluate(&account).unwrap().is_empty());
    }

    #[test]
    fn rejects_unknown_windows() {
        let directory = tempfile::tempdir().unwrap();
        let store = AlertStore::load(directory.path()).unwrap();
        assert!(store
            .save(
                "account-1",
                vec![UsageAlertSetting {
                    window_id: "daily".into(),
                    enabled: true,
                    threshold_percent: 20,
                }],
            )
            .is_err());
    }
}
