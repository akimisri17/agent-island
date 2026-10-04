//! Claude Code allow-rules: reading them, deciding whether a tool call was
//! already allowed, and adding rules to a project's local settings.

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
    let h = after.split(['/', '?', '#']).next()?.split('@').next_back()?.split(':').next()?;
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
