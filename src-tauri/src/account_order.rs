use crate::model::Account;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const ORDER_FILE_NAME: &str = "account-order.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountOrderFile {
    version: u32,
    account_ids: Vec<String>,
}

pub struct AccountOrderStore {
    path: PathBuf,
    account_ids: RwLock<Vec<String>>,
}

impl AccountOrderStore {
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
        let path = data_dir.join(ORDER_FILE_NAME);
        let account_ids = if path.exists() {
            let payload = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            serde_json::from_str::<AccountOrderFile>(&payload)
                .map_err(|error| format!("Unable to read saved account order: {error}"))?
                .account_ids
        } else {
            Vec::new()
        };
        Ok(Self {
            path,
            account_ids: RwLock::new(account_ids),
        })
    }

    pub fn apply(&self, accounts: Vec<Account>) -> Result<Vec<Account>, String> {
        let mut account_by_id = accounts
            .into_iter()
            .map(|account| (account.id.clone(), account))
            .collect::<HashMap<_, _>>();
        let mut ordered = Vec::with_capacity(account_by_id.len());
        let mut normalized_ids = Vec::with_capacity(account_by_id.len());

        for account_id in self.account_ids.read().iter() {
            if let Some(account) = account_by_id.remove(account_id) {
                normalized_ids.push(account.id.clone());
                ordered.push(account);
            }
        }

        let mut remaining = account_by_id.into_values().collect::<Vec<_>>();
        remaining.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
        for account in remaining {
            normalized_ids.push(account.id.clone());
            ordered.push(account);
        }

        if *self.account_ids.read() != normalized_ids {
            *self.account_ids.write() = normalized_ids;
            self.persist()?;
        }

        Ok(ordered)
    }

    pub fn save(&self, requested_ids: Vec<String>, accounts: Vec<Account>) -> Result<Vec<Account>, String> {
        if requested_ids.len() != accounts.len() {
            return Err("The account order must include every connected account exactly once.".into());
        }

        let expected = accounts.iter().map(|account| account.id.as_str()).collect::<HashSet<_>>();
        let requested = requested_ids.iter().map(String::as_str).collect::<HashSet<_>>();
        if requested.len() != requested_ids.len() || requested != expected {
            return Err("The account order contains a duplicate, missing, or unknown account.".into());
        }

        *self.account_ids.write() = requested_ids;
        self.persist()?;
        self.apply(accounts)
    }

    pub fn remove(&self, account_id: &str) -> Result<(), String> {
        let mut account_ids = self.account_ids.write();
        let previous_len = account_ids.len();
        account_ids.retain(|candidate| candidate != account_id);
        if account_ids.len() != previous_len {
            drop(account_ids);
            self.persist()?;
        }
        Ok(())
    }

    fn persist(&self) -> Result<(), String> {
        let file = AccountOrderFile {
            version: 1,
            account_ids: self.account_ids.read().clone(),
        };
        let payload = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
        atomic_write(&self.path, &payload)
    }
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
    use crate::model::{now_rfc3339, Provider};

    fn account(id: &str, label: &str) -> Account {
        let now = now_rfc3339();
        Account {
            id: id.into(),
            label: label.into(),
            provider: Provider::Openai,
            email: None,
            provider_account_id: None,
            chatgpt_account_id: None,
            plan: None,
            created_at: now.clone(),
            updated_at: now,
            last_usage: None,
            last_error: None,
            auth_required: false,
        }
    }

    #[test]
    fn saves_and_applies_requested_order() {
        let directory = tempfile::tempdir().unwrap();
        let store = AccountOrderStore::load(directory.path()).unwrap();
        let accounts = vec![account("a", "Alpha"), account("b", "Beta")];
        let ordered = store.save(vec!["b".into(), "a".into()], accounts).unwrap();
        assert_eq!(ordered.iter().map(|account| account.id.as_str()).collect::<Vec<_>>(), vec!["b", "a"]);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let directory = tempfile::tempdir().unwrap();
        let store = AccountOrderStore::load(directory.path()).unwrap();
        let accounts = vec![account("a", "Alpha"), account("b", "Beta")];
        assert!(store.save(vec!["a".into(), "a".into()], accounts).is_err());
    }
}
