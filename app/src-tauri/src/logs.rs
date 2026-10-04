//! Reads Claude Code and Codex session logs into per-session records.
//!
//! Mirrors `wrapped/src/claude.mjs` and `wrapped/src/codex.mjs` field for
//! field. The record is serialized to the webview, where the shared
//! `stats.mjs` turns it into the report, so the two must stay in step.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Default, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
}

#[derive(Serialize, Clone, Copy, Debug)]
pub struct Turn {
    pub start: i64,
    pub end: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LimitHit {
    pub ts: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub resets_at: i64,
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub agent: &'static str,
    pub id: String,
    pub file: String,
    pub project: Option<String>,
    /// Working directory, for finding the repository (daily recap).
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub prompts: Vec<i64>,
    pub turns: Vec<Turn>,
    pub tokens: Tokens,
    /// Output tokens per model.
    pub models: HashMap<String, u64>,
    /// Agent responses per model: the one measure every agent records.
    pub responses: HashMap<String, u64>,
    pub tools: HashMap<String, u64>,
    pub files_edited: Vec<String>,
    pub compactions: u32,
    pub limit_hits: Vec<LimitHit>,
    pub peak_quota_pct: Option<f64>,
    pub is_subagent: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub sessions: Vec<Session>,
    pub files: FileCounts,
    pub bytes: u64,
    pub seconds: f64,
    pub since: i64,
    pub until: i64,
}

#[derive(Serialize, Default)]
pub struct FileCounts {
    pub claude: usize,
    pub codex: usize,
    pub cursor: usize,
    pub antigravity: usize,
}

/// Session-building state shared by both adapters. A human prompt opens a
/// turn; any agent activity extends it.
pub(crate) struct Builder {
    pub(crate) s: Session,
    open: Option<Turn>,
    pub(crate) files: HashSet<String>,
}

impl Builder {
    pub(crate) fn new(agent: &'static str, id: String, file: &Path) -> Self {
        Builder {
            s: Session { agent, id, file: file.to_string_lossy().into_owned(), ..Default::default() },
            open: None,
            files: HashSet::new(),
        }
    }
    fn touch(&mut self, ts: i64) {
        self.s.start = Some(self.s.start.map_or(ts, |v| v.min(ts)));
        self.s.end = Some(self.s.end.map_or(ts, |v| v.max(ts)));
    }
    pub(crate) fn prompt(&mut self, ts: i64) {
        self.s.prompts.push(ts);
        if let Some(t) = self.open.take() {
            self.s.turns.push(t);
        }
        self.open = Some(Turn { start: ts, end: ts });
        self.touch(ts);
    }
    pub(crate) fn activity(&mut self, ts: i64) {
        self.touch(ts);
        if let Some(t) = self.open.as_mut() {
            if ts > t.end {
                t.end = ts;
            }
        }
    }
    pub(crate) fn model(&mut self, model: Option<&str>, output: u64) {
        match model {
            Some(m) if m != "<synthetic>" => *self.s.models.entry(m.to_string()).or_default() += output,
            _ => {}
        }
    }
    pub(crate) fn response(&mut self, model: Option<&str>) {
        match model {
            Some(m) if m != "<synthetic>" => *self.s.responses.entry(m.to_string()).or_default() += 1,
            _ => {}
        }
    }
    pub(crate) fn tool(&mut self, name: Option<&str>) {
        if let Some(n) = name {
            *self.s.tools.entry(n.to_string()).or_default() += 1;
        }
    }
    pub(crate) fn finish(mut self) -> Session {
        if let Some(t) = self.open.take() {
            self.s.turns.push(t);
        }
        let mut files: Vec<String> = self.files.into_iter().collect();
        files.sort();
        self.s.files_edited = files;
        self.s
    }
}

/// The project a working directory belongs to. Git worktrees count toward
/// their repository: /repo/.worktrees/x and /repo/.claude/worktrees/y are "repo".
pub fn project_of(cwd: &str) -> Option<String> {
    let parts: Vec<&str> = cwd.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if let Some(wt) = parts.iter().position(|p| *p == ".worktrees" || *p == "worktrees").filter(|&i| i > 0) {
        return parts[..wt].iter().rev().find(|p| **p != ".claude").map(|p| p.to_string());
    }
    parts.last().map(|p| p.to_string())
}

pub fn basename(p: &str) -> Option<String> {
    p.split(['/', '\\']).filter(|s| !s.is_empty()).last().map(str::to_string)
}

fn parse_ts(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}

/// Yields each line of a file. Unreadable bytes end the file early rather
/// than failing the whole scan.
fn lines(path: &Path) -> impl Iterator<Item = String> {
    let reader = File::open(path).ok().map(|f| BufReader::with_capacity(1 << 20, f));
    reader.into_iter().flat_map(|r| r.lines().map_while(Result::ok))
}

// ---------- Claude Code ----------

const EDIT_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];
pub(crate) const NOT_HUMAN_PREFIXES: [&str; 13] = [
    "<local-command",
    "<bash-input",
    "<bash-stdout",
    "<bash-stderr",
    "<artifact-view-context",
    "<command-name",
    "<command-message",
    "<command-args",
    "<task-notification",
    "<scheduled-task",
    "[SYSTEM",
    "<system-reminder",
    "This session is being continued from a previous conversation",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CLine<'a> {
    #[serde(rename = "type")]
    pub(crate) kind: Option<&'a str>,
    pub(crate) timestamp: Option<&'a str>,
    pub(crate) uuid: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) custom_title: Option<String>,
    pub(crate) agent_name: Option<String>,
    pub(crate) quota_limits: Option<Quota>,
    is_compact_summary: Option<bool>,
    pub(crate) is_sidechain: Option<bool>,
    is_meta: Option<bool>,
    tool_use_result: Option<serde::de::IgnoredAny>,
    origin: Option<Origin>,
    pub(crate) entrypoint: Option<&'a str>,
    #[serde(borrow)]
    pub(crate) message: Option<CMsg<'a>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Quota {
    pub(crate) status: Option<String>,
    pub(crate) rate_limit_type: Option<String>,
    pub(crate) resets_at: Option<f64>,
}

