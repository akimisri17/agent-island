// The panel. Rust reads the logs; the same stats and report code as the CLI
// (synced into ./lib by scripts/sync-lib.mjs) turns them into numbers.
import { computeStats, filterSessions, filterOptions } from './lib/stats.mjs';
import { renderHtml, fmtNum, AGENT_NAMES } from './lib/render.mjs';
import { buildRecap } from './recap.js';

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
let prefs = { hotkey: 'ctrl+alt+KeyJ', notifyLimits: true, recapWithClaude: false };
const IS_MAC = /Mac/.test(navigator.platform);

// "ctrl+alt+KeyJ" -> "⌃⌥J" on macOS, "Ctrl+Alt+J" elsewhere.
function hotkeyLabel(acc) {
  const mac = { ctrl: '⌃', control: '⌃', alt: '⌥', option: '⌥', shift: '⇧', super: '⌘', cmd: '⌘', command: '⌘', meta: '⌘' };
  const win = { ctrl: 'Ctrl', control: 'Ctrl', alt: 'Alt', option: 'Alt', shift: 'Shift', super: 'Win', cmd: 'Win', command: 'Win', meta: 'Win' };
  const parts = acc.split('+');
  const key = parts.pop().replace(/^Key/, '').replace(/^Digit/, '');
  const mods = parts.map((m) => (IS_MAC ? mac : win)[m.toLowerCase()] || m);
  return IS_MAC ? mods.join('') + key : [...mods, key].join('+');
}
const hotkeyHint = () => `${hotkeyLabel(prefs.hotkey)} jumps to the longest wait`;

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

let lastLive = [];
async function loadLive() {
  let sessions;
  try {
    sessions = await invoke('live');
  } catch (e) {
    if (view === 'waiting') $('updated').textContent = `Could not list sessions: ${e}`;
    return;
  }
  lastLive = sessions;
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
  if (view === 'waiting') $('updated').textContent = hotkeyHint();
}

const clock = (t) => new Date(t).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const windowName = (m) => (m === 300 ? '5-hour' : m === 10080 ? 'weekly' : m ? `${Math.round(m / 60)}-hour` : '');

let limitsAt = 0;
async function loadLimits(force = false) {
  if (!force && Date.now() - limitsAt < 60_000) return;
  limitsAt = Date.now();
  let l;
  try {
    l = await invoke('limits');
  } catch {
    return; // limits are a nice-to-have; the session list still works
  }
  const box = $('limits');
  const rows = [];
  let tone = '';
  const c = l.claude;
  if (c) {
    tone = c.risk;
    const status =
      c.risk === 'limited' ? `Limit reached · resets ${clock(c.limitedUntil)}`
      : c.windowResets ? `Window resets about ${clock(c.windowResets)}`
      : 'No usage in the last 5 hours';
    const row = el('div', 'limit-row');
    row.append(el('span', 'limit-name', 'Claude 5-hour'), el('span', 'limit-status', status));
    rows.push(row);
    if (c.pastHits > 0 && c.risk !== 'limited') {
      // One dot per past limit hit; filled once this window has passed the
      // usage that came before it. No percentage: the limit is shared with
      // claude.ai, which leaves no local trace.
      const hits = el('div', 'hits');
      hits.title = 'Each dot is a past time you hit the 5-hour limit. Filled: this window has already used more than you had used then.';
      for (let i = 0; i < c.pastHits; i++) hits.append(el('i', i < c.passed ? 'on' : ''));
      hits.append(el('span', '', `past ${c.passed} of your ${c.pastHits} limit hits`));
      rows.push(hits);
    }
    if (c.advice) rows.push(el('div', 'advice', c.advice));
  }
  for (const w of l.codex || []) {
    const row = el('div', 'limit-row');
    const detail = [w.minutesToFull && `full in ~${waitedFor(w.minutesToFull * 60_000)}`, w.resetsAt && `resets ${clock(w.resetsAt)}`].filter(Boolean).join(' · ');
    row.append(el('span', 'limit-name', `Codex ${windowName(w.windowMinutes)}`.trim()), el('span', 'limit-status', `${Math.round(w.usedPercent)}% used`));
    const meter = el('div', 'meter');
    const fill = el('b');
    fill.style.width = `${Math.min(100, w.usedPercent)}%`;
    meter.append(fill);
    rows.push(row, meter);
    if (detail) rows.push(el('div', 'hits', detail));
    if (w.usedPercent >= 80 && tone !== 'limited') tone = 'high';
  }
  box.className = `limits ${tone}`;
  box.replaceChildren(...rows);
  box.hidden = rows.length === 0;
}

