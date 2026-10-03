//! Live sessions: which running agents are waiting on the person, and how to
//! bring the right window forward.
//!
//! Runs only on demand (hotkey, panel, a once-a-minute badge refresh). It
//! lists running `claude` and `codex` processes, matches each to its session
//! log by working folder, and reads the end of that log:
//!   assistant stop_reason "end_turn"          -> waiting for your next message
//!   assistant tool_use with no result after   -> running a tool or waiting for approval
//!   anything else                             -> working
//! Sessions with no prompt typed by a person (plugins, scripts) are left out.
//!
//! Cursor runs every chat inside one app, so there is no process per chat.
//! While Cursor is running, chats active in the last 12 hours are read from
//! its database: generating -> working; a pending action -> approval;
//! completed with the agent's message last -> finished, waiting for you.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// A tool call older than this with no result is probably waiting for approval.
const APPROVAL_AFTER_MS: i64 = 20_000;
/// Past this, an unanswered tool call or approval means the session stopped
/// (interrupted, or left), not that it is asking for something now.
const IDLE_AFTER_MS: i64 = 30 * 60_000;
const TAIL_BYTES: u64 = 256 * 1024;
/// Cursor chats untouched for longer than this are not "live".
const CURSOR_LIVE_MS: i64 = 12 * 3_600_000;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Waiting,
    Approval,
    Working,
    /// Stuck on a tool call or approval for over 30 minutes.
    Idle,
}

/// Approval that has gone unanswered for a long time is Idle instead.
fn settle(state: State, since: i64, now: i64) -> State {
    if state == State::Approval && now - since > IDLE_AFTER_MS {
        State::Idle
    } else {
        state
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LiveSession {
    pub agent: &'static str,
    pub session_id: String,
    pub title: Option<String>,
    pub project: Option<String>,
    pub cwd: String,
    pub state: State,
    /// When the current state began, ms since epoch.
    pub since: i64,
    pub pid: u32,
    /// The app hosting the session (Terminal, iTerm, VS Code, Claude, …).
    pub host: Option<String>,
    /// Finished and not yet looked at (Cursor tracks this).
    pub unread: bool,
    #[serde(skip)]
    pub host_bundle: Option<PathBuf>,
}

impl LiveSession {
    pub fn needs_you(&self) -> bool {
        matches!(self.state, State::Waiting | State::Approval)
    }
}

struct Proc {
    pid: u32,
    agent: &'static str,
    cwd: PathBuf,
    resume: Option<String>,
    host_bundle: Option<PathBuf>,
}

fn agent_of(name: &str, cmd: &[String]) -> Option<&'static str> {
    let base = name.trim_end_matches(".exe");
    if base == "claude" {
        return Some("claude");
    }
    if base == "codex" {
        return Some("codex");
    }
    // npm installs run under node.
    if base == "node" {
        let joined = cmd.join(" ");
        if joined.contains("@anthropic-ai/claude-code") {
            return Some("claude");
        }
        if joined.contains("@openai/codex") {
            return Some("codex");
        }
    }
    None
}

fn resume_id(cmd: &[String]) -> Option<String> {
    let mut it = cmd.iter();
    while let Some(a) = it.next() {
        for flag in ["--resume", "--session-id"] {
            if let Some(v) = a.strip_prefix(&format!("{flag}=")) {
                return Some(v.to_string());
            }
            if a == flag {
                return it.next().filter(|v| !v.starts_with('-')).cloned();
            }
        }
    }
    None
}

/// Apps that host agent sessions on Windows, matched by executable name.
#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_HOSTS: [&str; 14] = [
    "windowsterminal.exe",
    "code.exe",
    "cursor.exe",
    "windsurf.exe",
    "zed.exe",
    "antigravity.exe",
    "claude.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "hyper.exe",
    "tabby.exe",
    "idea64.exe",
    "pycharm64.exe",
    "warp.exe",
];
/// Editors that focus the window for a folder when opened with it.
const EDITORS: [&str; 7] = ["Visual Studio Code", "Code", "Cursor", "Windsurf", "Zed", "VSCodium", "Antigravity"];

