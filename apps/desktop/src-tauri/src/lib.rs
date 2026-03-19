use std::sync::Arc;

use desktop_core::{DesktopConfig, DesktopRuntime, DesktopStatus};
use tauri::{Manager, State};

struct DesktopState {
    runtime: Arc<DesktopRuntime>,
}

#[tauri::command]
async fn desktop_status(state: State<'_, DesktopState>) -> Result<DesktopStatus, String> {
    Ok(state.runtime.status().await)
}

#[tauri::command]
async fn start_control_server(state: State<'_, DesktopState>) -> Result<String, String> {
    state
        .runtime
        .start_control_server()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_control_server(state: State<'_, DesktopState>) -> Result<(), String> {
    state
        .runtime
        .stop_control_server()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_secret(state: State<'_, DesktopState>, key: String, value: String) -> Result<(), String> {
    state
        .runtime
        .set_secret(&key, &value)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_secret(state: State<'_, DesktopState>, key: String) -> Result<Option<String>, String> {
    state
        .runtime
        .get_secret(&key)
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let runtime = Arc::new(
        DesktopRuntime::with_keyring_secrets(DesktopConfig::from_env())
            .expect("failed to initialize desktop runtime"),
    );

    tauri::Builder::default()
        .manage(DesktopState {
            runtime: runtime.clone(),
        })
        .invoke_handler(tauri::generate_handler![
            desktop_status,
            start_control_server,
            stop_control_server,
            set_secret,
            get_secret
        ])
        .setup(move |app| {
            let runtime = app.state::<DesktopState>().runtime.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = runtime.start_control_server().await {
                    tracing::error!("failed to start control server: {error}");
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