#[derive(Deserialize)]
struct Origin {
    kind: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CMsg<'a> {
    id: Option<String>,
    pub(crate) model: Option<String>,
    usage: Option<Usage>,
    #[serde(borrow)]
    pub(crate) content: Option<&'a RawValue>,
}

#[derive(Deserialize, Clone)]
struct Usage {
    input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
pub(crate) struct Block {
    #[serde(rename = "type")]
    pub(crate) kind: Option<String>,
    name: Option<String>,
    pub(crate) text: Option<String>,
    input: Option<ToolInput>,
}

#[derive(Deserialize)]
struct ToolInput {
    file_path: Option<String>,
    notebook_path: Option<String>,
}

pub fn parse_claude(path: &Path, since: i64, is_subagent: bool) -> Session {
    build_claude(read_claude(path, since, is_subagent), &mut Seen::default())
}

/// One counted line of a Claude log.
struct CEvent {
    uuid: Option<String>,
    ts: i64,
    kind: CKind,
    compact: bool,
    limit: Option<(String, f64)>,
}

enum CKind {
    Prompt,
    Activity,
    Assistant { msg_id: Option<String>, model: Option<String>, usage: Option<Usage>, tools: Vec<(Option<String>, Option<String>)> },
    Other,
}

/// A Claude log read into events. Resumed and forked sessions start a new
/// file that copies earlier lines with the same uuid and message id, so the
/// scan reads every file first and then builds sessions oldest first,
/// skipping lines an earlier file already counted.
pub(crate) struct ClaudeFile {
    path: PathBuf,
    is_subagent: bool,
    title: Option<String>,
    cwd: Option<String>,
    first: Option<i64>,
    born: i64,
    events: Vec<CEvent>,
}

/// Line uuids and message ids already counted by earlier files.
#[derive(Default)]
pub(crate) struct Seen {
    uuids: HashSet<String>,
    messages: HashSet<String>,
}

pub(crate) fn read_claude(path: &Path, since: i64, is_subagent: bool) -> ClaudeFile {
    let mut f = ClaudeFile { path: path.to_path_buf(), is_subagent, title: None, cwd: None, first: None, born: 0, events: Vec::new() };
    for line in lines(path) {
        let Ok(mut d) = serde_json::from_str::<CLine>(&line) else { continue };
        if d.kind == Some("custom-title") {
            if let Some(t) = d.custom_title.take().filter(|t| !t.is_empty()) {
                f.title = Some(t);
            }
        }
        if d.kind == Some("agent-name") && f.title.is_none() {
            f.title = d.agent_name.take().filter(|t| !t.is_empty());
        }
        let Some(ts) = d.timestamp.and_then(parse_ts) else { continue };
        f.first = Some(f.first.map_or(ts, |v| v.min(ts)));
        if ts < since {
            continue;
        }
        if f.cwd.is_none() {
            f.cwd = d.cwd.take();
        }
        let limit = d
            .quota_limits
            .as_ref()
            .filter(|q| q.status.as_deref() == Some("rejected"))
            .map(|q| (q.rate_limit_type.clone().unwrap_or_default(), q.resets_at.unwrap_or(0.0)));
        let compact = d.is_compact_summary == Some(true);
        let kind = match d.kind {
            Some("assistant") if d.message.is_some() => {
                let m = d.message.take().unwrap();
                let blocks: Vec<Block> = m.content.and_then(|c| serde_json::from_str(c.get()).ok()).unwrap_or_default();
                let tools = blocks
                    .into_iter()
                    .filter(|c| c.kind.as_deref() == Some("tool_use"))
                    .map(|c| (c.name, c.input.and_then(|i| i.file_path.or(i.notebook_path))))
                    .collect();
                CKind::Assistant { msg_id: m.id, model: m.model, usage: m.usage, tools }
            }
            Some("user") if !is_subagent && d.is_sidechain != Some(true) && is_human_prompt(&d) => CKind::Prompt,
            Some("user") => CKind::Activity,
            _ if compact || limit.is_some() => CKind::Other,
            _ => continue,
        };
        f.events.push(CEvent { uuid: d.uuid.take(), ts, kind, compact, limit });
    }
    f
}

pub(crate) fn build_claude(f: ClaudeFile, seen: &mut Seen) -> Session {
    let id = f.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut b = Builder::new("claude", id, &f.path);
    b.s.is_subagent = f.is_subagent;
    b.s.title = f.title;
    b.s.project = f.cwd.as_deref().and_then(project_of);
    b.s.cwd = f.cwd;
    let mut usage_by_id: HashMap<String, (Option<String>, Usage)> = HashMap::new();
    let mut limit_keys = HashSet::new();

    for e in f.events {
        if let Some(u) = e.uuid {
            if !seen.uuids.insert(u) {
                continue;
            }
        }
        if let Some((kind, resets)) = e.limit {
            if limit_keys.insert(format!("{kind}:{resets}")) {
                b.s.limit_hits.push(LimitHit { ts: e.ts, kind, resets_at: (resets * 1000.0) as i64 });
            }
        }
        if e.compact {
            b.s.compactions += 1;
        }
        match e.kind {
            CKind::Assistant { msg_id, model, usage, tools } => {
                b.activity(e.ts);
                if let (Some(id), Some(u)) = (msg_id, usage) {
                    if !seen.messages.contains(&id) {
                        usage_by_id.insert(id, (model, u));
                    }
                }
                for (name, file) in tools {
                    b.tool(name.as_deref());
                    if let (Some(name), Some(f)) = (name.as_deref(), file) {
                        if EDIT_TOOLS.contains(&name) {
                            b.files.insert(f);
                        }
                    }
                }
            }
            CKind::Prompt => b.prompt(e.ts),
            CKind::Activity => b.activity(e.ts),
            CKind::Other => {}
        }
    }

    for (id, (model, u)) in usage_by_id {
        seen.messages.insert(id);
        let out = u.output_tokens.unwrap_or(0);
        b.s.tokens.input += u.input_tokens.unwrap_or(0);
        b.s.tokens.cache_read += u.cache_read_input_tokens.unwrap_or(0);
        b.s.tokens.cache_write += u.cache_creation_input_tokens.unwrap_or(0);
        b.s.tokens.output += out;
        b.model(model.as_deref(), out);
        b.response(model.as_deref());
    }
    b.finish()
}

/// Builds sessions from many read files, oldest first, so a resumed or
/// forked copy never counts a line twice. Ties (a fork copies the first
/// timestamp too) go to the file created first, then to the path.
pub(crate) fn build_claude_all(mut files: Vec<ClaudeFile>) -> Vec<Session> {
    files.retain(|f| f.first.is_some());
    files.sort_by(|a, b| (a.first, a.born, &a.path).cmp(&(b.first, b.born, &b.path)));
    let mut seen = Seen::default();
    files.into_iter().map(|f| build_claude(f, &mut seen)).collect()
}

pub(crate) fn is_human_prompt(d: &CLine) -> bool {
    if d.tool_use_result.is_some() || d.is_meta == Some(true) {
        return false;
    }
    let text = prompt_text(d);
    // Scheduled tasks, injected commands and context are logged as human but
    // nobody typed them.
    if text.as_deref().is_some_and(|t| NOT_HUMAN_PREFIXES.iter().any(|p| t.trim_start().starts_with(p))) {
        return false;
    }
    if let Some(o) = &d.origin {
        return o.kind.as_deref() == Some("human");
    }
    // Programmatic runs (Agent SDK, `claude -p` from scripts and plugins such
    // as claude-mem) are not a person typing.
    if d.entrypoint.is_some_and(|e| e.starts_with("sdk")) {
        return false;
    }
    // Older logs have no origin field: fall back to the message shape.
    if let Some(raw) = d.message.as_ref().and_then(|m| m.content).filter(|r| !r.get().starts_with('"')) {
        let blocks: Vec<Block> = serde_json::from_str(raw.get()).unwrap_or_default();
        if blocks.iter().any(|c| c.kind.as_deref() == Some("tool_result")) {
            return false;
        }
    }
    match text {
        Some(t) => !NOT_HUMAN_PREFIXES.iter().any(|p| t.trim_start().starts_with(p)),
        None => false,
    }
}

pub(crate) fn prompt_text(d: &CLine) -> Option<String> {
    let raw = d.message.as_ref().and_then(|m| m.content)?;
    if raw.get().starts_with('"') {
        serde_json::from_str::<String>(raw.get()).ok()
    } else {
        let blocks: Vec<Block> = serde_json::from_str(raw.get()).unwrap_or_default();
        blocks.into_iter().find(|c| c.kind.as_deref() == Some("text")).and_then(|c| c.text)
    }
}

// ---------- Codex ----------

#[derive(Deserialize)]
struct XLine<'a> {
    #[serde(rename = "type")]
    kind: Option<&'a str>,
    timestamp: Option<&'a str>,
    #[serde(borrow)]
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize, Default)]
struct XPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    id: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    name: Option<String>,
    role: Option<String>,
    input: Option<serde_json::Value>,
    info: Option<XInfo>,
    rate_limits: Option<XRateLimits>,
}

