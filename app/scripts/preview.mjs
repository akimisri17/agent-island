// Dev-only: serves app/ui in a browser with window.__TAURI__ stubbed, fed by
// real data from the Rust examples (today's sessions and repo status). Never
// shipped: the stub is injected by this server, not stored in ui/.
//   npm run preview        -> http://localhost:5174 (PORT=… to change)
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
const PORT = Number(process.env.PORT) || 5174;

const ui = fileURLToPath(new URL('../ui/', import.meta.url));
const tauri = fileURLToPath(new URL('../src-tauri/', import.meta.url));
const run = (args) => execFileSync('cargo', ['run', '-q', '--release', '--example', ...args], { cwd: tauri, maxBuffer: 1 << 30 }).toString();

console.log('Reading today’s logs and repos with the Rust examples…');
const scan = JSON.parse(run(['dump', '--', '1']));
const scan30 = JSON.parse(run(['dump', '--', '30']));
const cwds = [...new Set(scan.sessions.map((s) => s.cwd).filter(Boolean))];
const repos = JSON.parse(run(['repos', '--', ...cwds]));
const data = { scan, scan30, repos };

const stub = `
const data = ${JSON.stringify(data)};
const handlers = {
  scan_today: () => data.scan, scan: () => data.scan30, repo_status: () => data.repos,
  recap_commits: () => [], live: () => [], limits: () => ({}),
  get_settings: () => ({ hotkey: 'ctrl+alt+KeyJ', notifyLimits: true, recapWithClaude: false, notifyMissedTasks: true }),
  set_settings: ({ next }) => next, open_report: () => null, jump: () => null, quit: () => null,
  repo_pull: () => 3,
  repo_delete_branches: ({ names }) => names,
  terminals: () => ['Ghostty', 'iTerm', 'Terminal'],
  open_terminal: () => null,
  cut_off: () => [
    { sessionId: 'aaaa-1', title: 'Repo board', project: 'agent-island', cwd: '/tmp', kind: 'limit', limitType: 'five_hour', at: Date.now() - 95 * 60000, resetsAt: Date.now() - 4 * 60000 },
    { sessionId: 'bbbb-2', title: null, project: 'erp-backend', cwd: '/tmp', kind: 'interrupted', limitType: null, at: Date.now() - 50 * 60000, resetsAt: null },
  ],
  resume_session: () => null,
  tasks_board: () => {
    const H = 3600000, now = Date.now();
    const last = (h, outcome) => ({ sessionId: 's', cwd: '/tmp', start: now - h * H, end: now - h * H + 180000, outcome });
    return [
      { name: 'personal-products-digest', description: 'Daily product digest', state: 'missed', last: last(28, 'done'), cadenceMs: 24 * H, days: ['ran', 'ran', 'ran', 'ran', 'ran', 'ran', 'missed'] },
      { name: 'plugin-radar-daily', description: 'Plugin radar', state: 'stopped', last: last(6, 'stopped'), cadenceMs: 24 * H, days: ['none', 'none', 'none', 'none', 'ran', 'ran', 'failed'] },
      { name: 'tts-eod-digest', description: 'EoD digest', state: 'ok', last: last(3, 'done'), cadenceMs: 24 * H, days: ['ran', 'ran', 'ran', 'ran', 'ran', 'ran', 'ran'] },
    ];
  },
  run_task: () => null,
  open_task_run: () => null,
  recipes_for: () => ({
    recipes: [{ id: 'r1', name: 'Fresh start', prompt: 'Pull all repos to dev, run migrations, start services, test end to end' }],
    suggestions: [{ text: 'switch to main and pull, then raise a PR for the current branch', count: 9, last: Date.now() }],
  }),
  save_recipe: ({ name, prompt }) => ({ recipes: [{ id: 'r1', name: 'Fresh start', prompt: 'Pull all repos to dev' }, { id: 'r2', name, prompt }], suggestions: [] }),
  delete_recipe: () => ({ recipes: [], suggestions: [] }),
  start_recipe: () => null,
  test_notification: () => null,
  polish_recap: () => 'Done\\n- preview: polished text',
};
window.__TAURI__ = {
  core: { invoke: async (cmd, args) => { if (!handlers[cmd]) throw new Error(cmd + ' is not stubbed'); return handlers[cmd](args || {}); } },
  event: { listen: async () => () => {} },
  window: { getCurrentWindow: () => ({ hide() {} }) },
};`;

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css' };
createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^\/+/, '') || 'index.html';
  if (path === '__stub.js') return res.writeHead(200, { 'content-type': 'text/javascript' }).end(stub);
  try {
    let body = await readFile(join(ui, path));
    if (path === 'index.html') {
      body = body.toString()
        .replace('<script type="module"', '<script src="__stub.js"></script>\n<script type="module"')
        .replace('</head>', '<style>body{width:360px;height:560px;margin:24px auto;outline:1px solid #8884}</style></head>');
    }
    res.writeHead(200, { 'content-type': TYPES[extname(path)] || 'application/octet-stream' }).end(body);
  } catch {
    res.writeHead(404).end();
  }
}).listen(PORT, '127.0.0.1', () => console.log(`Preview at http://localhost:${PORT}`));
