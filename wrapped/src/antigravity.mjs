import { readdir, stat } from 'node:fs/promises';
import { join } from 'node:path';
import { projectOf, newSession, createTurnTracker, addModel, addResponse, addTool, basename } from './lines.mjs';
import { openForeignDb } from './sqlite.mjs';
import { getString, getTimestamp } from './protobuf.mjs';

// Antigravity CLI keeps one SQLite file per conversation:
//   ~/.gemini/antigravity-cli/conversations/<id>.db
// Rows hold protobuf blobs with no published schema. The fields read here
// were identified from real files:
//   steps.step_type      14 = person's prompt, 15 = model response, 132 = tool call
//   steps.metadata       1 = created (Timestamp); 6, 7, 8 = later Timestamps;
//                        4.2 = tool name; 4.3 = tool arguments (JSON)
//   gen_metadata.data    1.19 = model name, one row per model call
// Token-like numbers exist but their meaning is unconfirmed, so tokens are
// not reported. The Antigravity IDE stores conversations encrypted; skipped.

const PROMPT = 14;
const TOOL = 132;
const EDIT_TOOLS = new Set(['replace_file_content', 'multi_replace_file_content', 'write_to_file']);

export async function listAntigravity(root, since) {
  const out = [];
  for (const sub of ['antigravity-cli', 'antigravity']) {
    const dir = join(root, sub, 'conversations');
    let names;
    try {
      names = await readdir(dir);
    } catch {
      continue;
    }
    for (const n of names) {
      if (!n.endsWith('.db')) continue;
      try {
        const st = await stat(join(dir, n));
        if (st.mtimeMs >= since) out.push({ path: join(dir, n), size: st.size });
      } catch {
        // vanished between readdir and stat
      }
    }
  }
  return out;
}

export async function parseAntigravityDb(path, { since }) {
  const db = await openForeignDb(path);
  if (!db) return null;
  try {
    return read(db, path, since);
  } catch {
    return null; // schema changed or file mid-write
  } finally {
    db.close();
  }
}

const bytes = (v) => (v instanceof Uint8Array ? v : v ? new Uint8Array(v) : new Uint8Array());

function read(db, path, since) {
  const s = newSession('antigravity', basename(path).replace(/\.db$/, ''), path);
  const turns = createTurnTracker(s);

  for (const row of db.prepare('SELECT step_type, metadata FROM steps ORDER BY idx').all()) {
    const meta = bytes(row.metadata);
    const created = getTimestamp(meta, [1]);
    if (created === undefined || created < since) continue;
    if (row.step_type === PROMPT) {
      turns.prompt(created);
      continue;
    }
    const later = [6, 7, 8].map((f) => getTimestamp(meta, [f]) ?? 0);
    turns.activity(Math.max(created, ...later));
    if (row.step_type !== TOOL) continue;
    const name = getString(meta, [4, 2]);
    addTool(s, name);
    let args = {};
    try {
      args = JSON.parse(getString(meta, [4, 3]) || '{}');
    } catch {
      // arguments are not always JSON
    }
    if (!s.project && typeof args.Cwd === 'string') {
      s.project = projectOf(args.Cwd);
      s.cwd = args.Cwd;
    }
    const file = args.TargetFile || args.AbsolutePath || args.FilePath;
    if (EDIT_TOOLS.has(name) && typeof file === 'string') s.filesEdited.add(file);
  }
  turns.finish();
  if (s.start === null) return s;

  for (const row of db.prepare('SELECT data FROM gen_metadata').all()) {
    const model = getString(bytes(row.data), [1, 19]);
    if (!model || model.startsWith('MODEL_')) continue;
    addResponse(s, model);
    addModel(s, model, 0);
  }
  return s;
}
