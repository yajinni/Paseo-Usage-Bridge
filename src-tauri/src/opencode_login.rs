use crate::{
    model::{
        now_rfc3339, Account, LoginStart, LoginStatus, OpenCodeGoSecret, Provider,
        ProviderSecret,
    },
    providers,
    state::AppState,
    store::save_provider_secret,
    usage,
};
use chrono::{Duration, Utc};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration as StdDuration,
};
use tauri::{
    webview::PageLoadEvent, AppHandle, Manager, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use url::Url;
use uuid::Uuid;

const LOGIN_WINDOW_LABEL: &str = "opencode-go-login";
const LOGIN_URL: &str = "https://opencode.ai/auth";
const LOGIN_TIMEOUT_MINUTES: i64 = 10;
const COOKIE_CAPTURE_ATTEMPTS: usize = 30;
const COOKIE_CAPTURE_RETRY_DELAY_MS: u64 = 500;

const CONNECT_BANNER_SCRIPT: &str = r#"
(() => {
  if (window.top !== window || window.location.hostname !== 'opencode.ai') return;

  const isGoRoute = () => /^\/workspace\/[^/]+\/go(?:\/|$)/.test(window.location.pathname);
  let lastHref = window.location.href;
  let reloadScheduled = false;

  const detectGoNavigation = () => {
    const currentHref = window.location.href;
    if (currentHref === lastHref) return;
    lastHref = currentHref;

    if (!isGoRoute() || reloadScheduled) return;
    reloadScheduled = true;
    window.setTimeout(() => window.location.reload(), 50);
  };

  for (const methodName of ['pushState', 'replaceState']) {
    const original = window.history[methodName].bind(window.history);
    window.history[methodName] = function (...args) {
      const result = original(...args);
      window.setTimeout(detectGoNavigation, 0);
      return result;
    };
  }

  window.addEventListener('popstate', detectGoNavigation);
  window.addEventListener('hashchange', detectGoNavigation);
  window.setInterval(detectGoNavigation, 250);

  const installBanner = () => {
    if (!document.body || document.getElementById('paseo-opencode-connect-banner')) return;

    const banner = document.createElement('div');
    banner.id = 'paseo-opencode-connect-banner';
    banner.setAttribute('role', 'status');
    banner.textContent = 'Paseo Usage Bridge: Sign in to OpenCode, then select Go from the sidebar. This window will close automatically when your Go usage page is detected.';
    Object.assign(banner.style, {
      position: 'fixed',
      top: '0',
      left: '0',
      right: '0',
      zIndex: '2147483647',
      boxSizing: 'border-box',
      padding: '14px 22px',
      background: '#211936',
      color: '#f7f4ff',
      borderBottom: '1px solid #7c52d9',
      fontFamily: 'system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif',
      fontSize: '14px',
      fontWeight: '650',
      lineHeight: '1.45',
      textAlign: 'center',
      boxShadow: '0 8px 24px rgba(0, 0, 0, .3)'
    });

    document.body.prepend(banner);
    const bannerHeight = Math.ceil(banner.getBoundingClientRect().height);
    const currentPadding = Number.parseFloat(getComputedStyle(document.body).paddingTop) || 0;
    document.body.style.paddingTop = `${currentPadding + bannerHeight}px`;
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', installBanner, { once: true });
  } else {
    installBanner();
  }
})();
"#;

pub async fn start_login(
    app: AppHandle,
    state: Arc<AppState>,
    label: String,
) -> Result<LoginStart, String> {
    if state
        .pending_login
        .read()
        .as_ref()
        .is_some_and(|login| login.status == "waiting")
    {
        return Err("Another provider login is already in progress.".into());
    }

    if let Some(window) = app.get_webview_window(LOGIN_WINDOW_LABEL) {
        let _ = window.destroy();
    }

    let attempt_id = Uuid::new_v4().to_string();
    let expires_at = (Utc::now() + Duration::minutes(LOGIN_TIMEOUT_MINUTES)).to_rfc3339();
    *state.pending_login.write() = Some(LoginStatus {
        attempt_id: attempt_id.clone(),
        status: "waiting".into(),
        message: Some("Sign in to OpenCode and open your Go subscription.".into()),
        account: None,
    });

    let (width, height) = login_window_size(&app);
    let login_url = Url::parse(LOGIN_URL).map_err(|error| error.to_string())?;
    let capture_started = Arc::new(AtomicBool::new(false));

    let page_state = state.clone();
    let page_attempt = attempt_id.clone();
    let page_label = label.clone();
    let page_capture_started = capture_started.clone();

    let login_window = WebviewWindowBuilder::new(
        &app,
        LOGIN_WINDOW_LABEL,
        WebviewUrl::External(login_url.clone()),
    )
    .title("Connect OpenCode Go — sign in, then select Go")
    .inner_size(width, height)
    .min_inner_size(820.0, 620.0)
    .center()
    .resizable(true)
    .incognito(true)
    .devtools(false)
    .initialization_script(CONNECT_BANNER_SCRIPT)
    .on_navigation(|url| {
        matches!(url.scheme(), "http" | "https") || url.as_str() == "about:blank"
    })
    .on_page_load(move |window, payload| {
        if !matches!(payload.event(), PageLoadEvent::Finished) {
            return;
        }
        let Some(workspace_id) = workspace_id_from_url(payload.url()) else {
            return;
        };
        if !is_waiting(&page_state, &page_attempt) {
            return;
        }
        if page_capture_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let cookie_window = window.clone();
        let completion_window = window.clone();
        let completion_state = page_state.clone();
        let completion_attempt = page_attempt.clone();
        let completion_label = page_label.clone();
        let capture_flag = page_capture_started.clone();

        std::thread::spawn(move || {
            let cookie_result = read_auth_cookie_with_retry(&cookie_window);

            tauri::async_runtime::spawn(async move {
                match cookie_result {
                    Ok(auth_cookie) => {
                        complete_login(
                            completion_state,
                            completion_attempt,
                            completion_label,
                            workspace_id,
                            auth_cookie,
                            completion_window,
                        )
                        .await;
                    }
                    Err(error) => {
                        capture_flag.store(false, Ordering::SeqCst);
                        update_waiting_message(
                            &completion_state,
                            &completion_attempt,
                            format!("{error} Navigate away from Go, then select Go again to retry."),
                        );
                    }
                }
            });
        });
    })
    .build()
    .map_err(|error| format!("Unable to open the OpenCode login window: {error}"))?;

    let close_state = state.clone();
    let close_attempt = attempt_id.clone();
    login_window.on_window_event(move |event| {
        if matches!(event, WindowEvent::CloseRequested { .. }) {
            fail_if_waiting(
                &close_state,
                &close_attempt,
                "OpenCode login was cancelled.".into(),
            );
        }
    });

    let timeout_state = state.clone();
    let timeout_attempt = attempt_id.clone();
    let timeout_app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(StdDuration::from_secs(
            (LOGIN_TIMEOUT_MINUTES * 60) as u64,
        ))
        .await;
        if is_waiting(&timeout_state, &timeout_attempt) {
            fail_if_waiting(
                &timeout_state,
                &timeout_attempt,
                "OpenCode login timed out. Start the connection again.".into(),
            );
            if let Some(window) = timeout_app.get_webview_window(LOGIN_WINDOW_LABEL) {
                let _ = window.destroy();
            }
        }
    });

    Ok(LoginStart {
        attempt_id,
        authorization_url: LOGIN_URL.into(),
        expires_at,
    })
}

