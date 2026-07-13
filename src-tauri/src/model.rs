use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Openai,
    Anthropic,
    Antigravity,
    OpencodeGo,
}

impl Provider {
    pub fn id(&self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Antigravity => "antigravity",
            Self::OpencodeGo => "opencode_go",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Openai => "OpenAI Codex",
            Self::Anthropic => "Anthropic Claude",
            Self::Antigravity => "Google Antigravity",
            Self::OpencodeGo => "OpenCode Go",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.id())
    }
}

impl FromStr for Provider {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "openai" | "codex" => Ok(Self::Openai),
            "anthropic" | "claude" => Ok(Self::Anthropic),
            "antigravity" | "google_antigravity" | "google" => Ok(Self::Antigravity),
            "opencode_go" | "opencode" | "go" => Ok(Self::OpencodeGo),
            _ => Err("Unsupported provider.".into()),
        }
    }
}

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
    #[serde(default)]
    pub provider: Provider,
    pub email: Option<String>,
    #[serde(default)]
    pub provider_account_id: Option<String>,
    #[serde(default)]
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

    pub fn effective_account_id(&self) -> Option<&str> {
        self.provider_account_id
            .as_deref()
            .or(self.chatgpt_account_id.as_deref())
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeGoSecret {
    pub workspace_id: String,
    pub auth_cookie: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "provider", content = "credentials", rename_all = "snake_case")]
pub enum ProviderSecret {
    Openai(OAuthSecret),
    Anthropic(OAuthSecret),
    Antigravity(OAuthSecret),
    OpencodeGo(OpenCodeGoSecret),
}

impl ProviderSecret {
    pub fn provider(&self) -> Provider {
        match self {
            Self::Openai(_) => Provider::Openai,
            Self::Anthropic(_) => Provider::Anthropic,
            Self::Antigravity(_) => Provider::Antigravity,
            Self::OpencodeGo(_) => Provider::OpencodeGo,
        }
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
pub struct AppUpdateStatus {
    pub current_version: String,
    pub available: bool,
    pub available_version: Option<String>,
    pub date: Option<String>,
    pub body: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicUsageAccount {
    pub id: String,
    pub label: String,
    pub provider: Provider,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub plan: Option<String>,
    pub status: String,
    pub source: Option<String>,
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