/// Index of the host in an ancestor chain (nearest first): the outermost
/// known app. `claude.exe` only counts as a host above the agent itself, so
/// the Claude desktop app is found but the agent binary is not.
#[cfg_attr(not(windows), allow(dead_code))]
fn pick_windows_host(chain: &[String]) -> Option<usize> {
    chain.iter().rposition(|exe| WINDOWS_HOSTS.contains(&exe.to_ascii_lowercase().as_str()))
}

fn ancestors(sys: &System, pid: Pid) -> Vec<(Pid, Option<PathBuf>)> {
    let mut out = Vec::new();
    let mut cur = sys.process(pid).and_then(|p| p.parent());
    while let Some(p) = cur.and_then(|c| sys.process(c)) {
        if out.len() >= 64 || out.iter().any(|(q, _)| *q == p.pid()) {
            break;
        }
        out.push((p.pid(), p.exe().map(Path::to_path_buf)));
        cur = p.parent();
    }
    out
}

/// The app the person sees for a session: on macOS the outermost .app bundle
/// among its ancestors (skipping bundles inside an agent's own install, like
/// claude-code/…/claude.app); on Windows the outermost known host app.
#[cfg(windows)]
fn host_bundle(sys: &System, pid: Pid) -> Option<PathBuf> {
    let chain = ancestors(sys, pid);
    let names: Vec<String> = chain.iter().map(|(_, e)| e.as_ref().and_then(|e| e.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).collect();
    pick_windows_host(&names).and_then(|i| chain[i].1.clone())
}

#[cfg(not(windows))]
fn host_bundle(sys: &System, pid: Pid) -> Option<PathBuf> {
    ancestors(sys, pid).iter().rev().find_map(|(_, exe)| {
        let s = exe.as_ref()?.to_string_lossy().into_owned();
        let bundle = &s[..s.find(".app/")? + 4];
        (!bundle.contains("/claude-code/")).then(|| PathBuf::from(bundle))
    })
}

/// The running Cursor app: its .app bundle on macOS, Cursor.exe on Windows.
fn cursor_bundle(sys: &System) -> Option<PathBuf> {
    if cfg!(windows) {
        return sys.processes().values().find_map(|p| {
            let exe = p.exe()?;
            exe.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Cursor.exe")).then(|| exe.to_path_buf())
        });
    }
    sys.processes().values().find_map(|p| {
        let exe = p.exe()?.to_string_lossy().into_owned();
        let i = exe.find(".app/Contents/MacOS/")?;
        let bundle = &exe[..i + 4];
        bundle.ends_with("/Cursor.app").then(|| PathBuf::from(bundle))
    })
}

fn processes() -> (Vec<Proc>, Option<PathBuf>) {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always).with_cwd(UpdateKind::Always).with_exe(UpdateKind::Always),
    );
    let mut out = Vec::new();
    for (pid, p) in sys.processes() {
        let cmd: Vec<String> = p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect();
        let Some(agent) = agent_of(&p.name().to_string_lossy(), &cmd) else { continue };
        let Some(cwd) = p.cwd().map(Path::to_path_buf) else { continue };
        out.push(Proc { pid: pid.as_u32(), agent, cwd, resume: resume_id(&cmd), host_bundle: host_bundle(&sys, *pid) });
    }
    (out, cursor_bundle(&sys))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorChat {
    composer_id: Option<String>,
    name: Option<String>,
    status: Option<String>,
    last_updated_at: Option<i64>,
    has_unread_messages: Option<bool>,
    has_blocking_pending_actions: Option<bool>,
    is_archived: Option<bool>,
    is_draft: Option<bool>,
    is_best_of_n_subcomposer: Option<bool>,
    generating_bubble_ids: Option<Vec<serde_json::Value>>,
    workspace_identifier: Option<serde_json::Value>,
    full_conversation_headers_only: Option<Vec<CursorHeader>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorHeader {
    #[serde(rename = "type")]
    kind: Option<u8>,
    started_at_ms: Option<i64>,
    completed_at_ms: Option<i64>,
}

/// State of one Cursor chat, or None when it is not a live conversation.
fn cursor_chat_state(c: &CursorChat, now: i64) -> Option<(State, i64)> {
    if c.is_archived == Some(true) || c.is_draft == Some(true) || c.is_best_of_n_subcomposer == Some(true) {
        return None;
    }
    let updated = c.last_updated_at?;
    if now - updated > CURSOR_LIVE_MS {
        return None;
    }
    if c.has_blocking_pending_actions == Some(true) {
        return Some((State::Approval, updated));
    }
    if c.status.as_deref() == Some("generating") || c.generating_bubble_ids.as_ref().is_some_and(|g| !g.is_empty()) {
        return Some((State::Working, updated));
    }
    let last = c.full_conversation_headers_only.as_ref()?.last()?;
    if c.status.as_deref() == Some("completed") && last.kind == Some(2) {
        return Some((State::Waiting, last.completed_at_ms.or(last.started_at_ms).unwrap_or(updated)));
    }
    None
}

fn cursor_sessions(db_path: &Path, bundle: &Path, now: i64) -> Vec<LiveSession> {
    let Some(db) = crate::foreign::open_foreign_db(db_path) else { return Vec::new() };
    let Ok(mut st) = db.prepare("SELECT value FROM cursorDiskKV WHERE key >= 'composerData:' AND key < 'composerData;'") else {
        return Vec::new();
    };
    // Cursor stores these values as TEXT; older versions used BLOB.
    let Ok(rows) = st.query_map([], |r| {
        Ok(match r.get_ref(0)? {
            rusqlite::types::ValueRef::Text(b) | rusqlite::types::ValueRef::Blob(b) => Some(b.to_vec()),
            _ => None,
        })
    }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for v in rows.filter_map(Result::ok).flatten() {
        let Ok(c) = serde_json::from_slice::<CursorChat>(&v) else { continue };
        let Some((state, since)) = cursor_chat_state(&c, now) else { continue };
        let state = settle(state, since, now);
        let cwd = c
            .workspace_identifier
            .as_ref()
            .and_then(|w| w.pointer("/uri/fsPath"))
            .and_then(|p| p.as_str())
            .unwrap_or_default()
            .to_string();
        out.push(LiveSession {
            agent: "cursor",
            session_id: c.composer_id.clone().unwrap_or_default(),
            title: c.name.clone().filter(|n| !n.is_empty()),
            project: crate::logs::project_of(&cwd),
            cwd,
            state,
            since,
            pid: 0,
            host: Some("Cursor".into()),
            unread: c.has_unread_messages == Some(true),
            host_bundle: Some(bundle.to_path_buf()),
        });
    }
    out
}

/// Claude Code's project folder name for a working directory: every
/// character that is not a letter or digit becomes "-".
fn claude_project_dir(projects: &Path, cwd: &Path) -> PathBuf {
    let name: String = cwd.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    projects.join(name)
}

fn newest_jsonl(dir: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files
}

fn tail(path: &Path, bytes: u64) -> String {
    let Ok(mut f) = File::open(path) else { return String::new() };
    let len = f.metadata().map_or(0, |m| m.len());
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(bytes)));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let s = String::from_utf8_lossy(&buf).into_owned();
    // Drop the first, probably partial, line unless we read from the start.
    if len > bytes {
        s.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default()
    } else {
        s
    }
}

