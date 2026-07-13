use super::{ProviderError, ProviderUsage};
use crate::{
    model::{Account, OAuthSecret, UsageWindow},
    oauth::decode_claims,
    state::AppState,
};
use chrono::{TimeZone, Utc};
use reqwest::{header::RETRY_AFTER, StatusCode};
use serde::Deserialize;
use serde_json::Value;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

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

pub async fn refresh(
    app: &AppState,
    account: &Account,
    mut secret: OAuthSecret,
) -> Result<(ProviderUsage, OAuthSecret), ProviderError> {
    if secret.expires_within(300) {
        secret = refresh_secret(app, secret).await?;
    }

    let raw = match call_usage(app, account, &secret).await {
        Err(ProviderError::Auth) => {
            secret = refresh_secret(app, secret).await?;
            call_usage(app, account, &secret).await?
        }
        result => result?,
    };

    let claims = decode_claims(&secret.access_token);
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
        return Err(ProviderError::Transient(
            "The OpenAI response contained no usable usage windows.".into(),
        ));
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

    Ok((
        ProviderUsage {
            plan: raw
                .plan_type
                .or_else(|| claims.as_ref().and_then(|claims| claims.plan.clone()))
                .or_else(|| account.plan.clone()),
            email: raw
                .email
                .or_else(|| claims.as_ref().and_then(|claims| claims.email.clone()))
                .or_else(|| account.email.clone()),
            provider_account_id: claims
                .and_then(|claims| claims.account_id)
                .or_else(|| account.effective_account_id().map(str::to_string)),
            windows,
            credits_usd,
            unlimited_credits,
            source: "wham".into(),
        },
        secret,
    ))
}

async fn call_usage(
    app: &AppState,
    account: &Account,
    secret: &OAuthSecret,
) -> Result<RawUsage, ProviderError> {
    let mut request = app
        .client
        .get(USAGE_URL)
        .bearer_auth(&secret.access_token)
        .header("Accept", "application/json");
    if let Some(account_id) = account.effective_account_id() {
        request = request.header("ChatGPT-Account-Id", account_id);
    }

    let response = request
        .send()
        .await
        .map_err(|error| ProviderError::Transient(format!("Usage request failed: {error}")))?;
    let status = response.status();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response.text().await.map_err(|error| {
        ProviderError::Transient(format!("Unable to read the usage response: {error}"))
    })?;

    if status == StatusCode::UNAUTHORIZED {
        return Err(ProviderError::Auth);
    }
    if status == StatusCode::FORBIDDEN {
        return Err(ProviderError::Transient(if body.trim_start().starts_with('<') {
            "OpenAI returned an HTML access challenge. Cached usage is being kept.".into()
        } else {
            "OpenAI denied the usage request. Cached usage is being kept.".into()
        }));
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::Transient(match retry_after {
            Some(value) => format!("OpenAI rate-limited the usage request. Retry after {value}."),
            None => "OpenAI rate-limited the usage request.".into(),
        }));
    }
    if !status.is_success() {
        return Err(ProviderError::Transient(format!(
            "OpenAI usage request returned {status}."
        )));
    }
    if body.trim_start().starts_with('<') {
        return Err(ProviderError::Transient(
            "OpenAI returned HTML instead of usage JSON.".into(),
        ));
    }
    serde_json::from_str(&body).map_err(|error| {
        ProviderError::Transient(format!("OpenAI returned incompatible usage data: {error}"))
    })
}

async fn refresh_secret(
    app: &AppState,
    secret: OAuthSecret,
) -> Result<OAuthSecret, ProviderError> {
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
        .map_err(|error| ProviderError::Transient(format!("Token refresh failed: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        if status == StatusCode::UNAUTHORIZED
            || status == StatusCode::FORBIDDEN
            || (status == StatusCode::BAD_REQUEST
                && body.to_ascii_lowercase().contains("invalid_grant"))
        {
            return Err(ProviderError::Auth);
        }
        return Err(ProviderError::Transient(format!(
            "OpenAI token refresh returned {status}."
        )));
    }
    let tokens: RefreshResponse = response.json().await.map_err(|error| {
        ProviderError::Transient(format!("Invalid OpenAI token refresh response: {error}"))
    })?;
    let claims = decode_claims(&tokens.access_token);
    let expires_at = claims
        .as_ref()
        .and_then(|claims| claims.expires_at)
        .unwrap_or_else(|| Utc::now().timestamp_millis() + tokens.expires_in * 1000);
    Ok(OAuthSecret {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.unwrap_or(secret.refresh_token),
        id_token: tokens.id_token.or(secret.id_token),
        expires_at,
    })
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
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
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
