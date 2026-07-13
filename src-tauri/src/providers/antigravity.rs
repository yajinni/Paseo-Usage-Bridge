use super::{ProviderError, ProviderUsage};
use crate::{
    model::{Account, OAuthSecret, UsageWindow},
    state::AppState,
};
use chrono::Utc;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use uuid::Uuid;

const CLIENT_ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const CLIENT_SECRET_BYTES: &[u8] = &[
    71, 79, 67, 83, 80, 88, 45, 75, 53, 56, 70, 87, 82, 52, 56, 54, 76, 100, 76, 74, 49, 109, 76,
    66, 56, 115, 88, 67, 52, 122, 54, 113, 68, 65, 102,
];
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const CLOUD_CODE_BASE: &str = "https://daily-cloudcode-pa.googleapis.com";
const IDE_VERSION: &str = "1.20.5";

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

pub async fn refresh(
    app: &AppState,
    account: &Account,
    mut secret: OAuthSecret,
) -> Result<(ProviderUsage, OAuthSecret), ProviderError> {
    if secret.expires_within(300) {
        secret = refresh_secret(app, secret).await?;
    }

    let result = match fetch_usage(app, account, &secret).await {
        Err(ProviderError::Auth) => {
            secret = refresh_secret(app, secret).await?;
            fetch_usage(app, account, &secret).await?
        }
        result => result?,
    };
    Ok((result, secret))
}

async fn fetch_usage(
    app: &AppState,
    account: &Account,
    secret: &OAuthSecret,
) -> Result<ProviderUsage, ProviderError> {
    let load = cloud_code_post(
        app,
        "/v1internal:loadCodeAssist",
        &json!({
            "metadata": metadata(None),
            "mode": "FULL_ELIGIBILITY_CHECK"
        }),
        &secret.access_token,
    )
    .await?;

    let project_id = extract_project_id(load.get("cloudaicompanionProject"))
        .or_else(|| account.provider_account_id.clone())
        .ok_or_else(|| {
            ProviderError::Transient(
                "Antigravity did not return a Cloud AI Companion project for this account.".into(),
            )
        })?;
    let plan = extract_plan(&load).or_else(|| account.plan.clone());

    let summary = cloud_code_post(
        app,
        "/v1internal:retrieveUserQuotaSummary",
        &json!({ "project": project_id }),
        &secret.access_token,
    )
    .await;

    let mut windows = match summary {
        Ok(value) => parse_quota_value(&value),
        Err(ProviderError::Auth) => return Err(ProviderError::Auth),
        Err(_) => Vec::new(),
    };
    let mut source = "antigravity_quota_summary".to_string();

    if windows.is_empty() {
        let models = cloud_code_post(
            app,
            "/v1internal:fetchAvailableModels",
            &json!({ "project": project_id }),
            &secret.access_token,
        )
        .await?;
        windows = parse_available_models(&models);
        source = "antigravity_available_models".into();
    }

    if windows.is_empty() {
        return Err(ProviderError::Transient(
            "Antigravity returned no usable quota buckets or model limits.".into(),
        ));
    }

    let email = if account.email.is_some() {
        account.email.clone()
    } else {
        fetch_email(app, &secret.access_token).await.ok()
    };

    Ok(ProviderUsage {
        plan,
        email,
        provider_account_id: Some(project_id),
        windows,
        credits_usd: None,
        unlimited_credits: false,
        source,
    })
}

async fn cloud_code_post(
    app: &AppState,
    path: &str,
    payload: &Value,
    access_token: &str,
) -> Result<Value, ProviderError> {
    let response = app
        .client
        .post(format!("{CLOUD_CODE_BASE}{path}"))
        .bearer_auth(access_token)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", user_agent())
        .header("x-activity-request-id", Uuid::new_v4().to_string())
        .json(payload)
        .send()
        .await
        .map_err(|error| ProviderError::Transient(format!("Antigravity quota request failed: {error}")))?;
    let status = response.status();
    let body = response.text().await.map_err(|error| {
        ProviderError::Transient(format!("Unable to read the Antigravity response: {error}"))
    })?;
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ProviderError::Auth);
    }
    if !status.is_success() {
        return Err(ProviderError::Transient(format!(
            "Antigravity endpoint {path} returned {status}."
        )));
    }
    serde_json::from_str(&body).map_err(|error| {
        ProviderError::Transient(format!("Antigravity returned incompatible quota data: {error}"))
    })
}

