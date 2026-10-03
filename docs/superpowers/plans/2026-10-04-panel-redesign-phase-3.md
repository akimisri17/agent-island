# Panel Redesign, Phase 3 (Resume After the Limit) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a Claude limit resets, the Waiting tab lists the sessions it cut off (and sessions the person interrupted and never went back to), each with **Resume**, which jumps to the session if it is still running or opens a terminal tab running `claude --resume <id>` in its folder.

**Architecture:** A new Rust module `cutoff.rs` reads only the tail of each Claude log touched in the last 24 hours and decides, per file, whether its last meaningful event is a limit rejection (whose reset has passed) or an interruption, with no human prompt after it. Because resuming or forking a session copies earlier lines (same `uuid`) into a new file, a candidate counts as resolved when another recent file contains that event's `uuid` and has a human prompt after it. `terminal.rs` gains `open_command` to run a program in a new terminal tab. Two new Tauri commands: `cut_off` and `resume_session`. In the panel, a pure `ui/cutoff.js` formats the section (tested); `panel.js` renders it under the waiting list with Resume and Dismiss.

**Tech Stack:** Rust (serde_json, std::fs), Tauri 2, vanilla JS, `node --test`, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-10-04-panel-redesign-design.md` → Waiting tab, Actions (Resume), New data (Cut off by the limit).

**What the logs look like (checked on real data):**
- A limit cut-off is an assistant line with `"isApiErrorMessage": true`, `"error": "rate_limit"`, `"quotaLimits": {"status": "rejected", "resetsAt": <unix s>, "rateLimitType": "five_hour" | "seven_day" | …}` and text like "You've hit your session limit · resets 4:20pm".
- An interruption is a user line whose content text is `[Request interrupted by user]` or `[Request interrupted by user for tool use]`.
- A human prompt is a user line that the existing `logs::is_human_prompt` rules accept (origin human, not a scheduled task, not tool results, …). This plan reuses that function.
- Lines carry `uuid`, `timestamp`, `sessionId`, `cwd`; titles come from `custom-title` / `agent-name` lines.

**Scope notes:**
- The section shows only sessions whose cut-off happened in the last 24 hours. Limit cut-offs show only after `resetsAt` has passed (before that, Resume would just hit the limit again).
- Codex and Cursor sessions are out of scope for this phase (their logs have no equivalent resume command flow yet).
- Dismissed rows are remembered per viewer in `localStorage` (a convenience; losing it just shows the rows again).

---

## Branch

```bash
cd /Users/akhilmisri/Business/Products/agent-island
git fetch -q
git worktree add -b feat/resume ../agent-island-resume origin/dev
cd ../agent-island-resume
```

Work only in that worktree. Never commit to `main` or `dev`. No Claude attribution lines in commits.

## File structure

| File | Responsibility |
|---|---|
| `app/src-tauri/src/logs.rs` | Make `is_human_prompt` and `CLine` usable from `cutoff.rs` (`pub(crate)`) |
| `app/src-tauri/src/cutoff.rs` (new) | Tail-read recent Claude logs; find cut-off sessions; resolve forks |
| `app/src-tauri/src/terminal.rs` | + `open_command`: run a program in a new terminal tab |
| `app/src-tauri/src/lib.rs` | + commands `cut_off`, `resume_session` |
| `app/ui/cutoff.js` (new) | Section heading and row text; dismiss filtering |
| `app/test/cutoff.test.mjs` (new) | Tests for the above |
| `app/ui/index.html`, `panel.css`, `panel.js` | Section in Waiting, Resume and Dismiss |
| `app/scripts/preview.mjs` | Stubs |

---

### Task 1: Share the human-prompt rule

**Files:**
- Modify: `app/src-tauri/src/logs.rs`

- [ ] **Step 1: Widen visibility**

In `app/src-tauri/src/logs.rs`:
- change `struct CLine<'a> {` to `pub(crate) struct CLine<'a> {`
- change `fn is_human_prompt(d: &CLine) -> bool {` to `pub(crate) fn is_human_prompt(d: &CLine) -> bool {`

Rust requires the fields used by other modules to be visible too. Make these `CLine` fields `pub(crate)`: `kind`, `timestamp`, `uuid`, `cwd`, `custom_title`, `agent_name`, `quota_limits`, `is_sidechain`, `message`. Make `Quota`'s fields `status`, `rate_limit_type`, `resets_at` and the struct itself `pub(crate)`; make `CMsg` and its `content` field `pub(crate)`; make `Block` and its `kind` and `text` fields `pub(crate)`.

- [ ] **Step 2: Build and test**

Run: `cd app/src-tauri && cargo test --lib`
Expected: all pass (no behaviour change).

- [ ] **Step 3: Commit**

```bash
git add app/src-tauri/src/logs.rs
git commit -m "Logs: share the Claude line type and human-prompt rule within the crate"
```

---

### Task 2: Find cut-off sessions

**Files:**
- Create: `app/src-tauri/src/cutoff.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `pub mod cutoff;` in alphabetical order)

- [ ] **Step 1: Write the failing tests**

Create `app/src-tauri/src/cutoff.rs` with only the tests:

```rust
//! Sessions stopped by a usage limit (or interrupted) that nobody went back
//! to: the "Cut off" section of the Waiting tab.

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const NOW: i64 = 1_790_000_000_000; // ms
    fn ts(ms: i64) -> String {
        chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }
    fn human(uuid: &str, at: i64, text: &str) -> String {
        format!(r#"{{"type":"user","uuid":"{uuid}","timestamp":"{}","sessionId":"s","cwd":"/w/shop","origin":{{"kind":"human"}},"message":{{"role":"user","content":"{text}"}}}}"#, ts(at))
    }
    fn assistant(uuid: &str, at: i64) -> String {
        format!(r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{}","sessionId":"s","cwd":"/w/shop","message":{{"id":"m{uuid}","model":"claude-opus-5","content":[{{"type":"text","text":"ok"}}]}}}}"#, ts(at))
    }
    fn limit(uuid: &str, at: i64, resets_s: i64) -> String {
        format!(r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{}","sessionId":"s","cwd":"/w/shop","isApiErrorMessage":true,"error":"rate_limit","quotaLimits":{{"status":"rejected","resetsAt":{resets_s},"rateLimitType":"five_hour"}},"message":{{"id":"x","model":"<synthetic>","content":[{{"type":"text","text":"You've hit your session limit"}}]}}}}"#, ts(at))
    }
    fn interrupted(uuid: &str, at: i64) -> String {
        format!(r#"{{"type":"user","uuid":"{uuid}","timestamp":"{}","sessionId":"s","cwd":"/w/shop","message":{{"role":"user","content":[{{"type":"text","text":"[Request interrupted by user]"}}]}}}}"#, ts(at))
    }
    fn title(t: &str) -> String {
        format!(r#"{{"type":"custom-title","customTitle":"{t}","sessionId":"s"}}"#)
    }
    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ai-cutoff-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("proj")).unwrap();
        d
    }
    fn write(d: &std::path::Path, name: &str, lines: &[String]) {
        let mut f = std::fs::File::create(d.join("proj").join(name)).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }
    const H: i64 = 3_600_000;

    #[test]
    fn limit_after_reset_is_cut_off() {
        let d = dir("limit");
        write(&d, "aaaa-1111.jsonl", &[title("Repo board"), human("u1", NOW - 3 * H, "go"), assistant("a1", NOW - 3 * H + 1000), limit("l1", NOW - 2 * H, (NOW - H) / 1000)]);
        let r = find(&d, NOW);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].session_id, "aaaa-1111");
        assert_eq!(r[0].kind, "limit");
        assert_eq!(r[0].limit_type.as_deref(), Some("five_hour"));
        assert_eq!(r[0].title.as_deref(), Some("Repo board"));
        assert_eq!(r[0].cwd, "/w/shop");
        assert_eq!(r[0].project.as_deref(), Some("shop"));
        assert_eq!(r[0].at, NOW - 2 * H);
        assert_eq!(r[0].resets_at, Some(NOW - H));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn limit_not_yet_reset_waits() {
        let d = dir("pending");
        write(&d, "bbbb.jsonl", &[human("u1", NOW - H, "go"), limit("l1", NOW - H / 2, (NOW + H) / 1000)]);
        assert!(find(&d, NOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_prompt_after_the_limit_means_you_went_back() {
        let d = dir("back");
        write(&d, "cccc.jsonl", &[human("u1", NOW - 3 * H, "go"), limit("l1", NOW - 2 * H, (NOW - H) / 1000), human("u2", NOW - H / 2, "Try again")]);
        assert!(find(&d, NOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn interrupted_and_left_is_cut_off() {
        let d = dir("int");
        write(&d, "dddd.jsonl", &[human("u1", NOW - 2 * H, "go"), assistant("a1", NOW - 2 * H + 5000), interrupted("i1", NOW - 2 * H + 9000)]);
        let r = find(&d, NOW);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, "interrupted");
        assert_eq!(r[0].resets_at, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn older_than_a_day_is_ignored() {
        let d = dir("old");
        write(&d, "eeee.jsonl", &[human("u1", NOW - 30 * H, "go"), limit("l1", NOW - 26 * H, (NOW - 25 * H) / 1000)]);
        assert!(find(&d, NOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resumed_in_a_new_file_is_resolved() {
        let d = dir("fork");
        let orig = [human("u1", NOW - 3 * H, "go"), limit("l1", NOW - 2 * H, (NOW - H) / 1000)];
        write(&d, "ffff-orig.jsonl", &orig);
        let mut copy = orig.to_vec();
        copy.push(human("u9", NOW - H / 2, "continue"));
        write(&d, "ffff-copy.jsonl", &copy);
        assert!(find(&d, NOW).is_empty(), "the copy has a prompt after the cut-off line");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn subagent_logs_are_ignored() {
        let d = dir("sub");
        std::fs::create_dir_all(d.join("proj/gggg/subagents")).unwrap();
        write(&d, "gggg/subagents/agent-1.jsonl", &[human("u1", NOW - 3 * H, "go"), limit("l1", NOW - 2 * H, (NOW - H) / 1000)]);
        assert!(find(&d, NOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
```

Add `pub mod cutoff;` to `app/src-tauri/src/lib.rs` (alphabetical among the `pub mod` lines).

- [ ] **Step 2: Run to verify they fail**

Run: `cd app/src-tauri && cargo test --lib cutoff::`
Expected: compile error, `find` not found.

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]` in `app/src-tauri/src/cutoff.rs`:

```rust
use crate::logs::{is_human_prompt, project_of, CLine};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const WINDOW_MS: i64 = 24 * 3_600_000;
/// Only the end of each log is read: the events that matter are the last ones.
const TAIL_BYTES: u64 = 256 * 1024;
const INTERRUPTED: [&str; 2] = ["[Request interrupted by user]", "[Request interrupted by user for tool use]"];

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CutOff {
    pub session_id: String,
    pub title: Option<String>,
    pub project: Option<String>,
    pub cwd: String,
    /// "limit" or "interrupted".
    pub kind: &'static str,
    /// e.g. "five_hour", for limit cut-offs.
    pub limit_type: Option<String>,
    /// When it was cut off, ms.
    pub at: i64,
    /// When the limit reset, ms (limit cut-offs only).
    pub resets_at: Option<i64>,
}

/// What the end of one log says.
struct Tail {
    path: PathBuf,
    mtime: i64,
    session_id: String,
    title: Option<String>,
    cwd: Option<String>,
    last_human: Option<i64>,
    /// The last cut-off event: (uuid, at, kind, limit type, resets at).
    cut: Option<(String, i64, &'static str, Option<String>, Option<i64>)>,
}

fn read_tail(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    let mut s = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        // The first line is probably cut in half.
        s = s.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default();
    }
    Some(s)
}

fn block_text(d: &CLine) -> Option<String> {
    let raw = d.message.as_ref()?.content?;
    if raw.get().starts_with('"') {
        return serde_json::from_str::<String>(raw.get()).ok();
    }
    let blocks: Vec<crate::logs::Block> = serde_json::from_str(raw.get()).ok()?;
    blocks.into_iter().find(|b| b.kind.as_deref() == Some("text")).and_then(|b| b.text)
}

fn parse_ts(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}

fn tail_of(path: &Path, mtime: i64) -> Option<Tail> {
    let text = read_tail(path)?;
    let session_id = path.file_stem()?.to_string_lossy().into_owned();
    let mut t = Tail { path: path.to_path_buf(), mtime, session_id, title: None, cwd: None, last_human: None, cut: None };
    for line in text.lines() {
        let Ok(mut d) = serde_json::from_str::<CLine>(line) else { continue };
        if d.kind == Some("custom-title") {
            t.title = d.custom_title.take().filter(|s| !s.is_empty()).or(t.title.take());
        }
        if d.kind == Some("agent-name") && t.title.is_none() {
            t.title = d.agent_name.take().filter(|s| !s.is_empty());
        }
        let Some(at) = d.timestamp.and_then(parse_ts) else { continue };
        if d.cwd.is_some() {
            t.cwd = d.cwd.clone();
        }
        let uuid = d.uuid.clone().unwrap_or_default();
        if let Some(q) = d.quota_limits.as_ref().filter(|q| q.status.as_deref() == Some("rejected")) {
            let resets = q.resets_at.map(|s| (s * 1000.0) as i64);
            t.cut = Some((uuid, at, "limit", q.rate_limit_type.clone(), resets));
            continue;
        }
        if d.kind == Some("user") && d.is_sidechain != Some(true) {
            if block_text(&d).is_some_and(|x| INTERRUPTED.contains(&x.trim())) {
                t.cut = Some((uuid, at, "interrupted", None, None));
            } else if is_human_prompt(&d) {
                t.last_human = Some(at);
            }
        }
    }
    Some(t)
}

fn recent_logs(dir: &Path, since: i64, out: &mut Vec<(PathBuf, i64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if p.file_name().is_some_and(|n| n == "subagents") {
                continue;
            }
            recent_logs(&p, since, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            let mtime = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
            if mtime >= since {
                out.push((p, mtime));
            }
        }
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Cut-off sessions under `projects` (~/.claude/projects) as of `now` (ms),
/// newest first.
pub fn find(projects: &Path, now: i64) -> Vec<CutOff> {
    let since = now - WINDOW_MS;
    let mut files = Vec::new();
    recent_logs(projects, since, &mut files);
    let tails: Vec<Tail> = files.iter().filter_map(|(p, m)| tail_of(p, *m)).collect();

    let mut out = Vec::new();
    for t in &tails {
        let Some((uuid, at, kind, limit_type, resets_at)) = &t.cut else { continue };
        if *at < since || t.last_human.is_some_and(|h| h > *at) {
            continue;
        }
        if resets_at.is_some_and(|r| r > now) {
            continue;
        }
        // Resumed or forked: another log copied this line and went on.
        let needle = format!("\"uuid\":\"{uuid}\"");
        let resolved = tails.iter().any(|o| {
            o.path != t.path
                && o.mtime >= *at
                && o.last_human.is_some_and(|h| h > *at)
                && std::fs::read(&o.path).is_ok_and(|b| contains(&b, needle.as_bytes()))
        });
        if resolved {
            continue;
        }
        let cwd = t.cwd.clone().unwrap_or_default();
        out.push(CutOff {
            session_id: t.session_id.clone(),
            title: t.title.clone(),
            project: project_of(&cwd),
            cwd,
            kind,
            limit_type: limit_type.clone(),
            at: *at,
            resets_at: *resets_at,
        });
    }
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out
}
```

Note: `logs::Block` must be `pub(crate)` (Task 1). `project_of` is already `pub`.

- [ ] **Step 4: Run to verify they pass**

Run: `cd app/src-tauri && cargo test --lib cutoff:: && cargo clippy --lib 2>&1 | grep -A3 cutoff.rs`
Expected: 7 pass; no clippy output for cutoff.rs.

- [ ] **Step 5: Check on real data**

Add `app/src-tauri/examples/cutoff.rs`:

```rust
//! Prints the cut-off sessions for the last 24 hours, for checking by hand.
//! cargo run --release --example cutoff
use agent_island_lib::{cutoff, logs};

fn main() {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).expect("no home dir");
    let roots = logs::Roots::default_for(std::path::Path::new(&home));
    let t = std::time::Instant::now();
    let r = cutoff::find(&roots.claude, logs::now_ms());
    eprintln!("{} cut off in {:.2}s", r.len(), t.elapsed().as_secs_f64());
    println!("{}", serde_json::to_string_pretty(&r).unwrap());
}
```

Run: `cd app/src-tauri && cargo run -q --release --example cutoff`
Expected: runs in well under a second; output is a JSON list (possibly empty). Spot-check one entry, if any, by opening its log and confirming the last events. Record the count and timing in the report.

- [ ] **Step 6: Commit**

```bash
git add app/src-tauri/src/cutoff.rs app/src-tauri/src/lib.rs app/src-tauri/examples/cutoff.rs
git commit -m "Cut off: find sessions stopped by a limit or interrupted and not resumed"
```

---

### Task 3: Run a command in a new terminal tab

**Files:**
- Modify: `app/src-tauri/src/terminal.rs`

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests` in `app/src-tauri/src/terminal.rs`:

```rust
    #[test]
    fn shell_quoting_survives_any_folder_name() {
        assert_eq!(sh_quote("/w/my shop"), "'/w/my shop'");
        assert_eq!(sh_quote("/w/it's"), r"'/w/it'\''s'");
    }

    #[test]
    fn command_launch_per_app() {
        let dir = Path::new("/w/my shop");
        let prog = Path::new("/u/.local/bin/claude");
        let args = vec!["--resume".to_string(), "ab-12".to_string()];
        if cfg!(target_os = "macos") {
            match command_launch("Ghostty", dir, prog, &args) {
                Launch::Exec(p, a) => {
                    assert_eq!(p, "open");
                    assert_eq!(a, vec!["-na", "Ghostty", "--args", "--working-directory=/w/my shop", "-e", "/u/.local/bin/claude", "--resume", "ab-12"]);
                }
                Launch::Script { .. } => panic!("Ghostty runs the program directly"),
            }
            for app in ["Terminal", "iTerm", "Warp"] {
                match command_launch(app, dir, prog, &args) {
                    Launch::Script { app: opener, body } => {
                        assert_eq!(opener, if app == "iTerm" { "iTerm" } else { "Terminal" }, "Warp can't run a script; Terminal does");
                        assert_eq!(body, "#!/bin/sh\ncd '/w/my shop' && exec '/u/.local/bin/claude' '--resume' 'ab-12'\n");
                    }
                    Launch::Exec(..) => panic!("{app} runs a .command script"),
                }
            }
        } else if cfg!(windows) {
            match command_launch("Windows Terminal", dir, prog, &args) {
                Launch::Exec(p, a) => {
                    assert_eq!(p, "wt");
                    assert_eq!(a, vec!["-d", "/w/my shop", "/u/.local/bin/claude", "--resume", "ab-12"]);
                }
                Launch::Script { .. } => panic!(),
            }
        }
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd app/src-tauri && cargo test --lib terminal::`
Expected: compile errors for `sh_quote`, `command_launch`, `Launch`.

- [ ] **Step 3: Implement**

Add to `app/src-tauri/src/terminal.rs`, above `#[cfg(test)]`:

```rust
/// How to start a program in a new terminal tab.
#[derive(Debug)]
pub enum Launch {
    /// Run this program with these arguments (no shell).
    Exec(String, Vec<String>),
    /// Write `body` to an executable .command file and open it with `app`.
    Script { app: String, body: String },
}

/// Single-quotes a string for /bin/sh.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// How `app` runs `program args…` in `dir`. Ghostty takes the program on its
/// command line; Terminal and iTerm run a .command script; Warp cannot run a
/// script, so Terminal does it instead.
pub fn command_launch(app: &str, dir: &Path, program: &Path, args: &[String]) -> Launch {
    let d = dir.to_string_lossy().into_owned();
    let p = program.to_string_lossy().into_owned();
    if cfg!(windows) {
        let mut a = vec!["-d".to_string(), d, p];
        a.extend(args.iter().cloned());
        return Launch::Exec("wt".into(), a);
    }
    if app == "Ghostty" {
        let mut a = vec!["-na".to_string(), "Ghostty".into(), "--args".into(), format!("--working-directory={d}"), "-e".into(), p];
        a.extend(args.iter().cloned());
        return Launch::Exec("open".into(), a);
    }
    let quoted: Vec<String> = std::iter::once(sh_quote(&p)).chain(args.iter().map(|a| sh_quote(a))).collect();
    let body = format!("#!/bin/sh\ncd {} && exec {}\n", sh_quote(&d), quoted.join(" "));
    let opener = if app == "iTerm" { "iTerm" } else { "Terminal" };
    Launch::Script { app: opener.into(), body }
}

/// Opens a new terminal tab in `dir` running `program args…`. `name` names
/// the script file when one is needed (letters, digits and dashes only).
pub fn open_command(pick: Option<&str>, dir: &Path, program: &Path, args: &[String], name: &str) -> Result<(), String> {
    if !dir.is_absolute() || !dir.is_dir() {
        return Err("That folder no longer exists.".into());
    }
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Bad script name.".into());
    }
    let app = choose(pick, &installed()).ok_or("No supported terminal app found.")?;
    match command_launch(&app, dir, program, args) {
        Launch::Exec(prog, a) => {
            let mut cmd = Command::new(&prog);
            cmd.args(&a);
            if cfg!(windows) {
                cmd.spawn().map(|_| ()).map_err(|e| format!("Could not open {app}: {e}"))
            } else {
                let ok = cmd.status().map_err(|e| format!("Could not open {app}: {e}"))?;
                if ok.success() { Ok(()) } else { Err(format!("Could not open {app}.")) }
            }
        }
        Launch::Script { app: opener, body } => {
            let dir = std::env::temp_dir().join("agent-island");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let script = dir.join(format!("{name}.command"));
            std::fs::write(&script, body).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
            }
            let ok = Command::new("open").args(["-a", &opener]).arg(&script).status().map_err(|e| format!("Could not open {opener}: {e}"))?;
            if ok.success() { Ok(()) } else { Err(format!("Could not open {opener}.")) }
        }
    }
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd app/src-tauri && cargo test --lib terminal:: && cargo clippy --lib 2>&1 | grep -A3 terminal.rs`
Expected: all pass, no clippy output for terminal.rs.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/terminal.rs
git commit -m "Terminal: run a program in a new tab (Ghostty directly, Terminal and iTerm via a script)"
```

---

### Task 4: Commands

**Files:**
- Modify: `app/src-tauri/src/lib.rs`

- [ ] **Step 1: Add the commands**

In `app/src-tauri/src/lib.rs`, after the `open_terminal` command:

```rust
/// Sessions cut off by a limit (now reset) or interrupted, last 24 hours.
#[tauri::command]
async fn cut_off(app: AppHandle) -> Result<Vec<cutoff::CutOff>, String> {
    let r = roots(&app)?;
    tauri::async_runtime::spawn_blocking(move || cutoff::find(&r.claude, logs::now_ms())).await.map_err(|e| e.to_string())
}

/// Resumes a Claude session: jumps to it if it is still running, otherwise
/// opens a terminal tab running `claude --resume <id>` in its folder.
#[tauri::command]
async fn resume_session(app: AppHandle, session_id: String, cwd: String) -> Result<(), String> {
    if session_id.is_empty() || session_id.len() > 64 || !session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("That is not a session id.".into());
    }
    let r = roots(&app)?;
    let pick = app.state::<Prefs>().0.lock().ok().and_then(|p| p.terminal.clone());
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let live = live::live_sessions(&r);
        if let Some(s) = live.iter().find(|s| s.session_id == session_id) {
            return live::jump(s);
        }
        let claude = recap::find_claude(&home).ok_or("Could not find the claude command.")?;
        terminal::open_command(pick.as_deref(), std::path::Path::new(&cwd), &claude, &["--resume".to_string(), session_id.clone()], &format!("resume-{session_id}"))
    })
    .await
    .map_err(|e| e.to_string())?
}
```

Add `cut_off, resume_session` to `tauri::generate_handler![...]` after `open_terminal`.

- [ ] **Step 2: Build, test, lint**

Run: `cd app/src-tauri && cargo build --lib && cargo test --lib && cargo clippy --lib 2>&1 | grep -E "src/(lib|cutoff|terminal)\.rs" -A3`
Expected: builds, all tests pass, no new clippy warnings (pre-existing in lib.rs: `type_complexity` in `compute_limits`, the unbounded `for` loop).

- [ ] **Step 3: Commit**

```bash
git add app/src-tauri/src/lib.rs
git commit -m "Commands: cut_off and resume_session"
```

---

### Task 5: Section text

**Files:**
- Create: `app/ui/cutoff.js`
- Create: `app/test/cutoff.test.mjs`

- [ ] **Step 1: Write the failing tests**

Create `app/test/cutoff.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cutoffSection } from '../ui/cutoff.js';

