use super::{ProviderError, ProviderUsage};
use crate::{
    model::{Account, OAuthSecret, UsageWindow},
    state::AppState,
};
use chrono::Utc;
use reqwest::{header::RETRY_AFTER, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const PROFILE_URL: &str = "https://api.anthropic.com/api/auth/oauth/profile";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const OAUTH_BETA: &str = "oauth-2025-04-20";

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RawWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawExtraUsage {
    utilization: Option<f64>,
    spent_usd: Option<f64>,
    limit_usd: Option<f64>,
    resets_at: Option<String>,
    is_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawUsage {
    five_hour: Option<RawWindow>,
    seven_day: Option<RawWindow>,
    seven_day_sonnet: Option<RawWindow>,
    seven_day_opus: Option<RawWindow>,
    extra_usage: Option<RawExtraUsage>,
    #[serde(flatten)]
    other: serde_json::Map<String, Value>,
}

pub async fn refresh(
    app: &AppState,
    account: &Account,
    mut secret: OAuthSecret,
) -> Result<(ProviderUsage, OAuthSecret), ProviderError> {
    if secret.expires_within(300) {
        secret = refresh_secret(app, secret).await?;
    }

    let raw = match call_usage(app, &secret).await {
        Err(ProviderError::Auth) => {
            secret = refresh_secret(app, secret).await?;
            call_usage(app, &secret).await?
        }
        result => result?,
    };

    let mut windows = Vec::new();
    push_window(&mut windows, "five_hour", "5 hour", raw.five_hour.as_ref(), Some(18_000));
    push_window(&mut windows, "weekly", "Weekly", raw.seven_day.as_ref(), Some(604_800));
    push_window(
        &mut windows,
        "sonnet_weekly",
        "Sonnet weekly",
        raw.seven_day_sonnet.as_ref(),
        Some(604_800),
    );
    push_window(
        &mut windows,
        "opus_weekly",
        "Opus weekly",
        raw.seven_day_opus.as_ref(),
        Some(604_800),
    );

    let known: HashSet<&str> = [
        "five_hour",
        "seven_day",
        "seven_day_sonnet",
        "seven_day_opus",
        "extra_usage",
    ]
    .into_iter()
    .collect();
    for (key, value) in raw.other.iter().filter(|(key, _)| !known.contains(key.as_str())) {
        let Some(object) = value.as_object() else { continue };
        let Some(utilization) = object.get("utilization").and_then(value_as_f64) else {
            continue;
        };
        let resets_at = object
            .get("resets_at")
            .and_then(Value::as_str)
            .map(str::to_string);
        windows.push(UsageWindow {
            id: slug(key),
            label: title_case(key),
            used_percent: Some(utilization.clamp(0.0, 100.0)),
            remaining_percent: Some((100.0 - utilization).clamp(0.0, 100.0)),
            resets_at,
            window_seconds: infer_window_seconds(key),
        });
    }

    if let Some(extra) = raw.extra_usage.as_ref() {
        if extra.is_enabled.unwrap_or(true) && extra.utilization.is_some() {
            let utilization = extra.utilization.unwrap_or_default().clamp(0.0, 100.0);
            windows.push(UsageWindow {
                id: "extra_usage".into(),
                label: "Extra usage".into(),
                used_percent: Some(utilization),
                remaining_percent: Some((100.0 - utilization).max(0.0)),
                resets_at: extra.resets_at.clone(),
                window_seconds: None,
            });
        }
    }

    if windows.is_empty() {
        return Err(ProviderError::Transient(
            "Anthropic returned no usable usage windows.".into(),
        ));
    }

    let profile = if account.email.is_none() || account.plan.is_none() {
        fetch_profile(app, &secret.access_token).await.ok()
    } else {
        None
    };
    let email = profile
        .as_ref()
        .and_then(|value| find_string(value, &["email", "email_address"]))
        .or_else(|| account.email.clone());
    let provider_account_id = profile
        .as_ref()
        .and_then(|value| find_string(value, &["account_id", "uuid", "id"]))
        .or_else(|| account.provider_account_id.clone());
    let plan = profile
        .as_ref()
        .and_then(|value| {
            find_string(
                value,
                &[
                    "subscription_type",
                    "subscription_tier",
                    "rate_limit_tier",
                    "plan",
                ],
            )
        })
        .or_else(|| account.plan.clone())
        .or_else(|| Some("Claude subscription".into()));

    let credits_usd = raw.extra_usage.as_ref().and_then(|extra| match (extra.limit_usd, extra.spent_usd) {
        (Some(limit), Some(spent)) => Some((limit - spent).max(0.0)),
        _ => None,
    });

    Ok((
        ProviderUsage {
            plan,
            email,
            provider_account_id,
            windows,
            credits_usd,
            unlimited_credits: false,
            source: "anthropic_oauth_usage".into(),
        },
        secret,
    ))
}

async fn call_usage(app: &AppState, secret: &OAuthSecret) -> Result<RawUsage, ProviderError> {
    let response = app
        .client
        .get(USAGE_URL)
        .bearer_auth(&secret.access_token)
        .header("Accept", "application/json")
        .header("anthropic-beta", OAUTH_BETA)
        .send()
        .await
        .map_err(|error| ProviderError::Transient(format!("Anthropic usage request failed: {error}")))?;
    let status = response.status();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response.text().await.map_err(|error| {
        ProviderError::Transient(format!("Unable to read the Anthropic usage response: {error}"))
    })?;

    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ProviderError::Auth);
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::Transient(match retry_after {
            Some(value) => format!("Anthropic rate-limited the usage request. Retry after {value}."),
            None => "Anthropic rate-limited the usage request.".into(),
        }));
    }
    if !status.is_success() {
        return Err(ProviderError::Transient(format!(
            "Anthropic usage request returned {status}."
        )));
    }
    serde_json::from_str(&body).map_err(|error| {
        ProviderError::Transient(format!("Anthropic returned incompatible usage data: {error}"))
    })
}

