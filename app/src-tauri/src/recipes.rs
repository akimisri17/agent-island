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
