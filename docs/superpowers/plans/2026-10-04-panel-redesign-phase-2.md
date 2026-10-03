# Panel Redesign, Phase 2 (Repo Actions) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the Repos tab act, not just report: **Pull** (in the app, with a confirm sheet), **Delete old branches** (in the app, with a confirm sheet), and **Terminal** (opens the person's terminal app in the repo folder), plus a terminal picker in Settings.

**Architecture:** All git and process work happens in Rust. `repos.rs` gains `pull` and `delete_old_branches`, which re-read the repository's state before acting and refuse anything the UI should not have offered. A new `terminal.rs` detects installed terminal apps and opens one in a folder with `open -a` (macOS) or `wt -d` (Windows) — no AppleScript, so no Automation permission prompt. Four new Tauri commands expose them. In the panel, `repos.js` gains pure formatters for the sheets and result lines (unit-tested); `panel.js` adds a selected-row action strip, one reusable confirm sheet, a per-row result line, and a Settings row.

**Tech Stack:** Rust (std::process::Command, no shell), Tauri 2 commands, vanilla JS modules, `node --test`, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-10-04-panel-redesign-design.md` → "Actions" and phase 2.

**Scope notes:**
- Terminal only opens a folder in this phase. Running a command in a new tab (Resume, recipes, Run now) is phase 3/5 and will extend `terminal.rs` then.
- Pull talks to the remote (it fetches), which is the one network action the person explicitly confirms. Nothing else fetches.
- The confirm sheet always shows the exact command.

---

## Branch

```bash
cd /Users/akhilmisri/Business/Products/agent-island
git fetch -q
git worktree add -b feat/repo-actions ../agent-island-repo-actions origin/dev
cd ../agent-island-repo-actions
```

Work only in that worktree. Never commit to `main` or `dev`. No Claude attribution lines in commits.

## File structure

| File | Responsibility |
|---|---|
| `app/src-tauri/src/repos.rs` | + `git_run` (captures stderr), `pull`, `delete_old_branches`, root check |
| `app/src-tauri/src/terminal.rs` (new) | Detect installed terminals; open one in a folder |
| `app/src-tauri/src/settings.rs` | + `terminal: Option<String>` |
| `app/src-tauri/src/lib.rs` | + commands `repo_pull`, `repo_delete_branches`, `terminals`, `open_terminal` |
| `app/ui/repos.js` | + `behind` on rows; `pullSheet`, `branchSheet`, `resultLine` |
| `app/test/repos.test.mjs` | Tests for the new formatters |
| `app/ui/index.html` | + sheet markup; + Terminal settings row |
| `app/ui/panel.css` | + sheet, selected row, action strip |
| `app/ui/panel.js` | Row selection, actions, sheet, results, Terminal setting |
| `app/scripts/preview.mjs` | Stubs for the new commands |

---

### Task 1: Pull in Rust

**Files:**
- Modify: `app/src-tauri/src/repos.rs`

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests` in `app/src-tauri/src/repos.rs`, after the existing tests:

```rust
    /// A clone of a bare remote, with a second clone that can push to it.
    fn remote_and_clone(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("ai-pull-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let sh = |dir: &Path, args: &[&str]| {
            let ok = Command::new("git").arg("-C").arg(dir).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap();
            assert!(ok.success(), "git {args:?}");
        };
        let remote = base.join("remote.git");
        sh(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        let mine = base.join("mine");
        let other = base.join("other");
        for c in [&mine, &other] {
            sh(&base, &["clone", "-q", &remote.to_string_lossy(), &c.to_string_lossy()]);
            sh(c, &["config", "user.email", "t@t"]);
            sh(c, &["config", "user.name", "t"]);
        }
        std::fs::write(other.join("a.txt"), "1").unwrap();
        sh(&other, &["add", "."]);
        sh(&other, &["commit", "-qm", "one"]);
        sh(&other, &["push", "-q", "origin", "main"]);
        sh(&mine, &["pull", "-q", "origin", "main"]);
        sh(&mine, &["branch", "-q", "--set-upstream-to=origin/main", "main"]);
        // Two new commits on the remote that `mine` has fetched but not merged.
        for n in ["2", "3"] {
            std::fs::write(other.join("a.txt"), n).unwrap();
            sh(&other, &["commit", "-qam", n]);
        }
        sh(&other, &["push", "-q", "origin", "main"]);
        sh(&mine, &["fetch", "-q"]);
        (base, mine, other)
    }

    #[test]
    fn pull_fast_forwards_and_counts_commits() {
        let (base, mine, _) = remote_and_clone("ok");
        assert_eq!(pull(&mine), Ok(2));
        let r = repo_status(&mine).unwrap();
        assert_eq!((r.behind, r.changes), (0, 0));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pull_refuses_with_local_changes_or_nothing_to_pull() {
        let (base, mine, _) = remote_and_clone("dirty");
        std::fs::write(mine.join("b.txt"), "local").unwrap();
        assert_eq!(pull(&mine), Err("Commit or stash your changes first.".to_string()));
        std::fs::remove_file(mine.join("b.txt")).unwrap();
        assert_eq!(pull(&mine), Ok(2));
        assert_eq!(pull(&mine), Err("Nothing to pull.".to_string()));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pull_stops_when_a_merge_would_be_needed() {
        let (base, mine, _) = remote_and_clone("diverged");
        std::fs::write(mine.join("c.txt"), "mine").unwrap();
        let ok = Command::new("git").arg("-C").arg(&mine).args(["add", "."]).status().unwrap().success()
            && Command::new("git").arg("-C").arg(&mine).args(["commit", "-qm", "mine"]).status().unwrap().success();
        assert!(ok);
        let err = pull(&mine).unwrap_err();
        assert!(!err.is_empty(), "git's reason is passed on");
        let r = repo_status(&mine).unwrap();
        assert_eq!((r.ahead, r.behind), (1, 2), "nothing was merged");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn actions_refuse_folders_that_are_not_a_repository_root() {
        let (base, mine, _) = remote_and_clone("root");
        std::fs::create_dir_all(mine.join("sub")).unwrap();
        assert_eq!(pull(&mine.join("sub")), Err("Not a repository folder.".to_string()));
        assert_eq!(pull(&base), Err("Not a repository folder.".to_string()));
        let _ = std::fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd app/src-tauri && cargo test --lib repos::tests::pull`
