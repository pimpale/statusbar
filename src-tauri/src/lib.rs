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
    state.wm_manager.grab_keyboard().map_err(|e| format!("Failed to grab keyboard: {}", e))
}

#[tauri::command]
async fn ungrab_keyboard(state: tauri::State<'_, Mutex<AppState>>) -> Result<(), String> {
    let state = state.lock().unwrap();
    state.wm_manager.ungrab_keyboard().map_err(|e| format!("Failed to ungrab keyboard: {}", e))
}

#[tauri::command]
async fn expand_window(window: tauri::Window) -> Result<(), String> {
    window.set_size(tauri::Size::Logical(tauri::LogicalSize { width: 500.0, height: 500.0 }))
        .map_err(|e| format!("Failed to expand window: {}", e))
}

#[tauri::command]
async fn unexpand_window(window: tauri::Window) -> Result<(), String> {
    window.set_size(tauri::Size::Logical(tauri::LogicalSize { width: 500.0, height: 200.0 }))
        .map_err(|e| format!("Failed to unexpand window: {}", e))
}

#[tauri::command]
async fn set_window_type(state: tauri::State<'_, Mutex<AppState>>, window_type: &str) -> Result<(), String> {
    let state = state.lock().unwrap();
    let window_type = match window_type {
        "dock" => wm_hints::WindowType::Dock,
        "toolbar" => wm_hints::WindowType::Toolbar,
        "menu" => wm_hints::WindowType::Menu,
        "utility" => wm_hints::WindowType::Utility,
        "splash" => wm_hints::WindowType::Splash,
        "dialog" => wm_hints::WindowType::Dialog,
        "dropdown_menu" => wm_hints::WindowType::DropdownMenu,
        "popup_menu" => wm_hints::WindowType::PopupMenu,
        "tooltip" => wm_hints::WindowType::Tooltip,
        "notification" => wm_hints::WindowType::Notification,
        "combo" => wm_hints::WindowType::Combo,
        "dnd" => wm_hints::WindowType::Dnd,
        "normal" => wm_hints::WindowType::Normal,
        _ => return Err(format!("Invalid window type: {}", window_type)),
    };
    state.wm_manager.set_window_type(window_type)
        .map_err(|e| format!("Failed to set window type: {}", e))
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
        .invoke_handler(tauri::generate_handler![
            greet,
            grab_keyboard,
            ungrab_keyboard,
            expand_window,
            unexpand_window,
            set_window_type
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
