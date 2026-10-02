import { readdir, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, sep } from 'node:path';
import { parseClaudeFile } from './claude.mjs';
import { parseCodexFile } from './codex.mjs';
import { parseCursorDb } from './cursor.mjs';
import { listAntigravity, parseAntigravityDb } from './antigravity.mjs';

export function defaultRoots() {
  const home = homedir();
  return {
    claude: join(process.env.CLAUDE_CONFIG_DIR || join(home, '.claude'), 'projects'),
    codex: join(process.env.CODEX_HOME || join(home, '.codex'), 'sessions'),
    cursor: join(cursorUserDir(home), 'globalStorage', 'state.vscdb'),
    gemini: process.env.GEMINI_HOME || join(home, '.gemini'),
  };
}

function cursorUserDir(home) {
  if (process.platform === 'darwin') return join(home, 'Library', 'Application Support', 'Cursor', 'User');
  if (process.platform === 'win32') return join(process.env.APPDATA || join(home, 'AppData', 'Roaming'), 'Cursor', 'User');
  return join(process.env.XDG_CONFIG_HOME || join(home, '.config'), 'Cursor', 'User');
}

// Recursively lists .jsonl files modified at or after `since`.
async function listJsonl(dir, since, out = []) {
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    const p = join(dir, e.name);
    if (e.isDirectory()) await listJsonl(p, since, out);
    else if (e.isFile() && e.name.endsWith('.jsonl')) {
      try {
        const st = await stat(p);
        if (st.mtimeMs >= since) out.push({ path: p, size: st.size });
      } catch {
        // file vanished between readdir and stat
      }
    }
  }
  return out;
}

async function pool(items, limit, fn) {
  const results = new Array(items.length);
  let next = 0;
  async function worker() {
    while (next < items.length) {
      const i = next++;
      results[i] = await fn(items[i], i);
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return results;
}

export async function scan({ since, roots = defaultRoots(), onProgress = () => {} }) {
  const [claudeFiles, codexFiles, agyFiles] = await Promise.all([
    listJsonl(roots.claude, since),
    listJsonl(roots.codex, since),
    roots.gemini ? listAntigravity(roots.gemini, since) : [],
  ]);
  const jobs = [
    ...claudeFiles.map((f) => ({ ...f, agent: 'claude', isSubagent: f.path.includes(`${sep}subagents${sep}`) })),
    ...codexFiles.map((f) => ({ ...f, agent: 'codex' })),
    ...agyFiles.map((f) => ({ ...f, agent: 'antigravity' })),
  ];
  let cursorSize = 0;
  try {
    cursorSize = roots.cursor ? (await stat(roots.cursor)).size : 0;
  } catch {
    // no Cursor on this machine
  }
  const totalBytes = jobs.reduce((n, j) => n + j.size, 0) + cursorSize;
  let doneBytes = 0;

  const sessions = await pool(jobs, 8, async (job) => {
    let s = null;
    try {
      if (job.agent === 'claude') s = await parseClaudeFile(job.path, { since, isSubagent: job.isSubagent });
      else if (job.agent === 'codex') s = await parseCodexFile(job.path, { since });
      else s = await parseAntigravityDb(job.path, { since });
    } catch {
      // unreadable file: skip it rather than fail the whole report
    }
    doneBytes += job.size;
    onProgress(doneBytes, totalBytes);
    return s;
  });

  const cursor = cursorSize ? await parseCursorDb(roots.cursor, { since }) : [];
  doneBytes += cursorSize;
  onProgress(doneBytes, totalBytes);

  return {
    sessions: [...sessions, ...cursor].filter((s) => s && s.start !== null),
    files: { claude: claudeFiles.length, codex: codexFiles.length, cursor: cursorSize ? 1 : 0, antigravity: agyFiles.length },
    bytes: totalBytes,
  };
}