Expected: compile error `cannot find function 'pull'`.

- [ ] **Step 3: Implement**

In `app/src-tauri/src/repos.rs`, add after the existing `fn git(...)`:

```rust
/// Runs git for an action. Never prompts (no terminal to answer in); a
/// failure returns git's first non-empty error line.
fn git_run(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Could not run git: {e}"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let line = err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("git failed");
    Err(line.trim_start_matches("fatal: ").trim_start_matches("error: ").to_string())
}

/// Actions only run on a repository's own top folder, as listed on the board.
fn check_root(path: &Path) -> Result<(), String> {
    match repo_root(path) {
        Some(root) if root == path => Ok(()),
        _ => Err("Not a repository folder.".into()),
    }
}

/// Fast-forwards the current branch to its upstream. Re-reads the state
/// first, so a stale button cannot pull over local changes. Returns how many
/// commits arrived.
pub fn pull(path: &Path) -> Result<u32, String> {
    check_root(path)?;
    let st = repo_status(path).ok_or("Could not read the repository.")?;
    if st.changes > 0 {
        return Err("Commit or stash your changes first.".into());
    }
    if st.behind == 0 {
        return Err("Nothing to pull.".into());
    }
    let before = git_run(path, &["rev-parse", "HEAD"])?;
    git_run(path, &["pull", "--ff-only", "--quiet"])?;
    let range = format!("{}..HEAD", before.trim());
    let n = git_run(path, &["rev-list", "--count", &range])?;
    Ok(n.trim().parse().unwrap_or(0))
}
```

