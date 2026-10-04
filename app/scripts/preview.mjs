// Dev-only: serves app/ui in a browser with window.__TAURI__ stubbed, fed by
// real data from the Rust examples (today's sessions and repo status). Never
// shipped: the stub is injected by this server, not stored in ui/.
//   npm run preview        -> http://localhost:5174 (PORT=… to change)
//   DEMO=1 npm run preview -> made-up data only, for screenshots (no Rust)
// URL options: ?view=<tab> opens that tab, ?shot=1 drops the margin and
// outline (body exactly 360x560), ?scroll=bottom scrolls the view down,
// ?click=<selector> clicks an element once data has loaded.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
const PORT = Number(process.env.PORT) || 5174;

const ui = fileURLToPath(new URL('../ui/', import.meta.url));
const tauri = fileURLToPath(new URL('../src-tauri/', import.meta.url));
const run = (args) => execFileSync('cargo', ['run', '-q', '--release', '--example', ...args], { cwd: tauri, maxBuffer: 1 << 30 }).toString();

const DEMO = process.env.DEMO === '1';
let data;
if (DEMO) {
  console.log('Demo mode: made-up data, no logs read.');
  data = (await import('./demo-data.mjs')).demoData();
} else {
  console.log('Reading today’s logs and repos with the Rust examples…');
  const scan = JSON.parse(run(['dump', '--', '1']));
  const scan30 = JSON.parse(run(['dump', '--', '30']));
  const cwds = [...new Set(scan.sessions.map((s) => s.cwd).filter(Boolean))];
  const repos = JSON.parse(run(['repos', '--', ...cwds]));
  data = { scan, scan30, repos };
}

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
  approvals: () => [
    { project: '/tmp/app', name: 'Applications', asked: 41, tools: [['Bash', 38], ['Edit', 3]], commands: [['shell script', 22], ['npm run', 12], ['docker compose', 4]], medianWaitMs: 360000, affirmations: 7, suggestions: ['Bash(npm run:*)', 'Bash(docker compose:*)'] },
    { project: '/tmp/erp', name: 'erp-backend', asked: 18, tools: [['Edit', 12], ['Bash', 6]], commands: [['cargo test', 6]], medianWaitMs: 180000, affirmations: 0, suggestions: ['Bash(cargo test:*)'] },
  ],
  allow_rules: ({ rules }) => rules,
  test_notification: () => null,
  polish_recap: () => 'Done\\n- preview: polished text',
};
if (data.demo) {
  const D = data.demo, now = Date.now(), M = 60000, H = 3600000;
  const at = (o) => ({ ...o, since: now - o.ago * M });
  Object.assign(handlers, {
    live: () => D.live.map(at),
    limits: () => ({
      claude: { risk: 'medium', limitedUntil: null, windowStart: now - 2 * H, windowResets: now + 3 * H, passed: 2, pastHits: 4, sessionsWorking: 2, largeModelShare: 0.4, advice: null },
      codex: [{ usedPercent: 38, windowMinutes: 300, resetsAt: now + 2.5 * H, minutesToFull: null }, { usedPercent: 21, windowMinutes: 10080, resetsAt: now + 4 * 24 * H, minutesToFull: null }],
    }),
    recap_commits: ({ cwds }) => D.commits.filter((r) => cwds.includes(r.cwd)).map((r) => ({ project: r.project, commits: r.commits.map((c) => ({ subject: c.subject, ts: now - c.ago * M })) })),
    cut_off: () => [
      { sessionId: 'demo-c1', title: 'Retry failed payments', project: 'shop', cwd: '/Users/demo/code/shop', kind: 'limit', limitType: 'five_hour', at: now - 95 * M, resetsAt: now - 4 * M },
      { sessionId: 'demo-c2', title: 'Rate limit middleware', project: 'api', cwd: '/Users/demo/code/api', kind: 'interrupted', limitType: null, at: now - 50 * M, resetsAt: null },
    ],
    tasks_board: () => {
      const last = (h, outcome) => ({ sessionId: 's', cwd: '/Users/demo/code/docs', start: now - h * H, end: now - h * H + 180000, outcome });
      return [
        { name: 'morning-dependency-check', description: 'Check for outdated packages', state: 'missed', last: last(28, 'done'), cadenceMs: 24 * H, days: ['ran', 'ran', 'ran', 'ran', 'ran', 'ran', 'missed'] },
        { name: 'docs-link-checker', description: 'Find broken links in the docs', state: 'stopped', last: last(6, 'stopped'), cadenceMs: 24 * H, days: ['none', 'none', 'ran', 'ran', 'ran', 'ran', 'failed'] },
        { name: 'weekly-changelog', description: 'Draft the changelog from merged PRs', state: 'ok', last: last(3, 'done'), cadenceMs: 24 * H, days: ['ran', 'ran', 'ran', 'ran', 'ran', 'ran', 'ran'] },
      ];
    },
    recipes_for: () => ({
      recipes: [{ id: 'r1', name: 'Fresh start', prompt: 'Pull main, install dependencies, run the tests and start the dev server' }],
      suggestions: [{ text: 'run the tests and fix anything that fails, then commit', count: 6, last: now }],
    }),
    approvals: () => [
      { project: '/Users/demo/code/shop', name: 'shop', asked: 41, tools: [['Bash', 35], ['Edit', 6]], commands: [['npm test', 18], ['npm run build', 11], ['git push', 6]], medianWaitMs: 240000, affirmations: 7, suggestions: ['Bash(npm test:*)', 'Bash(npm run build:*)'] },
      { project: '/Users/demo/code/api', name: 'api', asked: 18, tools: [['Bash', 12], ['Edit', 6]], commands: [['cargo test', 9]], medianWaitMs: 150000, affirmations: 2, suggestions: ['Bash(cargo test:*)'] },
    ],
  });
}
window.__TAURI__ = {
  core: { invoke: async (cmd, args) => { if (!handlers[cmd]) throw new Error(cmd + ' is not stubbed'); return handlers[cmd](args || {}); } },
  event: { listen: async () => () => {} },
  window: { getCurrentWindow: () => ({ hide() {} }) },
};`;

// Runs before panel.js: ?view=, ?shot=1, ?scroll=bottom, ?click=.
const pageOptions = `(() => {
  const q = new URLSearchParams(location.search);
  if (q.get('view')) try { localStorage.setItem('view', q.get('view')); } catch {}
  if (q.get('shot')) document.addEventListener('DOMContentLoaded', () => document.body.classList.add('shot'));
  if (q.get('click')) setTimeout(() => document.querySelector(q.get('click'))?.click(), 2000);
  if (q.get('scroll') === 'bottom') setTimeout(() => document.querySelectorAll('.view').forEach((v) => (v.scrollTop = v.scrollHeight)), 2500);
})();`;

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css' };
createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^\/+/, '') || 'index.html';
  if (path === '__stub.js') return res.writeHead(200, { 'content-type': 'text/javascript' }).end(stub);
  try {
    let body = await readFile(join(ui, path));
    if (path === 'index.html') {
      body = body.toString()
        .replace('<script type="module"', '<script src="__stub.js"></script>\n<script type="module"')
        .replace('</head>', `<script>${pageOptions}</script><style>body{width:360px;height:560px;margin:24px auto;outline:1px solid #8884}body.shot{margin:0;outline:0}</style></head>`);
    }
    res.writeHead(200, { 'content-type': TYPES[extname(path)] || 'application/octet-stream' }).end(body);
  } catch {
    res.writeHead(404).end();
  }
}).listen(PORT, '127.0.0.1', () => console.log(`Preview at http://localhost:${PORT}`));
