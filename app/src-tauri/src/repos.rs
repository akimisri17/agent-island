//! Repo board: the local git state of the repositories agents worked in
//! today. Read-only: no fetch, no checkout, no deletes. Ahead/behind comes
//! from the last fetch, and `GIT_OPTIONAL_LOCKS=0` keeps `git status` from
//! refreshing the index, so nothing in the repository is written.

use rayon::prelude::*;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub branch: Option<String>,
}

#[derive(Serialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    /// Repository folder name, which is also the project name sessions use.
    pub project: String,
    pub path: String,
    /// Current branch, or the short commit when detached.
    pub branch: String,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Changed, staged and untracked files.
    pub changes: u32,
    pub stashes: u32,
    /// Linked worktrees, not counting the main one.
    pub worktrees: Vec<Worktree>,
    pub default_branch: Option<String>,
    /// Local branches already merged into the default branch, other than
    /// the default itself and any branch checked out somewhere.
    pub merged: Vec<String>,
    /// Local branches whose upstream was deleted on the remote, usually
    /// because their PR was merged by rebase or squash, which leaves them
    /// unmerged as far as git can tell. Not repeated from `merged`.
    pub gone: Vec<String>,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The main repository folder for a working directory. Linked worktrees fold
/// into the repository they belong to.
fn repo_root(cwd: &Path) -> Option<PathBuf> {
    let common = git(cwd, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let common = PathBuf::from(common.trim());
    if common.file_name().is_some_and(|n| n == ".git") {
        return common.parent().map(Path::to_path_buf);
    }
    // Bare repository or unusual layout: fall back to the checkout itself.
    git(cwd, &["rev-parse", "--show-toplevel"]).map(|t| PathBuf::from(t.trim()))
}

/// Git state for each repository behind these folders, in parallel.
/// Folders that are not git repositories are skipped.
pub fn status(cwds: &[String]) -> Vec<RepoStatus> {
    let roots: BTreeSet<PathBuf> = cwds.par_iter().filter_map(|c| repo_root(Path::new(c))).collect();
    let mut out: Vec<RepoStatus> = roots.par_iter().filter_map(|r| repo_status(r)).collect();
    out.sort_by(|a, b| a.project.cmp(&b.project).then(a.path.cmp(&b.path)));
    out
}

fn repo_status(root: &Path) -> Option<RepoStatus> {
    let st = git(root, &["status", "--porcelain=v2", "--branch", "--untracked-files=normal"])?;
    let mut r = parse_status(&st);
    r.project = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    r.path = root.to_string_lossy().into_owned();
    if r.detached {
        if let Some(h) = git(root, &["rev-parse", "--short", "HEAD"]) {
            r.branch = h.trim().to_string();
        }
    }
    r.stashes = git(root, &["stash", "list"]).map_or(0, |s| s.lines().count() as u32);
    let all = git(root, &["worktree", "list", "--porcelain"]).map(|s| parse_worktrees(&s)).unwrap_or_default();
    r.default_branch = default_branch(root);
    let checked_out: BTreeSet<&str> = all.iter().filter_map(|w| w.branch.as_deref()).chain([r.branch.as_str()]).collect();
    if let Some(def) = &r.default_branch {
        let merged = git(root, &["branch", "--merged", def, "--format=%(refname:short)"]).unwrap_or_default();
        r.merged = merged
            .lines()
            .map(str::trim)
            .filter(|b| !b.is_empty() && *b != def && !checked_out.contains(b))
            .map(str::to_string)
            .collect();
    }
    let refs = git(root, &["for-each-ref", "--format=%(refname:short)%09%(upstream:track)", "refs/heads"]).unwrap_or_default();
    r.gone = parse_gone(&refs)
        .into_iter()
        .filter(|b| Some(b) != r.default_branch.as_ref() && !checked_out.contains(b.as_str()) && !r.merged.contains(b))
        .collect();
    r.worktrees = all.into_iter().filter(|w| Path::new(&w.path) != root).collect();
    Some(r)
}

/// The branch the repository merges into: origin's HEAD when known, else a
/// local main or master.
fn default_branch(root: &Path) -> Option<String> {
    if let Some(h) = git(root, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]) {
        let local = h.trim().trim_start_matches("origin/").to_string();
        if git(root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{local}")]).is_some() {
            return Some(local);
        }
    }
    ["main", "master"]
        .into_iter()
        .find(|b| git(root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{b}")]).is_some())
        .map(str::to_string)
}

/// Reads `git status --porcelain=v2 --branch`: the `# branch.*` headers and
/// one line per changed or untracked file.
pub fn parse_status(s: &str) -> RepoStatus {
    let mut r = RepoStatus::default();
    for l in s.lines() {
        if let Some(h) = l.strip_prefix("# branch.head ") {
            r.detached = h == "(detached)";
            r.branch = h.to_string();
        } else if let Some(u) = l.strip_prefix("# branch.upstream ") {
            r.upstream = Some(u.to_string());
        } else if let Some(ab) = l.strip_prefix("# branch.ab ") {
            for part in ab.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    r.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    r.behind = n.parse().unwrap_or(0);
                }
            }
        } else if !l.starts_with('#') && !l.starts_with('!') && !l.is_empty() {
            r.changes += 1;
        }
    }
    r
}

