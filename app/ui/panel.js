// The panel. Rust reads the logs; the same stats and report code as the CLI
// (synced into ./lib by scripts/sync-lib.mjs) turns them into numbers.
import { computeStats, filterSessions, filterOptions } from './lib/stats.mjs';
import { renderHtml, fmtNum, AGENT_NAMES } from './lib/render.mjs';
import { buildRecap } from './recap.js';
import { repoRow, sortRepos } from './repos.js';
import { icon, sprite } from './icons.js';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const STALE_MS = 5 * 60_000; // re-read logs when the panel opens after this long
const TABS = ['waiting', 'today', 'repos', 'wrapped'];
const TITLES = { waiting: 'Waiting', today: 'Today', repos: 'Repos', wrapped: 'Wrapped', settings: 'Settings' };
const $ = (id) => document.getElementById(id);
const cache = new Map(); // days -> { scan, at }: raw sessions, so filters need no re-read
let days = 7;
let busy = false;
const filter = { agent: null, maker: null };
let current = null; // { stats, meta } as shown, for the full report
let view = 'waiting';
let lastView = 'waiting';
const updatedAt = {}; // view -> ms of its last successful load
let prefs = { hotkey: 'ctrl+alt+KeyJ', notifyLimits: true, recapWithClaude: false };
const IS_MAC = /Mac/.test(navigator.platform);

// Icons: one sprite, then fill every placeholder.
document.body.insertAdjacentHTML('afterbegin', sprite());
for (const ph of document.querySelectorAll('[data-icon]')) ph.outerHTML = icon(ph.dataset.icon, ph.dataset.size || '');
$('more').innerHTML = icon('more');
$('back').innerHTML = icon('back');

// "ctrl+alt+KeyJ" -> "⌃⌥J" on macOS, "Ctrl+Alt+J" elsewhere.
function hotkeyLabel(acc) {
  const mac = { ctrl: '⌃', control: '⌃', alt: '⌥', option: '⌥', shift: '⇧', super: '⌘', cmd: '⌘', command: '⌘', meta: '⌘' };
  const win = { ctrl: 'Ctrl', control: 'Ctrl', alt: 'Alt', option: 'Alt', shift: 'Shift', super: 'Win', cmd: 'Win', command: 'Win', meta: 'Win' };
  const parts = acc.split('+');
  const key = parts.pop().replace(/^Key/, '').replace(/^Digit/, '');
  const mods = parts.map((m) => (IS_MAC ? mac : win)[m.toLowerCase()] || m);
  return IS_MAC ? mods.join('') + key : [...mods, key].join('+');
}

try {
  Object.assign(filter, JSON.parse(localStorage.getItem('filter') || '{}'));
  const saved = localStorage.getItem('view');
  view = TABS.includes(saved) ? saved : 'waiting';
} catch {
  // storage unavailable: start unfiltered, on Waiting
}

const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

// One line at the bottom for errors; empty and hidden otherwise.
function note(text = '') {
  $('note').textContent = text;
  $('note').hidden = !text;
}

function ago(ts) {
  if (!ts) return '';
  const m = Math.round((Date.now() - ts) / 60_000);
  if (m < 1) return 'Updated just now';
  if (m < 60) return `Updated ${m} min ago`;
  return `Updated ${Math.round(m / 60)} h ago`;
}

// --- Waiting ---

const AGENT_LABEL = { claude: 'Claude Code', codex: 'Codex', cursor: 'Cursor', antigravity: 'Antigravity' };
// From the log alone, a pending approval and a long-running tool look the
// same; after 30 minutes the backend calls it idle (stopped mid-tool).
const STATE_LABEL = { waiting: 'Done', approval: 'Approve', idle: 'Stopped', working: 'Working' };
const HOST_LABEL = { Claude: 'Claude app', 'Visual Studio Code': 'VS Code', iTerm: 'iTerm', iTerm2: 'iTerm', WindowsTerminal: 'Terminal' };

