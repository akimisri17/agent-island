//! Recipes: named kick-off prompts per repository, and suggestions from
//! prompts the person keeps retyping there.

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
}
