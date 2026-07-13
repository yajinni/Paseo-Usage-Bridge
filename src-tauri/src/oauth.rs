use crate::{
    model::{now_rfc3339, Account, LoginStart, LoginStatus, OAuthSecret, TokenClaims},
    state::AppState,
    store::save_secret,
};
use axum::{
    extract::{Query, State},
    response::Html,
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{Duration, Utc};
use parking_lot::RwLock;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::{net::TcpListener, sync::oneshot};
use url::Url;
use uuid::Uuid;

const OPENAI_ISSUER: &str = "https://auth.openai.com";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const REDIRECT_PORTS: [u16; 5] = [1455, 1456, 1457, 1458, 1459];
const LOGIN_TIMEOUT_MINUTES: i64 = 5;

#[derive(Clone)]
struct LoginContext {
    app: Arc<AppState>,
    attempt_id: String,
    label: String,
    verifier: String,
    expected_state: String,
    redirect_uri: String,
    shutdown: Arc<tokio::sync::Mutex<Option<oneshot::Sender<()>>>>,
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: i64,
}

#[derive(Debug, Serialize)]
struct TokenRequest<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    code: &'a str,
    code_verifier: &'a str,
    redirect_uri: &'a str,
}

pub async fn start_login(app: Arc<AppState>, label: String) -> Result<LoginStart, String> {
    {
        let pending = app.pending_login.read();
        if pending
            .as_ref()
            .is_some_and(|login| login.status == "waiting")
        {
            return Err("Another OpenAI login is already in progress.".into());
        }
    }

    let (listener, port) = bind_callback_port().await?;
    let verifier = random_base64(32);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let oauth_state = random_base64(24);
    let attempt_id = Uuid::new_v4().to_string();
    let redirect_uri = format!("http://localhost:{port}/auth/callback");
    let expires_at = (Utc::now() + Duration::minutes(LOGIN_TIMEOUT_MINUTES)).to_rfc3339();
    let authorization_url = build_authorization_url(&redirect_uri, &challenge, &oauth_state)?;

    *app.pending_login.write() = Some(LoginStatus {
        attempt_id: attempt_id.clone(),
        status: "waiting".into(),
        message: None,
        account: None,
    });

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let context = Arc::new(LoginContext {
        app: app.clone(),
        attempt_id: attempt_id.clone(),
        label,
        verifier,
        expected_state: oauth_state,
        redirect_uri,
        shutdown: Arc::new(tokio::sync::Mutex::new(Some(shutdown_tx))),
    });
    let router = Router::new()
        .route("/auth/callback", get(callback))
        .with_state(context.clone());

    let server_context = context.clone();
    tokio::spawn(async move {
        let result = axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
        if let Err(error) = result {
            fail_login(
                &server_context.app.pending_login,
                &server_context.attempt_id,
                format!("Callback server failed: {error}"),
            );
        }
    });

    let timeout_context = context.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(
            (LOGIN_TIMEOUT_MINUTES * 60) as u64,
        ))
        .await;
        let should_timeout = timeout_context
            .app
            .pending_login
            .read()
            .as_ref()
            .is_some_and(|login| {
                login.attempt_id == timeout_context.attempt_id && login.status == "waiting"
            });
        if should_timeout {
            fail_login(
                &timeout_context.app.pending_login,
                &timeout_context.attempt_id,
                "OpenAI login timed out. Start the login again.".into(),
            );
            stop_callback(&timeout_context).await;
        }
    });

    Ok(LoginStart {
        attempt_id,
        authorization_url,
        expires_at,
    })
}

pub fn login_status(app: &AppState, attempt_id: &str) -> Result<LoginStatus, String> {
    let pending = app.pending_login.read();
    let status = pending
        .as_ref()
        .ok_or_else(|| "No login attempt is available.".to_string())?;
    if status.attempt_id != attempt_id {
        return Err("The login attempt is no longer active.".into());
    }
    Ok(status.clone())
}