const NOW = Date.parse('2026-10-04T12:00:00Z');
const M = 60_000;
const c = (o) => ({ sessionId: 's1', title: 'Repo board', project: 'agent-island', cwd: '/w/agent-island', kind: 'limit', limitType: 'five_hour', at: NOW - 90 * M, resetsAt: NOW - 3 * M, ...o });

test('limit cut-offs: heading says when the limit reset', () => {
  const s = cutoffSection([c()], { now: NOW, dismissed: new Set() });
  assert.equal(s.heading, 'Cut off by the limit · reset 3m ago');
  assert.deepEqual(s.rows, [{ id: 's1', cwd: '/w/agent-island', title: 'Repo board', line: 'agent-island · stopped by the 5-hour limit' }]);
});

test('interruptions and mixed lists', () => {
  const s = cutoffSection([c({ sessionId: 's2', kind: 'interrupted', limitType: null, resetsAt: null, title: null, project: 'erp-backend' })], { now: NOW, dismissed: new Set() });
  assert.equal(s.heading, 'Stopped mid-task');
  assert.deepEqual(s.rows[0], { id: 's2', cwd: '/w/agent-island', title: 'erp-backend', line: 'erp-backend · you interrupted it' });
  const mixed = cutoffSection([c(), c({ sessionId: 's2', kind: 'interrupted', resetsAt: null })], { now: NOW, dismissed: new Set() });
  assert.equal(mixed.heading, 'Cut off by the limit · reset 3m ago');
  assert.equal(mixed.rows.length, 2);
});