pub async fn add_account(
    state: Arc<AppState>,
    label: String,
    workspace_id: String,
    auth_cookie: String,
) -> Result<Account, String> {
    let workspace_id = workspace_id.trim();
    if workspace_id.is_empty() || workspace_id.chars().count() > 160 {
        return Err("A valid OpenCode workspace ID is required.".into());
    }

    let auth_cookie = providers::opencode_go::normalize_cookie(&auth_cookie);
    if auth_cookie.is_empty() || auth_cookie.chars().count() > 4096 {
        return Err("A valid OpenCode console auth cookie is required.".into());
    }

    let provider = Provider::OpencodeGo;
    let duplicate = state
        .store
        .find_duplicate(&provider, Some(workspace_id), None);
    let now = now_rfc3339();
    let account = Account {
        id: duplicate
            .as_ref()
            .map(|account| account.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        label: if label.trim().is_empty() {
            "OpenCode Go".into()
        } else {
            label.trim().to_string()
        },
        provider,
        email: duplicate.as_ref().and_then(|account| account.email.clone()),
        provider_account_id: Some(workspace_id.to_string()),
        chatgpt_account_id: None,
        plan: Some("OpenCode Go".into()),
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

    save_provider_secret(
        &account.id,
        &ProviderSecret::OpencodeGo(OpenCodeGoSecret {
            workspace_id: workspace_id.to_string(),
            auth_cookie,
        }),
    )
    .map_err(|error| error.to_string())?;

    let account = state
        .store
        .upsert(account)
        .map_err(|error| error.to_string())?;
    usage::refresh_account(state, &account.id).await
}

async fn complete_login(
    state: Arc<AppState>,
    attempt_id: String,
    label: String,
    workspace_id: String,
    auth_cookie: String,
    window: WebviewWindow,
) {
    if !is_waiting(&state, &attempt_id) {
        return;
    }

    match add_account(state.clone(), label, workspace_id, auth_cookie).await {
        Ok(account) => {
            *state.pending_login.write() = Some(LoginStatus {
                attempt_id,
                status: "complete".into(),
                message: None,
                account: Some(account),
            });
            let _ = window.destroy();
        }
        Err(error) => {
            fail_if_waiting(&state, &attempt_id, error);
            let _ = window.destroy();
        }
    }
}

fn read_auth_cookie_with_retry(window: &WebviewWindow) -> Result<String, String> {
    let cookie_url = Url::parse("https://opencode.ai/")
        .map_err(|error| format!("Unable to prepare the OpenCode cookie request: {error}"))?;

    std::thread::sleep(StdDuration::from_millis(250));

    let mut last_read_error = None;
    for attempt in 0..COOKIE_CAPTURE_ATTEMPTS {
        match window.cookies_for_url(cookie_url.clone()) {
            Ok(cookies) => {
                if let Some(value) = cookies
                    .into_iter()
                    .find(|cookie| cookie.name() == "auth")
                    .map(|cookie| cookie.value().to_string())
                    .filter(|value| !value.trim().is_empty())
                {
                    return Ok(value);
                }
            }
            Err(error) => {
                last_read_error = Some(error.to_string());
            }
        }

        if attempt + 1 < COOKIE_CAPTURE_ATTEMPTS {
            std::thread::sleep(StdDuration::from_millis(
                COOKIE_CAPTURE_RETRY_DELAY_MS,
            ));
        }
    }

    match last_read_error {
        Some(error) => Err(format!(
            "Unable to read the OpenCode login session after retrying: {error}."
        )),
        None => Err(
            "OpenCode did not provide a usable login session after retrying. Confirm that sign-in completed."
                .into(),
        ),
    }
}

fn login_window_size(app: &AppHandle) -> (f64, f64) {
    let Some(main) = app.get_webview_window("main") else {
        return (1150.0, 760.0);
    };
    let Ok(size) = main.inner_size() else {
        return (1150.0, 760.0);
    };
    let scale = main.scale_factor().unwrap_or(1.0).max(0.1);
    let logical_width = size.width as f64 / scale;
    let logical_height = size.height as f64 / scale;
    (
        (logical_width * 0.8).clamp(820.0, 1500.0),
        (logical_height * 0.8).clamp(620.0, 1000.0),
    )
}

fn workspace_id_from_url(url: &Url) -> Option<String> {
    if url.host_str() != Some("opencode.ai") {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    let workspace_index = segments.iter().position(|segment| *segment == "workspace")?;
    if segments.get(workspace_index + 2).copied() != Some("go") {
        return None;
    }
    let workspace_id = segments.get(workspace_index + 1)?.trim();
    if workspace_id.is_empty() || workspace_id.chars().count() > 160 {
        return None;
    }
    Some(workspace_id.to_string())
}

fn is_waiting(state: &AppState, attempt_id: &str) -> bool {
    state
        .pending_login
        .read()
        .as_ref()
        .is_some_and(|login| login.attempt_id == attempt_id && login.status == "waiting")
}

fn update_waiting_message(state: &AppState, attempt_id: &str, message: String) {
    let mut pending = state.pending_login.write();
    if let Some(login) = pending.as_mut() {
        if login.attempt_id == attempt_id && login.status == "waiting" {
            login.message = Some(message);
        }
    }
}

fn fail_if_waiting(state: &AppState, attempt_id: &str, message: String) {
    if is_waiting(state, attempt_id) {
        *state.pending_login.write() = Some(LoginStatus {
            attempt_id: attempt_id.to_string(),
            status: "failed".into(),
            message: Some(message),
            account: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_on_the_opencode_auth_page() {
        assert_eq!(LOGIN_URL, "https://opencode.ai/auth");
    }

    #[test]
    fn detects_only_opencode_go_workspace_urls() {
        assert_eq!(
            workspace_id_from_url(
                &Url::parse("https://opencode.ai/workspace/mystic-patrol-3ls3t/go").unwrap()
            ),
            Some("mystic-patrol-3ls3t".into())
        );
        assert_eq!(
            workspace_id_from_url(
                &Url::parse(
                    "https://opencode.ai/workspace/mystic-patrol-3ls3t/go/?source=sidebar"
                )
                .unwrap()
            ),
            Some("mystic-patrol-3ls3t".into())
        );
        assert_eq!(
            workspace_id_from_url(
                &Url::parse("https://opencode.ai/workspace/mystic-patrol-3ls3t/settings").unwrap()
            ),
            None
        );
        assert_eq!(
            workspace_id_from_url(
                &Url::parse("https://example.com/workspace/mystic-patrol-3ls3t/go").unwrap()
            ),
            None
        );
    }
}
