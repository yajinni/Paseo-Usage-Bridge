use crate::model::{now_rfc3339, Account, OAuthSecret};
use keyring::Entry;
use parking_lot::RwLock;
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use thiserror::Error;

const CREDENTIAL_SERVICE: &str = "paseo-usage-bridge";
const BRIDGE_TOKEN_USER: &str = "bridge-api-token";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("credential store error: {0}")]
    Credential(String),
    #[error("metadata store error: {0}")]
    Io(String),
    #[error("invalid metadata: {0}")]
    Invalid(String),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountFile {
    version: u32,
    accounts: Vec<Account>,
}

pub struct AccountStore {
    data_dir: PathBuf,
    accounts: RwLock<Vec<Account>>,
}

impl AccountStore {
    pub fn load(data_dir: PathBuf) -> Result<Self, StoreError> {
        fs::create_dir_all(&data_dir).map_err(|error| StoreError::Io(error.to_string()))?;
        let accounts = read_account_file(&data_dir)?;
        Ok(Self {
            data_dir,
            accounts: RwLock::new(accounts),
        })
    }

    pub fn list(&self) -> Vec<Account> {
        let mut accounts = self.accounts.read().clone();
        accounts.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
        accounts
    }

    pub fn get(&self, id: &str) -> Option<Account> {
        self.accounts
            .read()
            .iter()
            .find(|account| account.id == id)
            .cloned()
    }

    pub fn upsert(&self, account: Account) -> Result<Account, StoreError> {
        let mut accounts = self.accounts.write();
        if let Some(existing) = accounts.iter_mut().find(|candidate| candidate.id == account.id) {
            *existing = account.clone();
        } else {
            accounts.push(account.clone());
        }
        write_account_file(&self.data_dir, &accounts)?;
        Ok(account)
    }

    pub fn mutate<F>(&self, id: &str, update: F) -> Result<Account, StoreError>
    where
        F: FnOnce(&mut Account),
    {
        let mut accounts = self.accounts.write();
        let account = accounts
            .iter_mut()
            .find(|account| account.id == id)
            .ok_or_else(|| StoreError::Invalid("account not found".into()))?;
        update(account);
        account.touch();
        let result = account.clone();
        write_account_file(&self.data_dir, &accounts)?;
        Ok(result)
    }

    pub fn remove(&self, id: &str) -> Result<(), StoreError> {
        let mut accounts = self.accounts.write();
        accounts.retain(|account| account.id != id);
        write_account_file(&self.data_dir, &accounts)?;
        delete_secret(id)?;
        Ok(())
    }

    pub fn find_duplicate(&self, account_id: Option<&str>, email: Option<&str>) -> Option<Account> {
        self.accounts
            .read()
            .iter()
            .find(|account| {
                account_id
                    .filter(|value| !value.is_empty())
                    .is_some_and(|value| account.chatgpt_account_id.as_deref() == Some(value))
                    || email
                        .filter(|value| !value.is_empty())
                        .is_some_and(|value| {
                            account
                                .email
                                .as_deref()
                                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(value))
                        })
            })
            .cloned()
    }
}

pub fn save_secret(account_id: &str, secret: &OAuthSecret) -> Result<(), StoreError> {
    let entry = Entry::new(CREDENTIAL_SERVICE, &format!("account:{account_id}"))
        .map_err(|error| StoreError::Credential(error.to_string()))?;
    let payload =
        serde_json::to_string(secret).map_err(|error| StoreError::Invalid(error.to_string()))?;
    entry
        .set_password(&payload)
        .map_err(|error| StoreError::Credential(error.to_string()))
}

pub fn load_secret(account_id: &str) -> Result<OAuthSecret, StoreError> {
    let entry = Entry::new(CREDENTIAL_SERVICE, &format!("account:{account_id}"))
        .map_err(|error| StoreError::Credential(error.to_string()))?;
    let payload = entry
        .get_password()
        .map_err(|error| StoreError::Credential(error.to_string()))?;
    serde_json::from_str(&payload).map_err(|error| StoreError::Invalid(error.to_string()))
}

