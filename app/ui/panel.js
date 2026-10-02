// The panel. Rust reads the logs; the same stats and report code as the CLI
// (synced into ./lib by scripts/sync-lib.mjs) turns them into numbers.
import { computeStats } from './lib/stats.mjs';
import { renderHtml, fmtNum } from './lib/render.mjs';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const STALE_MS = 5 * 60_000; // re-read logs when the panel opens after this long
const $ = (id) => document.getElementById(id);
const cache = new Map(); // days -> { stats, meta, at }
let days = 7;
let busy = false;

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

function show({ stats: st, meta, at }) {
  if (meta.files.claude + meta.files.codex + (meta.files.cursor || 0) === 0) {
    $('persona').textContent = 'No agent logs yet';
    $('line').textContent = `Nothing from Claude Code, Codex, or Cursor in the last ${days} days.`;
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
  $('meta').textContent = `${fmtNum(st.sessions)} sessions · ${fmtNum(st.filesEdited)} files · ${fmtNum(st.totalTokens)} tokens`;
  $('updated').textContent = ago(at);
  spark(st);
}

async function load(force = false) {
  const hit = cache.get(days);
  if (hit && !force && Date.now() - hit.at < STALE_MS) return show(hit);
  if (hit) show(hit);
  if (busy) return;
  busy = true;
  $('updated').textContent = 'Reading logs…';
  try {
    const r = await invoke('scan', { days });
    const stats = computeStats(r.sessions, { since: r.since, until: r.until });
    const entry = { stats, meta: { files: r.files, bytes: r.bytes, seconds: r.seconds }, at: Date.now() };
    cache.set(days, entry);
    show(entry);
  } catch (e) {
    $('updated').textContent = `Could not read logs: ${e}`;
    $('updated').classList.add('error');
  } finally {
    busy = false;
  }
}

async function openReport() {
  const entry = cache.get(days);
  if (!entry) return;
  try {
    await invoke('open_report', { html: renderHtml(entry.stats, entry.meta) });
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
