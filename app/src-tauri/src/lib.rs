pub mod antigravity;
pub mod cursor;
pub mod cutoff;
pub mod foreign;
pub mod limits;
pub mod live;
pub mod logs;
pub mod notify;
pub mod recap;
pub mod recipes;
pub mod repos;
pub mod settings;
pub mod tasks;
pub mod terminal;

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

/// Today's sessions, from local midnight.
#[tauri::command]
async fn scan_today(app: AppHandle) -> Result<logs::ScanResult, String> {
    let r = roots(&app)?;
    let midnight = chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
        .map_or(logs::now_ms() - 86_400_000, |t| t.timestamp_millis());
    tauri::async_runtime::spawn_blocking(move || logs::scan_since(&r, midnight)).await.map_err(|e| e.to_string())
}

/// Commits since `since` in the repositories behind these folders.
#[tauri::command]
async fn recap_commits(cwds: Vec<String>, since: i64) -> Result<Vec<recap::RepoCommits>, String> {
    tauri::async_runtime::spawn_blocking(move || recap::commits_since(&cwds, since)).await.map_err(|e| e.to_string())
}

/// Local git state of the repositories behind these folders (Repo board).
#[tauri::command]
async fn repo_status(cwds: Vec<String>) -> Result<Vec<repos::RepoStatus>, String> {
    tauri::async_runtime::spawn_blocking(move || repos::status(&cwds)).await.map_err(|e| e.to_string())
}

/// Pulls (fast-forward only). Returns how many commits arrived.
#[tauri::command]
async fn repo_pull(path: String) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || repos::pull(std::path::Path::new(&path))).await.map_err(|e| e.to_string())?
}

/// Deletes the named old branches. Returns the names deleted.
#[tauri::command]
async fn repo_delete_branches(path: String, names: Vec<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || repos::delete_old_branches(std::path::Path::new(&path), &names)).await.map_err(|e| e.to_string())?
}

/// Installed terminal apps, in preference order.
#[tauri::command]
fn terminals() -> Vec<String> {
    terminal::installed()
}

/// Opens the chosen terminal app in a folder.
#[tauri::command]
fn open_terminal(app: AppHandle, path: String) -> Result<(), String> {
    let pick = app.state::<Prefs>().0.lock().ok().and_then(|p| p.terminal.clone());
    terminal::open(pick.as_deref(), std::path::Path::new(&path))
}

/// Sessions cut off by a limit (now reset) or interrupted, last 24 hours.
#[tauri::command]
async fn cut_off(app: AppHandle) -> Result<Vec<cutoff::CutOff>, String> {
    let r = roots(&app)?;
    tauri::async_runtime::spawn_blocking(move || cutoff::find(&r.claude, logs::now_ms())).await.map_err(|e| e.to_string())
}

/// Resumes a Claude session: jumps to it if it is still running, otherwise
/// opens a terminal tab running `claude --resume <id>` in its folder.
#[tauri::command]
async fn resume_session(app: AppHandle, session_id: String) -> Result<(), String> {
    if session_id.is_empty() || session_id.len() > 64 || !session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("That is not a session id.".into());
    }
    let r = roots(&app)?;
    let pick = app.state::<Prefs>().0.lock().ok().and_then(|p| p.terminal.clone());
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let scripts = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("scripts");
    tauri::async_runtime::spawn_blocking(move || {
        let live = live::live_sessions(&r);
        if let Some(s) = live.iter().find(|s| s.session_id == session_id) {
            return live::jump(s);
        }
        let list = cutoff::find(&r.claude, logs::now_ms());
        let cwd = cutoff::cwd_of(&list, &session_id).ok_or("That session is no longer in the cut-off list.")?;
        let claude = recap::find_claude(&home).ok_or("Could not find the claude command.")?;
        terminal::open_command(pick.as_deref(), std::path::Path::new(&cwd), &claude, &["--resume".to_string(), session_id.clone()], &format!("resume-{session_id}"), &scripts)
    })
    .await
    .map_err(|e| e.to_string())?
}

type Board = (i64, Vec<tasks::TaskDef>, Vec<tasks::Run>, Vec<tasks::TaskRow>);
static TASK_CACHE: Mutex<Option<Board>> = Mutex::new(None);

