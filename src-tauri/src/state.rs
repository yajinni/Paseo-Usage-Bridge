use crate::{
    account_order::AccountOrderStore,
    alerts::AlertStore,
    model::LoginStatus,
    store::AccountStore,
};
use parking_lot::{Mutex, RwLock};
use reqwest::Client;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tauri::AppHandle;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone, Debug)]
pub struct ApiRuntime {
    pub endpoint: String,
    pub running: bool,
    pub error: Option<String>,
}

pub struct AppState {
    pub store: AccountStore,
    pub account_order: AccountOrderStore,
    pub alerts: AlertStore,
    pub client: Client,
    pub pending_login: RwLock<Option<LoginStatus>>,
    pub bridge_token: RwLock<String>,
    pub api_runtime: RwLock<ApiRuntime>,
    pub app_handle: RwLock<Option<AppHandle>>,
    account_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    #[allow(dead_code)]
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn new(data_dir: PathBuf, bridge_token: String) -> Result<Self, String> {
        let store = AccountStore::load(data_dir.clone()).map_err(|error| error.to_string())?;
        let account_order = AccountOrderStore::load(&data_dir)?;
        let alerts = AlertStore::load(&data_dir)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .user_agent("Paseo-Usage-Bridge/0.1")
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            store,
            account_order,
            alerts,
            client,
            pending_login: RwLock::new(None),
            bridge_token: RwLock::new(bridge_token),
            api_runtime: RwLock::new(ApiRuntime {
                endpoint: "http://127.0.0.1:47831/v1/paseo-usage".into(),
                running: false,
                error: None,
            }),
            app_handle: RwLock::new(None),
            account_locks: Mutex::new(HashMap::new()),
            data_dir,
        })
    }

    pub fn set_app_handle(&self, app_handle: AppHandle) {
        *self.app_handle.write() = Some(app_handle);
    }

    pub fn account_lock(&self, account_id: &str) -> Arc<AsyncMutex<()>> {
        let mut locks = self.account_locks.lock();
        locks
            .entry(account_id.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}