`repo_root` canonicalises through git (`--path-format=absolute`), and the board's paths come from the same function, so comparing `PathBuf`s is exact. On macOS, `std::env::temp_dir()` is under `/var`, which git reports as `/private/var`; the test helper canonicalises its base for that reason.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd app/src-tauri && cargo test --lib repos::`
Expected: all `repos::tests` pass (the 5 existing and the 4 new).

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/repos.rs
git commit -m "Repos: pull fast-forwards only, after re-checking the repository"
```

---

### Task 2: Delete old branches in Rust

**Files:**
- Modify: `app/src-tauri/src/repos.rs`

- [ ] **Step 1: Write the failing test**

Add inside `mod tests`:

```rust
    #[test]
    fn deletes_only_branches_still_listed_as_old() {
        let (base, mine, _) = remote_and_clone("delete");
        let sh = |args: &[&str]| assert!(Command::new("git").arg("-C").arg(&mine).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "git {args:?}");
        sh(&["branch", "done"]); // merged into main
        sh(&["checkout", "-qb", "squashed"]);
        std::fs::write(mine.join("s.txt"), "s").unwrap();
        sh(&["add", "s.txt"]);
        sh(&["commit", "-qm", "work"]);
        sh(&["push", "-q", "-u", "origin", "squashed"]);
        sh(&["checkout", "-q", "main"]);
        sh(&["push", "-q", "origin", "--delete", "squashed"]);
        sh(&["fetch", "-q", "--prune"]);
        sh(&["branch", "keep"]); // merged too, but not asked for
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        let deleted = delete_old_branches(&mine, &names(&["done", "squashed", "main", "-D", "nope"])).unwrap();
        assert_eq!(deleted, vec!["done", "squashed"], "only listed old branches; not the current one, not flags, not unknown names");
        let left = git_run(&mine, &["branch", "--format=%(refname:short)"]).unwrap();
        let left: Vec<&str> = left.lines().collect();
        assert!(left.contains(&"main") && left.contains(&"keep"));
        assert!(!left.contains(&"done") && !left.contains(&"squashed"));
        assert_eq!(delete_old_branches(&mine.join("nope"), &names(&["keep"])), Err("Not a repository folder.".to_string()));
        let _ = std::fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd app/src-tauri && cargo test --lib repos::tests::deletes_only`
Expected: compile error `cannot find function 'delete_old_branches'`.

- [ ] **Step 3: Implement**

Add after `pull` in `app/src-tauri/src/repos.rs`:

```rust
/// Deletes the asked-for local branches that the board still lists as old:
/// merged ones with `-d` (git refuses if anything is unmerged), gone ones
/// with `-D` (a squash or rebase merge leaves them unmerged as far as git can
/// tell). Anything else asked for is skipped. Returns the deleted names.
pub fn delete_old_branches(path: &Path, names: &[String]) -> Result<Vec<String>, String> {
    check_root(path)?;
    let st = repo_status(path).ok_or("Could not read the repository.")?;
    let mut deleted = Vec::new();
    for name in names {
        let flag = if st.merged.contains(name) {
            "-d"
        } else if st.gone.contains(name) {
            "-D"
        } else {
            continue;
        };
        // `--` keeps a name from ever being read as an option.
        git_run(path, &["branch", flag, "--", name])?;
        deleted.push(name.clone());
    }
    Ok(deleted)
}
```

`repo_status` already excludes the default branch and every branch checked out in any worktree from `merged` and `gone`, so those can never be deleted here.

- [ ] **Step 4: Run to verify it passes**

Run: `cd app/src-tauri && cargo test --lib repos::`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/repos.rs
git commit -m "Repos: delete old branches, only those still listed as merged or gone"
```

---

### Task 3: Terminal detection and opening

**Files:**
- Create: `app/src-tauri/src/terminal.rs`
- Modify: `app/src-tauri/src/settings.rs`

- [ ] **Step 1: Write the failing tests**

Create `app/src-tauri/src/terminal.rs` with only the tests first:

```rust
//! Opens the person's terminal app in a folder. macOS uses `open -a`, which
//! needs no Automation permission; Windows uses Windows Terminal.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_command_for_each_platform() {
        let dir = std::path::Path::new("/w/my shop");
        if cfg!(target_os = "macos") {
            assert_eq!(launch("iTerm", dir), ("open".to_string(), vec!["-a".into(), "iTerm".into(), "/w/my shop".into()]));
        } else if cfg!(windows) {
            assert_eq!(launch("Windows Terminal", dir), ("wt".to_string(), vec!["-d".into(), "/w/my shop".into()]));
        }
    }

    #[test]
    fn choice_falls_back_to_the_first_installed() {
        let installed = vec!["Ghostty".to_string(), "Terminal".to_string()];
        assert_eq!(choose(Some("Terminal"), &installed), Some("Terminal".to_string()));
        assert_eq!(choose(Some("Warp"), &installed), Some("Ghostty".to_string()), "a removed app falls back");
        assert_eq!(choose(None, &installed), Some("Ghostty".to_string()));
        assert_eq!(choose(None, &[]), None);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_always_has_terminal() {
        assert!(installed().contains(&"Terminal".to_string()));
    }
}
```

Register the module in `app/src-tauri/src/lib.rs` next to the others (keep alphabetical order):

```rust
pub mod terminal;
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd app/src-tauri && cargo test --lib terminal::`
Expected: compile errors for `launch`, `choose`, `installed`.

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]` in `app/src-tauri/src/terminal.rs`:

```rust
use std::path::{Path, PathBuf};
use std::process::Command;

/// Preference order when the person has not picked one.
#[cfg(target_os = "macos")]
const KNOWN: [&str; 4] = ["Ghostty", "iTerm", "Warp", "Terminal"];

/// Terminal apps installed on this machine, in preference order.
#[cfg(target_os = "macos")]
pub fn installed() -> Vec<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let dirs = [PathBuf::from("/Applications"), home.join("Applications"), PathBuf::from("/System/Applications/Utilities")];
    KNOWN
        .iter()
        .filter(|name| dirs.iter().any(|d| d.join(format!("{name}.app")).exists()))
        .map(|s| s.to_string())
        .collect()
}