/// Branches marked `[gone]` in `for-each-ref` output of
/// `%(refname:short)<TAB>%(upstream:track)`.
pub fn parse_gone(s: &str) -> Vec<String> {
    s.lines().filter_map(|l| l.split_once('\t')).filter(|(_, t)| *t == "[gone]").map(|(b, _)| b.to_string()).collect()
}

/// Reads `git worktree list --porcelain`: blank-line separated records.
pub fn parse_worktrees(s: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    let mut cur: Option<Worktree> = None;
    for l in s.lines().chain([""]) {
        if let Some(p) = l.strip_prefix("worktree ") {
            cur = Some(Worktree { path: p.to_string(), branch: None });
        } else if let Some(b) = l.strip_prefix("branch ") {
            if let Some(w) = cur.as_mut() {
                w.branch = Some(b.trim_start_matches("refs/heads/").to_string());
            }
        } else if l.is_empty() {
            out.extend(cur.take());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_headers_and_changes() {
        let r = parse_status(
            "# branch.oid abc\n# branch.head feat/x\n# branch.upstream origin/feat/x\n# branch.ab +2 -1\n1 .M N... 100644 100644 100644 a b src/a.rs\n? new.txt\n! ignored\n",
        );
        assert_eq!(r.branch, "feat/x");
        assert_eq!(r.upstream.as_deref(), Some("origin/feat/x"));
        assert_eq!((r.ahead, r.behind, r.changes), (2, 1, 2));
        assert!(!r.detached);
        assert!(parse_status("# branch.head (detached)\n").detached);
    }

    #[test]
    fn parses_gone_upstreams() {
        assert_eq!(parse_gone("main\t\nfeat/a\t[gone]\nfeat/b\t[ahead 1]\n"), vec!["feat/a"]);
    }

    #[test]
    fn parses_worktree_records() {
        let w = parse_worktrees("worktree /r\nHEAD abc\nbranch refs/heads/main\n\nworktree /r/.worktrees/x\nHEAD def\ndetached\n");
        assert_eq!(w, vec![
            Worktree { path: "/r".into(), branch: Some("main".into()) },
            Worktree { path: "/r/.worktrees/x".into(), branch: None },
        ]);
    }

    #[test]
    fn reads_a_real_repository_without_writing_to_it() {
        let dir = std::env::temp_dir().join(format!("ai-repos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let run = |args: &[&str]| {
            let ok = Command::new("git").arg("-C").arg(&dir).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap();
            assert!(ok.success(), "git {args:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "1").unwrap();
        std::fs::write(dir.join(".gitignore"), ".worktrees/\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "one"]);
        run(&["branch", "done"]); // merged into main
        run(&["branch", "busy"]);
        run(&["worktree", "add", "-q", ".worktrees/busy", "busy"]); // merged, but checked out
        // A branch with its own commit whose remote branch was deleted, as
        // after a squash or rebase merge.
        let remote = dir.with_extension("remote");
        let _ = std::fs::remove_dir_all(&remote);
        let ok = Command::new("git").args(["init", "-q", "--bare"]).arg(&remote).status().unwrap();
        assert!(ok.success());
        run(&["remote", "add", "origin", &remote.to_string_lossy()]);
        run(&["checkout", "-qb", "squashed"]);
        std::fs::write(dir.join("s.txt"), "s").unwrap();
        run(&["add", "s.txt"]);
        run(&["commit", "-qm", "work"]);
        run(&["push", "-q", "-u", "origin", "squashed"]);
        run(&["checkout", "-q", "main"]);
        run(&["push", "-q", "origin", "--delete", "squashed"]);
        std::fs::write(dir.join("a.txt"), "2").unwrap();
        run(&["stash", "-q"]);
        std::fs::write(dir.join("b.txt"), "new").unwrap();

        let sub = dir.join(".worktrees/busy").to_string_lossy().into_owned();
        let r = status(&[dir.to_string_lossy().into_owned(), sub]);
        assert_eq!(r.len(), 1, "a worktree folds into its repository");
        let r = &r[0];
        assert_eq!(r.branch, "main");
        assert_eq!(r.changes, 1);
        assert_eq!(r.stashes, 1);
        assert_eq!(r.default_branch.as_deref(), Some("main"));
        assert_eq!(r.merged, vec!["done"]);
        assert_eq!(r.gone, vec!["squashed"]);
        assert_eq!(r.worktrees.len(), 1);
        assert_eq!(r.worktrees[0].branch.as_deref(), Some("busy"));
        assert!(!dir.join(".git/index.lock").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&remote);
    }
}
