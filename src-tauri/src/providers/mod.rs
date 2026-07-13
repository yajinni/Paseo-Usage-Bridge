pub mod anthropic;
pub mod antigravity;
pub mod openai;
pub mod opencode_go;

use crate::{
    model::{Account, ProviderSecret, UsageWindow},
    state::AppState,
};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("authentication is required")]
    Auth,
    #[error("{0}")]
    Transient(String),
}

#[derive(Clone, Debug)]
pub struct ProviderUsage {
    pub plan: Option<String>,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub windows: Vec<UsageWindow>,
    pub credits_usd: Option<f64>,
    pub unlimited_credits: bool,
    pub source: String,
}

pub async fn refresh(
    app: Arc<AppState>,
    account: &Account,
    secret: ProviderSecret,
) -> Result<(ProviderUsage, ProviderSecret), ProviderError> {
    match secret {
        ProviderSecret::Openai(secret) => {
            let (usage, secret) = openai::refresh(app.as_ref(), account, secret).await?;
            Ok((usage, ProviderSecret::Openai(secret)))
        }
        ProviderSecret::Anthropic(secret) => {
            let (usage, secret) = anthropic::refresh(app.as_ref(), account, secret).await?;
            Ok((usage, ProviderSecret::Anthropic(secret)))
        }
        ProviderSecret::Antigravity(secret) => {
            let (usage, secret) = antigravity::refresh(app.as_ref(), account, secret).await?;
            Ok((usage, ProviderSecret::Antigravity(secret)))
        }
        ProviderSecret::OpencodeGo(secret) => {
            let usage = opencode_go::refresh(app.as_ref(), account, &secret).await?;
            Ok((usage, ProviderSecret::OpencodeGo(secret)))
        }
    }
}
