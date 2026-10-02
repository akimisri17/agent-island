pub mod antigravity;
pub mod cursor;
pub mod foreign;
pub mod limits;
pub mod live;
pub mod logs;
pub mod notify;
pub mod settings;

use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent,
};
use std::sync::Mutex;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
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
fn compute_limits(r: &logs::Roots) -> limits::Limits {
    static CLAUDE: Mutex<Option<(i64, Vec<limits::Spend>, Vec<limits::Hit>)>> = Mutex::new(None);
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
    let working = live::live_sessions(r).iter().filter(|s| s.agent == "claude" && s.state == live::State::Working).count();
    limits::Limits {
        claude: (!spend.is_empty()).then(|| limits::claude_limit(&spend, &hits, now, working)),
        codex: limits::codex_limits(&r.codex, now),
    }
}

#[tauri::command]
async fn limits(app: AppHandle) -> Result<limits::Limits, String> {
    let r = roots(&app)?;
    tauri::async_runtime::spawn_blocking(move || compute_limits(&r)).await.map_err(|e| e.to_string())
}

/// Lets the person check that notifications reach them (macOS may ask first).
#[tauri::command]
fn test_notification(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title("Agent Island")
        .body("Notifications work. You'll hear from it when you get close to a limit.")
        .show()
        .map_err(|e| e.to_string())
}

/// Shows any limit notifications that are due. Called every 5 minutes.
fn check_limit_notifications(app: &AppHandle, notifier: &Mutex<notify::Notifier>) {
    use tauri_plugin_notification::NotificationExt;
    let on = app.state::<Prefs>().0.lock().map(|p| p.notify_limits).unwrap_or(false);
    let Ok(r) = roots(app) else { return };
    if !on {
        return;
    }
    let l = compute_limits(&r);
    let Ok(mut n) = notifier.lock() else { return };
    for note in n.due(&l, logs::now_ms()) {
        let _ = app.notification().builder().title(&note.title).body(&note.body).show();
    }
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

struct Prefs(Mutex<settings::Settings>);

fn config_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_settings(app: AppHandle) -> settings::Settings {
    app.state::<Prefs>().0.lock().map(|s| s.clone()).unwrap_or_default()
}

/// Saves settings. A new hotkey is registered before the old one is
/// released, so a hotkey taken by another app leaves the old one working.
#[tauri::command]
fn set_settings(app: AppHandle, next: settings::Settings) -> Result<settings::Settings, String> {
    let prefs = app.state::<Prefs>();
    let current = prefs.0.lock().map_err(|e| e.to_string())?.clone();
    if next.hotkey != current.hotkey {
        let new: Shortcut = next.hotkey.parse().map_err(|_| format!("\"{}\" is not a valid shortcut", next.hotkey))?;
        let gs = app.global_shortcut();
        gs.register(new).map_err(|_| "That shortcut is taken by another app. Try another.".to_string())?;
        if let Ok(old) = current.hotkey.parse::<Shortcut>() {
            let _ = gs.unregister(old);
        }
    }
    settings::save(&config_dir(&app)?, &next).map_err(|e| e.to_string())?;
    *prefs.0.lock().map_err(|e| e.to_string())? = next.clone();
    Ok(next)
}

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
        .plugin(tauri_plugin_notification::init())
        .plugin(
            // One handler for whichever shortcut is registered: the app only ever has one.
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        jump_to_longest_wait(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![scan, open_report, quit, live, jump, limits, get_settings, set_settings, test_notification])
        .setup(|app| {
            let prefs = config_dir(app.handle()).map(|d| settings::load(&d)).unwrap_or_default();
            let hotkey = prefs.hotkey.parse::<Shortcut>().or_else(|_| settings::DEFAULT_HOTKEY.parse()).expect("default hotkey parses");
            if let Err(e) = app.global_shortcut().register(hotkey) {
                eprintln!("agent-island: could not register hotkey {}: {e}", prefs.hotkey);
            }
            app.manage(Prefs(Mutex::new(prefs)));

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
            // Every fifth tick, check whether a limit notification is due.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let notifier = std::sync::Arc::new(Mutex::new(notify::Notifier::default()));
                for tick in 0u64.. {
                    if let Ok(r) = roots(&handle) {
                        let n = tauri::async_runtime::spawn_blocking(move || live::live_sessions(&r).iter().filter(|s| s.needs_you()).count())
                            .await
                            .unwrap_or(0);
                        set_badge(&handle, n);
                    }
                    if tick % 5 == 0 {
                        let (h, n) = (handle.clone(), notifier.clone());
                        let _ = tauri::async_runtime::spawn_blocking(move || check_limit_notifications(&h, &n)).await;
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
