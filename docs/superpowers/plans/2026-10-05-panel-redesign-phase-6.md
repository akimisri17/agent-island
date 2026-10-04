# Panel Redesign, Phase 6 (Approval Friction) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In Wrapped, a "Where you approve the most" section: per project, about how many tool calls stopped for the person's approval in the chosen range, on which tools and commands, how long they waited, plus **Allow these…**, which shows a few broad allow-rules (e.g. `Bash(npm run:*)`) and adds the chosen ones to that project's `.claude/settings.local.json` on confirm.

**Architecture:** Claude logs do not record approvals. They do record every tool call (name and input) and the permission mode the session was in (`permissionMode` on each prompt line). A call needed approval when the session was in `default` mode and no allow-rule covered it. A new `perms.rs` reads allow-rules (global `~/.claude/settings.json` and `settings.local.json`, the project's `.claude/settings.json` and `.claude/settings.local.json`) and matches calls against them (exact, `prefix:*`, `*` globs, bare tool names, `domain:` for WebFetch) plus Claude Code's built-in read-only commands. A new `approvals.rs` scans the range's logs and groups the calls that needed approval per project, then proposes prefix rules for repeated Bash commands (never for destructive commands). Two Tauri commands: `approvals(days)` (cached 10 minutes) and `allow_rules(project, rules)` (re-derives the suggestions on the backend and writes only rules from them). In the panel, `ui/approvals.js` formats rows (tested) and Wrapped gets the section and a checkbox sheet.

**Tech Stack:** Rust (serde_json, rayon), Tauri 2, vanilla JS, `node --test`, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-10-04-panel-redesign-design.md` → Wrapped (Where you approve the most), New data (Approval friction), Actions (Allow these…).

**What the data looks like (checked on this machine):**
- `permissionMode` appears on prompt (user) lines: `default` (≈84k), `auto` (≈5k), `acceptEdits`, `bypassPermissions`, `plan`. A session's mode is the one on its latest prompt line before the call.
- Assistant `tool_use` blocks carry `name` and `input` (`input.command` for Bash, `input.url` for WebFetch, `input.file_path` for edits).
- The tool result line follows; the gap between call and result is the wait (approval plus run time, so it is reported as "median wait", not "approval time").
- Global allow-rules here: 455 — 404 exact Bash commands, 43 Bash globs, 3 Read globs, 3 WebFetch domains, 2 Skill. Exact single-command rules don't generalise, which is the friction this phase surfaces.
- Short affirmations ("go ahead", "yes please", "continue") are ~5% of typed prompts; counted per project as a second signal.

**Decisions:**
- Tools that need approval in `default` mode: `Bash`, `Edit`, `Write`, `MultiEdit`, `NotebookEdit`, `WebFetch`, `WebSearch`, and any `mcp__*` tool. In `acceptEdits` mode, only the non-edit ones. In `auto`, `bypassPermissions` and `plan` mode, none (counted as zero).
- Built-in read-only Bash commands are treated as allowed: `ls`, `pwd`, `cat`, `head`, `tail`, `wc`, `echo`, `which`, `grep`, `rg`, `find` (without `-exec`/`-delete`), `git status|diff|log|show|branch`. A command containing `&&`, `;`, `|` or `>` is checked by its first segment only and is never auto-allowed when any segment is not read-only.
- The count is labelled as an estimate ("about 41").
- Suggested rules: for Bash commands that needed approval 3+ times in a project, group by their first two words (first word alone for single-word commands) and suggest `Bash(<words>:*)`. Never suggest for: `rm`, `sudo`, `chmod`, `chown`, `dd`, `mkfs`, `kill`, `pkill`, `killall`, `shutdown`, `reboot`, `curl`, `wget`, `ssh`, `scp`, `git push`, `git reset`, `git clean`, `git checkout`, `git rebase`, `docker rm`, `docker system`. For WebFetch, suggest `WebFetch(domain:<host>)` for hosts fetched 3+ times. At most 6 suggestions per project.
- Rules are written to `<project>/.claude/settings.local.json` (`permissions.allow`, merged, de-duplicated, other keys untouched), where Claude Code reads per-person project rules. The sheet shows the exact rules and the file before anything is written.

---

## Branch

```bash
cd /Users/akhilmisri/Business/Products/agent-island
git fetch -q
git worktree add -b feat/approvals ../agent-island-approvals origin/dev
cd ../agent-island-approvals
```

Never commit to `main`. No Claude attribution lines in commits.

## File structure

| File | Responsibility |
|---|---|
| `app/src-tauri/src/perms.rs` (new) | Read allow-rules; match a tool call against them and the built-in read-only commands; merge rules into a settings file |
| `app/src-tauri/src/approvals.rs` (new) | Scan logs for calls that needed approval; group per project; suggest rules |
| `app/src-tauri/src/lib.rs` | Modules; commands `approvals`, `allow_rules` |
| `app/src-tauri/examples/approvals.rs` (new) | Print counts per project, for checking by hand |
| `app/ui/approvals.js` (new), `app/test/approvals.test.mjs` (new) | Row text |
| `app/ui/index.html`, `panel.css`, `panel.js` | Wrapped section and the Allow sheet |
| `app/scripts/preview.mjs` | Stubs |

---

### Task 1: Allow-rules and matching

**Files:**
- Create: `app/src-tauri/src/perms.rs`
- Modify: `app/src-tauri/src/lib.rs` (`pub mod perms;`, alphabetical)

- [ ] **Step 1: Write the failing tests**

Create `app/src-tauri/src/perms.rs` with only:

```rust
//! Claude Code allow-rules: reading them, deciding whether a tool call was
//! already allowed, and adding rules to a project's local settings.

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rules(v: &[&str]) -> Rules {
        Rules::parse(v.iter().map(|s| s.to_string()))
    }

    #[test]
    fn bash_rule_shapes() {
        let r = rules(&["Bash(npm run test)", "Bash(docker compose:*)", "Bash(gh pr view *)"]);
        assert!(r.allows("Bash", &json!({"command": "npm run test"})));
        assert!(!r.allows("Bash", &json!({"command": "npm run build"})), "exact rule is exact");
        assert!(r.allows("Bash", &json!({"command": "docker compose up -d"})));
        assert!(r.allows("Bash", &json!({"command": "docker compose"})));
        assert!(!r.allows("Bash", &json!({"command": "docker composer"})), "prefix ends at a word");
        assert!(r.allows("Bash", &json!({"command": "gh pr view 12 --json state"})));
        assert!(!r.allows("Bash", &json!({"command": "gh pr merge 12"})));
    }

    #[test]
    fn built_in_read_only_commands() {
        let r = rules(&[]);
        for c in ["ls -la", "git status", "git log --oneline -5", "cat a.txt | head -3", "grep -n x src/a.rs", "find . -name '*.rs'"] {
            assert!(r.allows("Bash", &json!({"command": c})), "{c}");
        }
        for c in ["find . -name x -delete", "git push", "ls && rm -rf x", "cat a > b", "npm test"] {
            assert!(!r.allows("Bash", &json!({"command": c})), "{c}");
        }
    }

    #[test]
    fn other_tools() {
        let r = rules(&["Edit", "WebFetch(domain:github.com)", "mcp__linear__list_issues"]);
        assert!(r.allows("Edit", &json!({"file_path": "/w/a.rs"})));
        assert!(!r.allows("Write", &json!({"file_path": "/w/a.rs"})));
        assert!(r.allows("WebFetch", &json!({"url": "https://github.com/a/b"})));
        assert!(r.allows("WebFetch", &json!({"url": "https://api.github.com/x"})), "subdomains of an allowed domain");
        assert!(!r.allows("WebFetch", &json!({"url": "https://example.com/"})));
        assert!(r.allows("mcp__linear__list_issues", &json!({})));
        assert!(!r.allows("mcp__linear__create_issue", &json!({})));
        assert!(r.allows("Read", &json!({"file_path": "/x"})), "tools that never ask");
    }

    #[test]
    fn reads_rules_from_settings_files() {
        let d = std::env::temp_dir().join(format!("ai-perms-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".claude")).unwrap();
        std::fs::write(d.join(".claude/settings.json"), r#"{"permissions":{"allow":["Bash(make:*)"]}}"#).unwrap();
        std::fs::write(d.join(".claude/settings.local.json"), r#"{"permissions":{"allow":["Bash(cargo test:*)"]},"other":1}"#).unwrap();
        let r = Rules::for_project(&d, &d.join("no-home"));
        assert!(r.allows("Bash", &json!({"command": "make build"})));
        assert!(r.allows("Bash", &json!({"command": "cargo test --lib"})));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn adds_rules_to_local_settings_without_touching_the_rest() {
        let d = std::env::temp_dir().join(format!("ai-perms-add-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".claude")).unwrap();
        std::fs::write(d.join(".claude/settings.local.json"), r#"{"permissions":{"allow":["Bash(make:*)"],"deny":["Bash(rm:*)"]},"env":{"A":"1"}}"#).unwrap();
        let added = add_local_rules(&d, &["Bash(make:*)".into(), "Bash(npm run:*)".into()]).unwrap();
        assert_eq!(added, vec!["Bash(npm run:*)".to_string()]);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(d.join(".claude/settings.local.json")).unwrap()).unwrap();
        assert_eq!(v["permissions"]["allow"], json!(["Bash(make:*)", "Bash(npm run:*)"]));
        assert_eq!(v["permissions"]["deny"], json!(["Bash(rm:*)"]));
        assert_eq!(v["env"]["A"], "1");
        // A new project gets the file created.
        let e = d.join("fresh");
        std::fs::create_dir_all(&e).unwrap();
        assert_eq!(add_local_rules(&e, &["Bash(make:*)".into()]).unwrap().len(), 1);
        assert!(e.join(".claude/settings.local.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
```

Add `pub mod perms;` to `lib.rs`.

- [ ] **Step 2: Run to verify they fail**

Run: `cd app/src-tauri && cargo test --lib perms::` → compile errors.

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]` in `perms.rs`:

```rust
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Tools that ask before running in default mode.
pub fn asks(tool: &str, mode: &str) -> bool {
    let edit = matches!(tool, "Edit" | "Write" | "MultiEdit" | "NotebookEdit");
    let other = matches!(tool, "Bash" | "WebFetch" | "WebSearch") || tool.starts_with("mcp__");
    match mode {
        "default" => edit || other,
        "acceptEdits" => other,
        _ => false,
    }
}

#[derive(Debug, Default)]
pub struct Rules {
    tools: Vec<String>,
    bash_exact: Vec<String>,
    bash_prefix: Vec<String>,
    bash_glob: Vec<String>,
    domains: Vec<String>,
}

const READ_ONLY: [&str; 10] = ["ls", "pwd", "cat", "head", "tail", "wc", "echo", "which", "grep", "rg"];
const READ_ONLY_GIT: [&str; 5] = ["status", "diff", "log", "show", "branch"];

fn read_only(segment: &str) -> bool {
    let words: Vec<&str> = segment.split_whitespace().collect();
    match words.as_slice() {
        [] => true,
        ["git", sub, ..] => READ_ONLY_GIT.contains(sub),
        ["find", rest @ ..] => !rest.iter().any(|w| matches!(*w, "-exec" | "-execdir" | "-delete" | "-ok")),
        [first, ..] => READ_ONLY.contains(first),
    }
}

/// `*` matches any run of characters; the whole command must match.
fn glob(pattern: &str, s: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == s;
    }
    let mut rest = s;
    for (i, p) in parts.iter().enumerate() {
        if i == 0 {
            let Some(r) = rest.strip_prefix(p) else { return false };
            rest = r;
        } else if i == parts.len() - 1 {
            return rest.ends_with(p);
        } else {
            let Some(at) = rest.find(p) else { return false };
            rest = &rest[at + p.len()..];
        }
    }
    true
}

fn host(url: &str) -> Option<String> {
    let after = url.split_once("://")?.1;
    let h = after.split(['/', '?', '#']).next()?.split('@').last()?.split(':').next()?;
    Some(h.to_lowercase()).filter(|h| !h.is_empty())
}

impl Rules {
    pub fn parse(rules: impl IntoIterator<Item = String>) -> Self {
        let mut r = Rules::default();
        for rule in rules {
            let rule = rule.trim().to_string();
            let Some(open) = rule.find('(').filter(|_| rule.ends_with(')')) else {
                r.tools.push(rule);
                continue;
            };
            let (tool, arg) = (&rule[..open], &rule[open + 1..rule.len() - 1]);
            match tool {
                "Bash" => {
                    if let Some(p) = arg.strip_suffix(":*") {
                        r.bash_prefix.push(p.to_string());
                    } else if arg.contains('*') {
                        r.bash_glob.push(arg.to_string());
                    } else {
                        r.bash_exact.push(arg.to_string());
                    }
                }
                "WebFetch" => {
                    if let Some(d) = arg.strip_prefix("domain:") {
                        r.domains.push(d.to_lowercase());
                    }
                }
                _ => {}
            }
        }
        r
    }

    fn from_file(path: &Path) -> Vec<String> {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("permissions")?.get("allow")?.as_array().cloned())
            .map(|a| a.into_iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    /// Global rules (from `home`/.claude) plus the project's own.
    pub fn for_project(project: &Path, home: &Path) -> Self {
        let files: [PathBuf; 4] = [
            home.join(".claude/settings.json"),
            home.join(".claude/settings.local.json"),
            project.join(".claude/settings.json"),
            project.join(".claude/settings.local.json"),
        ];
        Self::parse(files.iter().flat_map(|f| Self::from_file(f)))
    }

    fn bash_allowed(&self, cmd: &str) -> bool {
        let cmd = cmd.trim();
        if self.bash_exact.iter().any(|e| e == cmd) || self.bash_glob.iter().any(|g| glob(g, cmd)) {
            return true;
        }
        if self.bash_prefix.iter().any(|p| cmd == p || cmd.starts_with(&format!("{p} "))) {
            return true;
        }
        let segments: Vec<&str> = cmd.split(['|', ';', '&']).map(str::trim).filter(|s| !s.is_empty()).collect();
        !cmd.contains('>') && !segments.is_empty() && segments.iter().all(|s| read_only(s))
    }

    /// Whether this call ran without asking (allowed by a rule, built in, or
    /// a tool that never asks).
    pub fn allows(&self, tool: &str, input: &Value) -> bool {
        if !asks(tool, "default") {
            return true;
        }
        if self.tools.iter().any(|t| t == tool) {
            return true;
        }
        match tool {
            "Bash" => input.get("command").and_then(Value::as_str).is_some_and(|c| self.bash_allowed(c)),
            "WebFetch" => input
                .get("url")
                .and_then(Value::as_str)
                .and_then(host)
                .is_some_and(|h| self.domains.iter().any(|d| h == *d || h.ends_with(&format!(".{d}")))),
            _ => false,
        }
    }
}

/// Adds `rules` to `<project>/.claude/settings.local.json` (creating it),
/// keeping everything else. Returns the rules that were new.
pub fn add_local_rules(project: &Path, rules: &[String]) -> Result<Vec<String>, String> {
    let dir = project.join(".claude");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("settings.local.json");
    let mut v: Value = match std::fs::read(&file) {
        Ok(b) => serde_json::from_slice(&b).map_err(|_| "That project's settings.local.json isn't valid JSON; fix it first.".to_string())?,
        Err(_) => serde_json::json!({}),
    };
    let obj = v.as_object_mut().ok_or("That project's settings.local.json isn't a JSON object.")?;
    let perms = obj.entry("permissions").or_insert_with(|| serde_json::json!({}));
    let perms = perms.as_object_mut().ok_or("permissions isn't an object.")?;
    let allow = perms.entry("allow").or_insert_with(|| serde_json::json!([]));
    let allow = allow.as_array_mut().ok_or("permissions.allow isn't a list.")?;
    let mut added = Vec::new();
    for r in rules {
        if !allow.iter().any(|x| x.as_str() == Some(r)) {
            allow.push(Value::String(r.clone()));
            added.push(r.clone());
        }
    }
    let tmp = dir.join(format!("settings.local.json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    Ok(added)
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd app/src-tauri && cargo test --lib perms:: && cargo clippy --lib 2>&1 | grep -A3 perms.rs` → all pass, no clippy output for perms.rs.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/perms.rs app/src-tauri/src/lib.rs
git commit -m "Perms: read allow-rules and tell whether a tool call had to ask"
```

---

### Task 2: Calls that needed approval, per project

**Files:**
- Create: `app/src-tauri/src/approvals.rs`, `app/src-tauri/examples/approvals.rs`
- Modify: `app/src-tauri/src/lib.rs` (`pub mod approvals;`)

- [ ] **Step 1: Write the failing tests**

Create `app/src-tauri/src/approvals.rs` with only:

```rust
//! Approval friction: tool calls that stopped for the person's approval,
//! per project, and broad allow-rules that would have covered them.

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const T0: i64 = 1_790_000_000_000;
    fn ts(ms: i64) -> String {
        chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }
    fn prompt(at: i64, mode: &str, text: &str) -> String {
        format!(r#"{{"type":"user","uuid":"p{at}","timestamp":"{}","cwd":"/w/shop","permissionMode":"{mode}","origin":{{"kind":"human"}},"message":{{"role":"user","content":"{text}"}}}}"#, ts(at))
    }
    fn call(at: i64, id: &str, tool: &str, input: &str) -> String {
        format!(r#"{{"type":"assistant","uuid":"a{id}","timestamp":"{}","cwd":"/w/shop","message":{{"model":"claude-opus-5","content":[{{"type":"tool_use","id":"{id}","name":"{tool}","input":{input}}}]}}}}"#, ts(at))
    }
    fn result(at: i64, id: &str) -> String {
        format!(r#"{{"type":"user","uuid":"r{id}","timestamp":"{}","cwd":"/w/shop","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"ok"}}]}},"toolUseResult":{{}}}}"#, ts(at))
    }

    #[test]
    fn counts_calls_that_asked_and_suggests_rules() {
        let d = std::env::temp_dir().join(format!("ai-appr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("proj")).unwrap();
        let mut f = std::fs::File::create(d.join("proj/s.jsonl")).unwrap();
        let mut lines = vec![prompt(T0, "default", "run the tests")];
        for i in 0..4 {
            let at = T0 + 1000 + i * 100_000;
            lines.push(call(at, &format!("b{i}"), "Bash", &format!(r#"{{"command":"npm run test -- --grep {i}"}}"#)));
            lines.push(result(at + 30_000, &format!("b{i}")));
        }
        lines.push(call(T0 + 500_000, "ls", "Bash", r#"{"command":"ls -la"}"#)); // built in: no approval
        lines.push(result(T0 + 500_100, "ls"));
        lines.push(call(T0 + 600_000, "rm", "Bash", r#"{"command":"rm -rf dist"}"#)); // asked, but never suggested
        lines.push(result(T0 + 610_000, "rm"));
        lines.push(call(T0 + 700_000, "e1", "Edit", r#"{"file_path":"/w/shop/a.ts"}"#));
        lines.push(result(T0 + 705_000, "e1"));
        lines.push(prompt(T0 + 800_000, "default", "go ahead"));
        lines.push(prompt(T0 + 900_000, "auto", "now in auto"));
        lines.push(call(T0 + 901_000, "b9", "Bash", r#"{"command":"npm run test"}"#)); // auto mode: no approval
        lines.push(result(T0 + 902_000, "b9"));
        for l in &lines {
            writeln!(f, "{l}").unwrap();
        }
        drop(f);
        let root_of = |cwd: &str| Some(cwd.to_string());
        let rules_for = |_: &str| perms::Rules::parse(Vec::<String>::new());
        let r = scan(&d, T0 - 1, &root_of, &rules_for);
        assert_eq!(r.len(), 1);
        let p = &r[0];
        assert_eq!(p.project, "/w/shop");
        assert_eq!(p.name, "shop");
        assert_eq!(p.asked, 6, "4 npm, 1 rm, 1 Edit");
        assert_eq!(p.affirmations, 1);
        assert_eq!(p.tools, vec![("Bash".to_string(), 5), ("Edit".to_string(), 1)]);
        assert_eq!(p.commands[0], ("npm run".to_string(), 4));
        assert_eq!(p.median_wait_ms, Some(30_000));
        assert_eq!(p.suggestions, vec!["Bash(npm run:*)".to_string()], "rm is never suggested; Edit once is below 3");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn command_group_is_the_first_two_words() {
        assert_eq!(group("npm run test -- -x"), "npm run");
        assert_eq!(group("  make "), "make");
        assert_eq!(group("cd app && cargo test"), "cd app");
    }
}
```

Add `pub mod approvals;` to `lib.rs`.

- [ ] **Step 2: Run to verify they fail**

Run: `cd app/src-tauri && cargo test --lib approvals::` → compile errors.

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]` in `approvals.rs`:

```rust
use crate::perms;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const NEVER: [&str; 22] = [
    "rm", "sudo", "chmod", "chown", "dd", "mkfs", "kill", "pkill", "killall", "shutdown", "reboot", "curl", "wget", "ssh", "scp",
    "git push", "git reset", "git clean", "git checkout", "git rebase", "docker rm", "docker system",
];
const MIN_TIMES: u32 = 3;
const MAX_SUGGESTIONS: usize = 6;
const AFFIRM: [&str; 14] = ["yes", "yep", "y", "ok", "okay", "go ahead", "proceed", "do it", "continue", "sure", "go", "yes please", "go for it", "lgtm"];

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFriction {
    /// Project folder (repository root, or the working directory).
    pub project: String,
    pub name: String,
    /// Tool calls that had to ask (an estimate).
    pub asked: u32,
    /// Most-asked tools, (name, count), most first.
    pub tools: Vec<(String, u32)>,
    /// Most-asked Bash commands by their first two words.
    pub commands: Vec<(String, u32)>,
    /// Median time from call to result for calls that asked.
    pub median_wait_ms: Option<i64>,
    /// Short "go ahead" style replies typed.
    pub affirmations: u32,
    /// Allow-rules that would have covered the repeated ones.
    pub suggestions: Vec<String>,
}

/// First two words of a command (one for single-word commands).
pub fn group(cmd: &str) -> String {
    cmd.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
}

fn never_suggest(g: &str) -> bool {
    NEVER.iter().any(|n| g == *n || g.starts_with(&format!("{n} ")) || g.split_whitespace().next() == Some(n))
}

fn ts_of(d: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(d.get("timestamp")?.as_str()?).ok().map(|t| t.timestamp_millis())
}

fn files_since(dir: &Path, since: i64, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if p.file_name().is_some_and(|n| n == "subagents") {
                continue;
            }
            files_since(&p, since, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            let m = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
            if m >= since {
                out.push(p);
            }
        }
    }
}

/// One asked call: (cwd, tool, input, wait).
type Asked = (String, String, Value, Option<i64>);

/// Calls that asked in one log, plus (cwd, affirmations) counts.
fn in_file(path: &Path, since: i64, rules_for: &(dyn Fn(&str) -> perms::Rules + Sync), root_of: &(dyn Fn(&str) -> Option<String> + Sync)) -> (Vec<Asked>, HashMap<String, u32>) {
    let Ok(text) = std::fs::read_to_string(path) else { return (Vec::new(), HashMap::new()) };
    let mut mode = String::from("default");
    let mut pending: HashMap<String, (String, String, Value, i64)> = HashMap::new();
    let mut asked = Vec::new();
    let mut affirm: HashMap<String, u32> = HashMap::new();
    let mut rules: HashMap<String, perms::Rules> = HashMap::new();
    let mut root: HashMap<String, Option<String>> = HashMap::new();
    for line in text.lines() {
        let Ok(d) = serde_json::from_str::<Value>(line) else { continue };
        if d.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(at) = ts_of(&d) else { continue };
        let cwd = d.get("cwd").and_then(Value::as_str).unwrap_or_default().to_string();
        let kind = d.get("type").and_then(Value::as_str);
        if let Some(m) = d.get("permissionMode").and_then(Value::as_str) {
            mode = m.to_string();
        }
        let content = d.get("message").and_then(|m| m.get("content"));
        if kind == Some("user") {
            if let Some(s) = content.and_then(Value::as_str) {
                if at >= since && d.get("origin").and_then(|o| o.get("kind")).and_then(Value::as_str) == Some("human") {
                    let t = s.trim().trim_end_matches(['.', '!']).to_lowercase();
                    if AFFIRM.contains(&t.as_str()) {
                        *affirm.entry(cwd.clone()).or_default() += 1;
                    }
                }
            }
            for b in content.and_then(Value::as_array).into_iter().flatten() {
                if b.get("type").and_then(Value::as_str) != Some("tool_result") {
                    continue;
                }
                let Some(id) = b.get("tool_use_id").and_then(Value::as_str) else { continue };
                if let Some((c, tool, input, t0)) = pending.remove(id) {
                    asked.push((c, tool, input, Some(at - t0)));
                }
            }
        } else if kind == Some("assistant") && at >= since {
            for b in content.and_then(Value::as_array).into_iter().flatten() {
                if b.get("type").and_then(Value::as_str) != Some("tool_use") {
                    continue;
                }
                let (Some(id), Some(tool)) = (b.get("id").and_then(Value::as_str), b.get("name").and_then(Value::as_str)) else { continue };
                let input = b.get("input").cloned().unwrap_or(Value::Null);
                if !perms::asks(tool, &mode) {
                    continue;
                }
                let proj = root.entry(cwd.clone()).or_insert_with(|| root_of(&cwd)).clone().unwrap_or_else(|| cwd.clone());
                let r = rules.entry(proj.clone()).or_insert_with(|| rules_for(&proj));
                if r.allows(tool, &input) {
                    continue;
                }
                pending.insert(id.to_string(), (proj, tool.to_string(), input, at));
            }
        }
    }
    // Calls with no result (the session ended at the prompt) still asked.
    asked.extend(pending.into_values().map(|(c, t, i, _)| (c, t, i, None)));
    let affirm = affirm.into_iter().map(|(c, n)| (root_of(&c).unwrap_or(c), n)).collect();
    (asked, affirm)
}

/// Approval friction per project since `since`, most-asked first.
pub fn scan(
    projects: &Path,
    since: i64,
    root_of: &(dyn Fn(&str) -> Option<String> + Sync),
    rules_for: &(dyn Fn(&str) -> perms::Rules + Sync),
) -> Vec<ProjectFriction> {
    use rayon::prelude::*;
    let mut files = Vec::new();
    files_since(projects, since, &mut files);
    let parts: Vec<(Vec<Asked>, HashMap<String, u32>)> = files.par_iter().map(|p| in_file(p, since, rules_for, root_of)).collect();

    #[derive(Default)]
    struct Acc {
        asked: u32,
        tools: HashMap<String, u32>,
        commands: HashMap<String, u32>,
        domains: HashMap<String, u32>,
        waits: Vec<i64>,
        affirm: u32,
    }
    let mut by: HashMap<String, Acc> = HashMap::new();
    for (asked, affirm) in parts {
        for (proj, tool, input, wait) in asked {
            let a = by.entry(proj).or_default();
            a.asked += 1;
            *a.tools.entry(tool.clone()).or_default() += 1;
            if let Some(w) = wait {
                a.waits.push(w);
            }
            if tool == "Bash" {
                if let Some(c) = input.get("command").and_then(Value::as_str) {
                    *a.commands.entry(group(c)).or_default() += 1;
                }
            } else if tool == "WebFetch" {
                if let Some(h) = input.get("url").and_then(Value::as_str).and_then(|u| u.split_once("://")).and_then(|(_, r)| r.split('/').next()) {
                    *a.domains.entry(h.to_lowercase()).or_default() += 1;
                }
            }
        }
        for (proj, n) in affirm {
            by.entry(proj).or_default().affirm += n;
        }
    }
    let sorted = |m: HashMap<String, u32>| {
        let mut v: Vec<(String, u32)> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    };
    let mut out: Vec<ProjectFriction> = by
        .into_iter()
        .filter(|(_, a)| a.asked > 0)
        .map(|(project, mut a)| {
            a.waits.sort_unstable();
            let median_wait_ms = (!a.waits.is_empty()).then(|| a.waits[a.waits.len() / 2]);
            let commands = sorted(a.commands);
            let mut suggestions: Vec<String> = commands.iter().filter(|(g, n)| *n >= MIN_TIMES && !g.is_empty() && !never_suggest(g)).map(|(g, _)| format!("Bash({g}:*)")).collect();
            suggestions.extend(sorted(a.domains).into_iter().filter(|(_, n)| *n >= MIN_TIMES).map(|(h, _)| format!("WebFetch(domain:{h})")));
            suggestions.truncate(MAX_SUGGESTIONS);
            let name = crate::logs::basename(&project).unwrap_or_else(|| project.clone());
            ProjectFriction { name, asked: a.asked, tools: sorted(a.tools), commands: commands.into_iter().take(5).collect(), median_wait_ms, affirmations: a.affirm, suggestions, project }
        })
        .collect();
    out.sort_by(|a, b| b.asked.cmp(&a.asked).then(a.name.cmp(&b.name)));
    out
}
```

Create `app/src-tauri/examples/approvals.rs`:

```rust
//! Prints approval friction per project (counts and command groups only).
//! cargo run --release --example approvals -- 30
use agent_island_lib::{approvals, logs, perms, repos};

fn main() {
    let days: i64 = std::env::args().nth(1).and_then(|d| d.parse().ok()).unwrap_or(30);
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("no HOME"));
    let roots = logs::Roots::default_for(&home);
    let t = std::time::Instant::now();
    let root_of = |cwd: &str| repos::root_of(std::path::Path::new(cwd)).map(|p| p.to_string_lossy().into_owned());
    let rules_for = |p: &str| perms::Rules::for_project(std::path::Path::new(p), &home);
    let r = approvals::scan(&roots.claude, logs::now_ms() - days * 86_400_000, &root_of, &rules_for);
    eprintln!("{} projects in {:.2}s", r.len(), t.elapsed().as_secs_f64());
    for p in r.iter().take(8) {
        println!("{:<28} asked {:>4}  wait {:?}s  yes {:>3}  {:?}  -> {:?}", p.name, p.asked, p.median_wait_ms.map(|w| w / 1000), p.affirmations, p.commands.iter().take(3).collect::<Vec<_>>(), p.suggestions);
    }
}
```

- [ ] **Step 4: Run tests, then on real data**

Run: `cd app/src-tauri && cargo test --lib approvals:: perms:: && cargo clippy --lib 2>&1 | grep -A3 -E "approvals.rs|perms.rs"; cargo run -q --release --example approvals -- 30`
Expected: tests pass; no clippy output for these files; the example prints the top projects with counts, command groups and suggested rules within a few seconds. Report the project count, timing and the top 3 rows. Command groups (first two words) are fine to report; do not print full commands.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/approvals.rs app/src-tauri/src/lib.rs app/src-tauri/examples/approvals.rs
git commit -m "Approvals: tool calls that had to ask, per project, with rules that would have covered them"
```

---

### Task 3: Commands

**Files:**
- Modify: `app/src-tauri/src/lib.rs`

- [ ] **Step 1: Add the commands**

After the recipe commands in `lib.rs`:

```rust
/// Approval friction per range (days), cached ten minutes.
type FrictionCache = std::collections::HashMap<u32, (i64, Vec<approvals::ProjectFriction>)>;
static FRICTION_CACHE: Mutex<Option<FrictionCache>> = Mutex::new(None);

fn friction(r: &logs::Roots, home: &std::path::Path, days: u32) -> Vec<approvals::ProjectFriction> {
    let now = logs::now_ms();
    if let Some((at, v)) = FRICTION_CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(&days)) {
        if now - at < 600_000 {
            return v.clone();
        }
    }
    let root_of = |cwd: &str| repos::root_of(std::path::Path::new(cwd)).map(|p| p.to_string_lossy().into_owned());
    let rules_for = |p: &str| perms::Rules::for_project(std::path::Path::new(p), home);
    let v = approvals::scan(&r.claude, now - i64::from(days) * 86_400_000, &root_of, &rules_for);
    FRICTION_CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(days, (now, v.clone()));
    v
}

/// Rules changed: recompute on the next read.
fn clear_friction() {
    *FRICTION_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[tauri::command]
async fn approvals(app: AppHandle, days: u32) -> Result<Vec<approvals::ProjectFriction>, String> {
    let r = roots(&app)?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let days = days.clamp(1, 365);
    tauri::async_runtime::spawn_blocking(move || friction(&r, &home, days)).await.map_err(|e| e.to_string())
}

/// Adds the chosen suggested rules to the project's .claude/settings.local.json.
/// Only rules the app itself suggested for that project are accepted.
#[tauri::command]
async fn allow_rules(app: AppHandle, project: String, rules: Vec<String>, days: u32) -> Result<Vec<String>, String> {
    let r = roots(&app)?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let known = friction(&r, &home, days.clamp(1, 365));
        let p = known.iter().find(|p| p.project == project).ok_or("That project is no longer in the list.")?;
        if rules.is_empty() || rules.iter().any(|x| !p.suggestions.contains(x)) {
            return Err("Only the suggested rules can be added.".into());
        }
        let dir = std::path::Path::new(&project);
        if !dir.is_absolute() || !dir.is_dir() {
            return Err("That project folder no longer exists.".into());
        }
        let added = perms::add_local_rules(dir, &rules)?;
        clear_friction();
        Ok(added)
    })
    .await
    .map_err(|e| e.to_string())?
}
```

Add `approvals, allow_rules` to `generate_handler![...]`.

- [ ] **Step 2: Build, test, lint, commit**

Run: `cd app/src-tauri && cargo build --lib && cargo test --lib && cargo clippy --lib 2>&1 | grep -E "src/(lib|approvals|perms)\.rs" -A3` (only the known lib.rs warnings).

```bash
git add app/src-tauri/src/lib.rs
git commit -m "Commands: approvals and allow_rules"
```

---

### Task 4: Row text

**Files:**
- Create: `app/ui/approvals.js`, `app/test/approvals.test.mjs`

- [ ] **Step 1: Write the failing tests**

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { frictionRows } from '../ui/approvals.js';

const p = (o) => ({ project: '/w/app', name: 'Applications', asked: 41, tools: [['Bash', 38], ['Edit', 3]], commands: [['npm run', 20], ['docker compose', 9], ['git commit', 4]], medianWaitMs: 360000, affirmations: 7, suggestions: ['Bash(npm run:*)', 'Bash(docker compose:*)'], ...o });

test('rows: estimate, what asks, wait, allow button when there are suggestions', () => {
  const [r] = frictionRows([p()]);
  assert.equal(r.name, 'Applications');
  assert.equal(r.right, 'about 41×');
  assert.equal(r.line, 'npm run, docker compose, git commit · 6m median wait');
  assert.equal(r.sub, '7 "go ahead" replies');
  assert.equal(r.canAllow, true);
});

test('non-Bash tools, no wait, no replies, no suggestions', () => {
  const [r] = frictionRows([p({ commands: [], tools: [['Edit', 3], ['WebFetch', 2]], medianWaitMs: null, affirmations: 0, suggestions: [], asked: 5 })]);
  assert.equal(r.line, 'Edit, WebFetch');
  assert.equal(r.sub, null);
  assert.equal(r.canAllow, false);
});

test('top three projects only, and nothing when nothing asked', () => {
  const rows = frictionRows([p(), p({ name: 'b' }), p({ name: 'c' }), p({ name: 'd' })]);
  assert.deepEqual(rows.map((r) => r.name), ['Applications', 'b', 'c']);
  assert.deepEqual(frictionRows([]), []);
  assert.equal(frictionRows([p({ affirmations: 1 })])[0].sub, '1 "go ahead" reply');
});
```

- [ ] **Step 2: Run to verify it fails** — `cd app && node --test test/approvals.test.mjs` → module not found.

- [ ] **Step 3: Implement** — create `app/ui/approvals.js`:

```js
// Wrapped: where the person approves the most. Counts are estimates: logs
// record tool calls and the permission mode, not the approvals themselves.

function wait(ms) {
  const m = Math.round(ms / 60_000);
  return m < 1 ? `${Math.max(1, Math.round(ms / 1000))}s` : m < 60 ? `${m}m` : `${Math.round(m / 60)}h`;
}

export function frictionRows(projects) {
  return projects.slice(0, 3).map((p) => {
    const what = p.commands.length ? p.commands.slice(0, 3).map(([g]) => g) : p.tools.slice(0, 3).map(([t]) => t);
    const line = [what.join(', '), p.medianWaitMs != null && `${wait(p.medianWaitMs)} median wait`].filter(Boolean).join(' · ');
    return {
      project: p.project,
      name: p.name,
      right: `about ${p.asked}×`,
      line,
      sub: p.affirmations ? `${p.affirmations} "go ahead" ${p.affirmations === 1 ? 'reply' : 'replies'}` : null,
      canAllow: p.suggestions.length > 0,
      suggestions: p.suggestions,
    };
  });
}
```

- [ ] **Step 4:** `cd app && npm test` → pass. **Step 5: Commit**

```bash
git add app/ui/approvals.js app/test/approvals.test.mjs
git commit -m "Approvals: Wrapped row text"
```

---

### Task 5: The Wrapped section and the Allow sheet

**Files:**
- Modify: `app/ui/index.html`, `app/ui/panel.css`, `app/ui/panel.js`

- [ ] **Step 1: Markup** — in `index.html`, in the Wrapped view, insert between the `meta` div and the `report-row` div:

```html
  <div id="friction" hidden>
    <p class="sec">Where you approve the most</p>
    <ul class="rows" id="friction-list"></ul>
  </div>
```

- [ ] **Step 2: Styles** — append to `panel.css`:

```css
/* Wrapped: approvals */
#friction-list .sub-line { grid-column: 1 / -1; color: var(--meta); font-size: 11.5px; }
.rule-pick { display: flex; flex-direction: column; gap: 6px; }
.rule-pick label { display: flex; align-items: center; gap: 8px; font: 11.5px ui-monospace, SFMono-Regular, Menlo, monospace; color: var(--ink); }
.rule-pick input { accent-color: var(--primary); }
```

- [ ] **Step 3: Wiring** — in `panel.js`:

Import: `import { frictionRows } from './approvals.js';`

Add to the Wrapped section (after `show(...)`):

```js
let frictionSeq = 0;
async function loadFriction() {
  const seq = ++frictionSeq;
  let list;
  try {
    list = await invoke('approvals', { days });
  } catch {
    return; // a nice-to-have; Wrapped still works
  }
  if (seq !== frictionSeq) return;
  renderFriction(frictionRows(list));
}

function renderFriction(rows) {
  $('friction').hidden = rows.length === 0;
  $('friction-list').replaceChildren(
    ...rows.map((r) => {
      const li = el('li');
      li.append(el('span', 'name', r.name), el('span', 'right num', r.right), el('span', 'line', r.line));
      if (r.sub) li.append(el('span', 'sub-line', r.sub));
      if (r.canAllow) {
        const box = el('div', 'row-actions');
        box.style.paddingLeft = '0';
        box.append(actionButton('Allow these…', 'check', 'btn ghost', () => confirmAllow(r)));
        li.append(box);
      }
      return li;
    }),
  );
}

function confirmAllow(r) {
  const pick = el('div', 'rule-pick');
  const boxes = r.suggestions.map((rule) => {
    const label = el('label');
    const cb = el('input');
    cb.type = 'checkbox';
    cb.checked = true;
    cb.value = rule;
    label.append(cb, el('span', '', rule));
    pick.append(label);
    return cb;
  });
  const err = el('p', 'err');
  err.hidden = true;
  openSheet({
    title: `Stop asking in ${r.name}?`,
    body: [
      el('p', '', 'These rules let Claude run matching commands in this project without asking. They are added to:'),
      el('div', 'cmd', `${r.project}/.claude/settings.local.json`),
      pick,
      err,
    ],
    ok: 'Add rules',
    run: async () => {
      const rules = boxes.filter((b) => b.checked).map((b) => b.value);
      if (!rules.length) {
        err.textContent = 'Pick at least one rule.';
        err.hidden = false;
        return false;
      }
      try {
        const added = await invoke('allow_rules', { project: r.project, rules, days });
        note(added.length ? `Added ${added.length} rule${added.length === 1 ? '' : 's'} to ${r.name}.` : 'Those rules were already there.');
        loadFriction();
      } catch (e) {
        err.textContent = String(e);
        err.hidden = false;
        return false;
      }
    },
  });
}
```

Call `loadFriction()` at the end of `render()` (after `show(...)`), so it follows the 7d/30d choice; and in `setView`'s wrapped path nothing else is needed.

Icon: add `check: '<path d="M5 12l5 5L20 7"/>'` to `PATHS` in `icons.js` and `'check'` to the icons test names.

- [ ] **Step 4: Checks and commit** — syntax (`cp ui/panel.js /tmp/p.mjs && node --check /tmp/p.mjs`), id cross-check (prints nothing), `npm test`.

```bash
git add app/ui/index.html app/ui/panel.css app/ui/panel.js app/ui/icons.js app/test/icons.test.mjs
git commit -m "Wrapped: where you approve the most, with Allow these…"
```

---

### Task 6: Preview, checks and PR

- [ ] **Step 1: Stubs** — in `app/scripts/preview.mjs` handlers, after `start_recipe: () => null,`:

```js
  approvals: () => [
    { project: '/tmp/app', name: 'Applications', asked: 41, tools: [['Bash', 38], ['Edit', 3]], commands: [['npm run', 20], ['docker compose', 9], ['git commit', 4]], medianWaitMs: 360000, affirmations: 7, suggestions: ['Bash(npm run:*)', 'Bash(docker compose:*)'] },
    { project: '/tmp/erp', name: 'erp-backend', asked: 18, tools: [['Edit', 12], ['Bash', 6]], commands: [['cargo test', 6]], medianWaitMs: 180000, affirmations: 0, suggestions: ['Bash(cargo test:*)'] },
  ],
  allow_rules: ({ rules }) => rules,
```

Commit: `git commit -am "Preview: stubs for approvals"` (only preview.mjs changed).

- [ ] **Step 2: Visual check** — `PORT=5175 npm run preview`, Wrapped (key 5): "Where you approve the most" with two rows ("about 41×", "npm run, docker compose, git commit · 6m median wait", "7 "go ahead" replies", **Allow these…**). Allow opens the sheet with the file path and two checked rules; unchecking both and pressing Add rules shows "Pick at least one rule." and stays open; Add rules closes and shows "Added 2 rules to Applications.".

- [ ] **Step 3: All suites, install, PR** — run all three suites; build and install the app (one copy in `/Applications`, build copy removed); push and open the PR into `dev`:

```bash
gh pr create --base dev --title "Panel redesign, phase 6: where you approve the most" --body "Phase 6 of docs/superpowers/specs/2026-10-04-panel-redesign-design.md.

- Wrapped shows the top three projects by tool calls that had to ask for approval in the range (an estimate: logs record calls and the permission mode, not approvals), what asked (command groups or tools), the median wait, and how many short \"go ahead\" replies were typed.
- Allow these… proposes broad rules for repeated commands (e.g. Bash(npm run:*), WebFetch(domain:…)), never for destructive ones (rm, sudo, git push/reset/clean, curl, …). The sheet shows the rules and the file; chosen rules are merged into <project>/.claude/settings.local.json, nothing else in it changes. Only rules the app suggested for that project are accepted.
- Matching understands exact, prefix:*, glob and bare-tool rules, WebFetch domains, and Claude Code's built-in read-only commands; auto/bypass/plan sessions count as zero.
- Tests: Rust rule matching, settings merge, friction counting and suggestions; JS row text."
```

The person (or the controller, with standing permission for this run) merges into `dev`.

---

## Self-review notes

- Spec coverage: Wrapped "Where you approve the most" with counts, tools, average (median) wait, Allow these… that shows rules first and writes only on confirm (Tasks 1–5). The data rule is defined from real logs (calls that asked = default mode, no covering rule).
- Names: `perms::{asks, Rules::{parse, for_project, allows}, add_local_rules}`, `approvals::{scan, group, ProjectFriction{project, name, asked, tools, commands, medianWaitMs, affirmations, suggestions}}`; commands `approvals({days})`, `allow_rules({project, rules, days})`; JS `frictionRows`; ids `friction`, `friction-list`; icon `check`.
