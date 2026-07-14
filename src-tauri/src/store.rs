use crate::model::{now_rfc3339, Account, OAuthSecret, Provider, ProviderSecret};
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
const CHUNKED_CREDENTIAL_FORMAT: &str = "chunked-v1";
const CREDENTIAL_CHUNK_UTF16_UNITS: usize = 1800;
const MAX_CREDENTIAL_CHUNKS: usize = 32;
const CREDENTIAL_GENERATION_LENGTH: usize = 16;

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct CredentialGeneration {
    generation: String,
    chunks: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct CredentialManifest {
    format: String,
    active: CredentialGeneration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<CredentialGeneration>,
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

    pub fn find_duplicate(
        &self,
        provider: &Provider,
        account_id: Option<&str>,
        email: Option<&str>,
    ) -> Option<Account> {
        self.accounts
            .read()
            .iter()
            .find(|account| {
                if &account.provider != provider {
                    return false;
                }
                account_id
                    .filter(|value| !value.is_empty())
                    .is_some_and(|value| account.effective_account_id() == Some(value))
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

pub fn save_provider_secret(account_id: &str, secret: &ProviderSecret) -> Result<(), StoreError> {
    let payload =
        serde_json::to_string(secret).map_err(|error| StoreError::Invalid(error.to_string()))?;
    let chunks = split_utf16_chunks(&payload, CREDENTIAL_CHUNK_UTF16_UNITS);
    if chunks.is_empty() || chunks.len() > MAX_CREDENTIAL_CHUNKS {
        return Err(StoreError::Invalid(format!(
            "provider credentials require {} keyring chunks; supported range is 1-{MAX_CREDENTIAL_CHUNKS}",
            chunks.len()
        )));
    }

    let current_manifest = read_credential_manifest(account_id)?;
    if let Some(previous) = current_manifest
        .as_ref()
        .and_then(|manifest| manifest.previous.as_ref())
    {
        delete_credential_generation(account_id, previous)?;
    }

    let active = CredentialGeneration {
        generation: generate_credential_generation(),
        chunks: chunks.len(),
    };
    write_credential_generation(account_id, &active, &chunks)?;

    let manifest = CredentialManifest {
        format: CHUNKED_CREDENTIAL_FORMAT.into(),
        active: active.clone(),
        previous: current_manifest
            .as_ref()
            .map(|manifest| manifest.active.clone()),
    };
    if let Err(error) = write_credential_manifest(account_id, &manifest) {
        let _ = delete_credential_generation(account_id, &active);
        return Err(error);
    }

    if let Some(previous) = manifest.previous.as_ref() {
        if delete_credential_generation(account_id, previous).is_ok() {
            let cleaned_manifest = CredentialManifest {
                previous: None,
                ..manifest
            };
            let _ = write_credential_manifest(account_id, &cleaned_manifest);
        }
    }

    Ok(())
}

pub fn load_provider_secret(account_id: &str) -> Result<ProviderSecret, StoreError> {
    let entry = account_credential_entry(account_id)?;
    let stored = entry
        .get_password()
        .map_err(|error| StoreError::Credential(error.to_string()))?;

    if let Some(manifest) = parse_credential_manifest(&stored)? {
        let payload = read_credential_generation(account_id, &manifest.active)?;
        decode_provider_secret(&payload)
    } else {
        decode_provider_secret(&stored)
    }
}

pub fn delete_secret(account_id: &str) -> Result<(), StoreError> {
    let entry = account_credential_entry(account_id)?;
    let stored = match entry.get_password() {
        Ok(value) => Some(value),
        Err(keyring::Error::NoEntry) => None,
        Err(error) => return Err(StoreError::Credential(error.to_string())),
    };

    let mut first_error = None;
    if let Some(stored) = stored.as_deref() {
        if let Some(manifest) = parse_credential_manifest(stored)? {
            for generation in std::iter::once(&manifest.active).chain(manifest.previous.as_ref()) {
                if let Err(error) = delete_credential_generation(account_id, generation) {
                    first_error.get_or_insert(error);
                }
            }
        }
    }

    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => {
            first_error.get_or_insert(StoreError::Credential(error.to_string()));
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
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

fn account_credential_user(account_id: &str) -> String {
    format!("account:{account_id}")
}

fn account_credential_entry(account_id: &str) -> Result<Entry, StoreError> {
    credential_entry(&account_credential_user(account_id))
}

fn credential_chunk_user(account_id: &str, generation: &str, index: usize) -> String {
    format!("account:{account_id}:chunk:{generation}:{index}")
}

fn credential_entry(user: &str) -> Result<Entry, StoreError> {
    Entry::new(CREDENTIAL_SERVICE, user)
        .map_err(|error| StoreError::Credential(error.to_string()))
}

fn read_credential_manifest(account_id: &str) -> Result<Option<CredentialManifest>, StoreError> {
    let entry = account_credential_entry(account_id)?;
    match entry.get_password() {
        Ok(value) => parse_credential_manifest(&value),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(StoreError::Credential(error.to_string())),
    }
}

fn parse_credential_manifest(value: &str) -> Result<Option<CredentialManifest>, StoreError> {
    let Ok(manifest) = serde_json::from_str::<CredentialManifest>(value) else {
        return Ok(None);
    };
    if manifest.format != CHUNKED_CREDENTIAL_FORMAT {
        return Ok(None);
    }
    validate_credential_generation(&manifest.active)?;
    if let Some(previous) = manifest.previous.as_ref() {
        validate_credential_generation(previous)?;
    }
    Ok(Some(manifest))
}

fn validate_credential_generation(generation: &CredentialGeneration) -> Result<(), StoreError> {
    if generation.chunks == 0 || generation.chunks > MAX_CREDENTIAL_CHUNKS {
        return Err(StoreError::Invalid(format!(
            "credential manifest contains an invalid chunk count: {}",
            generation.chunks
        )));
    }
    if generation.generation.len() != CREDENTIAL_GENERATION_LENGTH
        || !generation
            .generation
            .bytes()
            .all(|value| value.is_ascii_alphanumeric())
    {
        return Err(StoreError::Invalid(
            "credential manifest contains an invalid generation identifier".into(),
        ));
    }
    Ok(())
}

fn write_credential_manifest(
    account_id: &str,
    manifest: &CredentialManifest,
) -> Result<(), StoreError> {
    let payload =
        serde_json::to_string(manifest).map_err(|error| StoreError::Invalid(error.to_string()))?;
    account_credential_entry(account_id)?
        .set_password(&payload)
        .map_err(|error| StoreError::Credential(error.to_string()))
}

fn write_credential_generation(
    account_id: &str,
    generation: &CredentialGeneration,
    chunks: &[String],
) -> Result<(), StoreError> {
    validate_credential_generation(generation)?;
    if chunks.len() != generation.chunks {
        return Err(StoreError::Invalid(
            "credential chunk count does not match its manifest".into(),
        ));
    }

    let mut written = 0;
    for (index, chunk) in chunks.iter().enumerate() {
        let user = credential_chunk_user(account_id, &generation.generation, index);
        match credential_entry(&user)?.set_password(chunk) {
            Ok(()) => written += 1,
            Err(error) => {
                for cleanup_index in 0..written {
                    let cleanup_user =
                        credential_chunk_user(account_id, &generation.generation, cleanup_index);
                    if let Ok(entry) = credential_entry(&cleanup_user) {
                        let _ = entry.delete_credential();
                    }
                }
                return Err(StoreError::Credential(error.to_string()));
            }
        }
    }
    Ok(())
}

fn read_credential_generation(
    account_id: &str,
    generation: &CredentialGeneration,
) -> Result<String, StoreError> {
    validate_credential_generation(generation)?;
    let mut payload = String::new();
    for index in 0..generation.chunks {
        let user = credential_chunk_user(account_id, &generation.generation, index);
        let chunk = credential_entry(&user)?
            .get_password()
            .map_err(|error| StoreError::Credential(error.to_string()))?;
        payload.push_str(&chunk);
    }
    Ok(payload)
}

fn delete_credential_generation(
    account_id: &str,
    generation: &CredentialGeneration,
) -> Result<(), StoreError> {
    validate_credential_generation(generation)?;
    let mut first_error = None;
    for index in 0..generation.chunks {
        let user = credential_chunk_user(account_id, &generation.generation, index);
        match credential_entry(&user)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(error) => {
                first_error.get_or_insert(StoreError::Credential(error.to_string()));
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn decode_provider_secret(payload: &str) -> Result<ProviderSecret, StoreError> {
    match serde_json::from_str::<ProviderSecret>(payload) {
        Ok(secret) => Ok(secret),
        Err(provider_error) => serde_json::from_str::<OAuthSecret>(payload)
            .map(ProviderSecret::Openai)
            .map_err(|legacy_error| {
                StoreError::Invalid(format!(
                    "unable to decode provider credentials ({provider_error}); legacy credentials also failed ({legacy_error})"
                ))
            }),
    }
}

fn split_utf16_chunks(value: &str, max_utf16_units: usize) -> Vec<String> {
    if value.is_empty() || max_utf16_units == 0 {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    let mut used_units = 0;
    for (index, character) in value.char_indices() {
        let character_units = character.len_utf16();
        if used_units + character_units > max_utf16_units {
            chunks.push(value[start..index].to_string());
            start = index;
            used_units = 0;
        }
        used_units += character_units;
    }
    chunks.push(value[start..].to_string());
    chunks
}

fn generate_credential_generation() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(CREDENTIAL_GENERATION_LENGTH)
        .map(char::from)
        .collect()
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
        version: 2,
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
                provider: Provider::Openai,
                email: Some("main@example.com".into()),
                provider_account_id: Some("account-1".into()),
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
        assert_eq!(reopened.list()[0].provider, Provider::Openai);
    }

    #[test]
    fn legacy_account_defaults_to_openai() {
        let raw = r#"{
          "version": 1,
          "accounts": [{
            "id": "legacy",
            "label": "Legacy",
            "email": null,
            "chatgptAccountId": "acct",
            "plan": "plus",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "lastUsage": null,
            "lastError": null,
            "authRequired": false
          }]
        }"#;
        let parsed: AccountFile = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.accounts[0].provider, Provider::Openai);
        assert_eq!(parsed.accounts[0].effective_account_id(), Some("acct"));
    }

    #[test]
    fn large_provider_secret_round_trips_through_chunks() {
        let secret = ProviderSecret::Openai(OAuthSecret {
            access_token: "a".repeat(4200),
            refresh_token: "r".repeat(500),
            id_token: Some("i".repeat(3600)),
            expires_at: 1_800_000_000_000,
        });
        let payload = serde_json::to_string(&secret).unwrap();
        let chunks = split_utf16_chunks(&payload, CREDENTIAL_CHUNK_UTF16_UNITS);
        assert!(chunks.len() > 1);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.encode_utf16().count() <= CREDENTIAL_CHUNK_UTF16_UNITS));
        let joined = chunks.concat();
        let decoded = decode_provider_secret(&joined).unwrap();
        match decoded {
            ProviderSecret::Openai(decoded) => {
                assert_eq!(decoded.access_token.len(), 4200);
                assert_eq!(decoded.refresh_token.len(), 500);
                assert_eq!(decoded.id_token.unwrap().len(), 3600);
            }
            _ => panic!("expected OpenAI credentials"),
        }
    }

    #[test]
    fn chunk_split_respects_utf16_surrogate_pairs() {
        let payload = format!("{}{}", "x".repeat(1799), "😀".repeat(5));
        let chunks = split_utf16_chunks(&payload, CREDENTIAL_CHUNK_UTF16_UNITS);
        assert_eq!(chunks.concat(), payload);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.encode_utf16().count() <= CREDENTIAL_CHUNK_UTF16_UNITS));
    }

    #[test]
    fn legacy_single_entry_secret_still_decodes() {
        let legacy = OAuthSecret {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            id_token: None,
            expires_at: 123,
        };
        let payload = serde_json::to_string(&legacy).unwrap();
        let decoded = decode_provider_secret(&payload).unwrap();
        assert!(matches!(decoded, ProviderSecret::Openai(_)));
    }
}
