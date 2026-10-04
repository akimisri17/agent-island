# Panel Redesign, Phase 4 (Tasks Tab) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A **Tasks** tab that answers "did my automations run?": one row per scheduled Claude task with its state (Missed, Failed, Stopped, Running, ran at…), how it's scheduled, a dot per day for the last 7 days, and **Run now** / **Last run** actions; a red tab dot and an optional notification when a task misses, fails or gets stuck.

**Architecture:** A new Rust module `tasks.rs` reads task definitions from `~/.claude/scheduled-tasks/*/SKILL.md` and finds runs in Claude logs from the last 14 days by reading only the head (to recognise a scheduled run and its task name) and the tail (to decide how it ended) of each log. A pure `board()` turns definitions and runs into rows (state, cadence, last run, 7 day marks in local time). The board is cached for 60 s. Commands expose the board and the two actions; the existing 60-second background loop also checks the board for new problems and notifies once. In the panel, a pure `ui/tasks.js` formats rows (tested) and `panel.js` adds the tab.

**Tech Stack:** Rust (serde_json, chrono Local), Tauri 2 (+ notification plugin, already used), vanilla JS, `node --test`, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-10-04-panel-redesign-design.md` → Tasks tab, New data (Scheduled tasks), Settings (notify missed tasks).

**What the logs look like (checked on real data, 125 scheduled runs):**
- A run is a Claude-desktop session. Its first line is a `queue-operation` whose `content` starts `<scheduled-task name="tts-eod-digest" file="/Users/…/.claude/scheduled-tasks/tts-eod-digest/SKILL.md">` followed by the task text. The first `user` line repeats it as `message.content`.
- Lines carry `cwd`, `timestamp`, `sessionId`.
- How a run ends, from its last meaningful line:
  - an assistant line (not `<synthetic>`) with `"stop_reason":"end_turn"` → **done**;
  - a `quotaLimits.status: "rejected"` line, or `"type":"system","subtype":"api_error"` → **failed**;
  - anything else (e.g. an assistant `tool_use` with nothing after it for hours, typically waiting for an approval) → **stopped**, unless the log changed in the last 5 minutes → **running**.
- `SKILL.md` starts with a `---` front-matter block holding `name:` and `description:`; the task text follows it.

**Decisions:**
- Expected cadence is inferred from past runs (median gap between starts; the schedule itself isn't in `SKILL.md`). A task is **Missed** when more than 1.5 × that gap has passed since its last start. Fewer than 2 runs → no cadence, never "missed".
- Day marks are computed in local time.
- **Run now** starts a new interactive `claude` session in a terminal tab, in the folder the task last ran in (home folder if it never ran), with the same `<scheduled-task …>` prompt the desktop app sends, so the run is recognised as a scheduled run and not counted as a person typing. **Last run** resumes the last run's session (`claude --resume`) in a terminal tab. Both look everything up on the backend by task name; nothing path-like comes from the webview.
- The tab bar grows to five: Waiting, Today, Repos, **Tasks**, Wrapped (keys 1–5).
- Notifications: off by default for nothing that was already wrong when the app started; afterwards one notification per new problem (a task's last start + state pair).

---

## Branch

```bash
cd /Users/akhilmisri/Business/Products/agent-island
git fetch -q
git worktree add -b feat/tasks-tab ../agent-island-tasks origin/dev
cd ../agent-island-tasks
```

Work only in that worktree. Never commit to `main` or `dev`. No Claude attribution lines in commits.

## File structure

| File | Responsibility |
|---|---|
| `app/src-tauri/src/tasks.rs` (new) | Task definitions, run detection, `board()`, notification check |
| `app/src-tauri/src/settings.rs` | + `notify_missed_tasks: bool` (default true) |
| `app/src-tauri/src/lib.rs` | + `pub mod tasks;`, cached board, commands `tasks_board`, `run_task`, `open_task_run`, loop hook |
| `app/src-tauri/examples/tasks.rs` (new) | Print the board, for checking by hand |
| `app/ui/tasks.js` (new) | Row text, tone, actions |
| `app/test/tasks.test.mjs` (new) | Tests for the above |
| `app/ui/index.html`, `panel.css`, `panel.js`, `icons.js` | Tab, view, Settings row |
| `app/scripts/preview.mjs` | Stubs |

---

### Task 1: Task definitions and runs

**Files:**
- Create: `app/src-tauri/src/tasks.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `pub mod tasks;` in alphabetical order)

- [ ] **Step 1: Write the failing tests**

Create `app/src-tauri/src/tasks.rs` with only:

```rust
//! Scheduled Claude tasks: what is defined, when each ran, and whether the
//! last run finished. Feeds the Tasks tab.

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
```

Add `pub mod tasks;` to `app/src-tauri/src/lib.rs` (alphabetical among the `pub mod` lines).

