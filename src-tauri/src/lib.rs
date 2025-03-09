use std::sync::Mutex;

use tauri::{Manager, UserAttentionType};
mod wm_hints;

const WINDOW_WIDTH: f64 = 500.0;
const WINDOW_HEIGHT: f64 = 80.0;
const WINDOW_HEIGHT_EXPANDED: f64 = 500.0;

struct AppState {
    container_wm_manager: wm_hints::WmHintsState,
}

#[tauri::command]
async fn set_focus_state(
    focused: bool,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let state = state.lock().unwrap();
    state
        .container_wm_manager
        .focus_window(focused)
        .map_err(|e| format!("Failed to focus window: {}", e))
}

#[tauri::command]
async fn set_expand_state(expanded: bool, window: tauri::Window) -> Result<(), String> {
    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: WINDOW_WIDTH,
            height: if expanded {
                WINDOW_HEIGHT_EXPANDED
            } else {
                WINDOW_HEIGHT
            },
        }))
        .map_err(|e| {
            format!(
                "Failed to {} window: {}",
                if expanded { "expand" } else { "unexpand" },
                e
            )
        })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            let container_wm_manager = wm_hints::create_state_mgr(&window).unwrap();

            container_wm_manager
                .dock_window(WINDOW_HEIGHT_EXPANDED as u32)
                .unwrap();
            app.manage(Mutex::new(AppState {
                container_wm_manager,
            }));
            window
                .request_user_attention(Some(UserAttentionType::Informational))
                .unwrap();
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![set_focus_state, set_expand_state,])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