async fn refresh_secret(
    app: &AppState,
    secret: OAuthSecret,
) -> Result<OAuthSecret, ProviderError> {
    let response = app
        .client
        .post(TOKEN_URL)
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": secret.refresh_token,
        }))
        .send()
        .await
        .map_err(|error| ProviderError::Transient(format!("Anthropic token refresh failed: {error}")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        if status == StatusCode::UNAUTHORIZED
            || status == StatusCode::FORBIDDEN
            || body.to_ascii_lowercase().contains("invalid_grant")
        {
            return Err(ProviderError::Auth);
        }
        return Err(ProviderError::Transient(format!(
            "Anthropic token refresh returned {status}."
        )));
    }
    let tokens: RefreshResponse = serde_json::from_str(&body).map_err(|error| {
        ProviderError::Transient(format!("Invalid Anthropic token refresh response: {error}"))
    })?;
    Ok(OAuthSecret {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.unwrap_or(secret.refresh_token),
        id_token: secret.id_token,
        expires_at: Utc::now().timestamp_millis() + tokens.expires_in.unwrap_or(3600) * 1000,
    })
}

async fn fetch_profile(app: &AppState, access_token: &str) -> Result<Value, ProviderError> {
    let response = app
        .client
        .get(PROFILE_URL)
        .bearer_auth(access_token)
        .header("Accept", "application/json")
        .header("anthropic-beta", OAUTH_BETA)
        .send()
        .await
        .map_err(|error| ProviderError::Transient(format!("Anthropic profile request failed: {error}")))?;
    if !response.status().is_success() {
        return Err(ProviderError::Transient(format!(
            "Anthropic profile request returned {}.",
            response.status()
        )));
    }
    response
        .json()
        .await
        .map_err(|error| ProviderError::Transient(format!("Invalid Anthropic profile response: {error}")))
}

fn push_window(
    windows: &mut Vec<UsageWindow>,
    id: &str,
    label: &str,
    raw: Option<&RawWindow>,
    window_seconds: Option<u64>,
) {
    let Some(raw) = raw else { return };
    let used = raw.utilization.map(|value| value.clamp(0.0, 100.0));
    windows.push(UsageWindow {
        id: id.into(),
        label: label.into(),
        used_percent: used,
        remaining_percent: used.map(|value| (100.0 - value).max(0.0)),
        resets_at: raw.resets_at.clone(),
        window_seconds,
    });
}

fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(object) => {
            for key in keys {
                if let Some(value) = object.get(*key).and_then(Value::as_str) {
                    if !value.trim().is_empty() {
                        return Some(value.to_string());
                    }
                }
            }
            object.values().find_map(|value| find_string(value, keys))
        }
        Value::Array(values) => values.iter().find_map(|value| find_string(value, keys)),
        _ => None,
    }
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

fn infer_window_seconds(key: &str) -> Option<u64> {
    let lower = key.to_ascii_lowercase();
    if lower.contains("five") || lower.contains("5h") {
        Some(18_000)
    } else if lower.contains("seven") || lower.contains("week") {
        Some(604_800)
    } else {
        None
    }
}

fn slug(value: &str) -> String {
    value
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn title_case(value: &str) -> String {
    value
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), characters.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_anthropic_windows() {
        let raw: RawUsage = serde_json::from_value(serde_json::json!({
            "five_hour": { "utilization": 25, "resets_at": "2026-07-13T20:00:00Z" },
            "seven_day": { "utilization": 50, "resets_at": "2026-07-19T00:00:00Z" }
        }))
        .unwrap();
        let mut windows = Vec::new();
        push_window(&mut windows, "five_hour", "5 hour", raw.five_hour.as_ref(), Some(18_000));
        assert_eq!(windows[0].remaining_percent, Some(75.0));
    }
}