- [ ] **Step 2: Run to verify they fail**

Run: `cd app/src-tauri && cargo test --lib tasks::`
Expected: compile errors (`definitions`, `runs`, `Outcome` missing).

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]` in `app/src-tauri/src/tasks.rs`:

```rust
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
    if ended != Outcome::Done && now - mtime < fresh_ms {
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
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd app/src-tauri && cargo test --lib tasks:: && cargo clippy --lib 2>&1 | grep -A3 tasks.rs`
Expected: 3 pass; no clippy output for tasks.rs (fix any it reports without changing behaviour, and mention it).

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/tasks.rs app/src-tauri/src/lib.rs
git commit -m "Tasks: read scheduled task definitions and their runs from Claude logs"
```

---

### Task 2: The board

**Files:**
- Modify: `app/src-tauri/src/tasks.rs`
- Create: `app/src-tauri/examples/tasks.rs`

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests` in `app/src-tauri/src/tasks.rs`:

```rust
    fn def(name: &str) -> TaskDef {
        TaskDef { name: name.into(), description: Some(format!("{name} description")), file: format!("/u/{name}/SKILL.md"), body: String::new() }
    }
    fn run(task: &str, start: i64, outcome: Outcome) -> Run {
        Run { task: task.into(), session_id: format!("{task}-{start}"), cwd: Some("/w".into()), start, end: start + 180_000, outcome }
    }
    /// Local midnight `days` days ago, plus `hour` hours.
    fn local(days: i64, hour: i64) -> i64 {
        let today = chrono::Local::now().date_naive();
        let d = today - chrono::Days::new(days as u64);
        d.and_hms_opt(0, 0, 0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().timestamp_millis() + hour * H
    }

    #[test]
    fn daily_task_on_time() {
        let runs: Vec<Run> = (0..7).map(|d| run("eod", local(d, 8), Outcome::Done)).collect();
        let b = board(&[def("eod")], &runs, local(0, 12));
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].state, "ok");
        assert_eq!(b[0].days, vec!["ran"; 7]);
        let c = b[0].cadence_ms.unwrap();
        assert!((23 * H..=25 * H).contains(&c), "about a day");
        assert_eq!(b[0].last.as_ref().unwrap().start, local(0, 8));
    }

    #[test]
    fn daily_task_that_stopped_running_is_missed() {
        let runs: Vec<Run> = (2..7).map(|d| run("eod", local(d, 8), Outcome::Done)).collect();
        let b = board(&[def("eod")], &runs, local(0, 12));
        assert_eq!(b[0].state, "missed");
        assert_eq!(b[0].days, vec!["ran", "ran", "ran", "ran", "ran", "missed", "missed"]);
    }

    #[test]
    fn failed_stopped_running_and_never() {
        let runs = vec![
            run("a", local(1, 8), Outcome::Done),
            run("a", local(0, 8), Outcome::Failed),
            run("b", local(0, 8), Outcome::Stopped),
            run("c", local(0, 11), Outcome::Running),
        ];
        let b = board(&[def("a"), def("b"), def("c"), def("d")], &runs, local(0, 12));
        let state = |n: &str| b.iter().find(|r| r.name == n).unwrap().state;
        assert_eq!(state("a"), "failed");
        assert_eq!(state("b"), "stopped");
        assert_eq!(state("c"), "running");
        assert_eq!(state("d"), "never");
        assert_eq!(b.iter().find(|r| r.name == "a").unwrap().days[6], "failed");
        // Problems first, then running, then fine, then never ran.
        assert_eq!(b.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn runs_of_tasks_no_longer_defined_still_show() {
        let b = board(&[], &[run("old", local(0, 8), Outcome::Done)], local(0, 12));
        assert_eq!(b[0].name, "old");
        assert_eq!(b[0].description, None);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd app/src-tauri && cargo test --lib tasks::`
Expected: compile errors (`board` missing).

- [ ] **Step 3: Implement**

Add to `app/src-tauri/src/tasks.rs` above `#[cfg(test)]`:

```rust
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskRow {
    pub name: String,
    pub description: Option<String>,
    /// "missed", "failed", "stopped", "running", "ok" or "never".
    pub state: &'static str,
    pub last: Option<Run>,
    /// Median gap between starts, when there are at least two runs.
    pub cadence_ms: Option<i64>,
    /// Last 7 local days, oldest first: "ran", "failed", "missed" or "none".
    pub days: Vec<&'static str>,
}

const ORDER: [&str; 6] = ["failed", "stopped", "missed", "running", "ok", "never"];

fn local_day(ms: i64) -> chrono::NaiveDate {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ms).single().map_or_else(|| chrono::Local::now().date_naive(), |t| t.date_naive())
}

/// One row per task (defined or seen running), problems first.
pub fn board(defs: &[TaskDef], runs: &[Run], now: i64) -> Vec<TaskRow> {
    let mut names: Vec<String> = defs.iter().map(|d| d.name.clone()).collect();
    for r in runs {
        if !names.contains(&r.task) {
            names.push(r.task.clone());
        }
    }
    let today = local_day(now);
    let mut rows: Vec<TaskRow> = names
        .into_iter()
        .map(|name| {
            let mut mine: Vec<&Run> = runs.iter().filter(|r| r.task == name).collect();
            mine.sort_by_key(|r| r.start);
            let mut gaps: Vec<i64> = mine.windows(2).map(|w| w[1].start - w[0].start).collect();
            gaps.sort_unstable();
            let cadence_ms = (!gaps.is_empty()).then(|| gaps[gaps.len() / 2]);
            let last = mine.last().map(|r| (*r).clone());
            let state = match &last {
                None => "never",
                Some(l) if l.outcome == Outcome::Running => "running",
                Some(l) if l.outcome == Outcome::Failed => "failed",
                Some(l) if l.outcome == Outcome::Stopped => "stopped",
                Some(l) if cadence_ms.is_some_and(|c| now - l.start > c * 3 / 2) => "missed",
                Some(_) => "ok",
            };
            let first_day = mine.first().map(|r| local_day(r.start));
            let daily = cadence_ms.is_some_and(|c| c <= 26 * 3_600_000);
            let days = (0..7)
                .rev()
                .map(|back| {
                    let day = today - chrono::Days::new(back);
                    let that: Vec<&&Run> = mine.iter().filter(|r| local_day(r.start) == day).collect();
                    if that.iter().any(|r| matches!(r.outcome, Outcome::Done | Outcome::Running)) {
                        "ran"
                    } else if !that.is_empty() {
                        "failed"
                    } else if daily && first_day.is_some_and(|f| day >= f) && (day < today || state == "missed") {
                        "missed"
                    } else {
                        "none"
                    }
                })
                .collect();
            let description = defs.iter().find(|d| d.name == name).and_then(|d| d.description.clone());
            TaskRow { name, description, state, last, cadence_ms, days }
        })
        .collect();
    rows.sort_by(|a, b| {
        let rank = |s: &str| ORDER.iter().position(|o| *o == s).unwrap_or(ORDER.len());
        rank(a.state).cmp(&rank(b.state)).then_with(|| a.name.cmp(&b.name))
    });
    rows
}
```

Note on `daily_task_that_stopped_running_is_missed`: today has no run and the state is missed, so today's mark is "missed"; yesterday has no run, so it is "missed" too.

Create `app/src-tauri/examples/tasks.rs`:

```rust
//! Prints the Tasks board, for checking by hand.
//! cargo run --release --example tasks
use agent_island_lib::{logs, tasks};

fn main() {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("no HOME"));
    let roots = logs::Roots::default_for(&home);
    let now = logs::now_ms();
    let t = std::time::Instant::now();
    let defs = tasks::definitions(&home.join(".claude/scheduled-tasks"));
    let runs = tasks::runs(&roots.claude, now - 14 * 86_400_000, now, 5 * 60_000);
    let b = tasks::board(&defs, &runs, now);
    eprintln!("{} tasks, {} runs in {:.2}s", b.len(), runs.len(), t.elapsed().as_secs_f64());
    for r in &b {
        println!("{:<28} {:<8} {:?} cadence {:?}h", r.name, r.state, r.days, r.cadence_ms.map(|c| c / 3_600_000));
    }
}
```

- [ ] **Step 4: Run to verify they pass, then on real data**

Run: `cd app/src-tauri && cargo test --lib tasks:: && cargo clippy --lib 2>&1 | grep -A3 tasks.rs && cargo run -q --release --example tasks`
Expected: all tasks tests pass, no clippy output for tasks.rs; the example prints one line per task (on this machine: `personal-products-digest`, `plugin-radar-daily`, `tts-eod-digest`) with states and day marks in well under a second. Record the output (names, states, timing) in the report; don't print log contents.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/tasks.rs app/src-tauri/examples/tasks.rs
git commit -m "Tasks: board with state, cadence and the last seven days"
```

---

### Task 3: Notifications for task problems

**Files:**
- Modify: `app/src-tauri/src/tasks.rs`, `app/src-tauri/src/settings.rs`

- [ ] **Step 1: Write the failing test**

Add inside `mod tests`:

```rust
    #[test]
    fn notifies_new_problems_once_and_not_what_was_wrong_at_start() {
        let mut w = TaskWatch::default();
        let row = |name: &str, state: &'static str, start: i64| TaskRow { name: name.into(), description: None, state, last: Some(run(name, start, Outcome::Done)), cadence_ms: None, days: vec![] };
        // First look: remember, don't notify.
        assert!(w.new_problems(&[row("a", "missed", 1)]).is_empty());
        // Same problem again: nothing.
        assert!(w.new_problems(&[row("a", "missed", 1)]).is_empty());
        // A new problem on another task, and a fresh failure of "a": both once.
        let notes = w.new_problems(&[row("a", "failed", 2), row("b", "stopped", 5), row("c", "ok", 5)]);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].0, "a failed");
        assert_eq!(notes[1].0, "b stopped mid-run");
        assert!(w.new_problems(&[row("a", "failed", 2), row("b", "stopped", 5)]).is_empty());
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd app/src-tauri && cargo test --lib tasks::tests::notifies`
Expected: compile error (`TaskWatch` missing).

- [ ] **Step 3: Implement**

Add to `app/src-tauri/src/tasks.rs` above `#[cfg(test)]`:

```rust
/// Remembers which task problems were already reported, so each new one is
/// notified once. Problems present at the first look are only remembered.
#[derive(Default)]
pub struct TaskWatch {
    started: bool,
    told: std::collections::HashSet<(String, &'static str, i64)>,
}

impl TaskWatch {
    /// (title, body) for each problem not seen before.
    pub fn new_problems(&mut self, rows: &[TaskRow]) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for r in rows {
            if !matches!(r.state, "missed" | "failed" | "stopped") {
                continue;
            }
            let key = (r.name.clone(), r.state, r.last.as_ref().map_or(0, |l| l.start));
            if self.told.insert(key) && self.started {
                out.push(match r.state {
                    "missed" => (format!("{} missed a run", r.name), "It hasn't run when it usually does.".to_string()),
                    "failed" => (format!("{} failed", r.name), "Its last run ended with an error or a usage limit.".to_string()),
                    _ => (format!("{} stopped mid-run", r.name), "It may be waiting for an approval in Claude.".to_string()),
                });
            }
        }
        self.started = true;
        out
    }
}
```

In `app/src-tauri/src/settings.rs` add to `Settings` after `notify_limits`:

```rust
    /// Notify when a scheduled task misses a run, fails or gets stuck.
    pub notify_missed_tasks: bool,
```

Update `Default` to include `notify_missed_tasks: true,` and the test literal in `round_trip_and_defaults` to include `notify_missed_tasks: false,`.

- [ ] **Step 4: Run to verify it passes**

Run: `cd app/src-tauri && cargo test --lib`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/tasks.rs app/src-tauri/src/settings.rs
git commit -m "Tasks: notify once per new missed, failed or stuck run; setting to turn it off"
```

---

### Task 4: Commands and the background check

**Files:**
- Modify: `app/src-tauri/src/lib.rs`

- [ ] **Step 1: Add the cached board and commands**

In `app/src-tauri/src/lib.rs`, after the `resume_session` command, add:

```rust
/// The Tasks board, cached for a minute (it reads the start and end of two
/// weeks of logs).
fn task_board(r: &logs::Roots, home: &std::path::Path) -> (Vec<tasks::TaskDef>, Vec<tasks::Run>, Vec<tasks::TaskRow>) {
    type Board = (i64, Vec<tasks::TaskDef>, Vec<tasks::Run>, Vec<tasks::TaskRow>);
    static CACHE: Mutex<Option<Board>> = Mutex::new(None);
    let now = logs::now_ms();
    if let Ok(c) = CACHE.lock() {
        if let Some((at, d, runs, rows)) = c.as_ref() {
            if now - at < 60_000 {
                return (d.clone(), runs.clone(), rows.clone());
            }
        }
    }
    let defs = tasks::definitions(&home.join(".claude/scheduled-tasks"));
    let runs = tasks::runs(&r.claude, now - 14 * 86_400_000, now, 5 * 60_000);
    let rows = tasks::board(&defs, &runs, now);
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((now, defs.clone(), runs.clone(), rows.clone()));
    }
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
    if name.is_empty() || name.len() > 80 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("That is not a task name.".into());
    }
    let r = roots(&app)?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let pick = app.state::<Prefs>().0.lock().ok().and_then(|p| p.terminal.clone());
    let scripts = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("scripts");
    tauri::async_runtime::spawn_blocking(move || {
        let (defs, runs, _) = task_board(&r, &home);
        let def = defs.iter().find(|d| d.name == name).ok_or("That task is no longer defined.")?;
        let cwd = runs.iter().filter(|x| x.task == name).max_by_key(|x| x.start).and_then(|x| x.cwd.clone()).unwrap_or_else(|| home.to_string_lossy().into_owned());
        let prompt = format!("<scheduled-task name=\"{}\" file=\"{}\">\n{}", def.name, def.file, def.body);
        let claude = recap::find_claude(&home).ok_or("Could not find the claude command.")?;
        terminal::open_command(pick.as_deref(), std::path::Path::new(&cwd), &claude, &[prompt], &format!("task-{}", name.replace('_', "-")), &scripts)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens a task's last run (`claude --resume`) in a terminal tab.
#[tauri::command]
async fn open_task_run(app: AppHandle, name: String) -> Result<(), String> {
    if name.is_empty() || name.len() > 80 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
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
```

Check `terminal::open_command`'s current signature in `app/src-tauri/src/terminal.rs` before using it; the call above assumes `(pick, dir, program, args, name, script_dir)` as merged in phase 3. The script name must be letters, digits and dashes, which is why `_` in a task name is replaced with `-` there.

Add `tasks_board, run_task, open_task_run` to `tauri::generate_handler![...]` after `resume_session`.

- [ ] **Step 2: Background check**

Add a function next to `check_limit_notifications`:

```rust
fn check_task_notifications(app: &AppHandle, watch: &Mutex<tasks::TaskWatch>) {
    use tauri_plugin_notification::NotificationExt;
    let on = app.state::<Prefs>().0.lock().map(|p| p.notify_missed_tasks).unwrap_or(false);
    let (Ok(r), Ok(home)) = (roots(app), app.path().home_dir()) else { return };
    let rows = task_board(&r, &home).2;
    let Ok(mut w) = watch.lock() else { return };
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
```

In the background loop in `run()`, create `let task_watch = std::sync::Arc::new(Mutex::new(tasks::TaskWatch::default()));` next to `notifier`, and inside `if tick % 5 == 0 { … }` add after the limit check:

```rust
                        let (h, w) = (handle.clone(), task_watch.clone());
                        let _ = tauri::async_runtime::spawn_blocking(move || check_task_notifications(&h, &w)).await;
```

- [ ] **Step 3: Build, test, lint**

Run: `cd app/src-tauri && cargo build --lib && cargo test --lib && cargo clippy --lib 2>&1 | grep -E "src/(lib|tasks)\.rs" -A3`
Expected: builds; all pass; no new clippy warnings (pre-existing in lib.rs: `compute_limits` type complexity, the unbounded `for` loop).

- [ ] **Step 4: Commit**

```bash
git add app/src-tauri/src/lib.rs
git commit -m "Commands: tasks_board, run_task, open_task_run; background task check"
```

---

### Task 5: Row text

**Files:**
- Create: `app/ui/tasks.js`
- Create: `app/test/tasks.test.mjs`

- [ ] **Step 1: Write the failing tests**

Create `app/test/tasks.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { taskView, tasksHaveProblems } from '../ui/tasks.js';

const H = 3_600_000;
const NOW = Date.parse('2026-10-04T12:00:00');
const last = (o) => ({ sessionId: 's', cwd: '/w', start: NOW - 4 * H, end: NOW - 4 * H + 3 * 60_000, outcome: 'done', ...o });
const row = (o) => ({ name: 'eod-digest', description: 'Daily digest', state: 'ok', last: last(), cadenceMs: 24 * H, days: ['ran', 'ran', 'ran', 'ran', 'ran', 'ran', 'ran'], ...o });

test('a task that ran: time and duration, daily, done dot', () => {
  const v = taskView(row(), NOW);
  assert.equal(v.dot, 'done');
  assert.match(v.right, / · 3m$/);
  assert.equal(v.line, 'Daily · last ran today');
  assert.deepEqual(v.actions, { runNow: false, lastRun: true });
  assert.deepEqual(v.days, ['ran', 'ran', 'ran', 'ran', 'ran', 'ran', 'ran']);
});

test('problems: missed, failed, stopped get a red dot and Run now', () => {
  for (const [state, right] of [['missed', 'Missed'], ['failed', 'Failed'], ['stopped', 'Stopped']]) {
    const v = taskView(row({ state, last: last({ start: NOW - 30 * H }) }), NOW);
    assert.equal(v.dot, 'failed');
    assert.equal(v.right, right);
    assert.equal(v.line, 'Daily · last ran yesterday');
    assert.equal(v.actions.runNow, true);
  }
  assert.equal(taskView(row({ state: 'stopped' }), NOW).note, 'It may be waiting for an approval in Claude.');
});

test('running, never ran, weekly and irregular', () => {
  assert.deepEqual(
    (({ dot, right }) => ({ dot, right }))(taskView(row({ state: 'running', last: last({ start: NOW - 4 * 60_000, outcome: 'running' }) }), NOW)),
    { dot: 'needs', right: 'Running · 4m' },
  );
  const never = taskView(row({ state: 'never', last: null, cadenceMs: null, days: Array(7).fill('none') }), NOW);
  assert.deepEqual([never.dot, never.right, never.line], ['', 'Never ran', 'Daily digest']);
  assert.deepEqual(never.actions, { runNow: true, lastRun: false });
  assert.equal(taskView(row({ cadenceMs: 7 * 24 * H, last: last({ start: NOW - 3 * 24 * H }) }), NOW).line, 'Weekly · last ran 3 days ago');
  assert.equal(taskView(row({ cadenceMs: 6 * H }), NOW).line, 'Every 6 h · last ran today');
  assert.equal(taskView(row({ cadenceMs: null }), NOW).line, 'Last ran today');
});

test('tab dot when any task has a problem', () => {
  assert.equal(tasksHaveProblems([row(), row({ state: 'running' })]), false);
  assert.equal(tasksHaveProblems([row(), row({ state: 'missed' })]), true);
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd app && node --test test/tasks.test.mjs`
Expected: FAIL, module not found.

- [ ] **Step 3: Implement**

Create `app/ui/tasks.js`:

```js
// The Tasks tab: one row per scheduled Claude task. Problems (missed, failed,
// stopped) get a red dot and Run now; the dots are the last seven days.

const H = 3_600_000;
const PROBLEM = new Set(['missed', 'failed', 'stopped']);
const RIGHT = { missed: 'Missed', failed: 'Failed', stopped: 'Stopped', never: 'Never ran' };
const NOTE = {
  stopped: 'It may be waiting for an approval in Claude.',
  failed: 'Its last run ended with an error or a usage limit.',
  missed: "It hasn't run when it usually does.",
};

function dur(ms) {
  const m = Math.max(1, Math.round(ms / 60_000));
  return m < 60 ? `${m}m` : `${Math.round(m / 60)}h`;
}

function startOfDay(t) {
  const d = new Date(t);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function dayWord(t, now) {
  const days = Math.round((startOfDay(now) - startOfDay(t)) / (24 * H));
  return days <= 0 ? 'today' : days === 1 ? 'yesterday' : `${days} days ago`;
}

function cadenceWord(ms) {
  if (!ms) return '';
  if (ms >= 20 * H && ms <= 28 * H) return 'Daily';
  if (ms >= 6 * 24 * H && ms <= 8 * 24 * H) return 'Weekly';
  return `Every ${Math.max(1, Math.round(ms / H))} h`;
}

const clock = (t) => new Date(t).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });

export function taskView(r, now) {
  const dot = PROBLEM.has(r.state) ? 'failed' : r.state === 'running' ? 'needs' : r.state === 'ok' ? 'done' : '';
  let right = RIGHT[r.state];
  if (r.state === 'running') right = `Running · ${dur(now - r.last.start)}`;
  if (r.state === 'ok') right = `${clock(r.last.start)} · ${dur(r.last.end - r.last.start)}`;
  let line;
  if (!r.last) line = r.description || '';
  else {
    const c = cadenceWord(r.cadenceMs);
    line = c ? `${c} · last ran ${dayWord(r.last.start, now)}` : `Last ran ${dayWord(r.last.start, now)}`;
  }
  return {
    name: r.name,
    dot,
    right,
    line,
    note: NOTE[r.state] || null,
    days: r.days,
    actions: { runNow: PROBLEM.has(r.state) || r.state === 'never', lastRun: !!r.last },
  };
}

export function tasksHaveProblems(rows) {
  return rows.some((r) => PROBLEM.has(r.state));
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd app && npm test`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add app/ui/tasks.js app/test/tasks.test.mjs
git commit -m "Tasks: row text, dots and actions"
```

---

### Task 6: The Tasks tab

**Files:**
- Modify: `app/ui/icons.js`, `app/test/icons.test.mjs`, `app/ui/index.html`, `app/ui/panel.css`, `app/ui/panel.js`

- [ ] **Step 1: Icon**

In `app/ui/icons.js` add to `PATHS`:

```js
  cal: '<rect x="3.5" y="5" width="17" height="15" rx="2"/><path d="M3.5 10h17M8 3v4M16 3v4"/>',
  history: '<path d="M3 12a9 9 0 1 0 3-6.7"/><path d="M3 4v4h4"/><path d="M12 8v4l3 2"/>',
```

Add `'cal', 'history'` to the names in `app/test/icons.test.mjs`. Run `cd app && npm test` → pass.

- [ ] **Step 2: Markup**

In `app/ui/index.html`:

Add the view after the Repos view section (before the Wrapped view):

```html
<section class="view" id="view-tasks" hidden>
  <p class="sub">Scheduled Claude tasks · last 7 days</p>
  <ul class="rows" id="tasks"></ul>
  <div class="empty" id="tasks-empty" hidden>
    <span data-icon="cal"></span>
    <b>No scheduled tasks</b>
    <span>Tasks you schedule in the Claude app show up here, with whether each one ran.</span>
  </div>
</section>
```

In the tab bar, insert between the Repos and Wrapped buttons:

```html
  <button role="tab" data-view="tasks" aria-selected="false"><span data-icon="cal"></span>Tasks<span class="tdot failed" id="tdot" hidden></span></button>
```

In the Settings view, after the Limit notifications row and its `notify-msg` paragraph, add:

```html
  <label class="set-row">
    <span><b>Task notifications</b><small>When a scheduled task misses a run, fails or gets stuck.</small></span>
    <input type="checkbox" id="notify-tasks">
  </label>
```

- [ ] **Step 3: Styles**

Append to `app/ui/panel.css`:

```css
/* Tasks */
.tdot.failed { background: var(--failed); }
.daymarks { grid-column: 1 / -1; display: flex; gap: 4px; padding: 3px 0 0 14px; }
.daymarks i { width: 7px; height: 7px; border-radius: 50%; background: var(--quiet); }
.daymarks i.ran { background: var(--done); }
.daymarks i.failed { background: var(--failed); }
.daymarks i.missed { background: none; box-shadow: inset 0 0 0 1.4px var(--failed); }
#tasks .note-line { grid-column: 1 / -1; padding-left: 14px; font-size: 11.5px; color: var(--meta); }
```

(`.row-actions` and its `.btn` styles from phase 2 are reused.)

- [ ] **Step 4: Wiring**

In `app/ui/panel.js`:

1. Import: `import { taskView, tasksHaveProblems } from './tasks.js';`
2. Tabs and titles:
   - `const TABS = ['waiting', 'today', 'repos', 'tasks', 'wrapped'];`
   - add `tasks: 'Tasks'` to `TITLES`.
3. In `setView`, before the final `return load();`, add: `if (v === 'tasks') return loadTasks();`
4. In `refresh()`, add: `if (view === 'tasks') return loadTasks();`
5. In the `panel-shown` listener, add `loadTasks();` (always: it keeps the Tasks tab dot current; the backend caches for a minute).
6. Add after the Repos section of the file:

```js
// --- Tasks ---

let taskRows = [];
let tasksSeq = 0;

async function loadTasks() {
  const seq = ++tasksSeq;
  try {
    const rows = await invoke('tasks_board');
    if (seq !== tasksSeq) return;
    taskRows = rows;
    updatedAt.tasks = Date.now();
  } catch (e) {
    if (view === 'tasks') note(`Could not read scheduled tasks: ${e}`);
    return;
  }
  $('tdot').hidden = !tasksHaveProblems(taskRows);
  renderTasks();
}

function renderTasks() {
  const now = Date.now();
  $('tasks-empty').hidden = taskRows.length > 0;
  $('tasks').replaceChildren(
    ...taskRows.map((r) => {
      const v = taskView(r, now);
      const li = el('li', 'dotted');
      const name = el('span', 'name');
      name.append(el('span', v.dot ? `dot ${v.dot}` : 'dot'), el('span', '', v.name));
      li.append(name, el('span', 'right num', v.right), el('span', 'line', v.line));
      const marks = el('span', 'daymarks');
      marks.setAttribute('aria-label', `Last 7 days: ${v.days.join(', ')}`);
      for (const d of v.days) marks.append(el('i', d));
      li.append(marks);
      if (v.note) li.append(el('span', 'note-line', v.note));
      if (v.actions.runNow || v.actions.lastRun) {
        const box = el('div', 'row-actions');
        if (v.actions.runNow) box.append(actionButton('Run now', 'play', 'btn', () => taskAction('run_task', r.name)));
        if (v.actions.lastRun) box.append(actionButton('Last run', 'history', 'btn ghost', () => taskAction('open_task_run', r.name)));
        li.append(box);
      }
      return li;
    }),
  );
}

async function taskAction(cmd, name) {
  try {
    await invoke(cmd, { name });
    window.__TAURI__.window.getCurrentWindow().hide();
  } catch (e) {
    note(String(e));
  }
}
```

(`actionButton` exists from phase 2; `play` icon exists from phase 3.)

7. Settings: in `showSettings`, add `$('notify-tasks').checked = prefs.notifyMissedTasks;`, and after the `notify` change listener add:

```js
$('notify-tasks').addEventListener('change', (e) => saveSettings({ notifyMissedTasks: e.target.checked }));
```

Also add `notifyMissedTasks: true` to the initial `prefs` object literal at the top of the file.

- [ ] **Step 5: Checks and commit**

```bash
cd app && cp ui/panel.js /tmp/p.mjs && node --check /tmp/p.mjs && comm -23 <(grep -o "\$('[a-z0-9-]*')" ui/panel.js | sed "s/\$('//;s/')//" | sort -u) <(grep -o 'id="[a-z0-9-]*"' ui/index.html | sed 's/id="//;s/"//' | sort -u) && npm test
```

Expected: syntax OK, cross-check prints nothing, tests pass.

```bash
git add app/ui/icons.js app/test/icons.test.mjs app/ui/index.html app/ui/panel.css app/ui/panel.js
git commit -m "Tasks tab: state, last seven days, Run now and Last run; tab dot; setting"
```

---

### Task 7: Preview and checks

**Files:**
- Modify: `app/scripts/preview.mjs`

- [ ] **Step 1: Stubs**

In the stub `handlers`, add after `resume_session: () => null,`:

```js
  tasks_board: () => {
    const H = 3600000, now = Date.now();
    const last = (h, outcome) => ({ sessionId: 's', cwd: '/tmp', start: now - h * H, end: now - h * H + 180000, outcome });
    return [
      { name: 'personal-products-digest', description: 'Daily product digest', state: 'missed', last: last(28, 'done'), cadenceMs: 24 * H, days: ['ran', 'ran', 'ran', 'ran', 'ran', 'ran', 'missed'] },
      { name: 'plugin-radar-daily', description: 'Plugin radar', state: 'stopped', last: last(6, 'stopped'), cadenceMs: 24 * H, days: ['none', 'none', 'none', 'none', 'ran', 'ran', 'failed'] },
      { name: 'tts-eod-digest', description: 'EoD digest', state: 'ok', last: last(3, 'done'), cadenceMs: 24 * H, days: ['ran', 'ran', 'ran', 'ran', 'ran', 'ran', 'ran'] },
    ];
  },
  run_task: () => null,
  open_task_run: () => null,
```

Also add `notifyMissedTasks: true` to the stubbed `get_settings` result.

Commit:

```bash
git add app/scripts/preview.mjs
git commit -m "Preview: stubs for the Tasks tab"
```

- [ ] **Step 2: Visual check**

`cd app && PORT=5175 npm run preview`, open `http://localhost:5175`, dark and light:
- Five tabs; key 4 opens Tasks; Tasks has a red dot.
- Rows in order: personal-products-digest (red dot, "Missed", "Daily · last ran yesterday", 7 marks ending in a red ring, note line, Run now + Last run), plugin-radar-daily ("Stopped", note "It may be waiting for an approval in Claude."), tts-eod-digest (green dot, "<time> · 3m", seven green marks, Last run only).
- Settings shows "Task notifications" checked.

- [ ] **Step 3: Real-app check (macOS)**

Build and install as before (`npx tauri build --bundles app`, replace `/Applications/Agent Island.app`, remove the build copy so only one app exists). Open Tasks: states should match `cargo run --release --example tasks`. Try Last run on a task (a terminal tab resumes it) and, only with the person's go-ahead, Run now (it starts a real run).

---

### Task 8: PR into dev

- [ ] **Step 1: All suites**

```bash
cd app && npm test && cd src-tauri && cargo test --lib && cd ../../wrapped && npm test
```

- [ ] **Step 2: Push and open the PR**

```bash
git branch --show-current   # feat/tasks-tab
git push -u origin feat/tasks-tab
gh pr create --base dev --title "Panel redesign, phase 4: Tasks tab" --body "Phase 4 of docs/superpowers/specs/2026-10-04-panel-redesign-design.md.

- New Tasks tab (key 4): one row per scheduled Claude task — Missed, Failed, Stopped (stuck mid-run, often waiting for an approval), Running, or when it last ran and for how long — with how it's scheduled and a dot per day for the last 7 days. Problems first.
- Run now (missed, failed, stopped or never ran) starts the task in a terminal tab with the same prompt the Claude app sends, in the folder it last ran in. Last run resumes its last session.
- Red dot on the tab when a task has a problem; optional notification once per new problem (Settings → Task notifications; problems already present at launch aren't announced).
- Definitions from ~/.claude/scheduled-tasks; runs from the start and end of two weeks of Claude logs, cached for a minute. Cadence is inferred from past runs (missed = 1.5× the usual gap).
- Tests: Rust for definitions, run detection and outcomes, board states and day marks, notification de-duplication; JS for row text."
```

The user merges the PR.

---

## Self-review notes

- Spec coverage: Tasks tab with state, schedule line, seven day marks, problems first (Tasks 2, 5, 6); Run now and Last run (Tasks 4, 6); tab dot (Task 6); missed rule (Task 2); failed rule (Task 1); notify missed tasks setting (Tasks 3, 4, 6). The spec's "Stopped" state is defined here from real logs (runs ending mid-step).
- Names: Rust `TaskDef{name, description, file, body}`, `Run{task, sessionId, cwd, start, end, outcome}`, `Outcome` serialised as `"done" | "failed" | "stopped" | "running"`, `TaskRow{name, description, state, last, cadenceMs, days}` — the same names `ui/tasks.js` and the preview stub use. Commands `tasks_board`, `run_task({name})`, `open_task_run({name})`. Ids `view-tasks`, `tasks`, `tasks-empty`, `tdot`, `notify-tasks`.