// Compact ages: "now", "47m", "17h", "2d".
function waitedFor(ms) {
  const m = Math.floor(ms / 60_000);
  if (m < 1) return 'now';
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h}h`;
  return `${Math.floor(h / 24)}d`;
}

function liveRow(s) {
  const li = el('li', 'dotted');
  const name = el('span', 'name');
  const needs = s.state === 'waiting' || s.state === 'approval';
  name.append(el('span', needs ? 'dot needs' : 'dot'), el('span', '', s.title || s.project || 'Untitled session'));
  const right = el('span', 'right num', s.state === 'working' ? STATE_LABEL.working : `${STATE_LABEL[s.state]} · ${waitedFor(Date.now() - s.since)}`);
  right.title = s.state === 'working' ? '' : `Since ${new Date(s.since).toLocaleString()}`;
  const where = (s.host && (HOST_LABEL[s.host] || s.host)) || AGENT_LABEL[s.agent] || s.agent;
  const line = el('span', 'line', [s.title && s.project, where, s.unread && 'unread'].filter(Boolean).join(' · '));
  li.append(name, right, line);
  if (needs) {
    li.tabIndex = 0;
    li.title = `Open in ${s.host || 'its app'}`;
    const go = async () => {
      try {
        await invoke('jump', { sessionId: s.sessionId });
        window.__TAURI__.window.getCurrentWindow().hide();
      } catch (e) {
        note(String(e));
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
    if (view === 'waiting') note(`Could not list sessions: ${e}`);
    return;
  }
  lastLive = sessions;
  updatedAt.waiting = Date.now();
  const needs = sessions.filter((s) => s.state === 'waiting' || s.state === 'approval');
  const idle = sessions.filter((s) => s.state === 'idle');
  const working = sessions.filter((s) => s.state === 'working');
  $('wdot').hidden = needs.length === 0;
  $('waiting-sub').textContent = needs.length
    ? `${plural(needs.length, 'session')} waiting · ${hotkeyLabel(prefs.hotkey)} jumps to the longest`
    : `${hotkeyLabel(prefs.hotkey)} jumps to the longest wait`;
  $('live').replaceChildren(...needs.map(liveRow));
  $('live-empty').hidden = needs.length > 0;
  // Idle and working sessions are folded away: they are not asking for you.
  for (const [name, list] of [['idle', idle], ['working', working]]) {
    $(name).replaceChildren(...list.map(liveRow));
    $(`${name}-count`).textContent = String(list.length);
    $(`${name}-group`).hidden = list.length === 0;
  }
}

const clock = (t) => new Date(t).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
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
      c.risk === 'limited' ? `Limited · resets ${clock(c.limitedUntil)}`
      : c.windowResets ? `resets about ${clock(c.windowResets)}`
      : 'no usage in 5 hours';
    const row = el('div', 'limit-row');
    row.append(el('span', 'limit-name', 'Claude · 5-hour limit'), el('span', 'limit-status', status));
    rows.push(row);
    // The dots only matter once this window has passed some earlier hit.
    if (c.pastHits > 0 && c.passed > 0 && c.risk !== 'limited') {
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
    const detail = [`${Math.round(w.usedPercent)}%`, w.minutesToFull && `full in ~${waitedFor(w.minutesToFull * 60_000)}`, w.resetsAt && `resets ${clock(w.resetsAt)}`].filter(Boolean).join(' · ');
    row.append(el('span', 'limit-name', `Codex · ${windowName(w.windowMinutes)}`.trim()), el('span', 'limit-status', detail));
    const meter = el('div', 'meter');
    const fill = el('b');
    fill.style.width = `${Math.min(100, w.usedPercent)}%`;
    meter.append(fill);
    rows.push(row, meter);
    if (w.usedPercent >= 80 && tone !== 'limited') tone = 'high';
  }
  box.className = `limits ${tone}`;
  box.replaceChildren(...rows);
  box.hidden = rows.length === 0;
}

// --- Navigation ---

function setView(v) {
  closeMenu();
  note();
  view = v;
  if (v !== 'settings') {
    lastView = v;
    try {
      localStorage.setItem('view', v);
    } catch {
      // not remembered across launches
    }
  }
  for (const b of document.querySelectorAll('#tabbar button')) b.setAttribute('aria-selected', String(b.dataset.view === v));
  for (const name of [...TABS, 'settings']) $(`view-${name}`).hidden = v !== name;
  $('title').textContent = TITLES[v];
  $('back').hidden = v !== 'settings';
  $('more').hidden = v === 'settings';
  if (v === 'settings') {
    for (const id of ['hotkey-msg', 'notify-msg', 'polish-msg']) $(id).textContent = '';
    return showSettings();
  }
  if (v === 'waiting') return loadLive(), loadLimits();
  if (v === 'today' || v === 'repos') return loadToday();
  return load();
}

function refresh() {
  if (view === 'waiting') return loadLive(), loadLimits(true);
  if (view === 'today' || view === 'repos') return loadToday(true);
  if (view === 'wrapped') return load(true);
}

function openMenu() {
  $('m-updated').textContent = ago(updatedAt[view]);
  $('m-updated').hidden = !updatedAt[view];
  $('menu').hidden = false;
  $('more').setAttribute('aria-expanded', 'true');
  $('m-refresh').focus();
}
function closeMenu() {
  $('menu').hidden = true;
  $('more').setAttribute('aria-expanded', 'false');
}

$('more').addEventListener('click', (e) => {
  e.stopPropagation();
  $('menu').hidden ? openMenu() : closeMenu();
});
document.addEventListener('click', (e) => {
  if (!$('menu').hidden && !$('menu').contains(e.target)) closeMenu();
});
$('m-refresh').addEventListener('click', () => (closeMenu(), refresh()));
$('m-settings').addEventListener('click', () => setView('settings'));
$('m-report').addEventListener('click', () => (closeMenu(), openReport()));
$('m-quit').addEventListener('click', () => invoke('quit'));
$('back').addEventListener('click', () => setView(lastView));
for (const b of document.querySelectorAll('#tabbar button')) b.addEventListener('click', () => setView(b.dataset.view));

let recordingHotkey = false;
document.addEventListener('keydown', (e) => {
  if (recordingHotkey) return;
  const mod = IS_MAC ? e.metaKey : e.ctrlKey;
  if (mod && e.key === 'r') return e.preventDefault(), refresh();
  if (mod && e.key === ',') return e.preventDefault(), setView('settings');
  if (mod && e.key === 'q') return e.preventDefault(), invoke('quit');
  if (e.key === 'Escape') {
    if (!$('menu').hidden) return closeMenu();
    if (view === 'settings') return setView(lastView);
    return window.__TAURI__.window.getCurrentWindow().hide();
  }
  const n = Number(e.key);
  if (!mod && !e.altKey && n >= 1 && n <= TABS.length && !(e.target instanceof HTMLInputElement)) setView(TABS[n - 1]);
});

// --- Wrapped ---

const hours = (h) => (h >= 10 || h === 0 ? Math.round(h).toString() : h.toFixed(1));

// One bar per day, oldest on the left. Days with no agent work get a faint
// stub so the time axis stays readable.
function spark(st) {
  const svg = $('spark');
  const n = st.window.days;
  const days = [];
  for (let i = 0; i < n; i++) {
    const d = new Date(st.window.until - (n - 1 - i) * 86_400_000);
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    days.push({ d, h: (st.minutesByDay[key] || 0) / 60 });
  }
  const max = Math.max(0.5, ...days.map((x) => x.h));
  const bw = 300 / n;
  const gap = n > 14 ? 1.5 : 4;
  svg.replaceChildren(...days.map(({ d, h }, i) => {
    const r = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
    const bh = h > 0 ? Math.max(3, (h / max) * 34) : 2;
    r.setAttribute('x', (i * bw + gap / 2).toFixed(1));
    r.setAttribute('y', (36 - bh).toFixed(1));
    r.setAttribute('width', Math.max(1, bw - gap).toFixed(1));
    r.setAttribute('height', bh.toFixed(1));
    r.setAttribute('rx', '1.5');
    if (h === 0) r.setAttribute('class', 'zero');
    const t = document.createElementNS('http://www.w3.org/2000/svg', 'title');
    t.textContent = `${d.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' })}: ${hours(h)} agent-hours`;
    r.append(t);
    return r;
  }));
  const fmt = (d) => d.toLocaleDateString([], { month: 'short', day: 'numeric' });
  $('axis-from').textContent = fmt(days[0].d);
  $('axis-to').textContent = 'today';
}