async fn refresh_secret(
    app: &AppState,
    secret: OAuthSecret,
) -> Result<OAuthSecret, ProviderError> {
    let client_secret = String::from_utf8_lossy(CLIENT_SECRET_BYTES).to_string();
    let response = app
        .client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", secret.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|error| ProviderError::Transient(format!("Google token refresh failed: {error}")))?;
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
            "Google token refresh returned {status}."
        )));
    }
    let tokens: RefreshResponse = serde_json::from_str(&body).map_err(|error| {
        ProviderError::Transient(format!("Invalid Google token refresh response: {error}"))
    })?;
    Ok(OAuthSecret {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.unwrap_or(secret.refresh_token),
        id_token: secret.id_token,
        expires_at: Utc::now().timestamp_millis() + tokens.expires_in * 1000,
    })
}

async fn fetch_email(app: &AppState, access_token: &str) -> Result<String, ProviderError> {
    let response = app
        .client
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|error| ProviderError::Transient(format!("Google user-info request failed: {error}")))?;
    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err(ProviderError::Auth);
    }
    if !response.status().is_success() {
        return Err(ProviderError::Transient(format!(
            "Google user-info request returned {}.",
            response.status()
        )));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|error| ProviderError::Transient(format!("Invalid Google user-info response: {error}")))?;
    value
        .get("email")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| ProviderError::Transient("Google user info did not contain an email address.".into()))
}

fn metadata(duet_project: Option<&str>) -> Value {
    let mut value = Map::new();
    value.insert("ideName".into(), Value::String("antigravity".into()));
    value.insert("ideType".into(), Value::String("ANTIGRAVITY".into()));
    value.insert("ideVersion".into(), Value::String(IDE_VERSION.into()));
    value.insert(
        "pluginVersion".into(),
        Value::String(env!("CARGO_PKG_VERSION").into()),
    );
    value.insert("platform".into(), Value::String(platform().into()));
    value.insert("updateChannel".into(), Value::String("stable".into()));
    value.insert("pluginType".into(), Value::String("GEMINI".into()));
    if let Some(project) = duet_project {
        value.insert("duetProject".into(), Value::String(project.into()));
    }
    Value::Object(value)
}

fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => "DARWIN_AMD64",
        ("macos", "aarch64") => "DARWIN_ARM64",
        ("windows", "x86_64") => "WINDOWS_AMD64",
        ("linux", "x86_64") => "LINUX_AMD64",
        ("linux", "aarch64") => "LINUX_ARM64",
        _ => "PLATFORM_UNSPECIFIED",
    }
}

fn user_agent() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("antigravity/{IDE_VERSION} {os}/{arch}")
}