fn ts(v: &serde_json::Value) -> Option<i64> {
    v.get("timestamp")?.as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|d| d.timestamp_millis())
}

/// State of a Claude Code session from the end of its log.
pub fn claude_state(log_tail: &str, now: i64) -> Option<(State, i64)> {
    for line in log_tail.lines().rev() {
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let kind = d.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if d.get("isSidechain").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        match kind {
            "assistant" => {
                let t = ts(&d)?;
                let stop = d.pointer("/message/stop_reason").and_then(|s| s.as_str());
                let has_tool = d
                    .pointer("/message/content")
                    .and_then(|c| c.as_array())
                    .is_some_and(|c| c.iter().any(|b| b.get("type").and_then(|x| x.as_str()) == Some("tool_use")));
                return Some(match (stop, has_tool) {
                    (Some("end_turn") | Some("stop_sequence"), _) => (State::Waiting, t),
                    (_, true) if now - t > APPROVAL_AFTER_MS => (State::Approval, t),
                    _ => (State::Working, t),
                });
            }
            "user" => return Some((State::Working, ts(&d)?)),
            _ => {}
        }
    }
    None
}

/// State of a Codex session from the end of its rollout log.
pub fn codex_state(log_tail: &str) -> Option<(State, i64)> {
    for line in log_tail.lines().rev() {
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if d.get("type").and_then(|t| t.as_str()) != Some("event_msg") {
            continue;
        }
        match d.pointer("/payload/type").and_then(|t| t.as_str()) {
            Some("task_complete") => return Some((State::Waiting, ts(&d)?)),
            Some("exec_approval_request" | "apply_patch_approval_request") => return Some((State::Approval, ts(&d)?)),
            Some("user_message" | "task_started") => return Some((State::Working, ts(&d)?)),
            _ => {}
        }
    }
    None
}

