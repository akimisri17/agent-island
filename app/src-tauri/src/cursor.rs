//! Reads Cursor agent chats from its local SQLite store. Mirrors
//! `wrapped/src/cursor.mjs`.
//!
//! Cursor keeps chats ("composers") in a key-value table:
//!   composerData:<id>            one row per chat: name, model, workspace, message headers
//!   bubbleId:<id>:<bubbleId>     one row per message: tool calls, edits
//! Headers carry type (1 = person, 2 = agent) and timestamps. Cursor stores no
//! token counts locally, so sessions from here have zero tokens.

use crate::foreign::open_foreign_db;
use crate::logs::{project_of, Builder, Session};
use rusqlite::Connection;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

const EDIT_TOOLS: [&str; 6] = ["edit_file", "edit_file_v2", "search_replace", "write", "apply_patch", "multi_edit"];
const USER: u8 = 1;
const AGENT: u8 = 2;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Composer {
    composer_id: Option<String>,
    name: Option<String>,
    last_updated_at: Option<i64>,
    model_config: Option<ModelConfig>,
    workspace_identifier: Option<Workspace>,
    is_best_of_n_subcomposer: Option<bool>,
    #[serde(default)]
    full_conversation_headers_only: Vec<Header>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelConfig {
    model_name: Option<String>,
}

#[derive(Deserialize)]
struct Workspace {
    uri: Option<WorkspaceUri>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceUri {
    fs_path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    #[serde(rename = "type")]
    kind: Option<u8>,
    created_at: Option<String>,
    started_at_ms: Option<i64>,
    completed_at_ms: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bubble {
    tool_former_data: Option<ToolFormer>,
}

#[derive(Deserialize)]
struct ToolFormer {
    name: Option<String>,
    params: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditParams {
    relative_workspace_path: Option<String>,
    target_file: Option<String>,
    file_path: Option<String>,
    path: Option<String>,
}

fn text(v: rusqlite::types::ValueRef) -> Option<String> {
    match v {
        rusqlite::types::ValueRef::Text(b) | rusqlite::types::ValueRef::Blob(b) => Some(String::from_utf8_lossy(b).into_owned()),
        _ => None,
    }
}

/// "default" is Cursor's Auto mode, which picks the model per request.
fn cursor_model(name: Option<&str>) -> String {
    match name {
        None | Some("") | Some("default") => "cursor-auto".to_string(),
        Some(n) => n.to_string(),
    }
}

/// Any failure (no database, locked, schema changed) yields no Cursor
/// sessions rather than failing the whole scan.
pub fn parse_cursor_db(path: &Path, since: i64) -> Vec<Session> {
    let Some(db) = open_foreign_db(path) else { return Vec::new() };
    read(&db, path, since).unwrap_or_default()
}

fn read(db: &Connection, path: &Path, since: i64) -> rusqlite::Result<Vec<Session>> {
    // Older Cursor versions have no composerHeaders table.
    let subagents: HashSet<String> = db
        .prepare("SELECT composerId FROM composerHeaders WHERE isSubagent = 1")
        .and_then(|mut st| st.query_map([], |r| r.get::<_, String>(0))?.collect())
        .unwrap_or_default();

    let mut composers = db.prepare("SELECT key, value FROM cursorDiskKV WHERE key >= 'composerData:' AND key < 'composerData;'")?;
    let mut bubbles = db.prepare("SELECT value FROM cursorDiskKV WHERE key >= ?1 AND key < ?2")?;
    let rows: Vec<(String, Option<String>)> =
        composers.query_map([], |r| Ok((r.get::<_, String>(0)?, text(r.get_ref(1)?))))?.filter_map(Result::ok).collect();

    let mut sessions = Vec::new();
    for (key, value) in rows {
        let Some(d) = value.and_then(|v| serde_json::from_str::<Composer>(&v).ok()) else { continue };
        if d.full_conversation_headers_only.is_empty() || d.last_updated_at.is_some_and(|t| t < since) {
            continue;
        }
        let id = d.composer_id.clone().unwrap_or_else(|| key["composerData:".len()..].to_string());
        let mut b = Builder::new("cursor", id.clone(), path);
        b.s.title = d.name.filter(|n| !n.is_empty());
        b.s.cwd = d.workspace_identifier.and_then(|w| w.uri).and_then(|u| u.fs_path);
        b.s.project = b.s.cwd.as_deref().and_then(project_of);
        b.s.is_subagent = subagents.contains(&id) || d.is_best_of_n_subcomposer == Some(true);
        let model = cursor_model(d.model_config.and_then(|m| m.model_name).as_deref());

        for h in &d.full_conversation_headers_only {
            let ts = h.started_at_ms.or_else(|| {
                h.created_at.as_deref().and_then(|c| chrono::DateTime::parse_from_rfc3339(c).ok()).map(|t| t.timestamp_millis())
            });
            let Some(ts) = ts.filter(|&t| t >= since) else { continue };
            if h.kind == Some(USER) && !b.s.is_subagent {
                b.prompt(ts);
            } else {
                b.activity(ts.max(h.completed_at_ms.unwrap_or(ts)));
                if h.kind == Some(AGENT) {
                    b.response(Some(&model));
                    b.model(Some(&model), 0);
                }
            }
        }
        if b.s.start.is_none() {
            continue;
        }

        let lo = format!("bubbleId:{id}:");
        let hi = format!("bubbleId:{id};");
        let values: Vec<Option<String>> =
            bubbles.query_map([&lo, &hi], |r| Ok(text(r.get_ref(0)?)))?.filter_map(Result::ok).collect();
        for v in values.into_iter().flatten() {
            let Some(tf) = serde_json::from_str::<Bubble>(&v).ok().and_then(|b| b.tool_former_data) else { continue };
            let Some(name) = tf.name else { continue };
            b.tool(Some(&name));
            if EDIT_TOOLS.contains(&name.as_str()) {
                let p = tf.params.as_deref().and_then(|p| serde_json::from_str::<EditParams>(p).ok());
                if let Some(f) = p.and_then(|p| p.relative_workspace_path.or(p.target_file).or(p.file_path).or(p.path)) {
                    b.files.insert(f);
                }
            }
        }
        sessions.push(b.finish());
    }
    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn cursor_matches_js_parser() {
        let db = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../wrapped/test/fixtures/cursor-state.vscdb");
        let since = 1_788_220_800_000; // 2026-09-01T00:00:00Z
        let sessions = parse_cursor_db(&db, since);
        let mut ids: Vec<_> = sessions.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["k1", "sub"]);

        let k = sessions.iter().find(|s| s.id == "k1").unwrap();
        assert_eq!(k.agent, "cursor");
        assert_eq!(k.title.as_deref(), Some("Catalog upload"));
        assert_eq!(k.project.as_deref(), Some("catalog"));
        assert_eq!(k.prompts.len(), 2);
        let mins: Vec<i64> = k.turns.iter().map(|t| (t.end - t.start) / 60_000).collect();
        assert_eq!(mins, [5, 2]);
        assert_eq!(k.responses.get("grok-4.7"), Some(&3));
        assert_eq!(k.tools.get("edit_file_v2"), Some(&1));
        assert_eq!(k.tools.get("run_terminal_command_v2"), Some(&1));
        assert_eq!(k.files_edited, ["/work/catalog/upload.ts"]);
        assert_eq!(k.tokens.output, 0);

        let sub = sessions.iter().find(|s| s.id == "sub").unwrap();
        assert!(sub.is_subagent);
        assert!(sub.prompts.is_empty());
        assert_eq!(sub.responses.get("cursor-auto"), Some(&1));
    }

    #[test]
    fn missing_db_is_empty() {
        assert!(parse_cursor_db(Path::new("/nonexistent/state.vscdb"), 0).is_empty());
    }
}