#[cfg(windows)]
pub fn installed() -> Vec<String> {
    let found = Command::new("where").arg("wt").output().is_ok_and(|o| o.status.success());
    if found { vec!["Windows Terminal".into()] } else { Vec::new() }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn installed() -> Vec<String> {
    Vec::new()
}

/// The terminal to use: the person's pick if still installed, otherwise the
/// first installed one.
pub fn choose(pick: Option<&str>, installed: &[String]) -> Option<String> {
    pick.filter(|p| installed.iter().any(|i| i == p)).map(str::to_string).or_else(|| installed.first().cloned())
}

/// Program and arguments that open `app` in `dir`. No shell is involved, so
/// the folder name is passed through as one argument whatever it contains.
pub fn launch(app: &str, dir: &Path) -> (String, Vec<String>) {
    let d = dir.to_string_lossy().into_owned();
    if cfg!(windows) {
        ("wt".into(), vec!["-d".into(), d])
    } else {
        ("open".into(), vec!["-a".into(), app.into(), d])
    }
}

/// Opens the chosen terminal in `dir`.
pub fn open(pick: Option<&str>, dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err("That folder no longer exists.".into());
    }
    let app = choose(pick, &installed()).ok_or("No supported terminal app found.")?;
    let (prog, args) = launch(&app, dir);
    let ok = Command::new(&prog).args(&args).status().map_err(|e| format!("Could not open {app}: {e}"))?;
    if ok.success() { Ok(()) } else { Err(format!("Could not open {app}.")) }
}
```

Then add the setting. In `app/src-tauri/src/settings.rs`, add a field to `Settings` after `recap_with_claude`:

```rust
    /// Terminal app for "Terminal" buttons, e.g. "iTerm". None: first installed.
    pub terminal: Option<String>,
```

Update `Default`:

```rust
        Settings { hotkey: DEFAULT_HOTKEY.into(), notify_limits: true, recap_with_claude: false, terminal: None }
```

and the test literal in `round_trip_and_defaults`:

```rust
        let s = Settings { hotkey: "ctrl+shift+KeyK".into(), notify_limits: false, recap_with_claude: true, terminal: Some("iTerm".into()) };
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd app/src-tauri && cargo test --lib terminal:: settings::`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/terminal.rs app/src-tauri/src/settings.rs app/src-tauri/src/lib.rs
git commit -m "Terminal: detect installed apps and open one in a folder; setting to pick"
```

---

### Task 4: Tauri commands

**Files:**
- Modify: `app/src-tauri/src/lib.rs`

- [ ] **Step 1: Add the commands**

In `app/src-tauri/src/lib.rs`, after `async fn repo_status(...)`:

```rust
/// Pulls (fast-forward only). Returns how many commits arrived.
#[tauri::command]
async fn repo_pull(path: String) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || repos::pull(std::path::Path::new(&path))).await.map_err(|e| e.to_string())?
}

/// Deletes the named old branches. Returns the names deleted.
#[tauri::command]
async fn repo_delete_branches(path: String, names: Vec<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || repos::delete_old_branches(std::path::Path::new(&path), &names)).await.map_err(|e| e.to_string())?
}

/// Installed terminal apps, in preference order.
#[tauri::command]
fn terminals() -> Vec<String> {
    terminal::installed()
}

/// Opens the chosen terminal app in a folder.
#[tauri::command]
fn open_terminal(app: AppHandle, path: String) -> Result<(), String> {
    let pick = app.state::<Prefs>().0.lock().ok().and_then(|p| p.terminal.clone());
    terminal::open(pick.as_deref(), std::path::Path::new(&path))
}
```

Add `repo_pull, repo_delete_branches, terminals, open_terminal` to the `tauri::generate_handler![...]` list, right after `repo_status`.

- [ ] **Step 2: Build and test**

Run: `cd app/src-tauri && cargo build --lib && cargo test --lib && cargo clippy --lib 2>&1 | grep -E "src/(repos|terminal|lib|settings)\.rs" -A3`
Expected: build OK, all tests pass, no clippy warnings in these files beyond the ones already on `dev` (`lib.rs` type_complexity at `compute_limits`, the `for` loop on an unbounded range).

- [ ] **Step 3: Commit**

```bash
git add app/src-tauri/src/lib.rs
git commit -m "Commands: repo_pull, repo_delete_branches, terminals, open_terminal"
```

---

### Task 5: Sheet and result formatters

**Files:**
- Modify: `app/ui/repos.js`
- Modify: `app/test/repos.test.mjs`

- [ ] **Step 1: Write the failing tests**

Append to `app/test/repos.test.mjs`, and change its import line to:

```js
import { repoRow, sortRepos, pullSheet, branchSheet, resultLine } from '../ui/repos.js';
```

