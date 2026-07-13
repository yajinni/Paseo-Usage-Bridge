use crate::{
    model::{now_rfc3339, Account, OAuthSecret, UsageFreshness, UsageSnapshot, UsageWindow},
    oauth::decode_claims,
    state::AppState,
    store::{load_secret, save_secret},
};
use chrono::{TimeZone, Utc};
use reqwest::{header::RETRY_AFTER, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

#[derive(Debug, Error)]
enum UsageError {
    #[error("authentication is required")]
    Auth,
    #[error("{0}")]
    Transient(String),
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: i64,
}

#[derive(Debug, Deserialize)]
struct RawUsage {
    plan_type: Option<String>,
    email: Option<String>,
    rate_limit: Option<RawRateLimit>,
    code_review_rate_limit: Option<RawCodeReviewRateLimit>,
    credits: Option<RawCredits>,
}

#[derive(Debug, Deserialize)]
struct RawRateLimit {
    primary_window: Option<RawWindow>,
    secondary_window: Option<RawWindow>,
}

#[derive(Debug, Deserialize)]
struct RawCodeReviewRateLimit {
    primary_window: Option<RawWindow>,
}

#[derive(Debug, Deserialize)]
struct RawWindow {
    used_percent: Option<Value>,
    reset_at: Option<Value>,
    reset_after_seconds: Option<Value>,
    limit_window_seconds: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RawCredits {
    unlimited: Option<bool>,
    balance: Option<Value>,
}

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
    let mut secret = load_secret(account_id).map_err(|error| error.to_string())?;

    if secret.expires_within(300) {
        secret = match refresh_oauth_secret(&app, &account, secret).await {
            Ok(secret) => secret,
            Err(error) => return save_failure(&app, account_id, error),
        };
    }

    let first = call_usage(&app, &account, &secret).await;
    let result = match first {
        Err(UsageError::Auth) => {
            let refreshed = match refresh_oauth_secret(&app, &account, secret).await {
                Ok(secret) => secret,
                Err(error) => return save_failure(&app, account_id, error),
            };
            call_usage(&app, &account, &refreshed).await
        }
        other => other,
    };

    match result {
        Ok(raw) => save_success(&app, account_id, raw),
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

async fn call_usage(
    app: &AppState,
    account: &Account,
    secret: &OAuthSecret,
) -> Result<RawUsage, UsageError> {
    let mut request = app
        .client
        .get(USAGE_URL)
        .bearer_auth(&secret.access_token)
        .header("Accept", "application/json");
    if let Some(account_id) = account.chatgpt_account_id.as_deref() {
        request = request.header("ChatGPT-Account-Id", account_id);
    }

    let response = request
        .send()
        .await
        .map_err(|error| UsageError::Transient(format!("Usage request failed: {error}")))?;
    let status = response.status();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response.text().await.map_err(|error| {
        UsageError::Transient(format!("Unable to read the usage response: {error}"))
    })?;

    if status == StatusCode::UNAUTHORIZED {
        return Err(UsageError::Auth);
    }
    if status == StatusCode::FORBIDDEN {
        return Err(UsageError::Transient(
            if body.trim_start().starts_with('<') {
                "OpenAI returned an HTML access challenge. Cached usage is being kept.".into()
            } else {
                "OpenAI denied the usage request. Cached usage is being kept.".into()
            },
        ));
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(UsageError::Transient(match retry_after {
            Some(value) => {
                format!("OpenAI rate-limited the usage request. Retry after {value}.")
            }
            None => "OpenAI rate-limited the usage request.".into(),
        }));
    }
    if !status.is_success() {
        return Err(UsageError::Transient(format!(
            "OpenAI usage request returned {status}."
        )));
    }
    if body.trim_start().starts_with('<') {
        return Err(UsageError::Transient(
            "OpenAI returned HTML instead of usage JSON.".into(),
        ));
    }
    serde_json::from_str(&body).map_err(|error| {
        UsageError::Transient(format!("OpenAI returned incompatible usage data: {error}"))
    })
}

async fn refresh_oauth_secret(
    app: &AppState,
    account: &Account,
    secret: OAuthSecret,
) -> Result<OAuthSecret, UsageError> {
    let response = app
        .client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", secret.refresh_token.as_str()),
        ])
        .send()
        .await
        .map_err(|error| UsageError::Transient(format!("Token refresh failed: {error}")))?;
    if response.status() == StatusCode::UNAUTHORIZED
        || response.status() == StatusCode::FORBIDDEN
    {
        return Err(UsageError::Auth);
    }
    if !response.status().is_success() {
        return Err(UsageError::Transient(format!(
            "Token refresh returned {}.",
            response.status()
        )));
    }
    let tokens: RefreshResponse = response.json().await.map_err(|error| {
        UsageError::Transient(format!("Invalid token refresh response: {error}"))
    })?;
    let claims = decode_claims(&tokens.access_token);
    let expires_at = claims
        .as_ref()
        .and_then(|claims| claims.expires_at)
        .unwrap_or_else(|| Utc::now().timestamp_millis() + tokens.expires_in * 1000);
    let refreshed = OAuthSecret {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.unwrap_or(secret.refresh_token),
        id_token: tokens.id_token.or(secret.id_token),
        expires_at,
    };
    save_secret(&account.id, &refreshed)
        .map_err(|error| UsageError::Transient(error.to_string()))?;
    if let Some(claims) = claims {
        let _ = app.store.mutate(&account.id, |record| {
            record.chatgpt_account_id = claims
                .account_id
                .or_else(|| record.chatgpt_account_id.clone());
            record.plan = claims.plan.or_else(|| record.plan.clone());
            record.email = claims.email.or_else(|| record.email.clone());
            record.auth_required = false;
        });
    }
    Ok(refreshed)
}

fn save_success(app: &AppState, account_id: &str, raw: RawUsage) -> Result<Account, String> {
    let fetched_at = now_rfc3339();
    let mut windows = Vec::new();
    if let Some(window) = raw
        .rate_limit
        .as_ref()
        .and_then(|rate| rate.primary_window.as_ref())
    {
        windows.push(normalize_window("session", "Session", window));
    }
    if let Some(window) = raw
        .rate_limit
        .as_ref()
        .and_then(|rate| rate.secondary_window.as_ref())
    {
        windows.push(normalize_window("weekly", "Weekly", window));
    }
    if let Some(window) = raw
        .code_review_rate_limit
        .as_ref()
        .and_then(|rate| rate.primary_window.as_ref())
    {
        windows.push(normalize_window("code_review", "Code review", window));
    }
    if windows.is_empty() {
        return save_failure(
            app,
            account_id,
            UsageError::Transient(
                "The OpenAI response contained no usable usage windows.".into(),
            ),
        );
    }

    let credits_usd = raw
        .credits
        .as_ref()
        .and_then(|credits| credits.balance.as_ref())
        .and_then(value_as_f64);
    let unlimited_credits = raw
        .credits
        .as_ref()
        .and_then(|credits| credits.unlimited)
        .unwrap_or(false);
    app.store
        .mutate(account_id, |account| {
            account.plan = raw.plan_type.clone().or_else(|| account.plan.clone());
            account.email = raw.email.clone().or_else(|| account.email.clone());
            account.last_usage = Some(UsageSnapshot {
                plan: account.plan.clone(),
                email: account.email.clone(),
                windows,
                credits_usd,
                unlimited_credits,
                fetched_at,
                freshness: UsageFreshness::Live,
                source: "wham".into(),
            });
            account.last_error = None;
            account.auth_required = false;
        })
        .map_err(|error| error.to_string())
}

fn save_failure(app: &AppState, account_id: &str, error: UsageError) -> Result<Account, String> {
    let is_auth = matches!(&error, UsageError::Auth);
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

fn normalize_window(id: &str, label: &str, raw: &RawWindow) -> UsageWindow {
    let used = raw
        .used_percent
        .as_ref()
        .and_then(value_as_f64)
        .map(|value| value.clamp(0.0, 100.0));
    let reset_at_seconds = raw.reset_at.as_ref().and_then(value_as_i64).or_else(|| {
        raw.reset_after_seconds
            .as_ref()
            .and_then(value_as_i64)
            .map(|seconds| Utc::now().timestamp() + seconds)
    });
    let resets_at = reset_at_seconds
        .and_then(|seconds| Utc.timestamp_opt(seconds, 0).single())
        .map(|value| value.to_rfc3339());
    UsageWindow {
        id: id.into(),
        label: label.into(),
        used_percent: used,
        remaining_percent: used.map(|value| (100.0 - value).max(0.0)),
        resets_at,
        window_seconds: raw
            .limit_window_seconds
            .as_ref()
            .and_then(value_as_u64),
    }
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_str()?.parse().ok())
}

fn value_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
        .or_else(|| value.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_string_and_numeric_fields() {
        let raw: RawWindow = serde_json::from_value(serde_json::json!({
            "used_percent": "20.5",
            "reset_at": 2000000000,
            "limit_window_seconds": "18000"
        }))
        .unwrap();
        let result = normalize_window("session", "Session", &raw);
        assert_eq!(result.remaining_percent, Some(79.5));
        assert_eq!(result.window_seconds, Some(18000));
        assert!(result.resets_at.is_some());
    }
}
