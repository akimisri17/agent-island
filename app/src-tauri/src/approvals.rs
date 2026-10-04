//! Approval friction: tool calls that stopped for the person's approval,
//! per project, and broad allow-rules that would have covered them.

use crate::perms;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const NEVER: &[&str] = &[
    "cd", "for", "do", "if", "then", "else", "elif", "while", "until", "done", "fi", "case", "esac", "select", "time", "git -C",
    "cat", "tee", "echo", "printf", "python", "python3", "node", "bash", "sh", "zsh", "eval", "exec", "xargs", "env", "source", "mv", "cp",
    "rm", "sudo", "chmod", "chown", "dd", "mkfs", "kill", "pkill", "killall", "shutdown", "reboot", "curl", "wget", "ssh", "scp",
    "git push", "git reset", "git clean", "git checkout", "git rebase", "docker rm", "docker system",
];
const SCRIPT: &str = "shell script";
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
    let c = cmd.trim();
    if ["for ", "while ", "until ", "if "].iter().any(|k| c.starts_with(k)) || c.contains("<<") {
        return SCRIPT.to_string();
    }
    let segs = perms::segments(cmd);
    let first_word = |s: &str| s.split_whitespace().next().map(str::to_string).unwrap_or_default();
    if segs.iter().any(|s| matches!(first_word(perms::strip_env(s)).as_str(), "for" | "while" | "until" | "if" | "do" | "then" | "else" | "done" | "fi")) {
        return SCRIPT.to_string();
    }
    let seg = segs
        .iter()
        .map(|s| perms::strip_env(s))
        .find(|s| !s.is_empty() && *s != "cd" && !s.starts_with("cd "))
        .unwrap_or_else(|| segs.first().copied().unwrap_or(""));
    seg.split_whitespace().take(2).collect::<Vec<_>>().join(" ").trim_end_matches(';').to_string()
}

fn never_suggest(g: &str) -> bool {
    let first = g.split_whitespace().next().unwrap_or("");
    let mut w = g.split_whitespace();
    let read_only = perms::READ_ONLY.contains(&first) || (first == "git" && w.nth(1).is_some_and(|s| perms::READ_ONLY_GIT.contains(&s)));
    g == SCRIPT || read_only || first.starts_with(['.', '/', '<']) || g.contains(['=', '>']) || NEVER.iter().any(|n| g == *n || g.starts_with(&format!("{n} ")) || first == *n)
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
        assert_eq!(group("cd app && cargo test"), "cargo test");
        assert_eq!(group("FOO=1 make build"), "make build");
    }

    #[test]
    fn unsafe_groups_are_never_suggested() {
        for g in ["cd /w", "cat >", ".venv/bin/python -", "python3 x.py", "DOC=1", "/bin/ls", "git push", "cat <<EOF"] {
            assert!(never_suggest(g), "{g}");
        }
        for g in ["shell script", "grep -n", "ls wiki/x", "git status", "git log"] {
            assert!(never_suggest(g), "{g}");
        }
        assert!(!never_suggest("npm run"));
    }

    #[test]
    fn loops_and_heredocs_are_one_group() {
        assert_eq!(group("for d in a b; do echo $d; done"), "shell script");
        assert_eq!(group("while true; do x; done"), "shell script");
        assert_eq!(group("cat <<EOF > f\nhi\nEOF"), "shell script");
        assert_eq!(group("cd x && npm test"), "npm test");
        assert_eq!(group("cd x && for d in a; do ls; done"), "shell script");
    }
}
