use crate::{
    model::{now_rfc3339, PublicUsageAccount, PublicUsageResponse, UsageFreshness},
    state::AppState,
};
use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;

const API_ADDR: &str = "127.0.0.1:47831";

pub async fn run(app: Arc<AppState>) {
    match TcpListener::bind(API_ADDR).await {
        Ok(listener) => {
            {
                let mut runtime = app.api_runtime.write();
                runtime.running = true;
                runtime.error = None;
            }
            let router = Router::new()
                .route("/v1/health", get(health))
                .route("/v1/paseo-usage", get(usage))
                .with_state(app.clone());
            if let Err(error) = axum::serve(listener, router).await {
                let mut runtime = app.api_runtime.write();
                runtime.running = false;
                runtime.error = Some(format!("Local API stopped: {error}"));
            }
        }
        Err(error) => {
            let mut runtime = app.api_runtime.write();
            runtime.running = false;
            runtime.error = Some(format!("Unable to bind {API_ADDR}: {error}"));
        }
    }
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true, "schemaVersion": 1 }))
}

async fn usage(State(app): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&app, &headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let accounts = app
        .store
        .list()
        .into_iter()
        .map(|account| {
            let usage = account.last_usage.clone();
            let status = if account.auth_required {
                "auth_required"
            } else {
                match usage.as_ref().map(|usage| &usage.freshness) {
                    Some(UsageFreshness::Live) => "available",
                    Some(UsageFreshness::Stale) => "stale",
                    Some(UsageFreshness::AuthRequired) => "auth_required",
                    _ => "unavailable",
                }
            };
            PublicUsageAccount {
                id: account.id,
                label: account.label,
                email: account.email,
                plan: account.plan,
                status: status.into(),
                windows: usage
                    .as_ref()
                    .map(|usage| usage.windows.clone())
                    .unwrap_or_default(),
                credits_usd: usage.as_ref().and_then(|usage| usage.credits_usd),
                fetched_at: usage.as_ref().map(|usage| usage.fetched_at.clone()),
                error: account.last_error,
            }
        })
        .collect();
    (
        StatusCode::OK,
        Json(PublicUsageResponse {
            schema_version: 1,
            generated_at: now_rfc3339(),
            accounts,
        }),
    )
        .into_response()
}

fn authorized(app: &AppState, headers: &HeaderMap) -> bool {
    let expected = app.bridge_token.read();
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|provided| constant_time_equal(provided.as_bytes(), expected.as_bytes()))
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        difference |= *left ^ *right;
    }
    difference == 0
}