fn clear_task_board() {
    *TASK_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Script name from a task name: anything but letters and digits becomes a dash.
fn script_name(prefix: &str, name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    format!("{prefix}-{s}")
}

/// The Tasks board, cached for a minute (it reads the start and end of two
/// weeks of logs).
fn task_board(r: &logs::Roots, home: &std::path::Path) -> (Vec<tasks::TaskDef>, Vec<tasks::Run>, Vec<tasks::TaskRow>) {
    let now = logs::now_ms();
    {
        let c = TASK_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, d, runs, rows)) = c.as_ref() {
            if now - at < 60_000 {
                return (d.clone(), runs.clone(), rows.clone());
            }
        }
    }
    let defs = tasks::definitions(&home.join(".claude/scheduled-tasks"));
    let runs = tasks::runs(&r.claude, now - 14 * 86_400_000, now, 5 * 60_000);
    let rows = tasks::board(&defs, &runs, now);
    *TASK_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((now, defs.clone(), runs.clone(), rows.clone()));
    (defs, runs, rows)
}

#[tauri::command]
async fn tasks_board(app: AppHandle) -> Result<Vec<tasks::TaskRow>, String> {
    let r = roots(&app)?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || task_board(&r, &home).2).await.map_err(|e| e.to_string())
}

/// Starts a scheduled task now, in a terminal tab, with the prompt the
/// desktop app sends, in the folder it last ran in.
#[tauri::command]
async fn run_task(app: AppHandle, name: String) -> Result<(), String> {
    if name.is_empty() || name.len() > 80 {
        return Err("That is not a task name.".into());
    }
    let r = roots(&app)?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let pick = app.state::<Prefs>().0.lock().ok().and_then(|p| p.terminal.clone());
    let scripts = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("scripts");
    tauri::async_runtime::spawn_blocking(move || {
        let (defs, runs, _) = task_board(&r, &home);
        let def = defs.iter().find(|d| d.name == name).ok_or("That task is no longer defined.")?;
        let cwd = runs.iter().filter(|x| x.task == name).max_by_key(|x| x.start).and_then(|x| x.cwd.clone()).filter(|c| std::path::Path::new(c).is_dir()).unwrap_or_else(|| home.to_string_lossy().into_owned());
        let prompt = format!("<scheduled-task name=\"{}\" file=\"{}\">\n{}", def.name, def.file, def.body);
        let claude = recap::find_claude(&home).ok_or("Could not find the claude command.")?;
        terminal::open_command(pick.as_deref(), std::path::Path::new(&cwd), &claude, &[prompt], &script_name("task", &name), &scripts)?;
        clear_task_board();
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens a task's last run (`claude --resume`) in a terminal tab.
#[tauri::command]
async fn open_task_run(app: AppHandle, name: String) -> Result<(), String> {
    if name.is_empty() || name.len() > 80 {
        return Err("That is not a task name.".into());
    }
    let r = roots(&app)?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let pick = app.state::<Prefs>().0.lock().ok().and_then(|p| p.terminal.clone());
    let scripts = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("scripts");
    tauri::async_runtime::spawn_blocking(move || {
        let (_, runs, _) = task_board(&r, &home);
        let last = runs.iter().filter(|x| x.task == name).max_by_key(|x| x.start).ok_or("This task has no runs yet.")?;
        let cwd = last.cwd.clone().ok_or("The last run has no folder.")?;
        let claude = recap::find_claude(&home).ok_or("Could not find the claude command.")?;
        terminal::open_command(pick.as_deref(), std::path::Path::new(&cwd), &claude, &["--resume".to_string(), last.session_id.clone()], &format!("resume-{}", last.session_id), &scripts)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
struct RecipeSheet {
    recipes: Vec<recipes::Recipe>,
    suggestions: Vec<recipes::Suggestion>,
}

/// Suggestions for every repository, cached for ten minutes (mining reads
/// 30 days of logs).
fn suggestions_for(r: &logs::Roots, repo: &str) -> Vec<recipes::Suggestion> {
    type Cache = (i64, std::collections::HashMap<String, Vec<recipes::Suggestion>>);
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    let now = logs::now_ms();
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if c.as_ref().is_none_or(|(at, _)| now - at > 600_000) {
        let root_of = |cwd: &str| repos::root_of(std::path::Path::new(cwd)).map(|p| p.to_string_lossy().into_owned());
        *c = Some((now, recipes::mine(&r.claude, now - 30 * 86_400_000, &root_of)));
    }
    c.as_ref().and_then(|(_, m)| m.get(repo).cloned()).unwrap_or_default()
}

/// Serialises edits to recipes.json (load, change, save).
static RECIPES_LOCK: Mutex<()> = Mutex::new(());

fn recipe_sheet(app: &AppHandle, repo: &str) -> Result<RecipeSheet, String> {
    let saved = recipes::load(&config_dir(app)?).remove(repo).unwrap_or_default();
    let known: std::collections::HashSet<String> = saved.iter().map(|x| recipes::key(&x.prompt)).collect();
    let suggestions = suggestions_for(&roots(app)?, repo).into_iter().filter(|s| !known.contains(&recipes::key(&s.text))).collect();
    Ok(RecipeSheet { recipes: saved, suggestions })
}

/// A repository's recipes and suggestions.
#[tauri::command]
async fn recipes_for(app: AppHandle, path: String) -> Result<RecipeSheet, String> {
    tauri::async_runtime::spawn_blocking(move || {
        repos::check_root(std::path::Path::new(&path))?;
        recipe_sheet(&app, &path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Adds or edits a recipe; returns the repository's sheet.
#[tauri::command]
async fn save_recipe(app: AppHandle, path: String, id: Option<String>, name: String, prompt: String) -> Result<RecipeSheet, String> {
    tauri::async_runtime::spawn_blocking(move || {
        repos::check_root(std::path::Path::new(&path))?;
        let dir = config_dir(&app)?;
        {
            let _g = RECIPES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut s = recipes::load(&dir);
            recipes::upsert(&mut s, &path, id.as_deref(), &name, &prompt)?;
            recipes::save(&dir, &s).map_err(|e| e.to_string())?;
        }
        recipe_sheet(&app, &path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Deletes a recipe; returns the repository's sheet.
#[tauri::command]
async fn delete_recipe(app: AppHandle, path: String, id: String) -> Result<RecipeSheet, String> {
    tauri::async_runtime::spawn_blocking(move || {
        repos::check_root(std::path::Path::new(&path))?;
        let dir = config_dir(&app)?;
        {
            let _g = RECIPES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut s = recipes::load(&dir);
            recipes::remove(&mut s, &path, &id);
            recipes::save(&dir, &s).map_err(|e| e.to_string())?;
        }
        recipe_sheet(&app, &path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Starts a recipe: a new Claude session in a terminal tab, in the repository.
#[tauri::command]
async fn start_recipe(app: AppHandle, path: String, id: String) -> Result<(), String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let pick = app.state::<Prefs>().0.lock().ok().and_then(|p| p.terminal.clone());
    let scripts = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("scripts");
    tauri::async_runtime::spawn_blocking(move || {
        let repo = std::path::Path::new(&path);
        repos::check_root(repo)?;
        let saved = recipes::load(&config_dir(&app)?).remove(&path).unwrap_or_default();
        let r = saved.into_iter().find(|r| r.id == id).ok_or("That recipe no longer exists.")?;
        let claude = recap::find_claude(&home).ok_or("Could not find the claude command.")?;
        terminal::open_command(pick.as_deref(), repo, &claude, &["--".to_string(), r.prompt], &script_name("recipe", &r.id), &scripts)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Rewrites the recap with the person's own `claude`, only when they allowed it.
#[tauri::command]
async fn polish_recap(app: AppHandle, text: String) -> Result<String, String> {
    let allowed = app.state::<Prefs>().0.lock().map(|p| p.recap_with_claude).unwrap_or(false);
    if !allowed {
        return Err("Turn on \"Polish recap with Claude\" in Settings first.".into());
    }
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let claude = recap::find_claude(&home).ok_or("Could not find the claude command.")?;
        recap::polish(&claude, &text)
    })
    .await
    .map_err(|e| e.to_string())?
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

fn check_task_notifications(app: &AppHandle, watch: &Mutex<tasks::TaskWatch>) {
    use tauri_plugin_notification::NotificationExt;
    let on = app.state::<Prefs>().0.lock().map(|p| p.notify_missed_tasks).unwrap_or(false);
    let (Ok(r), Ok(home)) = (roots(app), app.path().home_dir()) else { return };
    let rows = task_board(&r, &home).2;
    let mut w = watch.lock().unwrap_or_else(|e| e.into_inner());
    // Always look, so problems that existed while notifications were off
    // are not announced later as new.
    let notes = w.new_problems(&rows);
    if !on {
        return;
    }
    for (title, body) in notes {
        let _ = app.notification().builder().title(&title).body(&body).show();
    }
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

/// A full-screen app is its own Space, and a normal window opened from a
/// menu-bar app either lands on the desktop or pulls the person back there.
/// Menu-bar apps use a non-activating NSPanel instead: it floats above
/// full-screen apps on every Space and takes keystrokes without switching
/// Spaces. The panel window is turned into one at start-up.
#[cfg(target_os = "macos")]
mod mac_panel {
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{define_class, ClassType, MainThreadOnly};
    use objc2_app_kit::{NSPanel, NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};

    define_class!(
        // SAFETY: NSPanel adds no instance variables to NSWindow, so an
        // existing window can be switched to this subclass.
        #[unsafe(super(NSPanel, NSWindow))]
        #[thread_kind = MainThreadOnly]
        #[name = "AgentIslandPanel"]
        struct IslandPanel;

        impl IslandPanel {
            // A borderless panel refuses key status by default; this one
            // needs it for Esc and the keyboard shortcuts.
            #[unsafe(method(canBecomeKeyWindow))]
            fn can_become_key(&self) -> bool {
                true
            }
            #[unsafe(method(canBecomeMainWindow))]
            fn can_become_main(&self) -> bool {
                false
            }
        }
    );

    fn ns_window(w: &tauri::WebviewWindow) -> Option<&NSWindow> {
        let ptr = w.ns_window().ok()?;
        // SAFETY: Tauri returns the window's live NSWindow, used on the main thread.
        Some(unsafe { &*(ptr as *const NSWindow) })
    }

    pub fn convert(w: &tauri::WebviewWindow) {
        let Some(win) = ns_window(w) else { return };
        let obj: &AnyObject = win.as_ref();
        let cls: &AnyClass = IslandPanel::class();
        // SAFETY: see define_class above; the window is not in use yet.
        unsafe { AnyObject::set_class(obj, cls) };
        win.setStyleMask(win.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        win.setCollectionBehavior(
            win.collectionBehavior()
                | NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
        win.setLevel(NSStatusWindowLevel);
        // Panels hide when their app is inactive; this app is never active.
        win.setHidesOnDeactivate(false);
    }

    /// Brings the panel up over whatever is on screen, without activating
    /// the app (activating would switch away from a full-screen Space).
    pub fn show(w: &tauri::WebviewWindow) {
        let Some(win) = ns_window(w) else { return };
        win.makeKeyAndOrderFront(None);
        win.orderFrontRegardless();
    }
}

fn show_panel(app: &AppHandle) {
    let Some(w) = app.get_webview_window(PANEL) else { return };
    // The menu bar is at the top on macOS; the taskbar is usually at the bottom on Windows.
    #[cfg(target_os = "macos")]
    let _ = w.move_window(Position::TrayCenter);
    #[cfg(not(target_os = "macos"))]
    let _ = w.move_window(Position::TrayBottomCenter);
    #[cfg(target_os = "macos")]
    mac_panel::show(&w);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = w.show();
        let _ = w.set_focus();
    }
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
        .invoke_handler(tauri::generate_handler![scan, open_report, quit, live, jump, limits, get_settings, set_settings, test_notification, scan_today, recap_commits, repo_status, repo_pull, repo_delete_branches, terminals, open_terminal, cut_off, resume_session, tasks_board, run_task, open_task_run, recipes_for, save_recipe, delete_recipe, start_recipe, polish_recap])
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
            // Float over full-screen apps, on every Space, like a menu.
            #[cfg(target_os = "macos")]
            if let Some(w) = app.get_webview_window(PANEL) {
                mac_panel::convert(&w);
            }

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
                let task_watch = std::sync::Arc::new(Mutex::new(tasks::TaskWatch::default()));
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
                        let (h, w) = (handle.clone(), task_watch.clone());
                        let _ = tauri::async_runtime::spawn_blocking(move || check_task_notifications(&h, &w)).await;
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

#[cfg(test)]
mod tests {
    use super::script_name;

    #[test]
    fn script_names_are_safe() {
        assert_eq!(script_name("task", "Daily Report v1.2"), "task-Daily-Report-v1-2");
    }
}