function fillSelect(sel, all, values, label, selected) {
  sel.replaceChildren(new Option(all, ''), ...values.map((v) => new Option(label(v), v)));
  sel.value = values.includes(selected) ? selected : '';
  sel.disabled = values.length < 2 && !selected;
}

// Applies the agent and maker filters to the cached scan and redraws.
function render() {
  const hit = cache.get(days);
  if (!hit) return;
  const { scan } = hit;
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
  show({ stats, meta });
}

function show({ stats: st, meta }) {
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
      const s = el('span', '', b.name.replace(/^The /, ''));
      s.title = b.line;
      return s;
    }));
    $('report').disabled = false;
  }
  $('hours').textContent = hours(st.agentHours);
  $('waited').textContent = hours(st.waitHours);
  $('limit-hits').textContent = String(st.limitHits.length);
  $('parallel').textContent = String(st.peakParallel);
  // Cursor keeps no token counts locally: say so rather than show "0 tokens".
  const tokens = st.totalTokens ? `${fmtNum(st.totalTokens)} tokens` : 'tokens not recorded';
  $('meta').textContent = `${fmtNum(st.sessions)} sessions · ${fmtNum(st.filesEdited)} files · ${tokens}`;
  spark(st);
}

async function load(force = false) {
  const hit = cache.get(days);
  if (hit && !force && Date.now() - hit.at < STALE_MS) return render();
  if (hit) render();
  if (busy) return;
  busy = true;
  try {
    const scan = await invoke('scan', { days });
    cache.set(days, { scan, at: Date.now() });
    updatedAt.wrapped = Date.now();
    render();
  } catch (e) {
    note(`Could not read logs: ${e}`);
  } finally {
    busy = false;
  }
}

