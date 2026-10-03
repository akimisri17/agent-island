//! Daily recap helpers: today's git commits in the repositories agents
//! worked in, and an optional rewrite by the person's own `claude` command.

use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub ts: i64,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepoCommits {
    /// Repository folder name, which is also the project name sessions use.
    pub project: String,
    pub commits: Vec<Commit>,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Commits since `since` (ms) by this repository's configured author, one
/// entry per repository, skipping folders that are not git repositories.
pub fn commits_since(cwds: &[String], since: i64) -> Vec<RepoCommits> {
    let mut roots: BTreeMap<PathBuf, ()> = BTreeMap::new();
    for c in cwds {
        if let Some(top) = git(Path::new(c), &["rev-parse", "--show-toplevel"]) {
            roots.insert(PathBuf::from(top.trim()), ());
        }
    }
    let mut out = Vec::new();
    for root in roots.keys() {
        let author = git(root, &["config", "user.email"]).map(|s| s.trim().to_string()).unwrap_or_default();
        let mut args = vec!["log".to_string(), format!("--since=@{}", since / 1000), "--no-merges".into(), "--format=%h%x09%ct%x09%s".into()];
        if !author.is_empty() {
            args.push(format!("--author={author}"));
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let Some(log) = git(root, &args) else { continue };
        let commits = parse_log(&log);
        if !commits.is_empty() {
            let project = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(RepoCommits { project, commits });
        }
    }
    out
}

pub fn parse_log(log: &str) -> Vec<Commit> {
    log.lines()
        .filter_map(|l| {
            let mut p = l.splitn(3, '\t');
            let hash = p.next()?.to_string();
            let ts = p.next()?.parse::<i64>().ok()? * 1000;
            let subject = p.next()?.to_string();
            Some(Commit { hash, subject, ts })
        })
        .collect()
}

/// The person's `claude` command. GUI apps start with a short PATH, so the
/// usual install locations are checked before asking a login shell.
pub fn find_claude(home: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "claude.exe" } else { "claude" };
    let candidates = [
        home.join(".local/bin").join(exe),
        home.join(".claude/local").join(exe),
        PathBuf::from("/opt/homebrew/bin").join(exe),
        PathBuf::from("/usr/local/bin").join(exe),
        home.join("AppData/Roaming/npm/claude.cmd"),
    ];
    if let Some(p) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(p);
    }
    if cfg!(unix) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let out = Command::new(shell).args(["-lc", "command -v claude"]).output().ok()?;
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return (!p.is_empty()).then(|| PathBuf::from(p));
    }
    None
}

pub const POLISH_PROMPT: &str = "Rewrite the standup notes on stdin in the first person. \
Keep the Done, In progress and Next sections and every fact in them; add nothing. \
Turn commit messages and session titles into short plain phrases, like \"Fixed the cart race\". \
Do not mention tools, agents, AI or time spent. Plain text, under 100 words. Output only the update.";

/// Runs `claude -p` on the recap text: no tools, not saved as a session,
/// and only project settings (none, in an empty temporary folder), so user
/// plugins and hooks stay out while the login still works. Gives up after
/// two minutes.
pub fn polish(claude: &Path, text: &str) -> Result<String, String> {
    let dir = std::env::temp_dir().join("agent-island-recap");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut child = Command::new(claude)
        .args(["-p", POLISH_PROMPT, "--no-session-persistence", "--setting-sources", "project", "--tools", "", "--output-format", "text"])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start claude: {e}"))?;
    child.stdin.take().ok_or("no stdin")?.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break,
            None if started.elapsed() > Duration::from_secs(120) => {
                let _ = child.kill();
                return Err("claude took longer than two minutes".into());
            }
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("claude failed: {}", err.lines().next().unwrap_or("unknown error")));
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return Err("claude returned nothing".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_log_lines() {
        let c = parse_log("abc1234\t1790000000\tFix checkout race\ndef5678\t1790000600\tAdd: tabs\tin subject\nbroken line\n");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].hash, "abc1234");
        assert_eq!(c[0].ts, 1_790_000_000_000);
        assert_eq!(c[1].subject, "Add: tabs\tin subject");
    }

    #[test]
    fn commits_in_a_real_repository() {
        // This repository: commits by its configured author since the epoch.
        let here = env!("CARGO_MANIFEST_DIR").to_string();
        let r = commits_since(&[here.clone(), here], 0);
        assert!(r.len() <= 1, "one entry per repository");
        let none = commits_since(&[std::env::temp_dir().to_string_lossy().into_owned()], 0);
        assert!(none.is_empty(), "non-repositories are skipped");
    }
}
