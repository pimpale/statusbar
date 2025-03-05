use std::sync::Mutex;

use tauri::Manager;
mod wm_hints;

struct AppState {
    wm_manager: wm_hints::WmHintsState,
    // other app state variables
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn grab_keyboard(state: tauri::State<'_, Mutex<AppState>>) -> Result<(), String> {
    let state = state.lock().unwrap();
    match state.wm_manager.grab_keyboard() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to grab keyboard: {}", e)),
    }
}

#[tauri::command]
async fn ungrab_keyboard(state: tauri::State<'_, Mutex<AppState>>) -> Result<(), String> {
    let state = state.lock().unwrap();
    match state.wm_manager.ungrab_keyboard() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to ungrab keyboard: {}", e)),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            app.manage(Mutex::new(AppState {
                wm_manager: wm_hints::create_state_mgr(&window).unwrap(),
            }));
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .invoke_handler(tauri::generate_handler![grab_keyboard])
        .invoke_handler(tauri::generate_handler![ungrab_keyboard])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