#[derive(Deserialize)]
struct XInfo {
    total_token_usage: Option<XUsage>,
}

#[derive(Deserialize, Clone)]
struct XUsage {
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct XRateLimits {
    primary: Option<XWindow>,
    secondary: Option<XWindow>,
    rate_limit_reached_type: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct XWindow {
    used_percent: Option<f64>,
    resets_at: Option<f64>,
}

pub fn parse_codex(path: &Path, since: i64) -> Session {
    let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut b = Builder::new("codex", id, path);
    let mut model: Option<String> = None;
    let mut last_total: Option<XUsage> = None;
    let mut limit_keys = HashSet::new();

    for line in lines(path) {
        let Ok(d) = serde_json::from_str::<XLine>(&line) else { continue };
        let p: XPayload = d.payload.and_then(|r| serde_json::from_str(r.get()).ok()).unwrap_or_default();
        if d.kind == Some("session_meta") {
            if let Some(id) = p.id {
                b.s.id = id;
            }
            if let Some(cwd) = p.cwd.as_deref() {
                b.s.project = project_of(cwd);
                b.s.cwd = Some(cwd.to_string());
            }
            continue;
        }
        if d.kind == Some("turn_context") && p.model.is_some() {
            model = p.model.clone();
        }
        let Some(ts) = d.timestamp.and_then(parse_ts) else { continue };
        if ts < since {
            continue;
        }
        if d.kind == Some("compacted") {
            b.s.compactions += 1;
        }

        match d.kind {
            Some("event_msg") => {
                if p.kind.as_deref() == Some("user_message") {
                    b.prompt(ts)
                } else {
                    b.activity(ts)
                }
                if p.kind.as_deref() != Some("token_count") {
                    continue;
                }
                if let Some(t) = p.info.and_then(|i| i.total_token_usage) {
                    last_total = Some(t);
                }
                if let Some(rl) = p.rate_limits {
                    let pct = [&rl.primary, &rl.secondary]
                        .iter()
                        .filter_map(|w| w.as_ref().and_then(|w| w.used_percent))
                        .fold(f64::NEG_INFINITY, f64::max);
                    if pct.is_finite() {
                        b.s.peak_quota_pct = Some(b.s.peak_quota_pct.map_or(pct, |v| v.max(pct)));
                    }
                    let reached = match &rl.rate_limit_reached_type {
                        Some(serde_json::Value::String(s)) => Some(s.clone()),
                        Some(serde_json::Value::Null) | None => None,
                        Some(v) => Some(v.to_string()),
                    };
                    if let Some(kind) = reached {
                        let resets = rl.primary.as_ref().and_then(|w| w.resets_at).unwrap_or(0.0);
                        if limit_keys.insert(format!("{kind}:{resets}")) {
                            b.s.limit_hits.push(LimitHit { ts, kind, resets_at: (resets * 1000.0) as i64 });
                        }
                    }
                }
            }
            Some("response_item") => {
                b.activity(ts);
                let k = p.kind.as_deref();
                if k == Some("message") && p.role.as_deref() == Some("assistant") {
                    b.response(model.as_deref());
                }
                if k == Some("function_call") || k == Some("custom_tool_call") {
                    b.tool(p.name.as_deref());
                    if p.name.as_deref() == Some("apply_patch") {
                        if let Some(serde_json::Value::String(patch)) = &p.input {
                            for l in patch.lines() {
                                let f = l.strip_prefix("*** Update File: ").or_else(|| l.strip_prefix("*** Add File: "));
                                if let Some(f) = f {
                                    b.files.insert(f.trim().to_string());
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(t) = last_total {
        let cached = t.cached_input_tokens.unwrap_or(0);
        let out = t.output_tokens.unwrap_or(0);
        b.s.tokens.input += t.input_tokens.unwrap_or(0).saturating_sub(cached);
        b.s.tokens.cache_read += cached;
        b.s.tokens.output += out;
        b.model(model.as_deref(), out);
    }
    b.finish()
}

// ---------- Scan ----------

pub struct Roots {
    pub claude: PathBuf,
    pub codex: PathBuf,
    pub cursor: PathBuf,
    pub gemini: PathBuf,
}

impl Roots {
    pub fn default_for(home: &Path) -> Self {
        let claude = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| home.join(".claude"));
        let codex = std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex"));
        Roots {
            claude: claude.join("projects"),
            codex: codex.join("sessions"),
            cursor: cursor_user_dir(home).join("globalStorage").join("state.vscdb"),
            gemini: std::env::var_os("GEMINI_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".gemini")),
        }
    }
}

fn cursor_user_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Cursor/User")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData").join("Roaming")).join("Cursor").join("User")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config")).join("Cursor").join("User")
    }
}

fn list_jsonl(dir: &Path, since: i64, out: &mut Vec<(PathBuf, u64)>) {
    list_jsonl_born(dir, since, &mut |p, len, _| out.push((p, len)));
}

/// Like `list_jsonl`, also passing each file's creation time (ms).
fn list_jsonl_born(dir: &Path, since: i64, out: &mut dyn FnMut(PathBuf, u64, i64)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            list_jsonl_born(&p, since, out);
        } else if ft.is_file() && p.extension().is_some_and(|x| x == "jsonl") {
            let Ok(meta) = e.metadata() else { continue };
            let ms = |t: std::io::Result<SystemTime>| t.ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
            if ms(meta.modified()) >= since {
                out(p, meta.len(), ms(meta.created()));
            }
        }
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

pub fn scan(roots: &Roots, days: u32) -> ScanResult {
    scan_since(roots, now_ms() - i64::from(days) * 86_400_000)
}

pub fn scan_since(roots: &Roots, since: i64) -> ScanResult {
    let t0 = Instant::now();
    let until = now_ms();
    let (mut claude, mut codex) = (Vec::new(), Vec::new());
    list_jsonl_born(&roots.claude, since, &mut |p, len, born| claude.push((p, len, born)));
    list_jsonl(&roots.codex, since, &mut codex);
    let mut agy = Vec::new();
    crate::antigravity::list(&roots.gemini, since, &mut agy);
    let cursor_size = std::fs::metadata(&roots.cursor).map_or(0, |m| m.len());
    let bytes = claude.iter().map(|(_, n, _)| n).sum::<u64>()
        + codex.iter().chain(agy.iter()).map(|(_, n)| n).sum::<u64>()
        + cursor_size;
    let files = FileCounts { claude: claude.len(), codex: codex.len(), cursor: usize::from(cursor_size > 0), antigravity: agy.len() };

    // Claude files are read in parallel and built together so resumed and
    // forked copies are counted once.
    let read: Vec<ClaudeFile> = claude
        .par_iter()
        .map(|(p, _, born)| {
            let sub = p.components().any(|c| c.as_os_str() == "subagents");
            ClaudeFile { born: *born, ..read_claude(p, since, sub) }
        })
        .collect();
    let mut sessions: Vec<Session> = build_claude_all(read);
    sessions.retain(|s| s.start.is_some());
    sessions.par_extend(
        codex
            .par_iter()
            .map(|(p, _)| parse_codex(p, since))
            .chain(agy.par_iter().map(|(p, _)| crate::antigravity::parse(p, since)))
            .filter(|s| s.start.is_some()),
    );
    if cursor_size > 0 {
        sessions.extend(crate::cursor::parse_cursor_db(&roots.cursor, since));
    }
    sessions.sort_by_key(|s| s.start);

    ScanResult { sessions, files, bytes, seconds: t0.elapsed().as_secs_f64(), since, until }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Same fixtures and expectations as wrapped/test/wrapped.test.mjs.
    fn fx(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../wrapped/test/fixtures").join(name)
    }
    const SINCE: i64 = 1_788_220_800_000; // 2026-09-01T00:00:00Z

    #[test]
    fn claude_matches_js_parser() {
        let s = parse_claude(&fx("claude-session.jsonl"), SINCE, false);
        assert_eq!(s.title.as_deref(), Some("Fix login bug"));
        assert_eq!(s.project.as_deref(), Some("shop"));
        assert_eq!(s.cwd.as_deref(), Some("/work/shop"));
        assert_eq!(s.prompts.len(), 2);
        assert_eq!(s.tokens, Tokens { input: 15, cache_read: 3000, cache_write: 100, output: 130 });
        assert_eq!(s.models.get("claude-sonnet-5"), Some(&50));
        assert_eq!(s.models.get("claude-opus-5"), Some(&80));
        assert_eq!(s.files_edited, vec!["/work/shop/login.test.ts", "/work/shop/login.ts"]);
        assert_eq!(s.limit_hits.len(), 1);
        assert_eq!(s.compactions, 1);
        assert_eq!(s.turns.len(), 2);
        assert_eq!(s.turns[0].end - s.turns[0].start, 3 * 60_000);
    }

    #[test]
    fn sdk_prompts_are_not_human() {
        let s = parse_claude(&fx("claude-sdk.jsonl"), SINCE, false);
        assert_eq!(s.prompts.len(), 1);
    }

    #[test]
    fn injected_commands_and_context_are_not_human() {
        let s = parse_claude(&fx("claude-injected.jsonl"), SINCE, false);
        assert_eq!(s.prompts.len(), 1, "only the typed line counts");
    }

    #[test]
    fn scheduled_tasks_are_not_human() {
        let s = parse_claude(&fx("claude-scheduled.jsonl"), SINCE, false);
        assert_eq!(s.prompts.len(), 0);
        assert_eq!(s.tokens.output, 5);
    }

    #[test]
    fn forked_copy_counts_each_line_once() {
        let dir = fx("fork");
        let roots = Roots { claude: dir.clone(), codex: fx("none"), cursor: fx("none"), gemini: fx("none") };
        let mut sessions = scan_since(&roots, SINCE).sessions;
        sessions.sort_by_key(|s| s.start);
        assert_eq!(sessions.len(), 2);
        let (orig, copy) = (&sessions[0], &sessions[1]);
        assert_eq!(orig.prompts.len(), 2);
        assert_eq!(orig.tokens.output, 30);
        assert_eq!(copy.prompts.len(), 1);
        assert_eq!(copy.tokens.output, 40);
        assert_eq!(copy.start, parse_ts("2026-09-15T10:20:00Z"));
        assert_eq!(copy.turns.len(), 1);
        assert!(copy.files_edited.is_empty());
        let one = parse_claude(&dir.join("zzz-copy.jsonl"), SINCE, false);
        assert_eq!(orig.prompts.len() + copy.prompts.len(), one.prompts.len());
        assert_eq!(orig.tokens.output + copy.tokens.output, one.tokens.output);
    }

    #[test]
    fn claude_window_filter() {
        let since = parse_ts("2026-09-10T10:10:00Z").unwrap();
        assert_eq!(parse_claude(&fx("claude-session.jsonl"), since, false).prompts.len(), 1);
    }

    #[test]
    fn codex_matches_js_parser() {
        let s = parse_codex(&fx("codex-rollout.jsonl"), SINCE);
        assert_eq!(s.id, "c1");
        assert_eq!(s.project.as_deref(), Some("api"));
        assert_eq!(s.cwd.as_deref(), Some("/work/api"));
        assert_eq!(s.prompts.len(), 2);
        assert_eq!(s.tokens, Tokens { input: 1500, cache_read: 4500, cache_write: 0, output: 350 });
        assert_eq!(s.models.get("gpt-5.5"), Some(&350));
        assert_eq!(s.files_edited, vec!["/work/api/list.ts", "/work/api/page.ts"]);
        assert_eq!(s.peak_quota_pct, Some(100.0));
        assert_eq!(s.limit_hits.len(), 1);
        assert_eq!(s.tools.get("apply_patch"), Some(&1));
    }

    #[test]
    fn project_of_folds_worktrees() {
        assert_eq!(project_of("/u/me/shop/.worktrees/WO-1").as_deref(), Some("shop"));
        assert_eq!(project_of("/u/me/site/.claude/worktrees/fix").as_deref(), Some("site"));
        assert_eq!(project_of("C:\\work\\api\\.worktrees\\b").as_deref(), Some("api"));
        assert_eq!(project_of("/u/me/shop").as_deref(), Some("shop"));
    }

    #[test]
    fn basename_both_separators() {
        assert_eq!(basename("C:\\work\\shop").as_deref(), Some("shop"));
        assert_eq!(basename("/work/shop/").as_deref(), Some("shop"));
    }
}
