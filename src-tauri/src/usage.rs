use crate::{
    model::{now_rfc3339, Account, Provider, UsageFreshness, UsageSnapshot},
    providers::{self, ProviderError, ProviderUsage},
    state::AppState,
    store::{load_provider_secret, save_provider_secret},
};
use std::sync::Arc;

pub async fn refresh_account(
    app: Arc<AppState>,
    account_id: &str,
) -> Result<Account, String> {
    let lock = app.account_lock(account_id);
    let _guard = lock.lock().await;
    let account = app
        .store
        .get(account_id)
        .ok_or_else(|| "Account not found.".to_string())?;
    let secret = match load_provider_secret(account_id) {
        Ok(secret) if secret.provider() == account.provider => secret,
        Ok(_) => {
            return save_failure(
                &app,
                account_id,
                ProviderError::Auth,
            )
        }
        Err(error) => {
            return save_failure(
                &app,
                account_id,
                ProviderError::Transient(format!("Unable to load provider credentials: {error}")),
            )
        }
    };

    match providers::refresh(app.clone(), &account, secret).await {
        Ok((usage, refreshed_secret)) => {
            save_provider_secret(account_id, &refreshed_secret)
                .map_err(|error| format!("Unable to save refreshed credentials: {error}"))?;
            save_success(&app, account_id, usage)
        }
        Err(error) => save_failure(&app, account_id, error),
    }
}

pub async fn refresh_all(app: Arc<AppState>) -> Vec<Account> {
    let ids: Vec<String> = app
        .store
        .list()
        .into_iter()
        .map(|account| account.id)
        .collect();
    let mut refreshed = Vec::with_capacity(ids.len());
    for id in ids {
        match refresh_account(app.clone(), &id).await {
            Ok(account) => refreshed.push(account),
            Err(_) => {
                if let Some(account) = app.store.get(&id) {
                    refreshed.push(account);
                }
            }
        }
    }
    refreshed
}

fn save_success(
    app: &AppState,
    account_id: &str,
    usage: ProviderUsage,
) -> Result<Account, String> {
    if usage.windows.is_empty() {
        return save_failure(
            app,
            account_id,
            ProviderError::Transient("The provider returned no usable usage windows.".into()),
        );
    }
    let fetched_at = now_rfc3339();
    app.store
        .mutate(account_id, |account| {
            account.plan = usage.plan.clone().or_else(|| account.plan.clone());
            account.email = usage.email.clone().or_else(|| account.email.clone());
            account.provider_account_id = usage
                .provider_account_id
                .clone()
                .or_else(|| account.provider_account_id.clone());
            if account.provider == Provider::Openai {
                account.chatgpt_account_id = account.provider_account_id.clone();
            }
            account.last_usage = Some(UsageSnapshot {
                plan: account.plan.clone(),
                email: account.email.clone(),
                windows: usage.windows,
                credits_usd: usage.credits_usd,
                unlimited_credits: usage.unlimited_credits,
                fetched_at,
                freshness: UsageFreshness::Live,
                source: usage.source,
            });
            account.last_error = None;
            account.auth_required = false;
        })
        .map_err(|error| error.to_string())
}

fn save_failure(
    app: &AppState,
    account_id: &str,
    error: ProviderError,
) -> Result<Account, String> {
    let is_auth = matches!(&error, ProviderError::Auth);
    let message = error.to_string();
    app.store
        .mutate(account_id, |account| {
            if let Some(usage) = account.last_usage.as_mut() {
                usage.freshness = if is_auth {
                    UsageFreshness::AuthRequired
                } else {
                    UsageFreshness::Stale
                };
            }
            account.last_error = Some(message);
            account.auth_required = is_auth;
        })
        .map_err(|error| error.to_string())
}