fn has_human_prompt(path: &Path) -> bool {
    // Cheap check over the raw bytes: one typed prompt anywhere is enough.
    // Scheduled tasks are logged as human too, but nobody is waiting on them.
    let human = br#""origin":{"kind":"human"}"#;
    std::fs::read(path).is_ok_and(|b| {
        b.split(|&c| c == b'\n').any(|l| contains(l, human) && !contains(l, b"<scheduled-task"))
    })
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn claude_title(log_tail: &str) -> Option<String> {
    log_tail.lines().rev().find_map(|l| {
        let d: serde_json::Value = serde_json::from_str(l).ok()?;
        match d.get("type")?.as_str()? {
            "custom-title" => d.get("customTitle")?.as_str().map(str::to_string),
            "agent-name" => d.get("agentName")?.as_str().map(str::to_string),
            _ => None,
        }
    })
}

fn codex_rollout_for(sessions_root: &Path, cwd: &Path, taken: &HashSet<PathBuf>) -> Option<PathBuf> {
    // Rollouts live in YYYY/MM/DD folders; a live session started recently.
    let mut files = Vec::new();
    let today = chrono::Local::now().date_naive();
    for back in 0..3 {
        let d = today - chrono::Days::new(back);
        let dir = sessions_root.join(d.format("%Y/%m/%d").to_string());
        files.extend(newest_jsonl(&dir));
    }
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files.into_iter().map(|(p, _)| p).find(|p| {
        !taken.contains(p)
            && std::fs::read_to_string(p).ok().and_then(|s| {
                let first = s.lines().next()?.to_string();
                let v: serde_json::Value = serde_json::from_str(&first).ok()?;
                Some(v.pointer("/payload/cwd")?.as_str()? == cwd.to_string_lossy())
            }) == Some(true)
    })
}

pub fn live_sessions(roots: &crate::logs::Roots) -> Vec<LiveSession> {
    let now = crate::logs::now_ms();
    let (procs, cursor) = processes();
    let mut taken: HashSet<PathBuf> = HashSet::new();
    let mut out = Vec::new();

    // Explicit --resume ids first, so folder matching does not claim them.
    let mut ordered: Vec<&Proc> = procs.iter().filter(|p| p.resume.is_some()).collect();
    ordered.extend(procs.iter().filter(|p| p.resume.is_none()));
    let mut by_dir: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

    for p in ordered {
        let log = match p.agent {
            "claude" => {
                let dir = claude_project_dir(&roots.claude, &p.cwd);
                let explicit = p.resume.as_ref().map(|id| dir.join(format!("{id}.jsonl"))).filter(|f| f.exists());
                explicit.or_else(|| {
                    let files = by_dir.entry(dir.clone()).or_insert_with(|| newest_jsonl(&dir).into_iter().map(|(f, _)| f).collect());
                    files.iter().find(|f| !taken.contains(*f)).cloned()
                })
            }
            _ => codex_rollout_for(&roots.codex, &p.cwd, &taken),
        };
        let Some(log) = log else { continue };
        taken.insert(log.clone());
        let t = tail(&log, TAIL_BYTES);
        let (state, since) = match p.agent {
            "claude" => {
                if !has_human_prompt(&log) {
                    continue;
                }
                claude_state(&t, now)
            }
            _ => codex_state(&t),
        }
        .unwrap_or((State::Working, now));
        let state = settle(state, since, now);
        out.push(LiveSession {
            agent: p.agent,
            session_id: log.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            title: if p.agent == "claude" { claude_title(&t) } else { None },
            project: crate::logs::project_of(&p.cwd.to_string_lossy()),
            cwd: p.cwd.to_string_lossy().into_owned(),
            state,
            since,
            pid: p.pid,
            host: p.host_bundle.as_ref().and_then(|b| b.file_stem()).map(|s| s.to_string_lossy().into_owned()),
            unread: false,
            host_bundle: p.host_bundle.clone(),
        });
    }
    if let Some(bundle) = cursor {
        out.extend(cursor_sessions(&roots.cursor, &bundle, now));
    }
    // Longest wait first; working sessions last.
    out.sort_by_key(|s| (!s.needs_you(), s.since));
    out
}

/// Brings the session's window forward. Terminal and iTerm get the exact tab;
/// editors get the window for the folder; anything else is activated.
#[cfg(target_os = "macos")]
pub fn jump(s: &LiveSession) -> Result<(), String> {
    use std::process::Command;
    let Some(bundle) = &s.host_bundle else { return Err("no app found for this session".into()) };
    let app = bundle.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tty = (s.pid != 0).then(|| Command::new("ps")
        .args(["-o", "tty=", "-p", &s.pid.to_string()])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|t| !t.is_empty() && t != "??")
        .map(|t| format!("/dev/{t}"))).flatten();
    let script = match (app.as_str(), &tty) {
        ("Terminal", Some(tty)) => Some(format!(
            r#"tell application "Terminal"
                 repeat with w in windows
                   repeat with t in tabs of w
                     if tty of t is "{tty}" then
                       set selected of t to true
                       set index of w to 1
                     end if
                   end repeat
                 end repeat
                 activate
               end tell"#
        )),
        ("iTerm" | "iTerm2", Some(tty)) => Some(format!(
            r#"tell application "iTerm2"
                 repeat with w in windows
                   repeat with t in tabs of w
                     repeat with s in sessions of t
                       if tty of s is "{tty}" then
                         select t
                         select s
                         set index of w to 1
                       end if
                     end repeat
                   end repeat
                 end repeat
                 activate
               end tell"#
        )),
        _ => None,
    };
    if let Some(script) = script {
        let ok = Command::new("osascript").args(["-e", &script]).status().is_ok_and(|s| s.success());
        if ok {
            return Ok(());
        }
    }
    // Editors (VS Code, Cursor, Windsurf, Zed, …) focus the window for a folder
    // when opened with it; other apps just come forward.
    let mut cmd = Command::new("open");
    cmd.arg("-a").arg(bundle);
    if EDITORS.iter().any(|e| app.contains(e)) {
        cmd.arg(&s.cwd);
    }
    match cmd.status() {
        Ok(st) if st.success() => Ok(()),
        _ => Err(format!("could not open {app}")),
    }
}