test('weekly limits and unknown types read plainly', () => {
  assert.match(cutoffSection([c({ limitType: 'seven_day' })], { now: NOW, dismissed: new Set() }).rows[0].line, /weekly limit$/);
  assert.match(cutoffSection([c({ limitType: 'opus_x' })], { now: NOW, dismissed: new Set() }).rows[0].line, /stopped by a usage limit$/);
});

test('dismissed sessions are hidden; nothing left means no section', () => {
  assert.equal(cutoffSection([c()], { now: NOW, dismissed: new Set(['s1']) }), null);
  assert.equal(cutoffSection([], { now: NOW, dismissed: new Set() }), null);
});
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd app && node --test test/cutoff.test.mjs`
Expected: FAIL, module not found.

- [ ] **Step 3: Implement**

Create `app/ui/cutoff.js`:

```js
// The "Cut off" section of the Waiting tab: sessions a usage limit stopped
// (shown once the limit has reset) or that the person interrupted, and that
// nobody went back to.

const LIMIT_NAME = { five_hour: '5-hour limit', seven_day: 'weekly limit', seven_day_opus: 'weekly Opus limit' };

function ago(ms) {
  const m = Math.max(1, Math.round(ms / 60_000));
  if (m < 60) return `${m}m ago`;
  return `${Math.round(m / 60)}h ago`;
}