fn extract_project_id(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Object(object) => object
            .get("id")
            .or_else(|| object.get("projectId"))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn extract_plan(value: &Value) -> Option<String> {
    value
        .pointer("/planInfo/planType")
        .or_else(|| value.pointer("/paidTier/id"))
        .or_else(|| value.pointer("/currentTier/id"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn parse_quota_value(value: &Value) -> Vec<UsageWindow> {
    let mut windows = Vec::new();
    let mut ids = HashSet::new();
    walk_quota(value, None, &mut windows, &mut ids);
    windows
}

fn walk_quota(
    value: &Value,
    parent_label: Option<&str>,
    windows: &mut Vec<UsageWindow>,
    ids: &mut HashSet<String>,
) {
    match value {
        Value::Object(object) => {
            let own_label = object
                .get("displayName")
                .or_else(|| object.get("name"))
                .or_else(|| object.get("bucketId"))
                .or_else(|| object.get("window"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty());
            let label = match (parent_label, own_label) {
                (Some(parent), Some(own)) if !own.eq_ignore_ascii_case(parent) => {
                    Some(format!("{parent} · {own}"))
                }
                (Some(parent), _) => Some(parent.to_string()),
                (None, Some(own)) => Some(own.to_string()),
                _ => None,
            };

            if let Some(remaining) = object.get("remainingFraction").and_then(value_as_f64) {
                let remaining_percent = fraction_to_percent(remaining);
                let display = label.clone().unwrap_or_else(|| "Antigravity quota".into());
                let base_id = classify_id(&display);
                let id = unique_id(&base_id, ids);
                windows.push(UsageWindow {
                    id,
                    label: display.clone(),
                    used_percent: Some((100.0 - remaining_percent).max(0.0)),
                    remaining_percent: Some(remaining_percent),
                    resets_at: object
                        .get("resetTime")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    window_seconds: infer_window_seconds(&display),
                });
            }

            let next_parent = own_label.or(parent_label);
            for child in object.values() {
                walk_quota(child, next_parent, windows, ids);
            }
        }
        Value::Array(values) => {
            for child in values {
                walk_quota(child, parent_label, windows, ids);
            }
        }
        _ => {}
    }
}

fn parse_available_models(value: &Value) -> Vec<UsageWindow> {
    let mut windows = Vec::new();
    let Some(models) = value.get("models").and_then(Value::as_object) else {
        return windows;
    };
    for (model_id, model) in models {
        let Some(quota) = model.get("quotaInfo") else { continue };
        let Some(remaining) = quota.get("remainingFraction").and_then(value_as_f64) else {
            continue;
        };
        let label = model
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or(model_id);
        let remaining_percent = fraction_to_percent(remaining);
        windows.push(UsageWindow {
            id: format!("model_{}", slug(model_id)),
            label: label.to_string(),
            used_percent: Some((100.0 - remaining_percent).max(0.0)),
            remaining_percent: Some(remaining_percent),
            resets_at: quota
                .get("resetTime")
                .and_then(Value::as_str)
                .map(str::to_string),
            window_seconds: None,
        });
    }
    windows
}

fn fraction_to_percent(value: f64) -> f64 {
    if value <= 1.0 {
        (value * 100.0).clamp(0.0, 100.0)
    } else {
        value.clamp(0.0, 100.0)
    }
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

fn classify_id(label: &str) -> String {
    let lower = label.to_ascii_lowercase();
    if lower.contains("5h") || lower.contains("5 hour") || lower.contains("five hour") || lower.contains("rolling") {
        "five_hour".into()
    } else if lower.contains("week") || lower.contains("7 day") || lower.contains("seven day") {
        "weekly".into()
    } else if lower.contains("month") {
        "monthly".into()
    } else {
        slug(label)
    }
}

fn infer_window_seconds(label: &str) -> Option<u64> {
    match classify_id(label).as_str() {
        "five_hour" => Some(18_000),
        "weekly" => Some(604_800),
        _ => None,
    }
}

fn unique_id(base: &str, ids: &mut HashSet<String>) -> String {
    if ids.insert(base.to_string()) {
        return base.to_string();
    }
    for suffix in 2..1000 {
        let candidate = format!("{base}_{suffix}");
        if ids.insert(candidate.clone()) {
            return candidate;
        }
    }
    format!("{base}_{}", Uuid::new_v4())
}

fn slug(value: &str) -> String {
    let result = value
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if result.is_empty() { "quota".into() } else { result }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_grouped_quota_summary() {
        let value = json!({
            "groups": [{
                "displayName": "Claude models",
                "buckets": [{
                    "displayName": "5 hour",
                    "remainingFraction": 0.72,
                    "resetTime": "2026-07-13T20:00:00Z"
                }]
            }]
        });
        let windows = parse_quota_value(&value);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].remaining_percent, Some(72.0));
        assert_eq!(windows[0].window_seconds, Some(18_000));
    }
}