pub fn delete_secret(account_id: &str) -> Result<(), StoreError> {
    let entry = Entry::new(CREDENTIAL_SERVICE, &format!("account:{account_id}"))
        .map_err(|error| StoreError::Credential(error.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(StoreError::Credential(error.to_string())),
    }
}

pub fn load_or_create_bridge_token() -> Result<String, StoreError> {
    let entry = Entry::new(CREDENTIAL_SERVICE, BRIDGE_TOKEN_USER)
        .map_err(|error| StoreError::Credential(error.to_string()))?;
    match entry.get_password() {
        Ok(value) if value.len() >= 32 => Ok(value),
        Ok(_) | Err(keyring::Error::NoEntry) => {
            let token = generate_bridge_token();
            entry
                .set_password(&token)
                .map_err(|error| StoreError::Credential(error.to_string()))?;
            Ok(token)
        }
        Err(error) => Err(StoreError::Credential(error.to_string())),
    }
}

pub fn rotate_bridge_token() -> Result<String, StoreError> {
    let token = generate_bridge_token();
    let entry = Entry::new(CREDENTIAL_SERVICE, BRIDGE_TOKEN_USER)
        .map_err(|error| StoreError::Credential(error.to_string()))?;
    entry
        .set_password(&token)
        .map_err(|error| StoreError::Credential(error.to_string()))?;
    Ok(token)
}

fn generate_bridge_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}

fn account_path(data_dir: &Path) -> PathBuf {
    data_dir.join("accounts.json")
}

fn read_account_file(data_dir: &Path) -> Result<Vec<Account>, StoreError> {
    let path = account_path(data_dir);
    let backup = data_dir.join("accounts.json.bak");
    let source = if path.exists() {
        path
    } else if backup.exists() {
        backup
    } else {
        return Ok(Vec::new());
    };
    let raw = fs::read_to_string(source).map_err(|error| StoreError::Io(error.to_string()))?;
    let parsed: AccountFile =
        serde_json::from_str(&raw).map_err(|error| StoreError::Invalid(error.to_string()))?;
    Ok(parsed.accounts)
}

fn write_account_file(data_dir: &Path, accounts: &[Account]) -> Result<(), StoreError> {
    let file = AccountFile {
        version: 1,
        accounts: accounts.to_vec(),
    };
    let payload =
        serde_json::to_vec_pretty(&file).map_err(|error| StoreError::Invalid(error.to_string()))?;
    let path = account_path(data_dir);
    let temp = data_dir.join(format!(
        "accounts.{}.tmp",
        now_rfc3339().replace(':', "-").replace('.', "-")
    ));
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|error| StoreError::Io(error.to_string()))?;
    output
        .write_all(&payload)
        .map_err(|error| StoreError::Io(error.to_string()))?;
    output
        .sync_all()
        .map_err(|error| StoreError::Io(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))
            .map_err(|error| StoreError::Io(error.to_string()))?;
    }
    if path.exists() {
        let backup = data_dir.join("accounts.json.bak");
        let _ = fs::copy(&path, backup);
    }
    match fs::rename(&temp, &path) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            fs::remove_file(&path).map_err(|error| StoreError::Io(error.to_string()))?;
            fs::rename(&temp, &path).map_err(|error| StoreError::Io(error.to_string()))
        }
        Err(error) => Err(StoreError::Io(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn metadata_round_trip() {
        let dir = tempdir().unwrap();
        let store = AccountStore::load(dir.path().to_path_buf()).unwrap();
        let now = now_rfc3339();
        store
            .upsert(Account {
                id: "one".into(),
                label: "Main".into(),
                email: Some("main@example.com".into()),
                chatgpt_account_id: Some("account-1".into()),
                plan: Some("plus".into()),
                created_at: now.clone(),
                updated_at: now,
                last_usage: None,
                last_error: None,
                auth_required: false,
            })
            .unwrap();
        let reopened = AccountStore::load(dir.path().to_path_buf()).unwrap();
        assert_eq!(reopened.list().len(), 1);
        assert_eq!(reopened.list()[0].label, "Main");
    }
}
