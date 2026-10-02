// The panel. Rust reads the logs; the same stats and report code as the CLI
// (synced into ./lib by scripts/sync-lib.mjs) turns them into numbers.
import { computeStats, filterSessions, filterOptions } from './lib/stats.mjs';
import { renderHtml, fmtNum, AGENT_NAMES } from './lib/render.mjs';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const STALE_MS = 5 * 60_000; // re-read logs when the panel opens after this long
const $ = (id) => document.getElementById(id);
const cache = new Map(); // days -> { scan, at }: raw sessions, so filters need no re-read
let days = 7;
let busy = false;
const filter = { agent: null, maker: null };
let current = null; // { stats, meta } as shown, for the full report

try {
  Object.assign(filter, JSON.parse(localStorage.getItem('filter') || '{}'));
} catch {
  // storage unavailable: start unfiltered
}

const hours = (h) => (h >= 10 ? Math.round(h).toString() : h.toFixed(1));

function ago(ts) {
  const m = Math.round((Date.now() - ts) / 60_000);
  if (m < 1) return 'Updated just now';
  if (m < 60) return `Updated ${m} min ago`;
  return `Updated ${Math.round(m / 60)} h ago`;
}

function spark(st) {
  const svg = $('spark');
  const n = st.window.days;
  const vals = [];
  for (let i = 0; i < n; i++) {
    const d = new Date(st.window.until - (n - 1 - i) * 86_400_000);
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    vals.push((st.minutesByDay[key] || 0) / 60);
  }
  const max = Math.max(1, ...vals);
  const bw = 300 / n;
  svg.replaceChildren(...vals.map((v, i) => {
    const r = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
    const h = v > 0 ? Math.max(1.5, (v / max) * 34) : 0;
    r.setAttribute('x', (i * bw + 0.75).toFixed(1));
    r.setAttribute('y', (36 - h).toFixed(1));
    r.setAttribute('width', Math.max(0.5, bw - 1.5).toFixed(1));
    r.setAttribute('height', h.toFixed(1));
    r.setAttribute('rx', '1');
    return r;
  }));
}

function fillSelect(el, all, values, label, selected) {
  el.replaceChildren(new Option(all, ''), ...values.map((v) => new Option(label(v), v)));
  el.value = values.includes(selected) ? selected : '';
  el.disabled = values.length < 2 && !selected;
}

// Applies the agent and maker filters to the cached scan and redraws.
function render() {
  const hit = cache.get(days);
  if (!hit) return;
  const { scan, at } = hit;
  const agents = filterOptions(scan.sessions).agents;
  if (filter.agent && !agents.includes(filter.agent)) filter.agent = null;
  const byAgent = filterSessions(scan.sessions, { agent: filter.agent });
  const makers = filterOptions(byAgent).makers;
  if (filter.maker && !makers.includes(filter.maker)) filter.maker = null;
  fillSelect($('agent'), 'All agents', agents, (a) => AGENT_NAMES[a] || a, filter.agent);
  fillSelect($('maker'), 'All models', makers, (m) => `${m} models`, filter.maker);

  const stats = computeStats(filterSessions(byAgent, { maker: filter.maker }), { since: scan.since, until: scan.until });
  const meta = { files: scan.files, bytes: scan.bytes, seconds: scan.seconds, filter: { ...filter } };
  current = { stats, meta };
  show({ stats, meta, at });
}

function show({ stats: st, meta, at }) {
  if (meta.files.claude + meta.files.codex + (meta.files.cursor || 0) === 0) {
    $('persona').textContent = 'No agent logs yet';
    $('line').textContent = `Nothing from Claude Code, Codex, or Cursor in the last ${days} days.`;
    $('badges').replaceChildren();
    $('report').disabled = true;
  } else if (st.sessions === 0) {
    $('persona').textContent = 'Nothing for this filter';
    $('line').textContent = 'No sessions you started match it in this range.';
    $('badges').replaceChildren();
    $('report').disabled = true;
  } else {
    $('persona').textContent = st.persona.name;
    $('line').textContent = st.persona.line;
    $('badges').replaceChildren(...(st.persona.badges || []).map((b) => {
      const el = document.createElement('span');
      el.textContent = b.name.replace(/^The /, '');
      el.title = b.line;
      return el;
    }));
    $('report').disabled = false;
  }
  $('hours').textContent = hours(st.agentHours);
  $('waited').textContent = hours(st.waitHours);
  $('limits').textContent = String(st.limitHits.length);
  $('parallel').textContent = String(st.peakParallel);
  // Cursor keeps no token counts locally: say so rather than show "0 tokens".
  const tokens = st.totalTokens ? `${fmtNum(st.totalTokens)} tokens` : 'tokens not recorded';
  $('meta').textContent = `${fmtNum(st.sessions)} sessions · ${fmtNum(st.filesEdited)} files · ${tokens}`;
  $('updated').textContent = ago(at);
  spark(st);
}

async function load(force = false) {
  const hit = cache.get(days);
  if (hit && !force && Date.now() - hit.at < STALE_MS) return render();
  if (hit) render();
  if (busy) return;
  busy = true;
  $('updated').textContent = 'Reading logs…';
  try {
    const scan = await invoke('scan', { days });
    cache.set(days, { scan, at: Date.now() });
    render();
  } catch (e) {
    $('updated').textContent = `Could not read logs: ${e}`;
    $('updated').classList.add('error');
  } finally {
    busy = false;
  }
}

async function openReport() {
  if (!current) return;
  try {
    await invoke('open_report', { html: renderHtml(current.stats, current.meta) });
  } catch (e) {
    $('updated').textContent = `Could not open report: ${e}`;
  }
}

for (const b of document.querySelectorAll('.seg button')) {
  b.addEventListener('click', () => {
    days = Number(b.dataset.days);
    for (const o of document.querySelectorAll('.seg button')) o.setAttribute('aria-selected', String(o === b));
    load();
  });
}
for (const key of ['agent', 'maker']) {
  $(key).addEventListener('change', () => {
    filter[key] = $(key).value || null;
    if (key === 'agent') filter.maker = null; // makers depend on the agent
    try {
      localStorage.setItem('filter', JSON.stringify(filter));
    } catch {
      // not remembered across launches; still applied now
    }
    render();
  });
}
$('report').addEventListener('click', openReport);
$('refresh').addEventListener('click', () => load(true));
$('quit').addEventListener('click', () => invoke('quit'));
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') window.__TAURI__.window.getCurrentWindow().hide();
});

listen('panel-shown', () => load());
listen('open-report', async () => {
  if (!cache.get(days)) await load(true);
  openReport();
});
load();
