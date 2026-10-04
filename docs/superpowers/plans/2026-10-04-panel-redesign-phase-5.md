# Panel Redesign, Phase 5 (Recipes) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Named kick-off prompts per repository ("recipes") that the person can **Start** (a new Claude session in a terminal tab, in that repo, with the prompt), **Copy**, edit and delete — plus suggestions mined from prompts they keep retyping in that repo, one click to save.

**Architecture:** A new Rust module `recipes.rs` stores recipes as JSON in the app's config folder (`recipes.json`, keyed by repository root) and mines suggestions: human prompts from the last 30 days of Claude logs, de-duplicated by line `uuid` (resumed/forked copies), grouped by repository root and normalised text, kept when typed 3+ times. Mining is slow-ish (it reads every recent log), so it runs on demand and is cached for 10 minutes. Four Tauri commands: `recipes_for`, `save_recipe`, `delete_recipe`, `start_recipe`; all take a repository path that must be a repository's top folder, and Start looks the prompt up on the backend by id. In the panel, `ui/recipes.js` (pure, tested) shapes the sheet; the existing confirm sheet gains an optional OK button and a "stay open" result so it can host a list and an editor; Repos rows get a **Recipes** action.

**Tech Stack:** Rust (serde, serde_json, rayon), Tauri 2, vanilla JS, `node --test`, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-10-04-panel-redesign-design.md` → Repos (Recipes), New data (Recipes), Actions (Recipe Start).

**Decisions:**
- A suggestion is a human prompt (same rule as everywhere: `logs::is_human_prompt`) of 30–2000 characters, typed 3+ times in the same repository in 30 days, compared after collapsing whitespace and ignoring case. Short replies ("yes", "continue") never qualify because of the 30-character floor. Top 5 per repo, most frequent first. Suggestions equal to a saved recipe's prompt are hidden.
- Recipe names are 1–60 characters, prompts 1–8000, both trimmed.
- **Start** runs `claude "<prompt>"` in the repo folder via `terminal::open_command` (phase 3); it does not ask for confirmation, because it only opens a new session the person then sees.
- Prompts never leave the machine.

---

## Branch

```bash
cd /Users/akhilmisri/Business/Products/agent-island
git fetch -q
git worktree add -b feat/recipes ../agent-island-recipes origin/dev
cd ../agent-island-recipes
```

Work only in that worktree. Never commit to `main` or `dev`. No Claude attribution lines in commits.

## File structure

| File | Responsibility |
|---|---|
| `app/src-tauri/src/logs.rs` | `prompt_text` → `pub(crate)` |
| `app/src-tauri/src/repos.rs` | `root_of` (public wrapper of `repo_root`), `check_root` → `pub(crate)` |
| `app/src-tauri/src/recipes.rs` (new) | Storage (load, save, upsert, remove) and suggestion mining |
| `app/src-tauri/src/lib.rs` | `pub mod recipes;`, suggestion cache, four commands |
| `app/src-tauri/examples/recipes.rs` (new) | Print suggestions per repo, for checking by hand |
| `app/ui/recipes.js` (new) | Sheet model, validation, suggested names |
| `app/test/recipes.test.mjs` (new) | Tests |
| `app/ui/index.html`, `panel.css`, `panel.js`, `icons.js` | Sheet extension, Recipes action, list and editor |
| `app/scripts/preview.mjs` | Stubs |

---

### Task 1: Recipe storage

**Files:**
- Create: `app/src-tauri/src/recipes.rs`
- Modify: `app/src-tauri/src/lib.rs` (`pub mod recipes;`, alphabetical)

- [ ] **Step 1: Write the failing tests**

Create `app/src-tauri/src/recipes.rs` with only:

```rust
//! Recipes: named kick-off prompts per repository, and suggestions from
//! prompts the person keeps retyping there.

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ai-recipes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn add_edit_remove_and_persist() {
        let dir = tmp("store");
        let mut s = load(&dir);
        assert!(s.is_empty());
        let id = upsert(&mut s, "/w/shop", None, "  Fresh start ", "Pull all repos to dev, run migrations").unwrap();
        assert_eq!(s["/w/shop"], vec![Recipe { id: id.clone(), name: "Fresh start".into(), prompt: "Pull all repos to dev, run migrations".into() }]);
        let same = upsert(&mut s, "/w/shop", Some(&id), "Fresh start", "Pull, migrate, start services").unwrap();
        assert_eq!(same, id);
        assert_eq!(s["/w/shop"].len(), 1);
        assert_eq!(s["/w/shop"][0].prompt, "Pull, migrate, start services");
        let second = upsert(&mut s, "/w/shop", None, "E2E", "Run the QA agent").unwrap();
        assert_ne!(second, id);
        save(&dir, &s).unwrap();
        let mut back = load(&dir);
        assert_eq!(back, s);
        assert!(remove(&mut back, "/w/shop", &id));
        assert!(!remove(&mut back, "/w/shop", &id));
        assert_eq!(back["/w/shop"].len(), 1);
        assert!(remove(&mut back, "/w/shop", &second));
        assert!(!back.contains_key("/w/shop"), "empty repos are dropped");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validation() {
        let mut s = Store::new();
        assert_eq!(upsert(&mut s, "/w", None, "  ", "x"), Err("Give the recipe a name.".to_string()));
        assert_eq!(upsert(&mut s, "/w", None, "n", " "), Err("Write the prompt to start with.".to_string()));
        assert_eq!(upsert(&mut s, "/w", None, &"n".repeat(61), "x"), Err("Keep the name under 60 characters.".to_string()));
        assert_eq!(upsert(&mut s, "/w", None, "n", &"x".repeat(8001)), Err("Keep the prompt under 8,000 characters.".to_string()));
        assert_eq!(upsert(&mut s, "/w", Some("nope"), "n", "x"), Err("That recipe no longer exists.".to_string()));
        assert!(s.is_empty());
    }

    #[test]
    fn unreadable_file_means_no_recipes() {
        let dir = tmp("bad");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("recipes.json"), "not json").unwrap();
        assert!(load(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

Add `pub mod recipes;` to `app/src-tauri/src/lib.rs`.

- [ ] **Step 2: Run to verify they fail**

Run: `cd app/src-tauri && cargo test --lib recipes::`
Expected: compile errors (`load`, `upsert`, `Recipe`, `Store` missing).

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]`:

```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub prompt: String,
}

/// Recipes by repository root.
pub type Store = BTreeMap<String, Vec<Recipe>>;

fn file(dir: &Path) -> PathBuf {
    dir.join("recipes.json")
}

/// Missing or unreadable: no recipes.
pub fn load(dir: &Path) -> Store {
    std::fs::read(file(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(dir: &Path, s: &Store) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join("recipes.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    std::fs::rename(tmp, file(dir))
}

fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!("r{}{}", crate::logs::now_ms(), N.fetch_add(1, Ordering::Relaxed))
}

/// Adds a recipe (no `id`) or replaces one. Returns its id.
pub fn upsert(s: &mut Store, repo: &str, id: Option<&str>, name: &str, prompt: &str) -> Result<String, String> {
    let (name, prompt) = (name.trim(), prompt.trim());
    if name.is_empty() {
        return Err("Give the recipe a name.".into());
    }
    if prompt.is_empty() {
        return Err("Write the prompt to start with.".into());
    }
    if name.chars().count() > 60 {
        return Err("Keep the name under 60 characters.".into());
    }
    if prompt.chars().count() > 8000 {
        return Err("Keep the prompt under 8,000 characters.".into());
    }
    let list = s.entry(repo.to_string()).or_default();
    let id = match id {
        Some(id) => {
            let Some(r) = list.iter_mut().find(|r| r.id == id) else {
                if list.is_empty() {
                    s.remove(repo);
                }
                return Err("That recipe no longer exists.".into());
            };
            r.name = name.into();
            r.prompt = prompt.into();
            id.to_string()
        }
        None => {
            let id = new_id();
            list.push(Recipe { id: id.clone(), name: name.into(), prompt: prompt.into() });
            id
        }
    };
    Ok(id)
}

/// Removes a recipe; drops the repository when it has none left.
pub fn remove(s: &mut Store, repo: &str, id: &str) -> bool {
    let Some(list) = s.get_mut(repo) else { return false };
    let before = list.len();
    list.retain(|r| r.id != id);
    let removed = list.len() != before;
    if list.is_empty() {
        s.remove(repo);
    }
    removed
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd app/src-tauri && cargo test --lib recipes:: && cargo clippy --lib 2>&1 | grep -A3 recipes.rs`
Expected: 3 pass; no clippy output for recipes.rs.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/recipes.rs app/src-tauri/src/lib.rs
git commit -m "Recipes: store named prompts per repository"
```

---

### Task 2: Suggestions from repeated prompts

**Files:**
- Modify: `app/src-tauri/src/recipes.rs`, `app/src-tauri/src/logs.rs`, `app/src-tauri/src/repos.rs`
- Create: `app/src-tauri/examples/recipes.rs`

- [ ] **Step 1: Share helpers**

In `app/src-tauri/src/logs.rs`: change `fn prompt_text(d: &CLine) -> Option<String> {` to `pub(crate) fn prompt_text(d: &CLine) -> Option<String> {`.

In `app/src-tauri/src/repos.rs`:
- change `fn check_root(path: &Path) -> Result<(), String> {` to `pub(crate) fn check_root(path: &Path) -> Result<(), String> {`
- add after `fn repo_root`:

```rust
/// The repository folder a working directory belongs to (worktrees fold in).
pub fn root_of(cwd: &Path) -> Option<PathBuf> {
    repo_root(cwd)
}
```

- [ ] **Step 2: Write the failing tests**

Add inside `mod tests` in `recipes.rs`:

```rust
    use std::io::Write;

    const NOW: i64 = 1_790_000_000_000;
    fn ts(ms: i64) -> String {
        chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }
    fn prompt(uuid: &str, at: i64, cwd: &str, text: &str) -> String {
        format!(r#"{{"type":"user","uuid":"{uuid}","timestamp":"{}","cwd":"{cwd}","origin":{{"kind":"human"}},"message":{{"role":"user","content":"{text}"}}}}"#, ts(at))
    }
    fn log(dir: &Path, name: &str, lines: &[String]) {
        std::fs::create_dir_all(dir).unwrap();
        let mut f = std::fs::File::create(dir.join(name)).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }
    const LONG: &str = "switch to main and pull, then raise a PR for the current branch";

    #[test]
    fn repeated_prompts_become_suggestions_per_repo() {
        let d = tmp("mine");
        let p = d.join("proj");
        let shop = "/w/shop";
        log(&p, "a.jsonl", &[
            prompt("u1", NOW - 3_600_000, shop, LONG),
            prompt("u2", NOW - 2_600_000, "/w/shop/.worktrees/x", "Switch to main and pull,   then raise a PR for the current branch"),
            prompt("u3", NOW - 1_600_000, shop, "SWITCH TO MAIN AND PULL, THEN RAISE A PR FOR THE CURRENT BRANCH"),
            prompt("u4", NOW - 600_000, shop, "yes"),
            prompt("u5", NOW - 500_000, shop, "yes"),
            prompt("u6", NOW - 400_000, shop, "yes"),
            prompt("u7", NOW - 300_000, "/w/api", LONG),
        ]);
        // A resumed copy repeats u1..u3: they must not count twice.
        log(&p, "b.jsonl", &[
            prompt("u1", NOW - 3_600_000, shop, LONG),
            prompt("u2", NOW - 2_600_000, "/w/shop/.worktrees/x", "Switch to main and pull,   then raise a PR for the current branch"),
        ]);
        let root_of = |cwd: &str| Some(if cwd.starts_with("/w/shop") { "/w/shop".to_string() } else { cwd.to_string() });
        let m = mine(&d, 0, &root_of);
        let shop_s = &m["/w/shop"];
        assert_eq!(shop_s.len(), 1, "\"yes\" is too short to suggest");
        assert_eq!(shop_s[0].count, 3);
        assert_eq!(shop_s[0].last, NOW - 1_600_000);
        assert_eq!(shop_s[0].text, "SWITCH TO MAIN AND PULL, THEN RAISE A PR FOR THE CURRENT BRANCH", "the latest wording, whitespace collapsed");
        assert!(!m.contains_key("/w/api"), "once in api is not a habit");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn same_prompt_key_ignores_case_and_spaces() {
        assert_eq!(key("  Switch  to main\nand pull "), "switch to main and pull");
    }
```

- [ ] **Step 3: Run to verify they fail**

Run: `cd app/src-tauri && cargo test --lib recipes::`
Expected: compile errors (`mine`, `key` missing).

- [ ] **Step 4: Implement**

Add to `recipes.rs` above `#[cfg(test)]`:

```rust
use std::collections::{HashMap, HashSet};

const MIN_CHARS: usize = 30;
const MAX_CHARS: usize = 2000;
const MIN_TIMES: u32 = 3;
const TOP: usize = 5;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// The latest wording, whitespace collapsed.
    pub text: String,
    pub count: u32,
    /// When it was last typed, ms.
    pub last: i64,
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How two prompts are compared: whitespace collapsed, case ignored.
pub fn key(s: &str) -> String {
    collapse(s).to_lowercase()
}

fn jsonl_since(dir: &Path, since: i64, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if p.file_name().is_some_and(|n| n == "subagents") {
                continue;
            }
            jsonl_since(&p, since, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            let mtime = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
            if mtime >= since {
                out.push(p);
            }
        }
    }
}

/// (uuid, cwd, at, text) for every human prompt in one log since `since`.
fn prompts_in(path: &Path, since: i64) -> Vec<(String, String, i64, String)> {
    use crate::logs::{is_human_prompt, prompt_text, CLine};
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut out = Vec::new();
    for line in text.lines() {
        // Cheap filter before parsing: only user lines can be prompts.
        if !line.contains("\"type\":\"user\"") {
            continue;
        }
        let Ok(d) = serde_json::from_str::<CLine>(line) else { continue };
        if d.kind != Some("user") || d.is_sidechain == Some(true) || !is_human_prompt(&d) {
            continue;
        }
        let Some(at) = d.timestamp.and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp_millis()) else { continue };
        if at < since {
            continue;
        }
        let (Some(uuid), Some(cwd), Some(p)) = (d.uuid.clone(), d.cwd.clone(), prompt_text(&d)) else { continue };
        out.push((uuid, cwd, at, p));
    }
    out
}

/// Prompts typed at least three times in the same repository since `since`,
/// by repository root. `root_of` maps a working directory to its repository
/// (None: not a repository; skipped).
pub fn mine(projects: &Path, since: i64, root_of: &(dyn Fn(&str) -> Option<String> + Sync)) -> HashMap<String, Vec<Suggestion>> {
    use rayon::prelude::*;
    let mut files = Vec::new();
    jsonl_since(projects, since, &mut files);
    let all: Vec<(String, String, i64, String)> = files.par_iter().flat_map(|p| prompts_in(p, since)).collect();

    let mut seen = HashSet::new();
    let mut roots: HashMap<String, Option<String>> = HashMap::new();
    // (root, key) -> (count, last, latest text)
    let mut groups: HashMap<(String, String), (u32, i64, String)> = HashMap::new();
    for (uuid, cwd, at, text) in all {
        if !seen.insert(uuid) {
            continue; // a resumed or forked copy of a prompt already counted
        }
        let n = text.trim().chars().count();
        if !(MIN_CHARS..=MAX_CHARS).contains(&n) {
            continue;
        }
        let root = roots.entry(cwd.clone()).or_insert_with(|| root_of(&cwd)).clone();
        let Some(root) = root else { continue };
        let g = groups.entry((root, key(&text))).or_insert((0, 0, String::new()));
        g.0 += 1;
        if at >= g.1 {
            g.1 = at;
            g.2 = collapse(&text);
        }
    }
    let mut out: HashMap<String, Vec<Suggestion>> = HashMap::new();
    for ((root, _), (count, last, text)) in groups {
        if count >= MIN_TIMES {
            out.entry(root).or_default().push(Suggestion { text, count, last });
        }
    }
    for list in out.values_mut() {
        list.sort_by(|a, b| b.count.cmp(&a.count).then(b.last.cmp(&a.last)));
        list.truncate(TOP);
    }
    out
}
```

Create `app/src-tauri/examples/recipes.rs`:

```rust
//! Prints recipe suggestions per repository (counts and the first 60
//! characters only), for checking by hand.
//! cargo run --release --example recipes
use agent_island_lib::{logs, recipes, repos};

fn main() {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("no HOME"));
    let roots = logs::Roots::default_for(&home);
    let t = std::time::Instant::now();
    let root_of = |cwd: &str| repos::root_of(std::path::Path::new(cwd)).map(|p| p.to_string_lossy().into_owned());
    let m = recipes::mine(&roots.claude, logs::now_ms() - 30 * 86_400_000, &root_of);
    eprintln!("{} repos with suggestions in {:.2}s", m.len(), t.elapsed().as_secs_f64());
    for (repo, list) in &m {
        println!("{repo}");
        for s in list {
            println!("  {}×  {}", s.count, s.text.chars().take(60).collect::<String>());
        }
    }
}
```

- [ ] **Step 5: Run tests, then on real data**

Run: `cd app/src-tauri && cargo test --lib recipes:: && cargo clippy --lib 2>&1 | grep -A3 -E "recipes.rs|repos.rs|logs.rs" ; cargo run -q --release --example recipes 2>&1 | head -3`
Expected: all recipes tests pass; no new clippy output (the pre-existing `logs.rs:161` warning stays); the example's first line reports how many repos have suggestions and the time (expect a few seconds at most). In the report, give only the count and timing — not the prompts.

- [ ] **Step 6: Commit**

```bash
git add app/src-tauri/src/recipes.rs app/src-tauri/src/logs.rs app/src-tauri/src/repos.rs app/src-tauri/examples/recipes.rs
git commit -m "Recipes: suggest prompts typed three or more times in a repository"
```

---

### Task 3: Commands

**Files:**
- Modify: `app/src-tauri/src/lib.rs`

- [ ] **Step 1: Add the commands**

In `app/src-tauri/src/lib.rs`, after the `open_task_run` command:

```rust
#[derive(serde::Serialize)]
struct RecipeSheet {
    recipes: Vec<recipes::Recipe>,
    suggestions: Vec<recipes::Suggestion>,
}

/// Suggestions for every repository, cached for ten minutes (mining reads
/// 30 days of logs).
fn suggestions_for(r: &logs::Roots, repo: &str) -> Vec<recipes::Suggestion> {
    type Cache = (i64, std::collections::HashMap<String, Vec<recipes::Suggestion>>);
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    let now = logs::now_ms();
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if c.as_ref().is_none_or(|(at, _)| now - at > 600_000) {
        let root_of = |cwd: &str| repos::root_of(std::path::Path::new(cwd)).map(|p| p.to_string_lossy().into_owned());
        *c = Some((now, recipes::mine(&r.claude, now - 30 * 86_400_000, &root_of)));
    }
    c.as_ref().and_then(|(_, m)| m.get(repo).cloned()).unwrap_or_default()
}

/// Serialises edits to recipes.json (load, change, save).
static RECIPES_LOCK: Mutex<()> = Mutex::new(());

fn recipe_sheet(app: &AppHandle, repo: &str) -> Result<RecipeSheet, String> {
    let saved = recipes::load(&config_dir(app)?).remove(repo).unwrap_or_default();
    let known: std::collections::HashSet<String> = saved.iter().map(|x| recipes::key(&x.prompt)).collect();
    let suggestions = suggestions_for(&roots(app)?, repo).into_iter().filter(|s| !known.contains(&recipes::key(&s.text))).collect();
    Ok(RecipeSheet { recipes: saved, suggestions })
}

/// A repository's recipes and suggestions.
#[tauri::command]
async fn recipes_for(app: AppHandle, path: String) -> Result<RecipeSheet, String> {
    tauri::async_runtime::spawn_blocking(move || {
        repos::check_root(std::path::Path::new(&path))?;
        recipe_sheet(&app, &path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Adds or edits a recipe; returns the repository's sheet.
#[tauri::command]
async fn save_recipe(app: AppHandle, path: String, id: Option<String>, name: String, prompt: String) -> Result<RecipeSheet, String> {
    tauri::async_runtime::spawn_blocking(move || {
        repos::check_root(std::path::Path::new(&path))?;
        let dir = config_dir(&app)?;
        {
            let _g = RECIPES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut s = recipes::load(&dir);
            recipes::upsert(&mut s, &path, id.as_deref(), &name, &prompt)?;
            recipes::save(&dir, &s).map_err(|e| e.to_string())?;
        }
        recipe_sheet(&app, &path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Deletes a recipe; returns the repository's sheet.
#[tauri::command]
async fn delete_recipe(app: AppHandle, path: String, id: String) -> Result<RecipeSheet, String> {
    tauri::async_runtime::spawn_blocking(move || {
        repos::check_root(std::path::Path::new(&path))?;
        let dir = config_dir(&app)?;
        {
            let _g = RECIPES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut s = recipes::load(&dir);
            recipes::remove(&mut s, &path, &id);
            recipes::save(&dir, &s).map_err(|e| e.to_string())?;
        }
        recipe_sheet(&app, &path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Starts a recipe: a new Claude session in a terminal tab, in the repository.
#[tauri::command]
async fn start_recipe(app: AppHandle, path: String, id: String) -> Result<(), String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let pick = app.state::<Prefs>().0.lock().ok().and_then(|p| p.terminal.clone());
    let scripts = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("scripts");
    tauri::async_runtime::spawn_blocking(move || {
        let repo = std::path::Path::new(&path);
        repos::check_root(repo)?;
        let saved = recipes::load(&config_dir(&app)?).remove(&path).unwrap_or_default();
        let r = saved.into_iter().find(|r| r.id == id).ok_or("That recipe no longer exists.")?;
        let claude = recap::find_claude(&home).ok_or("Could not find the claude command.")?;
        terminal::open_command(pick.as_deref(), repo, &claude, &[r.prompt], &script_name("recipe", &r.id), &scripts)
    })
    .await
    .map_err(|e| e.to_string())?
}
```

Notes:
- `script_name(prefix, name)` already exists in lib.rs (phase 4). Check `terminal::open_command`'s signature before calling; the call assumes `(pick, dir, program, args, name, script_dir)`.
- `Option::is_none_or` needs Rust 1.82+; if the toolchain is older, use `!matches!(c.as_ref(), Some((at, _)) if now - at <= 600_000)`.
- `RECIPES_LOCK` is held across each load → change → save, so two quick saves can't overwrite each other.

Add `recipes_for, save_recipe, delete_recipe, start_recipe` to `tauri::generate_handler![...]` after `open_task_run`.

- [ ] **Step 2: Build, test, lint**

Run: `cd app/src-tauri && cargo build --lib && cargo test --lib && cargo clippy --lib 2>&1 | grep -E "src/(lib|recipes)\.rs" -A3`
Expected: builds; all pass; only the known lib.rs warnings.

- [ ] **Step 3: Commit**

```bash
git add app/src-tauri/src/lib.rs
git commit -m "Commands: recipes_for, save_recipe, delete_recipe, start_recipe"
```

---

### Task 4: Sheet text

**Files:**
- Create: `app/ui/recipes.js`, `app/test/recipes.test.mjs`

- [ ] **Step 1: Write the failing tests**

Create `app/test/recipes.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { recipeSheet, validateRecipe, suggestedName, preview } from '../ui/recipes.js';

test('preview: first line, at most 80 characters', () => {
  assert.equal(preview('Pull all repos to dev\nthen migrate'), 'Pull all repos to dev');
  assert.equal(preview('x'.repeat(100)), `${'x'.repeat(79)}…`);
});

test('sheet model: recipes, suggestions with counts, empty state', () => {
  const s = recipeSheet({
    recipes: [{ id: 'r1', name: 'Fresh start', prompt: 'Pull all repos to dev, run migrations' }],
    suggestions: [{ text: 'switch to main and pull, then raise a PR for the current branch', count: 9, last: 1 }],
  });
  assert.deepEqual(s.recipes, [{ id: 'r1', name: 'Fresh start', prompt: 'Pull all repos to dev, run migrations', preview: 'Pull all repos to dev, run migrations' }]);
  assert.equal(s.suggestions[0].line, 'You typed this 9 times');
  assert.equal(s.suggestions[0].preview, 'switch to main and pull, then raise a PR for the current branch');
  assert.equal(s.empty, false);
  assert.equal(recipeSheet({ recipes: [], suggestions: [] }).empty, true);
});

test('validation mirrors the backend', () => {
  assert.equal(validateRecipe({ name: ' ', prompt: 'x' }), 'Give the recipe a name.');
  assert.equal(validateRecipe({ name: 'n', prompt: '' }), 'Write the prompt to start with.');
  assert.equal(validateRecipe({ name: 'n'.repeat(61), prompt: 'x' }), 'Keep the name under 60 characters.');
  assert.equal(validateRecipe({ name: 'n', prompt: 'x'.repeat(8001) }), 'Keep the prompt under 8,000 characters.');
  assert.equal(validateRecipe({ name: 'Fresh start', prompt: 'Pull' }), null);
});

test('suggested name: first few words, capitalised', () => {
  assert.equal(suggestedName('switch to main and pull, then raise a PR'), 'Switch to main and');
  assert.equal(suggestedName('  run   tests  '), 'Run tests');
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd app && node --test test/recipes.test.mjs`
Expected: FAIL, module not found.

- [ ] **Step 3: Implement**

Create `app/ui/recipes.js`:

```js
// Recipes: named kick-off prompts per repository, and suggestions from
// prompts typed again and again there.

export function preview(text) {
  const first = String(text).split('\n')[0].trim();
  return first.length > 80 ? `${first.slice(0, 79)}…` : first;
}

export function recipeSheet({ recipes, suggestions }) {
  return {
    recipes: recipes.map((r) => ({ ...r, preview: preview(r.prompt) })),
    suggestions: suggestions.map((s) => ({ text: s.text, preview: preview(s.text), line: `You typed this ${s.count} times` })),
    empty: !recipes.length && !suggestions.length,
  };
}

// The same limits the backend enforces, checked before sending.
export function validateRecipe({ name, prompt }) {
  const n = name.trim();
  const p = prompt.trim();
  if (!n) return 'Give the recipe a name.';
  if (!p) return 'Write the prompt to start with.';
  if ([...n].length > 60) return 'Keep the name under 60 characters.';
  if ([...p].length > 8000) return 'Keep the prompt under 8,000 characters.';
  return null;
}

export function suggestedName(text) {
  const words = text.trim().split(/\s+/).slice(0, 4).join(' ').replace(/[,.;:]$/, '');
  return words.charAt(0).toUpperCase() + words.slice(1);
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd app && npm test` → all pass.

- [ ] **Step 5: Commit**

```bash
git add app/ui/recipes.js app/test/recipes.test.mjs
git commit -m "Recipes: sheet text, validation and suggested names"
```

---

### Task 5: Recipes in the panel

**Files:**
- Modify: `app/ui/icons.js`, `app/test/icons.test.mjs`, `app/ui/panel.css`, `app/ui/panel.js`

- [ ] **Step 1: Icons**

In `app/ui/icons.js` add to `PATHS`:

```js
  book: '<path d="M4 5a2 2 0 0 1 2-2h12v16H6a2 2 0 0 0-2 2z"/><path d="M4 19V5M8 7h6"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  pencil: '<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M14 6l4 4"/>',
```

Add `'book', 'plus', 'pencil'` to the names in `app/test/icons.test.mjs`. Run `cd app && npm test` → pass.

- [ ] **Step 2: Let the sheet host a list and an editor**

In `app/ui/panel.js`:

(a) Replace `function openSheet({ title, body, ok, run }) {` and its body with:

```js
// `ok` null: a list with only a Close button. `run` may return false to keep
// the sheet open (e.g. a form that failed validation).
let sheetOk = '';
function openSheet({ title, body, ok, run }) {
  closeMenu();
  sheetOpener = document.activeElement;
  sheetToken = {};
  sheetOk = ok || '';
  $('sheet-title').textContent = title;
  $('sheet-body').replaceChildren(...body);
  $('sheet-ok').hidden = !ok;
  $('sheet-ok').textContent = sheetOk;
  $('sheet-ok').disabled = false;
  $('sheet-cancel').textContent = ok ? 'Cancel' : 'Close';
  sheetRun = run || null;
  $('dim').hidden = $('sheet').hidden = false;
  (ok ? $('sheet-ok') : $('sheet').querySelector('input, textarea, button')).focus();
}
```

(b) In the `sheet-ok` click handler, replace:

```js
  try {
    await run();
  } finally {
    sheetBusy = false;
    if (sheetToken === token) closeSheet();
  }
```

with:

```js
  let keep = false;
  try {
    keep = (await run()) === false;
  } finally {
    sheetBusy = false;
    if (sheetToken === token) {
      if (keep) {
        $('sheet-ok').disabled = false;
        $('sheet-ok').textContent = sheetOk;
      } else closeSheet();
    }
  }
```

(c) Two-column action row in the sheet: `.sheet-actions` uses `grid-template-columns: 1fr 1fr`; when OK is hidden, Close spans both. Append to `app/ui/panel.css`:

```css
.sheet-actions:has(#sheet-ok[hidden]) { grid-template-columns: 1fr; }

/* Recipes */
.recipes { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; max-height: 260px; overflow-y: auto; }
.recipes li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 2px 8px; padding: 8px 0; border-top: 1px solid var(--line); }
.recipes li:first-child { border-top: 0; }
.recipes .rname { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.recipes .rline { grid-column: 1 / -1; color: var(--meta); font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.recipes .racts { display: flex; gap: 4px; }
.recipes .suggested .rname { font-weight: 500; color: var(--meta); }
.sheet label.field { display: flex; flex-direction: column; gap: 4px; font-size: 11.5px; color: var(--meta); }
.sheet input, .sheet textarea { font: inherit; font-size: 12.5px; color: var(--ink); background: var(--fill); border: 0; border-radius: 7px; padding: 6px 8px; user-select: text; -webkit-user-select: text; }
.sheet textarea { min-height: 110px; resize: vertical; }
.sheet .err { color: var(--ink); font-size: 11.5px; margin: 0; }
.sheet .err::before { content: ''; display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: var(--failed); margin-right: 6px; vertical-align: 1px; }
```

(d) Digit shortcuts must not fire while typing in the editor; they already skip `HTMLInputElement`/`HTMLTextAreaElement` and are off while a sheet is open. No change.

- [ ] **Step 3: Recipes action and sheets**

Import at the top of `panel.js`:

```js
import { recipeSheet, validateRecipe, suggestedName } from './recipes.js';
```

In `repoActions(r)`, insert before the Terminal button:

```js
  box.append(actionButton('Recipes', 'book', 'btn ghost', () => openRecipes(r)));
```

Add after `openTerminal`:

```js
// --- Recipes ---

async function openRecipes(r) {
  let data;
  try {
    data = await invoke('recipes_for', { path: r.path });
  } catch (e) {
    return note(String(e));
  }
  showRecipes(r, data);
}

function showRecipes(r, data) {
  const s = recipeSheet(data);
  const list = el('ul', 'recipes');
  for (const x of s.recipes) {
    const li = el('li');
    li.append(el('span', 'rname', x.name));
    const acts = el('span', 'racts');
    acts.append(
      actionButton('Start', 'play', 'btn', async () => {
        try {
          await invoke('start_recipe', { path: r.path, id: x.id });
          closeSheet();
          window.__TAURI__.window.getCurrentWindow().hide();
        } catch (e) {
          note(String(e));
        }
      }),
      actionButton('Copy', 'copy', 'btn ghost', async (ev) => {
        const label = ev.currentTarget.querySelector('span');
        try {
          await navigator.clipboard.writeText(x.prompt);
          label.textContent = 'Copied';
        } catch {
          label.textContent = 'Copy failed';
        }
        setTimeout(() => (label.textContent = 'Copy'), 1500);
      }),
      actionButton('Edit', 'pencil', 'btn ghost', () => editRecipe(r, x)),
    );
    li.append(acts, el('span', 'rline', x.preview));
    list.append(li);
  }
  for (const x of s.suggestions) {
    const li = el('li', 'suggested');
    li.append(el('span', 'rname', x.line));
    const acts = el('span', 'racts');
    acts.append(actionButton('Save', 'plus', 'btn ghost', () => editRecipe(r, { id: null, name: suggestedName(x.text), prompt: x.text })));
    li.append(acts, el('span', 'rline', `“${x.preview}”`));
    list.append(li);
  }
  const body = [];
  if (s.empty) body.push(el('p', '', 'No recipes yet. Save the prompts you start sessions with here, then start them with one click.'));
  else body.push(list);
  body.push(actionButton('New recipe', 'plus', 'btn ghost', () => editRecipe(r, { id: null, name: '', prompt: '' })));
  openSheet({ title: `Recipes · ${r.name}`, body, ok: null });
}

function editRecipe(r, x) {
  const name = el('input');
  name.value = x.name;
  name.maxLength = 60;
  name.placeholder = 'Fresh start';
  const prompt = el('textarea');
  prompt.value = x.prompt;
  prompt.placeholder = 'Pull all repos to dev, run migrations, start services, test end to end';
  const nameField = el('label', 'field', 'Name');
  nameField.append(name);
  const promptField = el('label', 'field', 'Prompt to start with');
  promptField.append(prompt);
  const err = el('p', 'err');
  err.hidden = true;
  const body = [nameField, promptField, err];
  if (x.id) {
    body.push(actionButton('Delete recipe', 'trash', 'btn ghost', async () => {
      try {
        showRecipes(r, await invoke('delete_recipe', { path: r.path, id: x.id }));
      } catch (e) {
        note(String(e));
      }
    }));
  }
  openSheet({
    title: x.id ? 'Edit recipe' : 'New recipe',
    body,
    ok: 'Save',
    run: async () => {
      const problem = validateRecipe({ name: name.value, prompt: prompt.value });
      if (problem) {
        err.textContent = problem;
        err.hidden = false;
        return false;
      }
      try {
        const data = await invoke('save_recipe', { path: r.path, id: x.id, name: name.value, prompt: prompt.value });
        // Back to the list once this sheet closes.
        setTimeout(() => showRecipes(r, data), 0);
      } catch (e) {
        err.textContent = String(e);
        err.hidden = false;
        return false;
      }
    },
  });
}
```

- [ ] **Step 4: Checks and commit**

```bash
cd app && cp ui/panel.js /tmp/p.mjs && node --check /tmp/p.mjs && comm -23 <(grep -o "\$('[a-z0-9-]*')" ui/panel.js | sed "s/\$('//;s/')//" | sort -u) <(grep -o 'id="[a-z0-9-]*"' ui/index.html | sed 's/id="//;s/"//' | sort -u) && npm test
```

Expected: syntax OK, cross-check prints nothing, tests pass.

```bash
git add app/ui/icons.js app/test/icons.test.mjs app/ui/panel.css app/ui/panel.js
git commit -m "Repos: Recipes sheet with Start, Copy, Edit, suggestions and New recipe"
```

---

### Task 6: Preview and checks

**Files:**
- Modify: `app/scripts/preview.mjs`

- [ ] **Step 1: Stubs**

In the stub `handlers`, add after `open_task_run: () => null,`:

```js
  recipes_for: () => ({
    recipes: [{ id: 'r1', name: 'Fresh start', prompt: 'Pull all repos to dev, run migrations, start services, test end to end' }],
    suggestions: [{ text: 'switch to main and pull, then raise a PR for the current branch', count: 9, last: Date.now() }],
  }),
  save_recipe: ({ name, prompt }) => ({ recipes: [{ id: 'r1', name: 'Fresh start', prompt: 'Pull all repos to dev' }, { id: 'r2', name, prompt }], suggestions: [] }),
  delete_recipe: () => ({ recipes: [], suggestions: [] }),
  start_recipe: () => null,
```

Commit:

```bash
git add app/scripts/preview.mjs
git commit -m "Preview: stubs for recipes"
```

- [ ] **Step 2: Visual check**

`cd app && PORT=5175 npm run preview`, `http://localhost:5175`, Repos tab, dark and light:
- Selecting a row shows **Recipes** next to Terminal.
- Recipes opens a sheet titled "Recipes · <repo>": "Fresh start" with Start / Copy / Edit and a preview line; a suggestion row "You typed this 9 times" with Save and the quoted prompt; "New recipe"; a single **Close** button spanning the row.
- Save on the suggestion opens the editor prefilled ("Switch to main and", the prompt). Clearing the name and pressing Save shows "Give the recipe a name." and keeps the sheet open; filling it and saving returns to the list with the new recipe.
- Edit → Delete recipe returns to the (now empty) list with the empty-state sentence.
- Esc closes the sheet; typing digits in the editor does not switch tabs.

- [ ] **Step 3: Real-app check**

Build and install as before (single copy in `/Applications`, build copy removed). In Repos → a repo → Recipes: real suggestions (if any) appear; create a recipe; Start opens a terminal tab with a new Claude session using that prompt (only with the person's go-ahead, since it starts a real session).

---

### Task 7: PR into dev

- [ ] **Step 1: All suites**

```bash
cd app && npm test && cd src-tauri && cargo test --lib && cd ../../wrapped && npm test
```

- [ ] **Step 2: Push and open the PR**

```bash
git branch --show-current   # feat/recipes
git push -u origin feat/recipes
gh pr create --base dev --title "Panel redesign, phase 5: Recipes" --body "Phase 5 of docs/superpowers/specs/2026-10-04-panel-redesign-design.md.

- Repos rows get **Recipes**: named kick-off prompts for that repository. **Start** opens a new Claude session with the prompt in a terminal tab in the repo; **Copy**, **Edit**, **Delete**, **New recipe**.
- Suggestions: prompts you typed 3+ times in the repo in the last 30 days (30+ characters, compared ignoring case and spacing; resumed/forked copies counted once), top 5, one click to save. Mined on demand, cached 10 minutes. Prompts never leave the machine.
- Stored as recipes.json in the app's config folder, keyed by repository folder. Every command requires a repository's top folder; Start looks the prompt up on the backend.
- The confirm sheet can now host a list (Close only) and a form that stays open on validation errors.
- Tests: Rust storage and validation, suggestion mining (case/spacing, worktrees fold into the repo, forked copies, short replies ignored); JS sheet text, validation, suggested names."
```

The user merges the PR.

---

## Self-review notes

- Spec coverage: recipes per repository with Start (terminal `claude "<prompt>"`) and Copy (Tasks 1, 3, 5); suggestions from prompts repeated 3+ times, normalised, first 80 characters shown (Tasks 2, 4, 5); stored as JSON in the config folder (Task 1); "+ New recipe" (Task 5).
- Names: Rust `Recipe{id, name, prompt}`, `Suggestion{text, count, last}`, `RecipeSheet{recipes, suggestions}`; commands `recipes_for({path})`, `save_recipe({path, id, name, prompt})`, `delete_recipe({path, id})`, `start_recipe({path, id})`; JS `recipeSheet`, `validateRecipe`, `suggestedName`, `preview`; icons `book`, `plus`, `pencil` (plus existing `play`, `copy`, `trash`).