function setView(v) {
  view = v;
  if (v !== 'settings') {
    try {
      localStorage.setItem('view', v);
    } catch {
      // not remembered across launches
    }
  }
  for (const b of document.querySelectorAll('.tabs button')) b.setAttribute('aria-selected', String(b.dataset.view === v));
  for (const name of ['waiting', 'today', 'wrapped', 'settings']) $(`view-${name}`).hidden = v !== name;
  document.querySelector('.wrapped-only').hidden = v !== 'wrapped';
  $('gear').setAttribute('aria-pressed', String(v === 'settings'));
  $('refresh').hidden = v === 'settings';
  if (v === 'settings') {
    $('updated').textContent = '';
    showSettings();
    return;
  }
  if (v === 'today') {
    $('updated').textContent = '';
    loadToday();
    return;
  }
  if (v === 'waiting') {
    $('updated').textContent = hotkeyHint();
    loadLive();
    loadLimits();
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
$('refresh').addEventListener('click', () => {
  if (view === 'waiting') return loadLive(), loadLimits(true);
  if (view === 'today') return loadToday(true);
  return load(true);
});
for (const b of document.querySelectorAll('.tabs button')) b.addEventListener('click', () => setView(b.dataset.view));
$('quit').addEventListener('click', () => invoke('quit'));
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') window.__TAURI__.window.getCurrentWindow().hide();
});

// --- Today ---

let recap = null;
let polishedText = null;
let todayAt = 0;

async function loadToday(force = false) {
  if (!force && recap && Date.now() - todayAt < 60_000) return renderToday();
  $('today-total').textContent = recap ? $('today-total').textContent : "Reading today's logs…";
  try {
    const [scan] = await Promise.all([invoke('scan_today'), loadLive()]);
    const cwds = [...new Set(scan.sessions.map((s) => s.cwd).filter(Boolean))];
    const commits = await invoke('recap_commits', { cwds, since: scan.since }).catch(() => []);
    recap = buildRecap({ sessions: scan.sessions, commits, live: lastLive });
    todayAt = Date.now();
    polishedText = null;
    renderToday();
  } catch (e) {
    $('today-total').textContent = 'Could not read today’s logs';
    $('today-sub').textContent = String(e);
  }
}

function renderToday() {
  const r = recap;
  const t = r.totals;
  $('today-total').textContent = r.projects.length ? `${t.time} of agent work` : 'No agent work yet today';
  $('today-sub').textContent = r.projects.length
    ? `${t.projects} project${t.projects === 1 ? '' : 's'} · ${t.agents} agent${t.agents === 1 ? '' : 's'} · ${t.commits} commit${t.commits === 1 ? '' : 's'}`
    : 'Sessions you start today show up here.';
  $('today-list').replaceChildren(
    ...r.projects.map((p) => {
      const li = el('li');
      const head = el('div', 'p-head');
      head.append(el('span', 'p-name', p.project), el('span', 'p-time', p.time));
      const counts = [p.files && `${p.files} file${p.files === 1 ? '' : 's'}`, p.commits.length && `${p.commits.length} commit${p.commits.length === 1 ? '' : 's'}`].filter(Boolean);
      li.append(head, el('div', 'p-line', [p.agents.join(', '), ...counts].join(' · ')));
      if (p.titles.length) li.append(el('div', 'p-line', p.titles.join(' · ')));
      return li;
    }),
  );
  const w = r.waiting;
  $('today-wait').hidden = !w.length;
  $('today-wait').textContent = w.length ? `Waiting on you: ${w.map((x) => `${x.title} (${x.waited})`).join(', ')}` : '';
  $('polished').hidden = !polishedText;
  $('polished').textContent = polishedText || '';
  $('copy-recap').disabled = !r.projects.length;
  $('copy-recap').textContent = polishedText ? 'Copy polished' : 'Copy for standup';
  $('polish').disabled = !r.projects.length;
  $('polish').title = prefs.recapWithClaude ? "Rewrites the recap with your own claude command" : 'Turn on "Polish recap with Claude" in Settings';
}

