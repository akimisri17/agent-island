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

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// A tool call older than this with no result is probably waiting for approval.
const APPROVAL_AFTER_MS: i64 = 20_000;
const TAIL_BYTES: u64 = 256 * 1024;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Waiting,
    Approval,
    Working,
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
    #[serde(skip)]
    pub host_bundle: Option<PathBuf>,
}

impl LiveSession {
    pub fn needs_you(&self) -> bool {
        self.state != State::Working
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

/// The outermost .app bundle among a process's ancestors: the app the person
/// sees. Bundles inside an agent's own install (claude-code/…/claude.app) are
/// skipped.
fn host_bundle(sys: &System, pid: Pid) -> Option<PathBuf> {
    let mut found = None;
    let mut cur = sys.process(pid).and_then(|p| p.parent());
    let mut guard = 0;
    while let (Some(p), true) = (cur.and_then(|c| sys.process(c)), guard < 64) {
        guard += 1;
        if let Some(exe) = p.exe() {
            let s = exe.to_string_lossy();
            if let Some(i) = s.find(".app/") {
                let bundle = &s[..i + 4];
                if !bundle.contains("/claude-code/") {
                    found = Some(PathBuf::from(bundle));
                }
            }
        }
        cur = p.parent();
    }
    found
}

fn processes() -> Vec<Proc> {
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
    std::fs::read(path).is_ok_and(|b| {
        let needle = br#""origin":{"kind":"human"}"#;
        b.windows(needle.len()).any(|w| w == needle)
    })
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
    let procs = processes();
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
            host_bundle: p.host_bundle.clone(),
        });
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
    let tty = Command::new("ps")
        .args(["-o", "tty=", "-p", &s.pid.to_string()])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|t| !t.is_empty() && t != "??")
        .map(|t| format!("/dev/{t}"));
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
    let editors = ["Visual Studio Code", "Code", "Cursor", "Windsurf", "Zed", "VSCodium", "Antigravity"];
    let mut cmd = Command::new("open");
    cmd.arg("-a").arg(bundle);
    if editors.iter().any(|e| app.contains(e)) {
        cmd.arg(&s.cwd);
    }
    match cmd.status() {
        Ok(st) if st.success() => Ok(()),
        _ => Err(format!("could not open {app}")),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn jump(_s: &LiveSession) -> Result<(), String> {
    Err("Jumping to a session is macOS-only for now".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_790_000_100_000;
    fn line(kind: &str, extra: &str) -> String {
        format!(r#"{{"type":"{kind}","timestamp":"2026-09-21T13:08:00.000Z"{extra}}}"#)
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

    #[test]
    fn project_dir_naming_matches_claude_code() {
        let p = claude_project_dir(Path::new("/p"), Path::new("/Users/me/.claude-mem/observer-sessions"));
        assert_eq!(p, Path::new("/p/-Users-me--claude-mem-observer-sessions"));
    }
}
