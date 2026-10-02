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
const NOT_HUMAN_PREFIXES: [&str; 4] = ["<local-command", "<task-notification", "[SYSTEM", "<system-reminder"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CLine<'a> {
    #[serde(rename = "type")]
    kind: Option<&'a str>,
    timestamp: Option<&'a str>,
    cwd: Option<String>,
    custom_title: Option<String>,
    agent_name: Option<String>,
    quota_limits: Option<Quota>,
    is_compact_summary: Option<bool>,
    is_sidechain: Option<bool>,
    is_meta: Option<bool>,
    tool_use_result: Option<serde::de::IgnoredAny>,
    origin: Option<Origin>,
    #[serde(borrow)]
    message: Option<CMsg<'a>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Quota {
    status: Option<String>,
    rate_limit_type: Option<String>,
    resets_at: Option<f64>,
}

#[derive(Deserialize)]
struct Origin {
    kind: Option<String>,
}

#[derive(Deserialize)]
struct CMsg<'a> {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
    #[serde(borrow)]
    content: Option<&'a RawValue>,
}

#[derive(Deserialize, Clone)]
struct Usage {
    input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: Option<String>,
    name: Option<String>,
    text: Option<String>,
    input: Option<ToolInput>,
}

#[derive(Deserialize)]
struct ToolInput {
    file_path: Option<String>,
    notebook_path: Option<String>,
}