export function cutoffSection(list, { now, dismissed }) {
  const shown = list.filter((c) => !dismissed.has(c.sessionId));
  if (!shown.length) return null;
  const resets = shown.filter((c) => c.kind === 'limit' && c.resetsAt).map((c) => c.resetsAt);
  const heading = resets.length ? `Cut off by the limit · reset ${ago(now - Math.max(...resets))}` : 'Stopped mid-task';
  const rows = shown.map((c) => {
    const why = c.kind === 'limit'
      ? (LIMIT_NAME[c.limitType] ? `stopped by the ${LIMIT_NAME[c.limitType]}` : 'stopped by a usage limit')
      : 'you interrupted it';
    const title = c.title || c.project || 'Untitled session';
    return { id: c.sessionId, cwd: c.cwd, title, line: [c.project, why].filter(Boolean).join(' · ') };
  });
  return { heading, rows };
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd app && npm test`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add app/ui/cutoff.js app/test/cutoff.test.mjs
git commit -m "Waiting: text for the cut-off section"
```

---

### Task 6: Section in the Waiting tab

**Files:**
- Modify: `app/ui/index.html`, `app/ui/panel.css`, `app/ui/icons.js`, `app/test/icons.test.mjs`, `app/ui/panel.js`

- [ ] **Step 1: Icons**

In `app/ui/icons.js` add to `PATHS`:

```js
  play: '<path d="M7 5l12 7-12 7z"/>',
  close: '<path d="M6 6l12 12M18 6L6 18"/>',
```

Add `'play', 'close'` to the names array in `app/test/icons.test.mjs`. Run `cd app && npm test` → pass.

- [ ] **Step 2: Markup**

In `app/ui/index.html`, in the Waiting view, insert right after the `live-empty` div (before the Working group):

```html
  <div id="cutoff" hidden>
    <p class="sec" id="cutoff-head"></p>
    <ul class="rows" id="cutoff-list"></ul>
  </div>
```

- [ ] **Step 3: Styles**

Append to `app/ui/panel.css`:

```css
/* Cut off: sessions to pick back up. */
#cutoff-list li { grid-template-columns: minmax(0, 1fr) auto auto; }
#cutoff-list .line { grid-column: 1 / -1; }
.resume { display: inline-flex; align-items: center; gap: 5px; border: 0; border-radius: 6px; background: var(--fill); color: var(--ink); font-size: 11.5px; font-weight: 650; padding: 3px 9px; }
.resume:disabled { opacity: .45; }
.dismiss { border: 0; background: none; color: var(--meta); padding: 3px; display: flex; border-radius: 5px; }
.dismiss:hover { color: var(--ink); background: var(--fill); }
```

- [ ] **Step 4: Wiring**

In `app/ui/panel.js`:

Add the import:

```js
import { cutoffSection } from './cutoff.js';
```

Add after the `loadLive` function:

```js
// --- Cut off ---

let cutoffs = [];
function dismissedCutoffs() {
  try {
    return new Set(JSON.parse(localStorage.getItem('dismissedCutoffs') || '[]'));
  } catch {
    return new Set();
  }
}
function dismissCutoff(id) {
  const d = dismissedCutoffs();
  d.add(id);
  // Keep only ids that could still show, so the list stays small.
  const keep = [...d].filter((x) => x === id || cutoffs.some((c) => c.sessionId === x));
  try {
    localStorage.setItem('dismissedCutoffs', JSON.stringify(keep));
  } catch {
    // not remembered; hidden until the next read
  }
  cutoffs = cutoffs.filter((c) => c.sessionId !== id);
  renderCutoff();
}

async function loadCutoff() {
  try {
    cutoffs = await invoke('cut_off');
  } catch {
    cutoffs = []; // a nice-to-have; Waiting still works
  }
  renderCutoff();
}

function renderCutoff() {
  const s = cutoffSection(cutoffs, { now: Date.now(), dismissed: dismissedCutoffs() });
  $('cutoff').hidden = !s;
  if (!s) return;
  $('cutoff-head').textContent = s.heading;
  $('cutoff-list').replaceChildren(
    ...s.rows.map((r) => {
      const li = el('li', 'dotted');
      const name = el('span', 'name');
      name.append(el('span', 'dot'), el('span', '', r.title));
      const resume = el('button', 'resume');
      resume.innerHTML = icon('play', 's');
      resume.append(el('span', '', 'Resume'));
      resume.title = 'Jump to it if it is still open, otherwise resume it in a terminal';
      resume.addEventListener('click', async () => {
        resume.disabled = true;
        try {
          await invoke('resume_session', { sessionId: r.id, cwd: r.cwd });
          window.__TAURI__.window.getCurrentWindow().hide();
        } catch (e) {
          note(String(e));
        } finally {
          resume.disabled = false;
        }
      });
      const close = el('button', 'dismiss');
      close.innerHTML = icon('close', 's');
      close.title = 'Hide this one';
      close.setAttribute('aria-label', `Hide ${r.title}`);
      close.addEventListener('click', () => dismissCutoff(r.id));
      li.append(name, resume, close, el('span', 'line', r.line));
      return li;
    }),
  );
}
```

Load it with the waiting list. In `setView`, change:

```js
  if (v === 'waiting') return loadLive(), loadLimits();
```

to:

```js
  if (v === 'waiting') return loadLive(), loadLimits(), loadCutoff();
```

In `refresh()`, change:

```js
  if (view === 'waiting') return loadLive(), loadLimits(true);
```

to:

```js
  if (view === 'waiting') return loadLive(), loadLimits(true), loadCutoff();
```

In the `listen('panel-shown', …)` handler, change:

```js
  if (view === 'waiting') loadLimits();
```

to:

```js
  if (view === 'waiting') loadLimits(), loadCutoff();
```

The "Nothing is waiting on you" empty state must not show when the cut-off section has rows. At the end of `renderCutoff`, and in `loadLive` where `live-empty` is set, use this one rule: `$('live-empty').hidden = $('live').children.length > 0 || !$('cutoff').hidden;`. Concretely:
- in `loadLive`, replace `$('live-empty').hidden = needs.length > 0;` with `$('live-empty').hidden = needs.length > 0 || !$('cutoff').hidden;`
- in `renderCutoff`, as its first line after computing `s` and setting `$('cutoff').hidden`, add `$('live-empty').hidden = $('live').children.length > 0 || !!s;` (before the `if (!s) return;`).

- [ ] **Step 5: Checks and commit**

Run the syntax check, the id cross-check and the tests:

```bash
cd app && cp ui/panel.js /tmp/p.mjs && node --check /tmp/p.mjs && comm -23 <(grep -o "\$('[a-z0-9-]*')" ui/panel.js | sed "s/\$('//;s/')//" | sort -u) <(grep -o 'id="[a-z0-9-]*"' ui/index.html | sed 's/id="//;s/"//' | sort -u) && npm test
```

Expected: syntax OK, cross-check prints nothing, tests pass.

```bash
git add app/ui/index.html app/ui/panel.css app/ui/icons.js app/test/icons.test.mjs app/ui/panel.js
git commit -m "Waiting: cut-off sessions with Resume and Hide"
```

---

### Task 7: Preview and checks

**Files:**
- Modify: `app/scripts/preview.mjs`

- [ ] **Step 1: Stubs**

In `app/scripts/preview.mjs`, inside the stub `handlers`, add after `open_terminal: () => null,`:

```js
  cut_off: () => [
    { sessionId: 'aaaa-1', title: 'Repo board', project: 'agent-island', cwd: '/tmp', kind: 'limit', limitType: 'five_hour', at: Date.now() - 95 * 60000, resetsAt: Date.now() - 4 * 60000 },
    { sessionId: 'bbbb-2', title: null, project: 'erp-backend', cwd: '/tmp', kind: 'interrupted', limitType: null, at: Date.now() - 50 * 60000, resetsAt: null },
  ],
  resume_session: () => null,
```

Commit:

```bash
git add app/scripts/preview.mjs
git commit -m "Preview: stubs for cut-off sessions"
```

- [ ] **Step 2: Visual check**

Run `cd app && PORT=5175 npm run preview`, open `http://localhost:5175`, Waiting tab, dark and light:
- Under the (empty) waiting list, a section headed "Cut off by the limit · reset 4m ago" lists "Repo board" (agent-island · stopped by the 5-hour limit) and "erp-backend" (erp-backend · you interrupted it), each with **Resume** and a ✕.
- "Nothing is waiting on you" is hidden while the section has rows.
- ✕ hides a row; hiding both hides the section and brings back the empty state; reload keeps them hidden.
- Resume shows no error in the preview.

- [ ] **Step 3: Real-app check (macOS)**

In the built or dev app (`npm run tauri dev`): with a session cut off by a limit that has reset (or one you interrupted and closed), press Resume with each installed terminal picked in Settings. Expected: a new tab in the session's folder running `claude --resume <id>`, which opens that conversation. With the session still open in its window, Resume brings that window forward instead. Note any terminal that fails in the PR.

---

### Task 8: PR into dev

- [ ] **Step 1: All suites**

```bash
cd app && npm test && cd src-tauri && cargo test --lib && cd ../../wrapped && npm test
```

- [ ] **Step 2: Push and open the PR**

```bash
git branch --show-current   # feat/resume
git push -u origin feat/resume
gh pr create --base dev --title "Panel redesign, phase 3: resume after the limit" --body "Phase 3 of docs/superpowers/specs/2026-10-04-panel-redesign-design.md.

- Waiting shows a \"Cut off by the limit\" section once a Claude limit resets: sessions it stopped, plus sessions you interrupted and left, from the last 24 hours. Each has Resume and Hide.
- Resume jumps to the session if it is still open; otherwise it opens a terminal tab running \`claude --resume <id>\` in the session's folder (Ghostty directly; Terminal and iTerm via a .command script; Warp falls back to Terminal; Windows Terminal on Windows).
- Detection reads only the tail of recent logs. A session resumed or forked into a new file counts as picked up (the new file copies the cut-off line and has a prompt after it).
- Tests: Rust fixtures for limit, not-yet-reset, prompt after, interruption, older than a day, fork resolution, subagents; quoting and launch commands per terminal; JS section text."
```

The user merges the PR.

---

## Self-review notes

- Spec coverage: "Cut off by the limit" data rule (Task 2: limit rejection or interruption, no human prompt after, limit reset, fork resolution), Resume = jump if running else `claude --resume` in a terminal tab (Tasks 3–4, 6), Windows Terminal (Task 3), section only after reset (Task 2), "you interrupted it" rows (Tasks 2, 5), Dismiss (Task 6).
- Names: `cutoff::find`, `CutOff { session_id, title, project, cwd, kind, limit_type, at, resets_at }` → JSON camelCase `sessionId, title, project, cwd, kind, limitType, at, resetsAt` (used by `cutoffSection` and the preview stub); `terminal::{Launch, sh_quote, command_launch, open_command}`; commands `cut_off`, `resume_session({sessionId, cwd})`; ids `cutoff`, `cutoff-head`, `cutoff-list`.