```js
test('pull sheet: exact command, plain title', () => {
  const s = pullSheet(repoRow({ ...base, project: 'crm', branch: 'dev', behind: 26 }));
  assert.deepEqual(s, {
    title: 'Pull 26 commits into dev?',
    command: 'git pull --ff-only',
    note: 'Only fast-forwards. Stops if it would need a merge.',
    ok: 'Pull',
  });
  assert.equal(pullSheet(repoRow({ ...base, behind: 1 })).title, 'Pull 1 commit into main?');
});

test('branch sheet: why each branch is old, one delete button', () => {
  const s = branchSheet(repoRow({ ...base, project: 'shop', merged: ['feat/a'], gone: ['fix/b', 'fix/c'] }));
  assert.equal(s.title, '3 old branches in shop');
  assert.match(s.note, /Only your local copies are deleted/);
  assert.deepEqual(s.items, [
    { name: 'feat/a', why: 'merged' },
    { name: 'fix/b', why: 'gone from remote' },
    { name: 'fix/c', why: 'gone from remote' },
  ]);
  assert.equal(s.ok, 'Delete 3');
});

test('result lines after an action', () => {
  assert.deepEqual(resultLine('pull', 26), { text: 'Pulled 26 commits', ok: true });
  assert.deepEqual(resultLine('pull', 1), { text: 'Pulled 1 commit', ok: true });
  assert.deepEqual(resultLine('delete', ['a', 'b']), { text: 'Deleted 2 old branches', ok: true });
  assert.deepEqual(resultLine('delete', []), { text: 'Nothing deleted: the list changed. Refresh and try again.', ok: false });
  assert.deepEqual(resultLine('error', 'Not possible to fast-forward, aborting.'), { text: 'Not possible to fast-forward, aborting.', ok: false });
});

test('rows carry the behind count for the pull sheet', () => {
  assert.equal(repoRow({ ...base, behind: 4 }).behind, 4);
});
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd app && node --test test/repos.test.mjs`
Expected: FAIL, `pullSheet` is not exported.

- [ ] **Step 3: Implement**

In `app/ui/repos.js`, add `behind: r.behind,` to the object returned by `repoRow` (after `canPull`). Then append:

```js
// What the confirm sheets say. The command is shown exactly as it will run.
export function pullSheet(row) {
  return {
    title: `Pull ${plural(row.behind, 'commit')} into ${row.branch}?`,
    command: 'git pull --ff-only',
    note: 'Only fast-forwards. Stops if it would need a merge.',
    ok: 'Pull',
  };
}

export function branchSheet(row) {
  const n = row.oldBranches.length;
  return {
    title: `${plural(n, 'old branch', 'old branches')} in ${row.name}`,
    note: 'Their work is already in the main branch, or their pull request was merged and the branch was deleted on the remote. Only your local copies are deleted.',
    items: row.oldBranches.map((b) => ({ name: b.name, why: b.reason === 'merged' ? 'merged' : 'gone from remote' })),
    ok: `Delete ${n}`,
  };
}

// The line a row shows after an action, until the next refresh.
export function resultLine(kind, value) {
  if (kind === 'pull') return { text: `Pulled ${plural(value, 'commit')}`, ok: true };
  if (kind === 'delete') {
    return value.length
      ? { text: `Deleted ${plural(value.length, 'old branch', 'old branches')}`, ok: true }
      : { text: 'Nothing deleted: the list changed. Refresh and try again.', ok: false };
  }
  return { text: String(value), ok: false };
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd app && npm test`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add app/ui/repos.js app/test/repos.test.mjs
git commit -m "Repos: confirm-sheet and result text for pull and old branches"
```

---

### Task 6: Sheet, action strip and Terminal setting (markup and styles)

**Files:**
- Modify: `app/ui/index.html`
- Modify: `app/ui/panel.css`
- Modify: `app/ui/icons.js`, `app/test/icons.test.mjs`

- [ ] **Step 1: Two icons**

In `app/ui/icons.js`, add to `PATHS` (after `spark`):

```js
  down: '<path d="M12 4v12M6 11l6 6 6-6M5 20h14"/>',
  term: '<rect x="3" y="4.5" width="18" height="15" rx="2"/><path d="M7 9.5l3 2.5-3 2.5M12.5 15h4"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-12M9 7V4h6v3"/>',
```

In `app/test/icons.test.mjs`, add `'down', 'term', 'trash'` to the names array in the loop.

Run: `cd app && npm test` → all pass.

- [ ] **Step 2: Markup**

In `app/ui/index.html`, insert right before `<p class="note" id="note" hidden></p>`:

```html
<div class="dim" id="dim" hidden></div>
<div class="sheet" id="sheet" role="dialog" aria-modal="true" aria-labelledby="sheet-title" hidden>
  <b id="sheet-title"></b>
  <div id="sheet-body"></div>
  <div class="sheet-actions">
    <button class="btn" id="sheet-cancel">Cancel</button>
    <button class="btn primary" id="sheet-ok"></button>
  </div>
</div>
```

In the Settings view, insert after the Jump hotkey's `<p class="set-msg" id="hotkey-msg" ...></p>`:

```html
  <label class="set-row">
    <span><b>Terminal</b><small>Opened by the Terminal button on Repos.</small></span>
    <select id="terminal" class="pick"></select>
  </label>
```

- [ ] **Step 3: Styles**

Append to `app/ui/panel.css`:

```css
/* Repos: the selected row shows its actions; a result replaces the status. */
#repos li { cursor: pointer; }
#repos li.selected { background: var(--fill); }
.row-actions { grid-column: 1 / -1; display: flex; gap: 6px; padding: 6px 0 2px 14px; }
.row-actions .btn { padding: 3px 9px; font-size: 11.5px; }
.row-actions .btn.ghost { background: none; color: var(--meta); padding: 3px 4px; }
.row-actions .btn.ghost:hover { color: var(--ink); }
.line.result { display: flex; align-items: center; gap: 6px; }

