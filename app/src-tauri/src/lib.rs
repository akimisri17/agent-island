pub mod cursor;
pub mod logs;

use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent,
};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_positioner::{Position, WindowExt};

const PANEL: &str = "panel";

/// Reads agent logs from the last `days` days. Runs off the main thread so
/// the menu bar never stalls while a few GB of logs are read.
#[tauri::command]
async fn scan(app: AppHandle, days: u32) -> Result<logs::ScanResult, String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || logs::scan(&logs::Roots::default_for(&home), days.clamp(1, 365)))
        .await
        .map_err(|e| e.to_string())
}

/// Writes the full report into the app's cache folder and opens it in the
/// default browser. The file never leaves the machine.
#[tauri::command]
fn open_report(app: AppHandle, html: String) -> Result<(), String> {
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("agent-wrapped.html");
    std::fs::write(&path, html).map_err(|e| e.to_string())?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

fn toggle_panel(app: &AppHandle) {
    let Some(w) = app.get_webview_window(PANEL) else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    // The menu bar is at the top on macOS; the taskbar is usually at the bottom on Windows.
    #[cfg(target_os = "macos")]
    let _ = w.move_window(Position::TrayCenter);
    #[cfg(not(target_os = "macos"))]
    let _ = w.move_window(Position::TrayBottomCenter);
    let _ = w.show();
    let _ = w.set_focus();
    let _ = w.emit_to(PANEL, "panel-shown", ());
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![scan, open_report, quit])
        .setup(|app| {
            // Menu-bar only: no Dock icon, no app switcher entry.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let report = MenuItem::with_id(app, "report", "Open full report", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Agent Island", true, Some("CmdOrCtrl+Q"))?;
            let menu = Menu::with_items(app, &[&report, &quit])?;

            TrayIconBuilder::with_id("main")
                .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
                .icon_as_template(true)
                .tooltip("Agent Island")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id().as_ref() {
                    "report" => {
                        let _ = app.emit_to(PANEL, "open-report", ());
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &e);
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        toggle_panel(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Behaves like a popover: clicking anywhere else closes it.
            if window.label() == PANEL {
                if let WindowEvent::Focused(false) = event {
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Agent Island");
}
