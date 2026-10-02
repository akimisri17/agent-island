//! Reads Antigravity CLI conversations. Mirrors `wrapped/src/antigravity.mjs`,
//! which documents the fields used.

use crate::foreign::{get_string, get_timestamp, open_foreign_db};
use crate::logs::{project_of, Builder, Session};
use std::path::{Path, PathBuf};

const PROMPT: i64 = 14;
const TOOL: i64 = 132;
const EDIT_TOOLS: [&str; 3] = ["replace_file_content", "multi_replace_file_content", "write_to_file"];

pub fn list(root: &Path, since: i64, out: &mut Vec<(PathBuf, u64)>) {
    for sub in ["antigravity-cli", "antigravity"] {
        let Ok(entries) = std::fs::read_dir(root.join(sub).join("conversations")) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "db") {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as i64);
            if mtime >= since {
                out.push((p, meta.len()));
            }
        }
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct Args {
    cwd: Option<String>,
    target_file: Option<String>,
    absolute_path: Option<String>,
    file_path: Option<String>,
}

pub fn parse(path: &Path, since: i64) -> Session {
    let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut b = Builder::new("antigravity", id, path);
    let Some(db) = open_foreign_db(path) else { return b.finish() };
    let _ = read(&db, &mut b, since);
    b.finish()
}

fn read(db: &rusqlite::Connection, b: &mut Builder, since: i64) -> rusqlite::Result<()> {
    let mut steps = db.prepare("SELECT step_type, metadata FROM steps ORDER BY idx")?;
    let rows: Vec<(i64, Vec<u8>)> = steps
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<Vec<u8>>>(1)?.unwrap_or_default())))?
        .filter_map(Result::ok)
        .collect();
    for (kind, meta) in rows {
        let Some(created) = get_timestamp(&meta, &[1]).filter(|&t| t >= since) else { continue };
        if kind == PROMPT {
            b.prompt(created);
            continue;
        }
        let end = [6, 7, 8].iter().filter_map(|&f| get_timestamp(&meta, &[f])).fold(created, i64::max);
        b.activity(end);
        if kind != TOOL {
            continue;
        }
        let name = get_string(&meta, &[4, 2]);
        b.tool(name.as_deref());
        let args: Args = get_string(&meta, &[4, 3]).and_then(|a| serde_json::from_str(&a).ok()).unwrap_or_default();
        if b.s.project.is_none() {
            b.s.project = args.cwd.as_deref().and_then(project_of);
        }
        if let (Some(n), Some(f)) = (name.as_deref(), args.target_file.or(args.absolute_path).or(args.file_path)) {
            if EDIT_TOOLS.contains(&n) {
                b.files.insert(f);
            }
        }
    }
    if b.s.start.is_none() {
        return Ok(());
    }
    let mut gens = db.prepare("SELECT data FROM gen_metadata")?;
    let models: Vec<String> = gens
        .query_map([], |r| r.get::<_, Option<Vec<u8>>>(0))?
        .filter_map(Result::ok)
        .filter_map(|d| get_string(&d.unwrap_or_default(), &[1, 19]))
        .filter(|m| !m.is_empty() && !m.starts_with("MODEL_"))
        .collect();
    for m in models {
        b.response(Some(&m));
        b.model(Some(&m), 0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../wrapped/test/fixtures")
    }

    #[test]
    fn antigravity_matches_js_parser() {
        let s = parse(&fixtures().join("antigravity-conv.db"), 1_788_220_800_000);
        assert_eq!(s.agent, "antigravity");
        assert_eq!(s.id, "antigravity-conv");
        assert_eq!(s.project.as_deref(), Some("forge"));
        assert_eq!(s.prompts.len(), 1);
        assert_eq!(s.turns.len(), 1);
        assert_eq!((s.turns[0].end - s.turns[0].start) / 1000, 120);
        assert_eq!(s.tools.get("run_command"), Some(&1));
        assert_eq!(s.tools.get("write_to_file"), Some(&1));
        assert_eq!(s.files_edited, ["/work/forge/notes.md"]);
        assert_eq!(s.responses.get("gemini-pro-default"), Some(&2));
        assert_eq!(s.responses.get("gemini-3.8-flash"), Some(&1));
        assert_eq!(s.tokens.output, 0);
    }

    #[test]
    fn wal_database_gets_no_sidecar_files() {
        let dir = std::env::temp_dir().join(format!("agent-island-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("state.vscdb");
        std::fs::copy(fixtures().join("cursor-state.vscdb"), &db).unwrap();
        rusqlite::Connection::open(&db).unwrap().pragma_update(None, "journal_mode", "WAL").unwrap();
        let listing = || {
            let mut v: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
            v.sort();
            v
        };
        assert_eq!(listing(), ["state.vscdb"]);
        assert_eq!(crate::cursor::parse_cursor_db(&db, 1_788_220_800_000).len(), 2);
        assert_eq!(listing(), ["state.vscdb"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