/* Confirm sheet: slides up over the view; shows the exact command. */
.dim { position: absolute; inset: 0; z-index: 20; background: rgba(0,0,0,.45); }
.sheet { position: absolute; left: 8px; right: 8px; bottom: 8px; z-index: 21; display: flex; flex-direction: column; gap: 8px;
  padding: 12px; border-radius: var(--r-sheet); background: var(--bg); border: 1px solid var(--line); box-shadow: 0 10px 30px rgba(0,0,0,.35);
  animation: sheet-in .18s ease-out; }
@keyframes sheet-in { from { transform: translateY(12px); opacity: 0; } }
.sheet p { margin: 0; color: var(--meta); font-size: 11.5px; line-height: 1.45; }
.sheet .cmd { font: 11px ui-monospace, SFMono-Regular, Menlo, monospace; padding: 7px 8px; border-radius: 7px; background: var(--fill); color: var(--ink); word-break: break-all; user-select: text; -webkit-user-select: text; }
.sheet ul { list-style: none; margin: 0; padding: 0; max-height: 150px; overflow-y: auto; }
.sheet li { font: 11.5px ui-monospace, SFMono-Regular, Menlo, monospace; padding: 2px 0; }
.sheet li span { font-family: -apple-system, BlinkMacSystemFont, sans-serif; color: var(--meta); }
.sheet-actions { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
.pick { font: inherit; font-size: 12px; color: var(--ink); background: var(--fill); border: 0; border-radius: 7px; padding: 4px 6px; flex: none; }
```

- [ ] **Step 4: Commit**

```bash
git add app/ui/index.html app/ui/panel.css app/ui/icons.js app/test/icons.test.mjs
git commit -m "Panel: confirm sheet, row action styles, Terminal setting row"
```

---

### Task 7: Wiring

**Files:**
- Modify: `app/ui/panel.js`

- [ ] **Step 1: Imports**

Change the repos import line to:

```js
import { repoRow, sortRepos, pullSheet, branchSheet, resultLine } from './repos.js';
```

- [ ] **Step 2: The sheet**

Add after the `// --- Navigation ---` section's `closeMenu` function:

```js
// One confirm sheet for every action that changes something. `run` does the
// work; the sheet closes when it settles, and `run` reports its own result.
let sheetRun = null;
function openSheet({ title, body, ok, run }) {
  closeMenu();
  $('sheet-title').textContent = title;
  $('sheet-body').replaceChildren(...body);
  $('sheet-ok').textContent = ok;
  $('sheet-ok').disabled = false;
  sheetRun = run;
  $('dim').hidden = $('sheet').hidden = false;
  $('sheet-ok').focus();
}
function closeSheet() {
  $('dim').hidden = $('sheet').hidden = true;
  sheetRun = null;
}
$('sheet-cancel').addEventListener('click', closeSheet);
$('dim').addEventListener('click', closeSheet);
$('sheet-ok').addEventListener('click', async () => {
  if (!sheetRun) return;
  const run = sheetRun;
  $('sheet-ok').disabled = true;
  $('sheet-ok').textContent = 'Working…';
  try {
    await run();
  } finally {
    closeSheet();
  }
});
```

In the global `keydown` handler, make Esc close the sheet first. Replace:

```js
  if (e.key === 'Escape') {
    if (!$('menu').hidden) return closeMenu();
```

with:

```js
  if (e.key === 'Escape') {
    if (!$('sheet').hidden) return closeSheet();
    if (!$('menu').hidden) return closeMenu();
```

and make digit shortcuts inactive while a sheet is open by changing the digit line's condition from `!isTyping(e.target)` to `!isTyping(e.target) && $('sheet').hidden`.

- [ ] **Step 3: Repo rows with actions and results**

Replace the whole `function renderRepos() { ... }` with:

```js
let selectedRepo = null; // path of the row showing its actions
const repoResults = new Map(); // path -> { text, ok }, until the next read

function renderRepos() {
  $('repos-empty').hidden = repos.length > 0;
  $('repos').replaceChildren(
    ...repos.map((r) => {
      const li = el('li', r.path === selectedRepo ? 'dotted selected' : 'dotted');
      li.tabIndex = 0;
      const result = repoResults.get(r.path);
      const name = el('span', 'name');
      name.append(el('span', `dot ${result ? (result.ok ? 'done' : 'failed') : r.dot}`), el('span', '', r.name));
      const branch = el('span', 'right mono', r.branch);
      branch.title = r.path;
      li.append(name, branch, el('span', 'line', result ? result.text : r.status));
      if (r.path === selectedRepo) li.append(repoActions(r));
      const toggle = () => {
        selectedRepo = selectedRepo === r.path ? null : r.path;
        renderRepos();
      };
      li.addEventListener('click', (e) => !e.target.closest('.row-actions') && toggle());
      li.addEventListener('keydown', (e) => e.key === 'Enter' && e.target === li && toggle());
      return li;
    }),
  );
}

function actionButton(label, iconName, cls, onClick) {
  const b = el('button', cls);
  b.innerHTML = icon(iconName, 's');
  b.append(el('span', '', label));
  b.addEventListener('click', onClick);
  return b;
}

function repoActions(r) {
  const box = el('div', 'row-actions');
  if (r.canPull) box.append(actionButton('Pull', 'down', 'btn', () => confirmPull(r)));
  if (r.oldBranches.length) box.append(actionButton(`Delete ${r.oldBranches.length} old`, 'trash', 'btn', () => confirmDelete(r)));
  box.append(actionButton('Terminal', 'term', 'btn ghost', () => openTerminal(r)));
  return box;
}

async function afterAction(r, result) {
  repoResults.set(r.path, result);
  selectedRepo = null;
  await loadToday(true); // re-read git; the result line stays until the next read after this
}

function confirmPull(r) {
  const s = pullSheet(r);
  openSheet({
    title: s.title,
    body: [el('p', '', r.name), el('div', 'cmd', s.command), el('p', '', s.note)],
    ok: s.ok,
    run: async () => {
      let res;
      try {
        res = resultLine('pull', await invoke('repo_pull', { path: r.path }));
      } catch (e) {
        res = resultLine('error', e);
      }
      await afterAction(r, res);
    },
  });
}

function confirmDelete(r) {
  const s = branchSheet(r);
  const list = el('ul');
  for (const it of s.items) {
    const li = el('li', '', it.name);
    li.append(el('span', '', ` · ${it.why}`));
    list.append(li);
  }
  openSheet({
    title: s.title,
    body: [el('p', '', s.note), list],
    ok: s.ok,
    run: async () => {
      let res;
      try {
        res = resultLine('delete', await invoke('repo_delete_branches', { path: r.path, names: s.items.map((i) => i.name) }));
      } catch (e) {
        res = resultLine('error', e);
      }
      await afterAction(r, res);
    },
  });
}

async function openTerminal(r) {
  try {
    await invoke('open_terminal', { path: r.path });
    window.__TAURI__.window.getCurrentWindow().hide();
  } catch (e) {
    note(String(e));
  }
}
```

A result must survive exactly one re-read: the one `afterAction` triggers. Any later read clears it. Add a flag next to `repoResults` (just below `const repoResults = new Map();` from the block above):

```js
let keepResults = false; // set by afterAction for exactly one re-read
```

In `afterAction`, set it before re-reading. Its body becomes:

```js
async function afterAction(r, result) {
  repoResults.set(r.path, result);
  selectedRepo = null;
  keepResults = true;
  await loadToday(true);
}
```

In `loadToday`, change the start of the `try` block from:

```js
  try {
    const [scan] = await Promise.all([invoke('scan_today'), loadLive()]);
```

to:

```js
  const keep = keepResults ? new Map(repoResults) : new Map();
  keepResults = false;
  repoResults.clear();
  try {
    const [scan] = await Promise.all([invoke('scan_today'), loadLive()]);
```

and, in the same function, right after `repos = sortRepos(repoList.map(repoRow));`, add:

```js
    for (const [k, v] of keep) repoResults.set(k, v);
```

(The early `return renderToday(), renderRepos();` for a cached read sits above this and does not touch results, so a result also stays while the cache is fresh, which is what the person expects when switching tabs.)

- [ ] **Step 4: Terminal setting**

Replace `function showSettings() { ... }` with:

```js
async function showSettings() {
  $('hotkey').textContent = hotkeyLabel(prefs.hotkey);
  $('notify').checked = prefs.notifyLimits;
  $('recap-claude').checked = prefs.recapWithClaude;
  const apps = await invoke('terminals').catch(() => []);
  const sel = $('terminal');
  sel.replaceChildren(...apps.map((a) => new Option(a, a)));
  if (!apps.length) sel.replaceChildren(new Option('None found', ''));
  sel.disabled = apps.length < 2;
  sel.value = apps.includes(prefs.terminal) ? prefs.terminal : apps[0] || '';
}
```

and add after the `recap-claude` change listener:

```js
$('terminal').addEventListener('change', (e) => saveSettings({ terminal: e.target.value || null }));
```

- [ ] **Step 5: Checks and commit**

Run:
```bash
cd app && node -e "import('node:fs').then(f=>f.writeFileSync('/tmp/p.mjs',f.readFileSync('ui/panel.js')))" && node --check /tmp/p.mjs && npm test
```
Expected: syntax OK, tests pass.

Every `$('…')` id in `panel.js` must exist in `index.html`:
```bash
cd app && comm -23 <(grep -o "\$('[a-z0-9-]*')" ui/panel.js | sed "s/\$('//;s/')//" | sort -u) <(grep -o 'id="[a-z0-9-]*"' ui/index.html | sed 's/id="//;s/"//' | sort -u)
```
Expected: no output.

```bash
git add app/ui/panel.js
git commit -m "Repos: Pull, Delete old branches and Terminal from the selected row"
```

---

### Task 8: Preview stubs and visual check

**Files:**
- Modify: `app/scripts/preview.mjs`

- [ ] **Step 1: Stubs**

In `app/scripts/preview.mjs`, inside the stub's `handlers` object, add after `quit: () => null,`:

```js
  repo_pull: () => 3,
  repo_delete_branches: ({ names }) => names,
  terminals: () => ['Ghostty', 'iTerm', 'Terminal'],
  open_terminal: () => null,
  test_notification: () => null,
  polish_recap: () => 'Done\n- preview: polished text',
```

(The stub never touches git; actions are checked against real repos in the Rust tests.)

- [ ] **Step 2: Visual check**

Run `cd app && npm run preview` and open `http://localhost:5174`. Check, in dark and light:
- Repos: clicking a row highlights it and shows its actions: **Pull** only on rows with commits to pull and nothing uncommitted, **Delete N old** only with old branches, **Terminal** always. Clicking the row again hides them; only one row is open at a time.
- **Pull** opens the sheet: title "Pull N commits into <branch>?", the repo name, `git pull --ff-only` in a code box, the note, Cancel and Pull. Esc, Cancel and clicking the dimmed area close it. Pull shows "Working…", closes, and the row shows "Pulled 3 commits" with a green dot.
- **Delete N old** lists each branch with "merged" or "gone from remote" and a "Delete N" button; after it the row shows "Deleted N old branches".
- Settings shows a Terminal row with a picker listing Ghostty, iTerm, Terminal.
- Number keys don't switch tabs while a sheet is open.

Fix anything that fails, re-run `npm test`, commit each fix with a message saying what was wrong.

- [ ] **Step 3: Real app check (macOS)**

Run `cd app && npm run tauri dev`. In a scratch repo that is behind its upstream, use Pull and confirm the row updates; use Terminal with each installed terminal app (pick it in Settings) and confirm it opens in the repo folder. Note any app that opens in the wrong folder in the PR description.

- [ ] **Step 4: Commit**

```bash
git add app/scripts/preview.mjs
git commit -m "Preview: stubs for repo actions and terminals"
```

---

### Task 9: PR into dev

- [ ] **Step 1: All suites**

```bash
cd app && npm test && cd src-tauri && cargo test --lib && cd ../../wrapped && npm test
```
Expected: all pass.

- [ ] **Step 2: Push and open the PR**

```bash
git branch --show-current   # feat/repo-actions
git push -u origin feat/repo-actions
gh pr create --base dev --title "Panel redesign, phase 2: Pull, Delete old branches, Terminal" --body "Phase 2 of docs/superpowers/specs/2026-10-04-panel-redesign-design.md.

- Repos rows show actions when selected: Pull (fast-forward only, nothing uncommitted), Delete N old (merged with -d, gone with -D), Terminal.
- Pull and Delete go through a confirm sheet that shows the exact command; the row then shows the result.
- Rust re-reads the repository before acting and refuses anything the board would not offer; only a repository's own top folder is accepted; branch names are passed after \`--\`; git never prompts.
- Terminal opens the chosen app in the repo folder with \`open -a\` (macOS: Ghostty, iTerm, Warp, Terminal; no Automation prompt) or \`wt -d\` (Windows). Picker in Settings.
- Tests: Rust tests against real temporary repositories with a bare remote (pull, refusal with local changes, refusal when a merge is needed, root check, branch deletion rules); JS tests for sheet and result text."
```

The user merges the PR.

---

## Self-review notes

- Spec "Actions" coverage for phase 2: Pull in app with confirm (Tasks 1, 4, 5, 7), Delete old branches with reasons, `-d`/`-D`, local only (Tasks 2, 4, 5, 7), Terminal new tab in repo folder (Tasks 3, 4, 7), terminal picker auto-detected with fallback (Tasks 3, 7), errors show git's first line in the row with a failed dot (Tasks 1, 5, 7), no shell and `GIT_TERMINAL_PROMPT=0` (Task 1), Windows via `wt -d` (Task 3). Copy fallback when no terminal is found is replaced by a clear error note ("No supported terminal app found."), since there is no command to copy for "open a folder".
- Names across tasks: `pull`, `delete_old_branches`, `git_run`, `check_root` (Rust); `installed`, `choose`, `launch`, `open` (terminal.rs); commands `repo_pull`, `repo_delete_branches`, `terminals`, `open_terminal`; JS `pullSheet`, `branchSheet`, `resultLine`, `row.behind`; ids `dim`, `sheet`, `sheet-title`, `sheet-body`, `sheet-ok`, `sheet-cancel`, `terminal`.
