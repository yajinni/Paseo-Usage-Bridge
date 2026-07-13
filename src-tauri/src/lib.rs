mod bridge_api;
mod model;
mod oauth;
mod state;
mod store;
mod usage;

use crate::{
    model::{Account, BridgeInfo, DashboardSnapshot, LoginStart, LoginStatus},
    state::AppState,
    store::{load_or_create_bridge_token, rotate_bridge_token},
};
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, State, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;

#[tauri::command]
async fn get_dashboard_snapshot(state: State<'_, Arc<AppState>>) -> Result<DashboardSnapshot, String> {
    Ok(DashboardSnapshot {
        accounts: state.store.list(),
        bridge: bridge_info(state.inner().as_ref()),
    })
}

#[tauri::command]
async fn start_login(state: State<'_, Arc<AppState>>, label: String) -> Result<LoginStart, String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("Account label is required.".into());
    }
    if label.chars().count() > 80 {
        return Err("Account label must be 80 characters or fewer.".into());
    }
    oauth::start_login(state.inner().clone(), label.to_string()).await
}

#[tauri::command]
fn get_login_status(state: State<'_, Arc<AppState>>, attempt_id: String) -> Result<LoginStatus, String> {
    oauth::login_status(state.inner().as_ref(), &attempt_id)
}

#[tauri::command]
async fn refresh_account(state: State<'_, Arc<AppState>>, account_id: String) -> Result<Account, String> {
    usage::refresh_account(state.inner().clone(), &account_id).await
}

#[tauri::command]
async fn refresh_all(state: State<'_, Arc<AppState>>) -> Result<Vec<Account>, String> {
    Ok(usage::refresh_all(state.inner().clone()).await)
}

#[tauri::command]
fn rename_account(state: State<'_, Arc<AppState>>, account_id: String, label: String) -> Result<Account, String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("Account label is required.".into());
    }
    if label.chars().count() > 80 {
        return Err("Account label must be 80 characters or fewer.".into());
    }
    state
        .store
        .mutate(&account_id, |account| account.label = label.to_string())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn remove_account(state: State<'_, Arc<AppState>>, account_id: String) -> Result<(), String> {
    state.store.remove(&account_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn regenerate_bridge_token(state: State<'_, Arc<AppState>>) -> Result<BridgeInfo, String> {
    let token = rotate_bridge_token().map_err(|error| error.to_string())?;
    *state.bridge_token.write() = token;
    Ok(bridge_info(state.inner().as_ref()))
}

fn bridge_info(state: &AppState) -> BridgeInfo {
    let runtime = state.api_runtime.read();
    BridgeInfo {
        endpoint: runtime.endpoint.clone(),
        token: state.bridge_token.read().clone(),
        running: runtime.running,
        error: runtime.error.clone(),
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let token = load_or_create_bridge_token()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let state = Arc::new(
                AppState::new(data_dir, token)
                    .map_err(std::io::Error::other)?,
            );
            app.manage(state.clone());
            tauri::async_runtime::spawn(bridge_api::run(state.clone()));
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                loop {
                    let _ = usage::refresh_all(state.clone()).await;
                    tokio::time::sleep(std::time::Duration::from_secs(5 * 60)).await;
                }
            });

            let show = MenuItem::with_id(app, "show", "Open Paseo Usage Bridge", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                if std::env::args().any(|argument| argument == "--hidden") {
                    let _ = window.hide();
                }
                let window_for_event = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_for_event.hide();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_dashboard_snapshot,
            start_login,
            get_login_status,
            refresh_account,
            refresh_all,
            rename_account,
            remove_account,
            regenerate_bridge_token,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Paseo Usage Bridge");
}