$('copy-recap').addEventListener('click', async () => {
  const btn = $('copy-recap');
  try {
    await navigator.clipboard.writeText(polishedText || recap.text);
    btn.textContent = 'Copied';
  } catch {
    btn.textContent = 'Copy failed';
  }
  setTimeout(renderToday, 1500);
});

$('polish').addEventListener('click', async () => {
  if (!prefs.recapWithClaude) {
    lastView = 'today';
    setView('settings');
    $('hotkey-msg').textContent = 'Turn on "Polish recap with Claude" below to use it.';
    return;
  }
  const btn = $('polish');
  btn.disabled = true;
  btn.textContent = 'Asking Claude…';
  try {
    polishedText = await invoke('polish_recap', { text: recap.text });
  } catch (e) {
    $('today-sub').textContent = String(e);
  } finally {
    btn.textContent = 'Polish with Claude';
    renderToday();
  }
});

// --- Settings ---

let lastView = 'waiting';
function showSettings() {
  $('hotkey').textContent = hotkeyLabel(prefs.hotkey);
  $('notify').checked = prefs.notifyLimits;
  $('recap-claude').checked = prefs.recapWithClaude;
}

async function saveSettings(next) {
  try {
    prefs = await invoke('set_settings', { next: { ...prefs, ...next } });
    $('hotkey-msg').textContent = '';
    return true;
  } catch (e) {
    $('hotkey-msg').textContent = String(e);
    return false;
  } finally {
    showSettings();
  }
}

$('gear').addEventListener('click', () => {
  if (view === 'settings') return setView(lastView);
  lastView = view;
  setView('settings');
});
$('back').addEventListener('click', () => setView(lastView));
$('notify').addEventListener('change', (e) => saveSettings({ notifyLimits: e.target.checked }));
$('test-notify').addEventListener('click', async (e) => {
  e.preventDefault();
  try {
    await invoke('test_notification');
    $('hotkey-msg').textContent = '';
  } catch (err) {
    $('hotkey-msg').textContent = `Could not notify: ${err}`;
  }
});
$('recap-claude').addEventListener('change', (e) => saveSettings({ recapWithClaude: e.target.checked }));

// Click the key, then press a shortcut. Needs a modifier unless it is F1–F24.
$('hotkey').addEventListener('click', () => {
  const btn = $('hotkey');
  btn.classList.add('recording');
  btn.textContent = 'Press keys…';
  $('hotkey-msg').textContent = '';
  const onKey = async (e) => {
    e.preventDefault();
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return; // wait for the real key
    document.removeEventListener('keydown', onKey, true);
    btn.classList.remove('recording');
    if (e.key === 'Escape') return showSettings();
    const mods = [e.ctrlKey && 'ctrl', e.altKey && 'alt', e.shiftKey && 'shift', e.metaKey && 'super'].filter(Boolean);
    if (!mods.length && !/^F\d{1,2}$/.test(e.code)) {
      $('hotkey-msg').textContent = 'Use at least one modifier key, such as ⌃, ⌥, ⇧ or ⌘.';
      return showSettings();
    }
    await saveSettings({ hotkey: [...mods, e.code].join('+') });
  };
  document.addEventListener('keydown', onKey, true);
});

listen('panel-shown', () => {
  loadLive(); // cheap: keeps the count current on both tabs
  if (view === 'waiting') loadLimits();
  if (view === 'wrapped') load();
  if (view === 'today') loadToday();
});
listen('open-report', async () => {
  if (!cache.get(days)) await load(true);
  openReport();
});
invoke('get_settings')
  .then((p) => (prefs = p))
  .catch(() => {})
  .finally(() => setView(view));