async fn callback(
    State(context): State<Arc<LoginContext>>,
    Query(query): Query<CallbackQuery>,
) -> Html<String> {
    let result = complete_callback(context.clone(), query).await;
    if let Err(error) = &result {
        fail_login(
            &context.app.pending_login,
            &context.attempt_id,
            error.clone(),
        );
    }
    stop_callback(&context).await;
    match result {
        Ok(account) => Html(format!(
            r#"<!doctype html><html><body style="background:#101412;color:#f4f6f8;font-family:system-ui;padding:50px;text-align:center"><h1>Account connected</h1><p>{}</p><p style="color:#8e9791">You can close this tab and return to Paseo Usage Bridge.</p></body></html>"#,
            escape_html(account.email.as_deref().unwrap_or(&account.label))
        )),
        Err(error) => Html(format!(
            r#"<!doctype html><html><body style="background:#101412;color:#f4f6f8;font-family:system-ui;padding:50px;text-align:center"><h1>Authentication failed</h1><p style="color:#ff9d9d">{}</p><p style="color:#8e9791">Return to the app and try again.</p></body></html>"#,
            escape_html(&error)
        )),
    }
}

async fn complete_callback(
    context: Arc<LoginContext>,
    query: CallbackQuery,
) -> Result<Account, String> {
    if let Some(error) = query.error {
        let message = query.error_description.unwrap_or(error);
        fail_login(
            &context.app.pending_login,
            &context.attempt_id,
            message.clone(),
        );
        return Err(message);
    }
    let code = query
        .code
        .ok_or_else(|| "OpenAI did not return an authorization code.".to_string())?;
    if query.state.as_deref() != Some(context.expected_state.as_str()) {
        let message = "OAuth state validation failed.".to_string();
        fail_login(
            &context.app.pending_login,
            &context.attempt_id,
            message.clone(),
        );
        return Err(message);
    }

    let token_url = format!("{OPENAI_ISSUER}/oauth/token");
    let response = context
        .app
        .client
        .post(token_url)
        .form(&TokenRequest {
            grant_type: "authorization_code",
            client_id: CLIENT_ID,
            code: &code,
            code_verifier: &context.verifier,
            redirect_uri: &context.redirect_uri,
        })
        .send()
        .await
        .map_err(|error| format!("Token exchange failed: {error}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = format!(
            "Token exchange failed ({status}): {}",
            body.chars().take(180).collect::<String>()
        );
        fail_login(
            &context.app.pending_login,
            &context.attempt_id,
            message.clone(),
        );
        return Err(message);
    }
    let tokens: TokenResponse = response
        .json()
        .await
        .map_err(|error| format!("Invalid token response: {error}"))?;
    let refresh_token = tokens
        .refresh_token
        .clone()
        .ok_or_else(|| "OpenAI did not return a refresh token.".to_string())?;

    let access_claims = decode_claims(&tokens.access_token);
    let id_claims = tokens.id_token.as_deref().and_then(decode_claims);
    let mut claims = merge_claims(access_claims, id_claims);
    if claims.email.is_none() {
        claims.email = fetch_user_email(&context.app, &tokens.access_token).await;
    }
    let expires_at = claims
        .expires_at
        .unwrap_or_else(|| Utc::now().timestamp_millis() + tokens.expires_in * 1000);

    let duplicate = context
        .app
        .store
        .find_duplicate(claims.account_id.as_deref(), claims.email.as_deref());
    let now = now_rfc3339();
    let account = Account {
        id: duplicate
            .as_ref()
            .map(|account| account.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        label: context.label.trim().to_string(),
        email: claims.email,
        chatgpt_account_id: claims.account_id,
        plan: claims.plan,
        created_at: duplicate
            .as_ref()
            .map(|account| account.created_at.clone())
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
        last_usage: duplicate
            .as_ref()
            .and_then(|account| account.last_usage.clone()),
        last_error: None,
        auth_required: false,
    };
    save_secret(
        &account.id,
        &OAuthSecret {
            access_token: tokens.access_token,
            refresh_token,
            id_token: tokens.id_token,
            expires_at,
        },
    )
    .map_err(|error| error.to_string())?;
    let account = context
        .app
        .store
        .upsert(account)
        .map_err(|error| error.to_string())?;
    *context.app.pending_login.write() = Some(LoginStatus {
        attempt_id: context.attempt_id.clone(),
        status: "complete".into(),
        message: None,
        account: Some(account.clone()),
    });
    Ok(account)
}

fn build_authorization_url(
    redirect_uri: &str,
    challenge: &str,
    state: &str,
) -> Result<String, String> {
    let mut url =
        Url::parse(&format!("{OPENAI_ISSUER}/oauth/authorize")).map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid profile email offline_access")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("audience", "https://api.openai.com/v1")
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("originator", "codex_cli_rs");
    Ok(url.to_string())
}

async fn bind_callback_port() -> Result<(TcpListener, u16), String> {
    for port in REDIRECT_PORTS {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => return Ok((listener, port)),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(error) => return Err(format!("Unable to start OAuth callback server: {error}")),
        }
    }
    Err("OAuth callback ports 1455–1459 are already in use.".into())
}

