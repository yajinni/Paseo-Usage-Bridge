use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub id: String,
    pub label: String,
    pub used_percent: Option<f64>,
    pub remaining_percent: Option<f64>,
    pub resets_at: Option<String>,
    pub window_seconds: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub plan: Option<String>,
    pub email: Option<String>,
    pub windows: Vec<UsageWindow>,
    pub credits_usd: Option<f64>,
    pub unlimited_credits: bool,
    pub fetched_at: String,
    pub freshness: UsageFreshness,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageFreshness {
    Live,
    Stale,
    Unavailable,
    AuthRequired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub chatgpt_account_id: Option<String>,
    pub plan: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_usage: Option<UsageSnapshot>,
    pub last_error: Option<String>,
    pub auth_required: bool,
}

impl Account {
    pub fn touch(&mut self) {
        self.updated_at = Utc::now().to_rfc3339();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthSecret {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: Option<String>,
    pub expires_at: i64,
}

impl OAuthSecret {
    pub fn expires_within(&self, seconds: i64) -> bool {
        self.expires_at <= Utc::now().timestamp_millis() + seconds * 1000
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub attempt_id: String,
    pub authorization_url: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    pub attempt_id: String,
    pub status: String,
    pub message: Option<String>,
    pub account: Option<Account>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInfo {
    pub endpoint: String,
    pub token: String,
    pub running: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSnapshot {
    pub accounts: Vec<Account>,
    pub bridge: BridgeInfo,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicUsageAccount {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub status: String,
    pub windows: Vec<UsageWindow>,
    pub credits_usd: Option<f64>,
    pub fetched_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicUsageResponse {
    pub schema_version: u32,
    pub generated_at: String,
    pub accounts: Vec<PublicUsageAccount>,
}

#[derive(Clone, Debug)]
pub struct TokenClaims {
    pub email: Option<String>,
    pub account_id: Option<String>,
    pub plan: Option<String>,
    pub expires_at: Option<i64>,
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

pub fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}
