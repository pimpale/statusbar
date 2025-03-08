use std::sync::Mutex;

use tauri::{Manager, UserAttentionType};
mod wm_hints;

const WINDOW_WIDTH: f64 = 500.0;
const WINDOW_HEIGHT: f64 = 50.0;
const WINDOW_HEIGHT_EXPANDED: f64 = 500.0;


struct AppState {
    container_wm_manager: wm_hints::WmHintsState,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn focus_window(state: tauri::State<'_, Mutex<AppState>>) -> Result<(), String> {
    let state = state.lock().unwrap();
    state
        .container_wm_manager
        .focus_window()
        .map_err(|e| format!("Failed to focus window: {}", e))
}

#[tauri::command]
async fn unfocus_window(state: tauri::State<'_, Mutex<AppState>>) -> Result<(), String> {
    let state = state.lock().unwrap();
    state
        .container_wm_manager
        .unfocus_window()
        .map_err(|e| format!("Failed to unfocus window: {}", e))
}

#[tauri::command]
async fn expand_window(window: tauri::Window) -> Result<(), String> {
    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT_EXPANDED,
        }))
        .map_err(|e| format!("Failed to expand window: {}", e))
}

#[tauri::command]
async fn unexpand_window(window: tauri::Window) -> Result<(), String> {
    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }))
        .map_err(|e| format!("Failed to unexpand window: {}", e))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            let container_wm_manager = wm_hints::create_state_mgr(&window).unwrap();

            container_wm_manager.dock_window(WINDOW_HEIGHT_EXPANDED as u32).unwrap();
            app.manage(Mutex::new(AppState {
                container_wm_manager,
            }));
            window.request_user_attention(Some(UserAttentionType::Informational)).unwrap();
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            focus_window,
            unfocus_window,
            expand_window,
            unexpand_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
