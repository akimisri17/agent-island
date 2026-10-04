// Dev-only: made-up sessions, repos and live state for `DEMO=1 npm run preview`,
// so screenshots never show real projects. Deterministic for a given day.
const M = 60_000, H = 60 * M, D = 24 * H;
const ROOT = '/Users/demo/code';

const PROJECTS = {
  shop: ['Checkout: retry failed payments', 'Cart totals off by a cent', 'Add Apple Pay button', 'Product page image zoom', 'Order emails in HTML'],
  api: ['Rate limit middleware', 'Paginate /orders', 'Fix flaky auth test', 'Webhook signature check', 'Move config to env vars'],
  site: ['New pricing page', 'Blog RSS feed', 'Dark mode for the docs nav', 'Speed up the hero image'],
  'mobile-app': ['Push notification settings', 'Offline mode for the cart', 'Crash on login with SSO'],
  docs: ['Getting started rewrite', 'API reference from OpenAPI', 'Fix broken links'],
};
const WEIGHT = { shop: 5, api: 4, site: 2, 'mobile-app': 2, docs: 1 };
const MODELS = ['claude-sonnet-4-5', 'claude-opus-4-1', 'gpt-5-codex'];

function rng(seed) {
  return () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
}

function session(r, i, project, start, agent) {
  const titles = PROJECTS[project];
  const nTurns = 3 + Math.floor(r() * 9);
  const turns = [], prompts = [];
  let t = start;
  for (let k = 0; k < nTurns; k++) {
    const len = (1 + r() * 9) * M;
    prompts.push(t);
    turns.push({ start: t, end: t + len });
    t += len + (0.5 + r() * 6) * M;
  }
  const end = turns.at(-1).end;
  const out = Math.round(2000 + r() * 30000);
  const model = agent === 'codex' ? 'gpt-5-codex' : MODELS[r() < 0.75 ? 0 : 1];
  const files = Array.from({ length: Math.floor(r() * 6) }, (_, k) => `${ROOT}/${project}/src/file${k}.ts`);
  return {
    agent, id: `demo-${start}-${i}`, file: `${ROOT}/.logs/demo-${i}.jsonl`, project, cwd: `${ROOT}/${project}`,
    title: titles[Math.floor(r() * titles.length)], start, end, prompts, turns,
    tokens: { input: Math.round(out * 0.6), cacheRead: out * 40, cacheWrite: out * 3, output: out },
    models: { [model]: nTurns * 4 }, responses: { [model]: nTurns * 4 },
    tools: { Bash: Math.floor(r() * 20), Edit: files.length * 2, Read: Math.floor(r() * 25), Grep: Math.floor(r() * 8) },
    filesEdited: files, compactions: r() < 0.1 ? 1 : 0, limitHits: [], peakQuotaPct: null, isSubagent: false,
  };
}

function pick(r) {
  const total = Object.values(WEIGHT).reduce((a, b) => a + b, 0);
  let x = r() * total;
  for (const [p, w] of Object.entries(WEIGHT)) if ((x -= w) < 0) return p;
  return 'shop';
}

export function demoData(now = Date.now()) {
  const midnight = new Date(now).setHours(0, 0, 0, 0);
  const r = rng(Math.floor(midnight / D));
  const sessions = [];
  let i = 0;
  // Past 29 days: a few sessions on most days, fewer on weekends.
  for (let d = 29; d >= 1; d--) {
    const day = midnight - d * D;
    const weekend = [0, 6].includes(new Date(day).getDay());
    const n = weekend ? Math.floor(r() * 2) : 2 + Math.floor(r() * 4);
    for (let k = 0; k < n; k++) {
      const s = session(r, i++, pick(r), day + (9 + r() * 9) * H, r() < 0.2 ? 'codex' : 'claude');
      if (d === 12 && k === 0) s.limitHits.push({ ts: s.end, type: 'five_hour', resetsAt: s.end + 2 * H });
      sessions.push(s);
    }
  }
  // Today: spread over the hours since midnight (at least the last 6 hours).
  const from = Math.max(midnight, now - 6 * H);
  const today = [['shop', 'claude'], ['api', 'claude'], ['shop', 'claude'], ['site', 'codex'], ['api', 'claude'], ['docs', 'claude']]
    .map(([p, a], k) => session(r, i++, p, from + (k / 6) * (now - from - 50 * M), a))
    .map((s) => ({ ...s, end: Math.min(s.end, now), turns: s.turns.filter((t) => t.end <= now) }));
  const all = [...sessions, ...today];
  const scan = (list, since) => ({
    sessions: list, files: { claude: list.filter((s) => s.agent === 'claude').length, codex: list.filter((s) => s.agent === 'codex').length, cursor: 0, antigravity: 0 },
    bytes: list.length * 410_000, seconds: 0.4, since, until: now,
  });

  const repo = (project, o) => ({ project, path: `${ROOT}/${project}`, branch: 'main', detached: false, upstream: `origin/${o.branch || 'main'}`, ahead: 0, behind: 0, changes: 0, stashes: 0, worktrees: [], defaultBranch: 'main', merged: [], gone: [], ...o });
  const repos = [
    repo('shop', { branch: 'feat/apple-pay', upstream: 'origin/feat/apple-pay', changes: 4, ahead: 2 }),
    repo('api', { branch: 'main', behind: 5 }),
    repo('site', { branch: 'pricing-page', upstream: null, changes: 1 }),
    repo('docs', { branch: 'main', merged: ['fix-links', 'getting-started'], gone: ['api-ref-openapi'] }),
    repo('mobile-app', { branch: 'main' }),
  ];

  return {
    scan: scan(today, midnight),
    scan30: scan(all, midnight - 29 * D),
    repos,
    demo: {
      live: [
        { agent: 'claude', sessionId: 'demo-l1', title: 'Webhook signature check', project: 'api', cwd: `${ROOT}/api`, state: 'approval', ago: 6, pid: 1, host: 'Ghostty', unread: false },
        { agent: 'claude', sessionId: 'demo-l2', title: 'Add Apple Pay button', project: 'shop', cwd: `${ROOT}/shop`, state: 'waiting', ago: 14, pid: 2, host: 'Code', unread: false },
        { agent: 'codex', sessionId: 'demo-l3', title: 'Speed up the hero image', project: 'site', cwd: `${ROOT}/site`, state: 'waiting', ago: 3, pid: 3, host: 'iTerm', unread: false },
        { agent: 'claude', sessionId: 'demo-l4', title: 'Getting started rewrite', project: 'docs', cwd: `${ROOT}/docs`, state: 'working', ago: 2, pid: 4, host: 'Terminal', unread: false },
        { agent: 'claude', sessionId: 'demo-l5', title: 'Offline mode for the cart', project: 'mobile-app', cwd: `${ROOT}/mobile-app`, state: 'idle', ago: 48, pid: 5, host: 'Ghostty', unread: false },
      ],
      commits: [
        { project: 'shop', cwd: `${ROOT}/shop`, commits: [{ subject: 'Retry failed card payments once', ago: 40 }, { subject: 'Show Apple Pay on supported devices', ago: 120 }] },
        { project: 'api', cwd: `${ROOT}/api`, commits: [{ subject: 'Add per-key rate limits', ago: 75 }] },
        { project: 'docs', cwd: `${ROOT}/docs`, commits: [{ subject: 'Rewrite getting started', ago: 20 }] },
      ],
    },
  };
}
