//! Scheduled Claude tasks: what is defined, when each ran, and whether the
//! last run finished. Feeds the Tasks tab.

use serde::Serialize;
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const HEAD_BYTES: u64 = 16 * 1024;
const TAIL_BYTES: u64 = 64 * 1024;

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskDef {
    pub name: String,
    pub description: Option<String>,
    /// The SKILL.md path.
    pub file: String,
    /// The task text after the front matter.
    #[serde(skip)]
    pub body: String,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Done,
    Failed,
    /// Ended mid-step with nothing after it (often waiting for an approval).
    Stopped,
    Running,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub task: String,
    pub session_id: String,
    pub cwd: Option<String>,
    pub start: i64,
    pub end: i64,
    pub outcome: Outcome,
}

/// Tasks defined in `dir` (~/.claude/scheduled-tasks), by name.
pub fn definitions(dir: &Path) -> Vec<TaskDef> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<TaskDef> = entries
        .flatten()
        .filter_map(|e| {
            let file = e.path().join("SKILL.md");
            let text = std::fs::read_to_string(&file).ok()?;
            let (front, body) = split_front_matter(&text);
            let field = |k: &str| front.lines().find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string())).filter(|v| !v.is_empty());
            let name = field("name:").or_else(|| e.file_name().to_str().map(str::to_string))?;
            Some(TaskDef { name, description: field("description:"), file: file.to_string_lossy().into_owned(), body: body.trim().to_string() })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn split_front_matter(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---\n") else { return ("", text) };
    match rest.find("\n---") {
        Some(i) => {
            let after = &rest[i + 4..];
            (&rest[..i], after.strip_prefix('\n').unwrap_or(after))
        }
        None => ("", text),
    }
}

fn read_range(path: &Path, from_end: bool, bytes: u64) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let (start, n) = if from_end { (len.saturating_sub(bytes), bytes.min(len)) } else { (0, bytes.min(len)) };
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = vec![0; n as usize];
    f.read_exact(&mut buf).ok()?;
    let s = String::from_utf8_lossy(&buf).into_owned();
    // Drop a line cut in half at either edge.
    Some(if from_end && start > 0 { s.split_once('\n').map(|(_, r)| r.to_string()).unwrap_or_default() } else if !from_end && n < len { s.rsplit_once('\n').map(|(l, _)| l.to_string()).unwrap_or_default() } else { s })
}

fn ts_of(d: &Value) -> Option<i64> {
    let s = d.get("timestamp")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.timestamp_millis())
}

fn text_of(d: &Value) -> Option<&str> {
    if let Some(s) = d.get("content").and_then(Value::as_str) {
        return Some(s);
    }
    let c = d.get("message")?.get("content")?;
    c.as_str().or_else(|| c.as_array()?.iter().find(|b| b.get("type").and_then(Value::as_str) == Some("text"))?.get("text")?.as_str())
}

/// The task name from a `<scheduled-task name="…" …>` header.
fn task_name(text: &str) -> Option<String> {
    let rest = text.trim_start().strip_prefix("<scheduled-task")?;
    let after = &rest[rest.find("name=\"")? + 6..];
    Some(after[..after.find('"')?].to_string()).filter(|n| !n.is_empty())
}

fn run_of(path: &Path, mtime: i64, now: i64, fresh_ms: i64) -> Option<Run> {
    let head = read_range(path, false, HEAD_BYTES)?;
    let mut task = None;
    let mut start = None;
    let mut cwd = None;
    for line in head.lines() {
        let Ok(d) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(t) = ts_of(&d) {
            start = Some(start.map_or(t, |s: i64| s.min(t)));
        }
        if cwd.is_none() {
            cwd = d.get("cwd").and_then(Value::as_str).map(str::to_string);
        }
        if task.is_none() {
            let kind = d.get("type").and_then(Value::as_str);
            if matches!(kind, Some("queue-operation") | Some("user")) {
                task = text_of(&d).and_then(task_name);
                if task.is_none() && kind == Some("user") && d.get("origin").is_some() {
                    return None; // the first prompt is not a scheduled task
                }
            }
        }
    }
    let task = task?;
    let tail = read_range(path, true, TAIL_BYTES)?;
    let mut end = start?;
    let mut ended = Outcome::Stopped;
    for line in tail.lines() {
        let Ok(d) = serde_json::from_str::<Value>(line) else { continue };
        let Some(t) = ts_of(&d) else { continue };
        end = end.max(t);
        let kind = d.get("type").and_then(Value::as_str);
        let rejected = d.get("quotaLimits").and_then(|q| q.get("status")).and_then(Value::as_str) == Some("rejected");
        if rejected || (kind == Some("system") && d.get("subtype").and_then(Value::as_str) == Some("api_error")) {
            ended = Outcome::Failed;
        } else if kind == Some("assistant") {
            let m = d.get("message");
            let synthetic = m.and_then(|m| m.get("model")).and_then(Value::as_str) == Some("<synthetic>");
            if !synthetic {
                ended = if m.and_then(|m| m.get("stop_reason")).and_then(Value::as_str) == Some("end_turn") { Outcome::Done } else { Outcome::Stopped };
            }
        } else if kind == Some("user") {
            ended = Outcome::Stopped;
        }
    }
    if ended != Outcome::Done && fresh_ms > 0 && now - mtime < fresh_ms {
        ended = Outcome::Running;
    }
    let session_id = path.file_stem()?.to_string_lossy().into_owned();
    Some(Run { task, session_id, cwd, start: start?, end, outcome: ended })
}

