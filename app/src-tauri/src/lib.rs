pub mod antigravity;
pub mod cursor;
pub mod foreign;
pub mod limits;
pub mod live;
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

fn roots(app: &AppHandle) -> Result<logs::Roots, String> {
    Ok(logs::Roots::default_for(&app.path().home_dir().map_err(|e| e.to_string())?))
}

/// Running sessions, longest wait first.
#[tauri::command]
async fn live(app: AppHandle) -> Result<Vec<live::LiveSession>, String> {
    let r = roots(&app)?;
    let sessions = tauri::async_runtime::spawn_blocking(move || live::live_sessions(&r)).await.map_err(|e| e.to_string())?;
    set_badge(&app, sessions.iter().filter(|s| s.needs_you()).count());
    Ok(sessions)
}

#[tauri::command]
async fn jump(app: AppHandle, session_id: String) -> Result<(), String> {
    let r = roots(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let sessions = live::live_sessions(&r);
        let s = sessions.iter().find(|s| s.session_id == session_id).ok_or("that session is no longer running")?;
        live::jump(s)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Where you stand against usage limits. Reading 35 days of Claude logs
/// takes a few seconds, so Claude's part is cached for 5 minutes.
#[tauri::command]
async fn limits(app: AppHandle) -> Result<limits::Limits, String> {
    use std::sync::Mutex;
    static CLAUDE: Mutex<Option<(i64, Vec<limits::Spend>, Vec<limits::Hit>)>> = Mutex::new(None);
    let r = roots(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let now = logs::now_ms();
        let cached = CLAUDE.lock().ok().and_then(|c| c.clone()).filter(|(at, _, _)| now - at < 5 * 60_000);
        let (spend, hits) = match cached {
            Some((_, s, h)) => (s, h),
            None => {
                let (s, h) = limits::read_claude(&r.claude, now - 35 * 86_400_000);
                if let Ok(mut c) = CLAUDE.lock() {
                    *c = Some((now, s.clone(), h.clone()));
                }
                (s, h)
            }
        };
        let working = live::live_sessions(&r).iter().filter(|s| s.agent == "claude" && s.state == live::State::Working).count();
        limits::Limits {
            claude: (!spend.is_empty()).then(|| limits::claude_limit(&spend, &hits, now, working)),
            codex: limits::codex_limits(&r.codex, now),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// The hotkey: bring forward the session that has waited longest. With
/// nothing waiting, open the panel instead.
fn jump_to_longest_wait(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(r) = roots(&app) else { return };
        let sessions = tauri::async_runtime::spawn_blocking(move || live::live_sessions(&r)).await.unwrap_or_default();
        set_badge(&app, sessions.iter().filter(|s| s.needs_you()).count());
        match sessions.iter().find(|s| s.needs_you()) {
            Some(s) if live::jump(s).is_ok() => {}
            _ => show_panel(&app),
        }
    });
}

/// The number next to the menu-bar icon: sessions waiting on you.
fn set_badge(app: &AppHandle, waiting: usize) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_title(if waiting > 0 { Some(waiting.to_string()) } else { None });
        let _ = tray.set_tooltip(Some(match waiting {
            0 => "Agent Island".to_string(),
            1 => "Agent Island: 1 session waiting on you".to_string(),
            n => format!("Agent Island: {n} sessions waiting on you"),
        }));
    }
}

const HOTKEY: &str = "ctrl+alt+KeyJ";

fn toggle_panel(app: &AppHandle) {
    let Some(w) = app.get_webview_window(PANEL) else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    show_panel(app);
}

fn show_panel(app: &AppHandle) {
    let Some(w) = app.get_webview_window(PANEL) else { return };
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
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts([HOTKEY])
                .expect("valid hotkey")
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        jump_to_longest_wait(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![scan, open_report, quit, live, jump, limits])
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

            // Keep the badge current: a light check once a minute (one
            // process listing plus the tail of each live session's log).
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    if let Ok(r) = roots(&handle) {
                        let n = tauri::async_runtime::spawn_blocking(move || live::live_sessions(&r).iter().filter(|s| s.needs_you()).count())
                            .await
                            .unwrap_or(0);
                        set_badge(&handle, n);
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }
            });
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