pub fn parse_claude(path: &Path, since: i64, is_subagent: bool) -> Session {
    let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut b = Builder::new("claude", id, path);
    b.s.is_subagent = is_subagent;
    let mut usage_by_id: HashMap<String, (Option<String>, Usage)> = HashMap::new();
    let mut limit_keys = HashSet::new();

    for line in lines(path) {
        let Ok(mut d) = serde_json::from_str::<CLine>(&line) else { continue };
        if d.kind == Some("custom-title") {
            if let Some(t) = d.custom_title.take().filter(|t| !t.is_empty()) {
                b.s.title = Some(t);
            }
        }
        if d.kind == Some("agent-name") && b.s.title.is_none() {
            b.s.title = d.agent_name.take().filter(|t| !t.is_empty());
        }
        let Some(ts) = d.timestamp.and_then(parse_ts) else { continue };
        if ts < since {
            continue;
        }
        if b.s.project.is_none() {
            b.s.project = d.cwd.as_deref().and_then(basename);
        }
        if let Some(q) = &d.quota_limits {
            if q.status.as_deref() == Some("rejected") {
                let kind = q.rate_limit_type.clone().unwrap_or_default();
                let resets = q.resets_at.unwrap_or(0.0);
                if limit_keys.insert(format!("{kind}:{resets}")) {
                    b.s.limit_hits.push(LimitHit { ts, kind, resets_at: (resets * 1000.0) as i64 });
                }
            }
        }
        if d.is_compact_summary == Some(true) {
            b.s.compactions += 1;
        }

        match d.kind {
            Some("assistant") => {
                let Some(m) = d.message.take() else { continue };
                b.activity(ts);
                if let (Some(id), Some(u)) = (m.id, m.usage) {
                    usage_by_id.insert(id, (m.model, u));
                }
                let blocks: Vec<Block> = m.content.and_then(|c| serde_json::from_str(c.get()).ok()).unwrap_or_default();
                for c in blocks {
                    if c.kind.as_deref() != Some("tool_use") {
                        continue;
                    }
                    b.tool(c.name.as_deref());
                    let file = c.input.and_then(|i| i.file_path.or(i.notebook_path));
                    if let (Some(name), Some(f)) = (c.name.as_deref(), file) {
                        if EDIT_TOOLS.contains(&name) {
                            b.files.insert(f);
                        }
                    }
                }
            }
            Some("user") => {
                let human = !is_subagent && d.is_sidechain != Some(true) && is_human_prompt(&d);
                if human {
                    b.prompt(ts)
                } else {
                    b.activity(ts)
                }
            }
            _ => {}
        }
    }

    for (model, u) in usage_by_id.into_values() {
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

fn is_human_prompt(d: &CLine) -> bool {
    if d.tool_use_result.is_some() || d.is_meta == Some(true) {
        return false;
    }
    if let Some(o) = &d.origin {
        return o.kind.as_deref() == Some("human");
    }
    // Older logs have no origin field: fall back to the message shape.
    let Some(raw) = d.message.as_ref().and_then(|m| m.content) else { return false };
    let text = if raw.get().starts_with('"') {
        serde_json::from_str::<String>(raw.get()).ok()
    } else {
        let blocks: Vec<Block> = serde_json::from_str(raw.get()).unwrap_or_default();
        if blocks.iter().any(|c| c.kind.as_deref() == Some("tool_result")) {
            return false;
        }
        blocks.into_iter().find(|c| c.kind.as_deref() == Some("text")).and_then(|c| c.text)
    };
    match text {
        Some(t) => !NOT_HUMAN_PREFIXES.iter().any(|p| t.trim_start().starts_with(p)),
        None => false,
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
                b.s.project = basename(cwd);
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
}

impl Roots {
    pub fn default_for(home: &Path) -> Self {
        let claude = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| home.join(".claude"));
        let codex = std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex"));
        Roots {
            claude: claude.join("projects"),
            codex: codex.join("sessions"),
            cursor: cursor_user_dir(home).join("globalStorage").join("state.vscdb"),
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
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            list_jsonl(&p, since, out);
        } else if ft.is_file() && p.extension().is_some_and(|x| x == "jsonl") {
            let Ok(meta) = e.metadata() else { continue };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as i64);
            if mtime >= since {
                out.push((p, meta.len()));
            }
        }
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

pub fn scan(roots: &Roots, days: u32) -> ScanResult {
    let t0 = Instant::now();
    let until = now_ms();
    let since = until - i64::from(days) * 86_400_000;
    let (mut claude, mut codex) = (Vec::new(), Vec::new());
    list_jsonl(&roots.claude, since, &mut claude);
    list_jsonl(&roots.codex, since, &mut codex);
    let cursor_size = std::fs::metadata(&roots.cursor).map_or(0, |m| m.len());
    let bytes = claude.iter().chain(codex.iter()).map(|(_, n)| n).sum::<u64>() + cursor_size;
    let files = FileCounts { claude: claude.len(), codex: codex.len(), cursor: usize::from(cursor_size > 0) };

    let mut sessions: Vec<Session> = claude
        .par_iter()
        .map(|(p, _)| {
            let sub = p.components().any(|c| c.as_os_str() == "subagents");
            parse_claude(p, since, sub)
        })
        .chain(codex.par_iter().map(|(p, _)| parse_codex(p, since)))
        .filter(|s| s.start.is_some())
        .collect();
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
    fn claude_window_filter() {
        let since = parse_ts("2026-09-10T10:10:00Z").unwrap();
        assert_eq!(parse_claude(&fx("claude-session.jsonl"), since, false).prompts.len(), 1);
    }

    #[test]
    fn codex_matches_js_parser() {
        let s = parse_codex(&fx("codex-rollout.jsonl"), SINCE);
        assert_eq!(s.id, "c1");
        assert_eq!(s.project.as_deref(), Some("api"));
        assert_eq!(s.prompts.len(), 2);
        assert_eq!(s.tokens, Tokens { input: 1500, cache_read: 4500, cache_write: 0, output: 350 });
        assert_eq!(s.models.get("gpt-5.5"), Some(&350));
        assert_eq!(s.files_edited, vec!["/work/api/list.ts", "/work/api/page.ts"]);
        assert_eq!(s.peak_quota_pct, Some(100.0));
        assert_eq!(s.limit_hits.len(), 1);
        assert_eq!(s.tools.get("apply_patch"), Some(&1));
    }

    #[test]
    fn basename_both_separators() {
        assert_eq!(basename("C:\\work\\shop").as_deref(), Some("shop"));
        assert_eq!(basename("/work/shop/").as_deref(), Some("shop"));
    }
}