async function openReport() {
  if (!current) await load(true);
  if (!current) return;
  try {
    await invoke('open_report', { html: renderHtml(current.stats, current.meta) });
  } catch (e) {
    note(`Could not open report: ${e}`);
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

// --- Today and Repos (one read of today's logs feeds both) ---

let recap = null;
let polishedText = null;
let todayAt = 0;
let repos = [];

async function loadToday(force = false) {
  if (!force && recap && Date.now() - todayAt < 60_000) return renderToday(), renderRepos();
  try {
    const [scan] = await Promise.all([invoke('scan_today'), loadLive()]);
    const cwds = [...new Set(scan.sessions.map((s) => s.cwd).filter(Boolean))];
    // Git runs in parallel with itself, and alongside the commit lookup.
    const [commits, repoList] = await Promise.all([
      invoke('recap_commits', { cwds, since: scan.since }).catch(() => []),
      invoke('repo_status', { cwds }).catch(() => []),
    ]);
    repos = sortRepos(repoList.map(repoRow));
    recap = buildRecap({ sessions: scan.sessions, commits, live: lastLive });
    todayAt = Date.now();
    updatedAt.today = updatedAt.repos = todayAt;
    polishedText = null;
    renderToday();
    renderRepos();
  } catch (e) {
    $('today-sub').textContent = 'Could not read today’s logs';
    note(String(e));
  }
}

function renderToday() {
  const r = recap;
  const t = r.totals;
  const sub = $('today-sub');
  if (r.projects.length) {
    sub.replaceChildren(el('b', '', t.time), ` of agent work · ${plural(t.projects, 'project')} · ${plural(t.commits, 'commit')}`);
  } else {
    sub.textContent = '';
  }
  $('today-empty').hidden = r.projects.length > 0;
  $('today-list').replaceChildren(
    ...r.projects.map((p) => {
      const li = el('li');
      li.append(el('span', 'name', p.project), el('span', 'right strong num', p.time));
      if (p.titles.length) li.append(el('span', 'line', p.titles.join(' · ')));
      const counts = [p.commits.length && plural(p.commits.length, 'commit'), p.files && plural(p.files, 'file')].filter(Boolean);
      if (counts.length) li.append(el('span', 'line', counts.join(' · ')));
      return li;
    }),
  );
  $('polished').hidden = !polishedText;
  $('polished').textContent = polishedText || '';
  const copy = $('copy-recap');
  copy.disabled = !r.projects.length;
  copy.innerHTML = `${icon('copy', 's')}<span>${polishedText ? 'Copy polished' : 'Copy standup'}</span>`;
  const polish = $('polish');
  polish.disabled = !r.projects.length;
  polish.innerHTML = `${icon('spark', 's')}<span>Polish</span>`;
  polish.title = prefs.recapWithClaude ? 'Rewrites the recap with your own claude command' : 'Turn on "Polish recap with Claude" in Settings';
}

function renderRepos() {
  $('repos-empty').hidden = repos.length > 0;
  $('repos').replaceChildren(
    ...repos.map((r) => {
      const li = el('li', 'dotted');
      const name = el('span', 'name');
      name.append(el('span', `dot ${r.dot}`), el('span', '', r.name));
      const branch = el('span', 'right mono', r.branch);
      branch.title = r.path;
      li.append(name, branch, el('span', 'line', r.status));
      return li;
    }),
  );
}

$('copy-recap').addEventListener('click', async () => {
  const label = $('copy-recap').querySelector('span');
  try {
    await navigator.clipboard.writeText(polishedText || recap.text);
    label.textContent = 'Copied';
  } catch {
    label.textContent = 'Copy failed';
  }
  setTimeout(renderToday, 1500);
});

$('polish').addEventListener('click', async () => {
  if (!prefs.recapWithClaude) {
    setView('settings');
    lastView = 'today';
    $('polish-msg').textContent = 'Turn this on to polish the recap.';
    return;
  }
  const btn = $('polish');
  btn.disabled = true;
  btn.querySelector('span').textContent = 'Asking Claude…';
  try {
    polishedText = await invoke('polish_recap', { text: recap.text });
  } catch (e) {
    note(String(e));
  } finally {
    renderToday();
  }
});

// --- Settings ---

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

$('notify').addEventListener('change', (e) => saveSettings({ notifyLimits: e.target.checked }));
$('test-notify').addEventListener('click', async (e) => {
  e.preventDefault();
  try {
    await invoke('test_notification');
    $('notify-msg').textContent = '';
  } catch (err) {
    $('notify-msg').textContent = `Could not notify: ${err}`;
  }
});
$('recap-claude').addEventListener('change', (e) => saveSettings({ recapWithClaude: e.target.checked }));

// Click the key, then press a shortcut. Needs a modifier unless it is F1–F24.
$('hotkey').addEventListener('click', () => {
  const btn = $('hotkey');
  btn.classList.add('recording');
  btn.textContent = 'Press keys…';
  $('hotkey-msg').textContent = '';
  recordingHotkey = true;
  const onKey = async (e) => {
    e.preventDefault();
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return; // wait for the real key
    document.removeEventListener('keydown', onKey, true);
    btn.classList.remove('recording');
    recordingHotkey = false;
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
  loadLive(); // cheap: keeps the Waiting dot current on every tab
  if (view === 'waiting') loadLimits();
  if (view === 'wrapped') load();
  if (view === 'today' || view === 'repos') loadToday();
});
listen('open-report', async () => {
  if (!cache.get(days)) await load(true);
  openReport();
});
invoke('get_settings')
  .then((p) => (prefs = p))
  .catch(() => {})
  .finally(() => setView(view));