fn random_base64(bytes: usize) -> String {
    let mut value = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

async fn fetch_user_email(app: &AppState, access_token: &str) -> Option<String> {
    let response = app
        .client
        .get(format!("{OPENAI_ISSUER}/userinfo"))
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: Value = response.json().await.ok()?;
    value
        .get("email")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub fn decode_claims(token: &str) -> Option<TokenClaims> {
    let segment = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(segment).ok()?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    let auth = value.get("https://api.openai.com/auth");
    let email = string_at(&value, "email")
        .or_else(|| auth.and_then(|value| string_at(value, "email")));
    let account_id = auth
        .and_then(|value| string_at(value, "chatgpt_account_id"))
        .or_else(|| string_at(&value, "chatgpt_account_id"));
    let plan = auth
        .and_then(|value| string_at(value, "chatgpt_plan_type"))
        .or_else(|| string_at(&value, "chatgpt_plan_type"));
    let expires_at = value
        .get("exp")
        .and_then(Value::as_i64)
        .map(|seconds| seconds * 1000);
    Some(TokenClaims {
        email,
        account_id,
        plan,
        expires_at,
    })
}

fn merge_claims(primary: Option<TokenClaims>, secondary: Option<TokenClaims>) -> TokenClaims {
    let primary = primary.unwrap_or(TokenClaims {
        email: None,
        account_id: None,
        plan: None,
        expires_at: None,
    });
    let secondary = secondary.unwrap_or(TokenClaims {
        email: None,
        account_id: None,
        plan: None,
        expires_at: None,
    });
    TokenClaims {
        email: secondary.email.or(primary.email),
        account_id: secondary.account_id.or(primary.account_id),
        plan: secondary.plan.or(primary.plan),
        expires_at: primary.expires_at.or(secondary.expires_at),
    }
}

fn string_at(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn fail_login(store: &RwLock<Option<LoginStatus>>, attempt_id: &str, message: String) {
    *store.write() = Some(LoginStatus {
        attempt_id: attempt_id.into(),
        status: "failed".into(),
        message: Some(message),
        account: None,
    });
}

async fn stop_callback(context: &LoginContext) {
    if let Some(sender) = context.shutdown.lock().await.take() {
        let _ = sender.send(());
    }
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_claims() {
        let payload = serde_json::json!({
            "email": "person@example.com",
            "exp": 2000000000,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acct_123",
                "chatgpt_plan_type": "plus"
            }
        });
        let token = format!("x.{}.y", URL_SAFE_NO_PAD.encode(payload.to_string()));
        let claims = decode_claims(&token).unwrap();
        assert_eq!(claims.email.as_deref(), Some("person@example.com"));
        assert_eq!(claims.account_id.as_deref(), Some("acct_123"));
        assert_eq!(claims.plan.as_deref(), Some("plus"));
    }
}