fn logs_since(dir: &Path, since: i64, out: &mut Vec<(PathBuf, i64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if p.file_name().is_some_and(|n| n == "subagents") {
                continue;
            }
            logs_since(&p, since, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            let mtime = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
            if mtime >= since {
                out.push((p, mtime));
            }
        }
    }
}

/// Scheduled-task runs in Claude logs under `projects` modified since
/// `since`. A run whose log changed within `fresh_ms` of `now` and hasn't
/// finished counts as running.
pub fn runs(projects: &Path, since: i64, now: i64, fresh_ms: i64) -> Vec<Run> {
    use rayon::prelude::*;
    let mut files = Vec::new();
    logs_since(projects, since, &mut files);
    files.par_iter().filter_map(|(p, m)| run_of(p, *m, now, fresh_ms)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    pub(super) const H: i64 = 3_600_000;
    pub(super) fn ts(ms: i64) -> String {
        chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }
    pub(super) fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ai-tasks-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    /// A run log: queue line with the task header, then `ending` lines.
    pub(super) fn run_log(dir: &Path, id: &str, task: &str, start: i64, ending: &[String]) {
        std::fs::create_dir_all(dir).unwrap();
        let mut f = std::fs::File::create(dir.join(format!("{id}.jsonl"))).unwrap();
        writeln!(f, r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"{}","sessionId":"{id}","content":"<scheduled-task name=\"{task}\" file=\"/u/.claude/scheduled-tasks/{task}/SKILL.md\">\nDo the digest."}}"#, ts(start)).unwrap();
        writeln!(f, r#"{{"type":"user","timestamp":"{}","sessionId":"{id}","cwd":"/w/vault","origin":{{"kind":"human"}},"message":{{"role":"user","content":"<scheduled-task name=\"{task}\">\nDo the digest."}}}}"#, ts(start + 500)).unwrap();
        for l in ending {
            writeln!(f, "{l}").unwrap();
        }
    }
    pub(super) fn done(at: i64) -> String {
        format!(r#"{{"type":"assistant","timestamp":"{}","message":{{"model":"claude-opus-5","stop_reason":"end_turn","content":[{{"type":"text","text":"Done."}}]}}}}"#, ts(at))
    }
    pub(super) fn tool_use(at: i64) -> String {
        format!(r#"{{"type":"assistant","timestamp":"{}","message":{{"model":"claude-opus-5","stop_reason":"tool_use","content":[{{"type":"tool_use","name":"Bash"}}]}}}}"#, ts(at))
    }
    pub(super) fn api_error(at: i64) -> String {
        format!(r#"{{"type":"system","subtype":"api_error","timestamp":"{}"}}"#, ts(at))
    }

    #[test]
    fn reads_task_definitions() {
        let d = tmp("defs");
        std::fs::create_dir_all(d.join("eod-digest")).unwrap();
        std::fs::write(d.join("eod-digest/SKILL.md"), "---\nname: eod-digest\ndescription: Daily digest of repo activity\n---\n\nWrite the digest.\n").unwrap();
        std::fs::create_dir_all(d.join("no-skill")).unwrap();
        let defs = definitions(&d);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "eod-digest");
        assert_eq!(defs[0].description.as_deref(), Some("Daily digest of repo activity"));
        assert_eq!(defs[0].body, "Write the digest.");
        assert!(defs[0].file.ends_with("eod-digest/SKILL.md"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn recognises_runs_and_how_they_ended() {
        let d = tmp("runs");
        let p = d.join("proj");
        let t0 = 1_790_000_000_000;
        run_log(&p, "r-done", "eod-digest", t0, &[tool_use(t0 + 60_000), done(t0 + 180_000)]);
        run_log(&p, "r-fail", "eod-digest", t0 + 24 * H, &[api_error(t0 + 24 * H + 5000)]);
        run_log(&p, "r-stuck", "radar", t0 + 30 * H, &[tool_use(t0 + 30 * H + 9000)]);
        // An ordinary session is not a run.
        std::fs::write(p.join("normal.jsonl"), format!(r#"{{"type":"user","timestamp":"{}","origin":{{"kind":"human"}},"message":{{"role":"user","content":"hello"}}}}"#, ts(t0)) + "\n").unwrap();
        let now = t0 + 40 * H; // files were just written, so pass `fresh_ms = 0` to not call anything running
        let mut runs = runs(&d, now - 14 * 24 * H, now, 0);
        runs.sort_by_key(|r| r.start);
        assert_eq!(runs.len(), 3);
        assert_eq!((runs[0].task.as_str(), runs[0].outcome), ("eod-digest", Outcome::Done));
        assert_eq!(runs[0].session_id, "r-done");
        assert_eq!(runs[0].cwd.as_deref(), Some("/w/vault"));
        assert_eq!(runs[0].start, t0);
        assert_eq!(runs[0].end, t0 + 180_000);
        assert_eq!(runs[1].outcome, Outcome::Failed);
        assert_eq!((runs[2].task.as_str(), runs[2].outcome), ("radar", Outcome::Stopped));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_log_written_just_now_is_running() {
        let d = tmp("running");
        let t0 = 1_790_000_000_000;
        run_log(&d.join("proj"), "r1", "radar", t0, &[tool_use(t0 + 1000)]);
        let r = runs(&d, 0, t0 + H, 10 * 365 * 24 * H); // every file counts as fresh
        assert_eq!(r[0].outcome, Outcome::Running);
        let _ = std::fs::remove_dir_all(&d);
    }
}
