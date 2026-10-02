import { projectOf, newSession, createTurnTracker, addModel, addResponse, addTool, basename } from './lines.mjs';
import { openForeignDb } from './sqlite.mjs';

// Cursor keeps agent chats ("composers") in a SQLite key-value table:
//   composerData:<id>            one row per chat: name, model, workspace, message headers
//   bubbleId:<id>:<bubbleId>     one row per message: tool calls, edits
// Message headers carry type (1 = person, 2 = agent) and timestamps, which is
// enough for prompts, turns, and waits. Cursor stores no token counts locally.

const EDIT_TOOLS = new Set(['edit_file', 'edit_file_v2', 'search_replace', 'write', 'apply_patch', 'multi_edit']);
const USER = 1;
const AGENT = 2;

export async function parseCursorDb(path, { since }) {
  const db = await openForeignDb(path);
  if (!db) return [];
  try {
    return readComposers(db, path, since);
  } catch {
    return []; // schema changed: skip Cursor rather than fail
  } finally {
    db.close();
  }
}

const text = (v) => (typeof v === 'string' ? v : v ? Buffer.from(v).toString('utf8') : '');

function readComposers(db, path, since) {
  const subagents = new Set();
  try {
    for (const r of db.prepare('SELECT composerId FROM composerHeaders WHERE isSubagent = 1').all()) subagents.add(r.composerId);
  } catch {
    // older Cursor versions have no composerHeaders table
  }
  const bubbles = db.prepare('SELECT value FROM cursorDiskKV WHERE key >= ? AND key < ?');
  const sessions = [];

  for (const row of db.prepare("SELECT key, value FROM cursorDiskKV WHERE key >= 'composerData:' AND key < 'composerData;'").all()) {
    let d;
    try {
      d = JSON.parse(text(row.value));
    } catch {
      continue;
    }
    const headers = d.fullConversationHeadersOnly || [];
    if (!headers.length || (d.lastUpdatedAt && d.lastUpdatedAt < since)) continue;

    const id = d.composerId || row.key.slice('composerData:'.length);
    const s = newSession('cursor', id, path);
    s.title = d.name || null;
    s.cwd = d.workspaceIdentifier?.uri?.fsPath || null;
    s.project = projectOf(s.cwd);
    s.isSubagent = subagents.has(id) || Boolean(d.isBestOfNSubcomposer);
    const model = cursorModel(d.modelConfig?.modelName);
    const turns = createTurnTracker(s);

    for (const h of headers) {
      const ts = h.startedAtMs ?? Date.parse(h.createdAt);
      if (!Number.isFinite(ts) || ts < since) continue;
      if (h.type === USER && !s.isSubagent) turns.prompt(ts);
      else {
        turns.activity(Math.max(ts, h.completedAtMs ?? ts));
        if (h.type === AGENT) {
          addResponse(s, model);
          addModel(s, model, 0);
        }
      }
    }
    turns.finish();
    if (s.start === null) continue;

    for (const b of bubbles.all(`bubbleId:${id}:`, `bubbleId:${id};`)) {
      let v;
      try {
        v = JSON.parse(text(b.value));
      } catch {
        continue;
      }
      const tf = v.toolFormerData;
      if (!tf?.name) continue;
      addTool(s, tf.name);
      if (EDIT_TOOLS.has(tf.name)) {
        try {
          const p = JSON.parse(tf.params || '{}');
          const f = p.relativeWorkspacePath || p.targetFile || p.file_path || p.path;
          if (f) s.filesEdited.add(f);
        } catch {
          // params are not always JSON
        }
      }
    }
    sessions.push(s);
  }
  return sessions;
}

// "default" is Cursor's Auto mode, which picks the model per request.
function cursorModel(name) {
  if (!name || name === 'default') return 'cursor-auto';
  return name;
}