/// Windows: editors are reopened on the session's folder, which focuses that
/// window; any other host's visible top-level window is brought forward.
#[cfg(windows)]
pub fn jump(s: &LiveSession) -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, GW_OWNER,
        SW_RESTORE,
    };
    let Some(exe) = &s.host_bundle else { return Err("no app found for this session".into()) };
    let app = exe.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if EDITORS.iter().any(|e| app.eq_ignore_ascii_case(e)) && !s.cwd.is_empty() {
        return std::process::Command::new(exe).arg(&s.cwd).spawn().map(|_| ()).map_err(|e| format!("could not open {app}: {e}"));
    }

    // Pids of the session's ancestors; the host is one of them.
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::Always));
    let pids: Vec<u32> = ancestors(&sys, Pid::from_u32(s.pid)).iter().map(|(p, _)| p.as_u32()).collect();

    struct Search {
        pids: Vec<u32>,
        found: Vec<(usize, HWND)>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let top_level = unsafe { GetWindow(hwnd, GW_OWNER) }.map_or(true, |o| o.is_invalid());
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool() && unsafe { GetWindowTextLengthW(hwnd) } > 0;
        if visible && top_level {
            if let Some(i) = search.pids.iter().position(|p| *p == pid) {
                search.found.push((i, hwnd));
            }
        }
        true.into()
    }
    let mut search = Search { pids, found: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut search as *mut Search as isize));
    }
    // Prefer the outermost ancestor's window: the terminal or app itself.
    let Some((_, hwnd)) = search.found.into_iter().max_by_key(|(i, _)| *i) else {
        return Err(format!("could not find a window for {app}"));
    };
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if SetForegroundWindow(hwnd).as_bool() {
            Ok(())
        } else {
            Err(format!("Windows did not let {app} come to the front"))
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn jump(_s: &LiveSession) -> Result<(), String> {
    Err("Jumping to a session is not supported on this system yet".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_790_000_100_000;
    fn line(kind: &str, extra: &str) -> String {
        format!(r#"{{"type":"{kind}","timestamp":"2026-09-21T13:08:00.000Z"{extra}}}"#)
    }

    #[test]
    fn scheduled_task_sessions_are_not_waiting_on_anyone() {
        let fx = |n: &str| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../wrapped/test/fixtures").join(n);
        assert!(!has_human_prompt(&fx("claude-scheduled.jsonl")));
        assert!(has_human_prompt(&fx("fork/aaa-original.jsonl")));
    }

    #[test]
    fn end_turn_means_waiting() {
        let log = [
            line("user", r#","origin":{"kind":"human"}"#),
            line("assistant", r#","message":{"stop_reason":"end_turn","content":[{"type":"text"}]}"#),
            line("system", r#","subtype":"stop_hook_summary""#),
        ]
        .join("\n");
        assert_eq!(claude_state(&log, NOW).unwrap().0, State::Waiting);
    }

    #[test]
    fn old_unanswered_tool_call_means_approval() {
        let log = line("assistant", r#","message":{"stop_reason":"tool_use","content":[{"type":"tool_use"}]}"#);
        assert_eq!(claude_state(&log, NOW).unwrap().0, State::Approval);
        let ts = chrono::DateTime::parse_from_rfc3339("2026-09-21T13:08:00.000Z").unwrap().timestamp_millis();
        assert_eq!(claude_state(&log, ts + 1_000).unwrap().0, State::Working);
    }

    #[test]
    fn long_unanswered_approval_is_idle() {
        assert_eq!(settle(State::Approval, NOW - 31 * 60_000, NOW), State::Idle);
        assert_eq!(settle(State::Approval, NOW - 5 * 60_000, NOW), State::Approval);
        assert_eq!(settle(State::Waiting, NOW - 99 * 60 * 60_000, NOW), State::Waiting);
    }

    #[test]
    fn tool_result_means_working() {
        let log = [
            line("assistant", r#","message":{"stop_reason":"tool_use","content":[{"type":"tool_use"}]}"#),
            line("user", r#","toolUseResult":{},"message":{"content":[{"type":"tool_result"}]}"#),
        ]
        .join("\n");
        assert_eq!(claude_state(&log, NOW).unwrap().0, State::Working);
    }

    #[test]
    fn subagent_lines_are_ignored() {
        let log = [
            line("assistant", r#","message":{"stop_reason":"end_turn","content":[]}"#),
            line("assistant", r#","isSidechain":true,"message":{"stop_reason":"tool_use","content":[{"type":"tool_use"}]}"#),
        ]
        .join("\n");
        assert_eq!(claude_state(&log, NOW).unwrap().0, State::Waiting);
    }

    #[test]
    fn codex_task_complete_means_waiting() {
        let log = [
            r#"{"type":"event_msg","timestamp":"2026-09-21T13:00:00.000Z","payload":{"type":"user_message"}}"#,
            r#"{"type":"event_msg","timestamp":"2026-09-21T13:05:00.000Z","payload":{"type":"task_complete"}}"#,
        ]
        .join("\n");
        assert_eq!(codex_state(&log).unwrap().0, State::Waiting);
    }

    #[test]
    fn process_matching() {
        assert_eq!(agent_of("claude", &[]), Some("claude"));
        assert_eq!(agent_of("node", &["/x/node_modules/@anthropic-ai/claude-code/cli.js".into()]), Some("claude"));
        assert_eq!(agent_of("codex.exe", &[]), Some("codex"));
        assert_eq!(agent_of("zsh", &[]), None);
        assert_eq!(resume_id(&["claude".into(), "--resume=abc".into()]).as_deref(), Some("abc"));
        assert_eq!(resume_id(&["claude".into(), "--resume".into(), "def".into()]).as_deref(), Some("def"));
        assert_eq!(resume_id(&["claude".into(), "--resume".into(), "--verbose".into()]), None);
    }

    fn chat(json: &str) -> CursorChat {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn cursor_chat_states() {
        let now = 1_790_000_000_000_i64;
        let recent = now - 600_000;
        let done = format!(r#"{{"status":"completed","lastUpdatedAt":{recent},"generatingBubbleIds":null,"fullConversationHeadersOnly":[{{"type":1}},{{"type":2,"completedAtMs":{recent}}}]}}"#);
        assert_eq!(cursor_chat_state(&chat(&done), now), Some((State::Waiting, recent)));
        let gen = format!(r#"{{"status":"generating","lastUpdatedAt":{recent},"fullConversationHeadersOnly":[{{"type":1}}]}}"#);
        assert_eq!(cursor_chat_state(&chat(&gen), now).unwrap().0, State::Working);
        let blocked = format!(r#"{{"status":"completed","hasBlockingPendingActions":true,"lastUpdatedAt":{recent}}}"#);
        assert_eq!(cursor_chat_state(&chat(&blocked), now).unwrap().0, State::Approval);
        let old = format!(r#"{{"status":"completed","lastUpdatedAt":{},"fullConversationHeadersOnly":[{{"type":2}}]}}"#, now - 13 * 3_600_000);
        assert_eq!(cursor_chat_state(&chat(&old), now), None);
        let user_last = format!(r#"{{"status":"completed","lastUpdatedAt":{recent},"fullConversationHeadersOnly":[{{"type":1}}]}}"#);
        assert_eq!(cursor_chat_state(&chat(&user_last), now), None);
        let archived = format!(r#"{{"status":"completed","isArchived":true,"lastUpdatedAt":{recent},"fullConversationHeadersOnly":[{{"type":2}}]}}"#);
        assert_eq!(cursor_chat_state(&chat(&archived), now), None);
    }

    #[test]
    fn windows_host_is_outermost_known_app() {
        let chain = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // claude.exe <- pwsh <- OpenConsole <- WindowsTerminal <- explorer
        assert_eq!(pick_windows_host(&chain(&["pwsh.exe", "OpenConsole.exe", "WindowsTerminal.exe", "explorer.exe"])), Some(2));
        // VS Code's integrated terminal: shell <- Code (helper) <- Code (main)
        assert_eq!(pick_windows_host(&chain(&["powershell.exe", "Code.exe", "Code.exe", "explorer.exe"])), Some(2));
        // Claude desktop app runs the agent directly.
        assert_eq!(pick_windows_host(&chain(&["Claude.exe", "explorer.exe"])), Some(0));
        assert_eq!(pick_windows_host(&chain(&["cmd.exe", "explorer.exe"])), None);
    }

    #[test]
    fn project_dir_naming_matches_claude_code() {
        let p = claude_project_dir(Path::new("/p"), Path::new("/Users/me/.claude-mem/observer-sessions"));
        assert_eq!(p, Path::new("/p/-Users-me--claude-mem-observer-sessions"));
    }
}
