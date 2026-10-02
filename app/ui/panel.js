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
let view = 'waiting';
const HOTKEY_HINT = '⌃⌥J jumps to the longest wait';

try {
  Object.assign(filter, JSON.parse(localStorage.getItem('filter') || '{}'));
  view = localStorage.getItem('view') === 'wrapped' ? 'wrapped' : 'waiting';
} catch {
  // storage unavailable: start unfiltered, on Waiting
}

const AGENT_LABEL = { claude: 'Claude Code', codex: 'Codex', cursor: 'Cursor' };
const STATE_LABEL = { waiting: 'Finished', approval: 'Tool running or needs approval', working: 'Working' };

function waitedFor(ms) {
  const m = Math.floor(ms / 60_000);
  if (m < 1) return 'now';
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h} h`;
  return `${Math.floor(h / 24)} d`;
}

function liveRow(s) {
  const li = document.createElement('li');
  li.className = s.state;
  const dot = document.createElement('span');
  dot.className = 'dot';
  const title = document.createElement('span');
  title.className = 'title';
  title.textContent = s.title || s.project || 'Untitled session';
  const age = document.createElement('span');
  age.className = 'age';
  age.textContent = s.state === 'working' ? '' : waitedFor(Date.now() - s.since);
  if (s.state !== 'working') {
    const small = document.createElement('small');
    small.textContent = 'waiting';
    age.append(small);
  }
  const sub = document.createElement('span');
  sub.className = 'sub';
  const host = s.host && s.host !== AGENT_LABEL[s.agent] ? `in ${s.host}` : null;
  sub.textContent = [s.unread ? `${STATE_LABEL[s.state]}, unread` : STATE_LABEL[s.state], AGENT_LABEL[s.agent] || s.agent, host, s.title && s.project].filter(Boolean).join(' · ');
  li.append(dot, title, age, sub);
  if (s.state !== 'working') {
    li.tabIndex = 0;
    li.title = `Open in ${s.host || 'its app'}`;
    const go = async () => {
      try {
        await invoke('jump', { sessionId: s.sessionId });
        window.__TAURI__.window.getCurrentWindow().hide();
      } catch (e) {
        $('updated').textContent = String(e);
      }
    };
    li.addEventListener('click', go);
    li.addEventListener('keydown', (e) => e.key === 'Enter' && go());
  }
  return li;
}

async function loadLive() {
  let sessions;
  try {
    sessions = await invoke('live');
  } catch (e) {
    if (view === 'waiting') $('updated').textContent = `Could not list sessions: ${e}`;
    return;
  }
  const waiting = sessions.filter((s) => s.state !== 'working');
  const working = sessions.filter((s) => s.state === 'working');
  $('wcount').textContent = waiting.length ? String(waiting.length) : '';
  const items = waiting.map(liveRow);
  if (working.length) {
    const label = document.createElement('li');
    label.className = 'section-label';
    label.style.cssText = 'border:0;background:none;padding:0;cursor:default;display:block';
    label.textContent = `Working (${working.length})`;
    items.push(label, ...working.map(liveRow));
  }
  $('live').replaceChildren(...items);
  $('live-empty').hidden = waiting.length > 0;
  if (view === 'waiting') $('updated').textContent = HOTKEY_HINT;
}

function setView(v) {
  view = v;
  try {
    localStorage.setItem('view', v);
  } catch {
    // not remembered across launches
  }
  for (const b of document.querySelectorAll('.tabs button')) b.setAttribute('aria-selected', String(b.dataset.view === v));
  $('view-waiting').hidden = v !== 'waiting';
  $('view-wrapped').hidden = v !== 'wrapped';
  document.querySelector('.wrapped-only').hidden = v !== 'wrapped';
  if (v === 'waiting') {
    $('updated').textContent = HOTKEY_HINT;
    loadLive();
  } else {
    load();
  }
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
  if (Object.values(meta.files).every((n) => !n)) {
    $('persona').textContent = 'No agent logs yet';
    $('line').textContent = `Nothing from Claude Code, Codex, Cursor, or Antigravity in the last ${days} days.`;
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
  if (view === 'wrapped') $('updated').textContent = ago(at);
  spark(st);
}

async function load(force = false) {
  const hit = cache.get(days);
  if (hit && !force && Date.now() - hit.at < STALE_MS) {
    render();
    $('updated').textContent = ago(hit.at);
    return;
  }
  if (hit) render();
  if (busy) return;
  busy = true;
  if (view === 'wrapped') $('updated').textContent = 'Reading logs…';
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
$('refresh').addEventListener('click', () => (view === 'waiting' ? loadLive() : load(true)));
for (const b of document.querySelectorAll('.tabs button')) b.addEventListener('click', () => setView(b.dataset.view));
$('quit').addEventListener('click', () => invoke('quit'));
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') window.__TAURI__.window.getCurrentWindow().hide();
});

listen('panel-shown', () => {
  loadLive(); // cheap: keeps the count current on both tabs
  if (view === 'wrapped') load();
});
listen('open-report', async () => {
  if (!cache.get(days)) await load(true);
  openReport();
});
setView(view);
