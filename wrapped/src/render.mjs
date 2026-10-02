import { fmtHour } from './stats.mjs';
import { prettyModel } from './models.mjs';

export const AGENT_NAMES = { claude: 'Claude Code', codex: 'Codex', cursor: 'Cursor', antigravity: 'Antigravity' };
const agentName = (a) => AGENT_NAMES[a] || a;

const esc = (v) => String(v ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

export function fmtNum(n) {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)}B`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e4) return `${(n / 1e3).toFixed(0)}K`;
  return Math.round(n).toLocaleString('en-US');
}
const fmtHours = (h) => (h >= 10 ? Math.round(h).toString() : h.toFixed(1));
const pct = (x) => `${Math.round(x * 100)}%`;
const fmtDate = (ts) => new Date(ts).toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
const fmtDateTime = (ts) => new Date(ts).toLocaleString('en-US', { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });
const prettyTool = (t) => (t.startsWith('mcp__') ? t.split('__').slice(1).join(' · ').replace(/_/g, ' ') : t);
const label = (s) => s.title || s.project || 'untitled';

function dayBars(st) {
  const days = [];
  for (let t = st.window.since; t <= st.window.until; t += 86_400_000) {
    const d = new Date(t);
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    days.push({ key, t, hours: (st.minutesByDay[key] || 0) / 60 });
  }
  const max = Math.max(1, ...days.map((d) => d.hours));
  const w = 720;
  const h = 140;
  const bw = w / days.length;
  const bars = days.map((d, i) => {
    const bh = Math.max(d.hours > 0 ? 2 : 0, (d.hours / max) * (h - 18));
    return `<rect x="${(i * bw + 1).toFixed(1)}" y="${(h - 16 - bh).toFixed(1)}" width="${(bw - 2).toFixed(1)}" height="${bh.toFixed(1)}" rx="1.5" class="bar"><title>${esc(fmtDate(d.t))}: ${d.hours.toFixed(1)} agent-hours</title></rect>`;
  }).join('');
  const first = `<text x="0" y="${h - 2}" class="axis">${esc(fmtDate(days[0].t))}</text>`;
  const last = `<text x="${w}" y="${h - 2}" class="axis" text-anchor="end">${esc(fmtDate(days[days.length - 1].t))}</text>`;
  return `<svg viewBox="0 0 ${w} ${h}" class="chart" role="img" aria-label="Agent-hours per day">${bars}${first}${last}</svg>`;
}

function hourBars(st) {
  const max = Math.max(1, ...st.minutesByHour);
  const w = 720;
  const h = 110;
  const bw = w / 24;
  const bars = st.minutesByHour.map((m, i) => {
    const bh = Math.max(m > 0 ? 2 : 0, (m / max) * (h - 18));
    const cls = i === st.rushHour ? 'bar hot' : 'bar';
    return `<rect x="${(i * bw + 2).toFixed(1)}" y="${(h - 16 - bh).toFixed(1)}" width="${(bw - 4).toFixed(1)}" height="${bh.toFixed(1)}" rx="1.5" class="${cls}"><title>${fmtHour(i)}: ${(m / 60).toFixed(1)} agent-hours started</title></rect>`;
  }).join('');
  const ticks = [0, 6, 12, 18].map((i) => `<text x="${(i * bw + bw / 2).toFixed(1)}" y="${h - 2}" class="axis" text-anchor="middle">${fmtHour(i)}</text>`).join('');
  return `<svg viewBox="0 0 ${w} ${h}" class="chart" role="img" aria-label="Agent-hours by hour of day">${bars}${ticks}</svg>`;
}

function rankList(rows, fmt, { mask = false } = {}) {
  const max = Math.max(1, ...rows.map((r) => r[1]));
  return `<ol class="rank">${rows.map(([name, v]) => `
    <li><span class="rank-name${mask ? ' private' : ''}">${esc(name)}</span><span class="rank-val">${esc(fmt(v))}</span>
    <span class="rank-bar" style="width:${((v / max) * 100).toFixed(1)}%"></span></li>`).join('')}</ol>`;
}

export function renderHtml(st, meta) {
  const agents = Object.keys(st.byAgent).map(agentName).join(' + ') || 'no agents';
  const f = meta.filter || {};
  const filterLabel = [f.agent && agentName(f.agent), f.maker && `${f.maker} models`].filter(Boolean).join(' · ');
  const badges = st.persona.badges || [];
  const agentRows = Object.entries(st.byAgent).sort((a, b) => b[1].hours - a[1].hours).map(([a, r]) => `
    <tr><td>${esc(agentName(a))}</td><td>${fmtNum(r.sessions)}</td><td>${fmtNum(r.prompts)}</td><td>${fmtHours(r.hours)}</td><td>${fmtNum(r.responses)}</td><td>${r.tokens ? fmtNum(r.tokens) : '<span class="muted">not recorded</span>'}</td></tr>`).join('');
  const limitRows = st.limitHits.slice(-8).reverse().map((h) => `
    <li><span>${esc(fmtDateTime(h.ts))}</span><span class="muted">${esc(h.agent === 'claude' ? 'Claude' : 'Codex')} · ${esc(String(h.type).replace(/_/g, ' '))}</span><span class="muted">reset ${esc(h.resetsAt ? fmtDateTime(h.resetsAt) : '-')}</span></li>`).join('');

  const share = {
    persona: st.persona,
    badges: badges.map((b) => b.name.replace(/^The /, '')),
    window: `${fmtDate(st.window.since)} – ${fmtDate(st.window.until)}`,
    agents: filterLabel ? `${agents} · ${filterLabel} only` : agents,
    stats: [
      [fmtHours(st.agentHours), 'agent-hours'],
      [fmtNum(st.sessions), 'sessions'],
      [fmtNum(st.filesEdited), 'files touched'],
      [String(st.peakParallel), 'agents at once, peak'],
      [fmtHours(st.waitHours), 'hours agents waited on me'],
      [String(st.limitHits.length), 'times I hit the limit'],
    ],
  };

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data: blob:">
<title>Agent Wrapped</title>
<style>
:root {
  --bg: #f6f4ef; --panel: #ffffff; --ink: #1b1a17; --muted: #6d6a62; --line: #e4e0d6;
  --accent: #c2410c; --accent-soft: #fbe3d6; --bar: #2f2d28;
}
@media (prefers-color-scheme: dark) {
  :root { --bg: #121110; --panel: #1b1a18; --ink: #efece4; --muted: #9c978b; --line: #2c2a26;
    --accent: #fb923c; --accent-soft: #3a2316; --bar: #d8d3c7; }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--ink);
  font: 15px/1.5 -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif; }
main { max-width: 820px; margin: 0 auto; padding: 48px 16px 80px; }
header { margin-bottom: 36px; }
.eyebrow { color: var(--muted); font-size: 13px; letter-spacing: .04em; text-transform: uppercase; }
.eyebrow .filter { color: var(--accent); }
h1 { font-size: clamp(36px, 7vw, 56px); line-height: 1.05; margin: 8px 0 10px; letter-spacing: -.02em; }
h1 em { font-style: normal; color: var(--accent); }
.lede { font-size: 18px; color: var(--muted); margin: 0; }
.badges { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 14px; padding: 0; list-style: none; }
.badges li { border: 1px solid var(--line); background: var(--panel); border-radius: 999px; padding: 4px 12px; font-size: 13px; }
.badges b { font-weight: 600; }
table.agents { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
table.agents th { text-align: right; color: var(--muted); font-weight: 500; font-size: 12px; padding: 0 0 8px; }
table.agents td { text-align: right; padding: 8px 0; border-top: 1px solid var(--line); }
table.agents th:first-child, table.agents td:first-child { text-align: left; }
.table-wrap { overflow-x: auto; }
h2 { font-size: 13px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); margin: 0 0 14px; font-weight: 600; }
section { background: var(--panel); border: 1px solid var(--line); border-radius: 14px; padding: 22px; margin-bottom: 16px; }
.grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 1px; background: var(--line);
  border: 1px solid var(--line); border-radius: 14px; overflow: hidden; margin-bottom: 16px; }
.cell { background: var(--panel); padding: 18px 20px; }
.num { font-size: 34px; font-weight: 650; letter-spacing: -.02em; font-variant-numeric: tabular-nums; }
.cap { color: var(--muted); font-size: 13px; }
.callout { border-color: var(--accent); background: linear-gradient(0deg, var(--accent-soft), var(--accent-soft)), var(--panel); }
.callout .big { font-size: 28px; font-weight: 650; letter-spacing: -.01em; }
.callout .big b { color: var(--accent); }
.chart { width: 100%; height: auto; display: block; }
.bar { fill: var(--bar); opacity: .85; }
.bar.hot { fill: var(--accent); opacity: 1; }
.axis { fill: var(--muted); font-size: 11px; }
.two { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
.two section { margin-bottom: 0; }
.rank { list-style: none; margin: 0; padding: 0; }
.rank li { position: relative; display: flex; justify-content: space-between; gap: 12px; padding: 7px 0; border-top: 1px solid var(--line); }
.rank li:first-child { border-top: 0; }
.rank-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rank-val { color: var(--muted); font-variant-numeric: tabular-nums; }
.rank-bar { position: absolute; left: 0; bottom: -1px; height: 2px; background: var(--accent); opacity: .6; }
.facts { display: grid; grid-template-columns: 1fr 1fr; gap: 8px 24px; margin: 0; }
.facts div { display: flex; justify-content: space-between; border-top: 1px solid var(--line); padding-top: 8px; }
.facts dt { color: var(--muted); } .facts dd { margin: 0; font-variant-numeric: tabular-nums; text-align: right; }
.limits { list-style: none; padding: 0; margin: 0; }
.limits li { display: grid; grid-template-columns: 1fr 1.2fr 1fr; gap: 8px; padding: 7px 0; border-top: 1px solid var(--line); font-size: 14px; }
.muted { color: var(--muted); }
.share canvas { width: 100%; height: auto; border-radius: 10px; border: 1px solid var(--line); display: block; }
.row { display: flex; gap: 10px; flex-wrap: wrap; align-items: center; margin-top: 14px; }
button { font: inherit; padding: 9px 16px; border-radius: 9px; border: 1px solid var(--line); background: var(--panel); color: var(--ink); cursor: pointer; }
button.primary { background: var(--ink); color: var(--bg); border-color: var(--ink); }
label { color: var(--muted); font-size: 14px; display: flex; gap: 6px; align-items: center; }
body.masked .private { filter: blur(6px); }
footer { color: var(--muted); font-size: 13px; margin-top: 28px; }
footer ul { padding-left: 18px; }
@media (max-width: 640px) {
  .grid { grid-template-columns: repeat(2, 1fr); }
  .two, .facts { grid-template-columns: 1fr; }
  .limits li { grid-template-columns: 1fr; gap: 0; }
}
</style>
</head>
<body>
<main>
<header>
  <div class="eyebrow">Agent Wrapped · ${esc(share.window)} · ${esc(agents)}${filterLabel ? ` · <b class="filter">${esc(filterLabel)} only</b>` : ''}</div>
  <h1>You are <em>${esc(st.persona.name)}</em>.</h1>
  <p class="lede">${esc(st.persona.line)}</p>
  ${badges.length ? `<ul class="badges">${badges.map((b) => `<li title="${esc(b.line)}"><b>${esc(b.name)}</b> · ${esc(b.line)}</li>`).join('')}</ul>` : ''}
</header>

<div class="grid">
  <div class="cell"><div class="num">${fmtHours(st.agentHours)}</div><div class="cap">agent-hours of work</div></div>
  <div class="cell"><div class="num">${fmtNum(st.sessions)}</div><div class="cap">sessions you started</div></div>
  <div class="cell"><div class="num">${fmtNum(st.prompts)}</div><div class="cap">prompts you typed</div></div>
  <div class="cell"><div class="num">${fmtNum(st.filesEdited)}</div><div class="cap">files touched</div></div>
  <div class="cell"><div class="num">${st.peakParallel}</div><div class="cap">agents at once, peak${st.peakParallelAt ? ` (${esc(fmtDate(st.peakParallelAt))})` : ''}</div></div>
  <div class="cell"><div class="num">${st.limitHits.length}</div><div class="cap">times you hit a usage limit</div></div>
</div>

<section class="callout">
  <h2>Your agents waited on you</h2>
  <div class="big"><b>${fmtHours(st.waitHours)} hours</b> of finished work sat waiting for your next message.</div>
  <p class="muted">${fmtNum(st.waits)} times an agent finished and waited. ${fmtNum(st.longWaits)} of those waits were longer than 5 minutes. Gaps over an hour are counted as you being away, not waiting.</p>
</section>

<section>
  <h2>Agent-hours per day</h2>
  ${dayBars(st)}
  <p class="muted">Active ${st.daysActive} of ${st.window.days} days. Longest streak ${st.longestStreak} days.${st.busiestDay ? ` Busiest: ${esc(fmtDate(Date.parse(st.busiestDay.day + 'T12:00:00')))}, ${fmtHours(st.busiestDay.hours)} agent-hours across parallel sessions.` : ''}</p>
</section>

<section>
  <h2>When your agents work</h2>
  ${hourBars(st)}
  <p class="muted">${st.rushHour !== null ? `Rush hour is ${fmtHour(st.rushHour)}.` : ''}</p>
</section>

<section>
  <h2>Agents</h2>
  <div class="table-wrap"><table class="agents">
    <thead><tr><th>Agent</th><th>Sessions</th><th>Prompts</th><th>Agent-hours</th><th>Responses</th><th>Tokens</th></tr></thead>
    <tbody>${agentRows}</tbody>
  </table></div>
</section>

<div class="two">
  <section>
    <h2>Where the tokens went</h2>
    ${rankList(st.projects, fmtNum, { mask: true })}
    <p class="muted">${st.projectCount} projects in total.</p>
  </section>
  <section>
    <h2>Models, by responses</h2>
    ${rankList(st.modelResponses.map(([m, n]) => [prettyModel(m), n]), fmtNum)}
    <p class="muted">By maker: ${st.makers.map(([m, n]) => `${esc(m)} ${pct(n / Math.max(1, st.responses))}`).join(' · ') || '-'}</p>
  </section>
</div>
<div style="height:16px"></div>
<div class="two">
  <section>
    <h2>Most used tools</h2>
    ${rankList(st.tools.map(([t, n]) => [prettyTool(t), n]), fmtNum)}
    <p class="muted">${fmtNum(st.toolCalls)} tool calls in total.</p>
  </section>
  <section>
    <h2>How the context was spent</h2>
    <dl class="facts">
      <div><dt>Total tokens</dt><dd>${fmtNum(st.totalTokens)}</dd></div>
      <div><dt>Re-read from cache</dt><dd>${pct(st.cacheShare)}</dd></div>
      <div><dt>Written by models</dt><dd>${fmtNum(st.tokens.output)}</dd></div>
      <div><dt>Used by subagents</dt><dd>${pct(st.subagentShare)}</dd></div>
      <div><dt>Plugins and scripts</dt><dd>${pct(st.automatedShare)}</dd></div>
      <div><dt>Context compactions</dt><dd>${fmtNum(st.compactions)}</dd></div>
    </dl>
  </section>
</div>
<div style="height:16px"></div>

<div class="two">
  <section>
    <h2>Longest session</h2>
    ${st.longestSession ? `<div class="num" style="font-size:26px">${fmtHours(st.longestSession.hours)} agent-hours</div><div class="private">${esc(label(st.longestSession))}</div>` : '<div class="muted">None yet.</div>'}
  </section>
  <section>
    <h2>Biggest session</h2>
    ${st.biggestSession ? `<div class="num" style="font-size:26px">${fmtNum(st.biggestSession.tokens)} tokens</div><div class="private">${esc(label(st.biggestSession))}</div>` : '<div class="muted">None yet.</div>'}
  </section>
</div>
<div style="height:16px"></div>

<section>
  <h2>The wall</h2>
  ${st.limitHits.length ? `<p class="muted" style="margin-top:0">You hit a usage limit ${st.limitHits.length} times. Most recent first.</p><ul class="limits">${limitRows}</ul>` : '<p class="muted" style="margin:0">No usage limits hit in this window.</p>'}
  ${st.codexPeakQuotaPct !== null ? `<p class="muted">Codex peak window usage: ${Math.round(st.codexPeakQuotaPct)}%.</p>` : ''}
</section>

<section class="share">
  <h2>Share card</h2>
  <canvas id="card" width="1200" height="630"></canvas>
  <div class="row">
    <button class="primary" id="save">Save image</button>
    <button id="copy">Copy text</button>
    <label><input type="checkbox" id="mask" checked> Blur project names on this page</label>
  </div>
  <p class="muted">The card shows numbers only. No project names, prompts, or file paths.</p>
</section>

<footer>
  Made on this Mac from ${fmtNum(meta.files.claude)} Claude Code and ${fmtNum(meta.files.codex)} Codex log files${meta.files.cursor ? ', the Cursor chat database' : ''}${meta.files.antigravity ? `, ${fmtNum(meta.files.antigravity)} Antigravity conversations` : ''} (${(meta.bytes / 1e9).toFixed(1)} GB) in ${meta.seconds.toFixed(1)}s. Nothing was sent anywhere.
  <ul>
    <li>Agent-hours: time from your prompt to the agent's last action in that turn, capped at 3 hours per turn. Parallel sessions add up.</li>
    <li>Sessions you started: ones with at least one prompt you typed. ${fmtNum(st.automatedRuns)} runs driven by plugins or scripts and ${fmtNum(st.subagentRuns)} subagent runs are counted in tokens only.</li>
    <li>Models are compared by number of responses, because Cursor keeps no token counts on disk. Cursor and Antigravity keep no token counts we can read, so token figures cover Claude Code and Codex only.</li>
    <li>Gemini CLI, Copilot, Qwen Code, the Antigravity IDE, and web chats are not read yet.</li>
  </ul>
</footer>
</main>
<script>
const SHARE = ${JSON.stringify(share).replace(/</g, '\\u003c')};
const canvas = document.getElementById('card');
function draw() {
  const c = canvas.getContext('2d');
  const W = 1200, H = 630;
  c.fillStyle = '#121110'; c.fillRect(0, 0, W, H);
  c.fillStyle = '#fb923c'; c.fillRect(0, 0, 10, H);
  const font = (w, s) => w + ' ' + s + 'px -apple-system, BlinkMacSystemFont, "Helvetica Neue", sans-serif';
  c.fillStyle = '#9c978b'; c.font = font(500, 22);
  c.fillText(('Agent Wrapped · ' + SHARE.window + ' · ' + SHARE.agents).toUpperCase(), 70, 82);
  c.fillStyle = '#efece4'; c.font = font(700, 64);
  c.fillText('I am ', 70, 172);
  const w = c.measureText('I am ').width;
  c.fillStyle = '#fb923c'; c.fillText(SHARE.persona.name + '.', 70 + w, 172);
  c.fillStyle = '#9c978b'; c.font = font(400, 28);
  c.fillText(SHARE.persona.line, 70, 220);
  if (SHARE.badges.length) {
    c.fillStyle = '#efece4'; c.font = font(500, 24);
    c.fillText('+ ' + SHARE.badges.join('  ·  '), 70, 262);
  }
  SHARE.stats.forEach(([n, l], i) => {
    const x = 70 + (i % 3) * 360, y = 360 + Math.floor(i / 3) * 128;
    c.fillStyle = '#efece4'; c.font = font(700, 58); c.fillText(n, x, y);
    c.fillStyle = '#9c978b'; c.font = font(400, 24); c.fillText(l, x, y + 38);
  });
  c.fillStyle = '#6d6a62'; c.font = font(400, 20);
  c.fillText('Made locally from my agent logs. npx agent-wrapped', 70, H - 40);
}
draw();
document.getElementById('save').onclick = () => {
  canvas.toBlob((b) => {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(b); a.download = 'agent-wrapped.png'; a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  });
};
document.getElementById('copy').onclick = async (e) => {
  const text = 'I am ' + SHARE.persona.name + '. ' + SHARE.persona.line +
    (SHARE.badges.length ? ' Also: ' + SHARE.badges.join(', ') + '.' : '') + '\\n' +
    SHARE.stats.map(([n, l]) => n + ' ' + l).join(' · ') + '\\n(' + SHARE.window + ', ' + SHARE.agents + ')';
  try { await navigator.clipboard.writeText(text); e.target.textContent = 'Copied'; }
  catch { e.target.textContent = 'Copy failed'; }
  setTimeout(() => (e.target.textContent = 'Copy text'), 1500);
};
const mask = document.getElementById('mask');
const applyMask = () => document.body.classList.toggle('masked', mask.checked);
mask.onchange = applyMask; applyMask();
</script>
</body>
</html>
`;
}
