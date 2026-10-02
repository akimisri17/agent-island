import { makerOf, tierOf } from './models.mjs';
import { pickPersonas } from './personas.mjs';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const MAX_TURN = 3 * HOUR; // longer than this is a forgotten loop, not work
const MAX_WAIT = HOUR; // longer than this the person was away, not waiting

const sum = (xs) => xs.reduce((a, b) => a + b, 0);
const totalTokens = (t) => t.input + t.cacheRead + t.cacheWrite + t.output;
const dayKey = (ts) => {
  const d = new Date(ts);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};

function topEntries(obj, n) {
  return Object.entries(obj).sort((a, b) => b[1] - a[1]).slice(0, n);
}

export function computeStats(sessions, { since, until }) {
  // "Yours" means a person typed at least one prompt. Sessions with no human
  // prompt are plugins and scripts driving the agent (for example claude-mem).
  const main = sessions.filter((s) => !s.isSubagent && s.prompts.length > 0);
  const subs = sessions.filter((s) => s.isSubagent);
  const automated = sessions.filter((s) => !s.isSubagent && s.prompts.length === 0);

  const tokens = { input: 0, cacheRead: 0, cacheWrite: 0, output: 0 };
  const models = {};
  const responses = {};
  const tools = {};
  const projects = {};
  const files = new Set();
  for (const s of sessions) {
    for (const k of Object.keys(tokens)) tokens[k] += s.tokens[k];
    for (const [m, n] of Object.entries(s.models)) models[m] = (models[m] || 0) + n;
    for (const [m, n] of Object.entries(s.responses || {})) responses[m] = (responses[m] || 0) + n;
    for (const [t, n] of Object.entries(s.tools)) tools[t] = (tools[t] || 0) + n;
    for (const f of s.filesEdited) files.add(f);
    const proj = s.project || 'unknown';
    projects[proj] = (projects[proj] || 0) + totalTokens(s.tokens);
  }
  const subagentTokens = sum(subs.map((s) => totalTokens(s.tokens)));
  const automatedTokens = sum(automated.map((s) => totalTokens(s.tokens)));

  // Working time, waiting time, and when it happened.
  const intervals = [];
  const minutesByDay = {};
  const minutesByHour = new Array(24).fill(0);
  const hoursByAgent = {};
  let weekendMs = 0;
  let workMs = 0;
  let waitMs = 0;
  let waits = 0;
  let longWaits = 0;
  for (const s of main) {
    s.turns.forEach((t, i) => {
      const dur = Math.min(t.end - t.start, MAX_TURN);
      if (dur > 0) {
        workMs += dur;
        intervals.push([t.start, t.start + dur]);
        const day = dayKey(t.start);
        minutesByDay[day] = (minutesByDay[day] || 0) + dur / MIN;
        minutesByHour[new Date(t.start).getHours()] += dur / MIN;
        hoursByAgent[s.agent] = (hoursByAgent[s.agent] || 0) + dur / HOUR;
        const dow = new Date(t.start).getDay();
        if (dow === 0 || dow === 6) weekendMs += dur;
      }
      const next = s.turns[i + 1];
      if (next) {
        const gap = next.start - t.end;
        if (gap > 0 && gap <= MAX_WAIT) {
          waitMs += gap;
          waits++;
          if (gap >= 5 * MIN) longWaits++;
        }
      }
    });
  }

  // Peak number of agents working at the same moment.
  const edges = [];
  for (const [a, b] of intervals) edges.push([a, 1], [b, -1]);
  edges.sort((x, y) => x[0] - y[0] || x[1] - y[1]);
  let live = 0;
  let peak = 0;
  let peakAt = null;
  for (const [ts, d] of edges) {
    live += d;
    if (live > peak) {
      peak = live;
      peakAt = ts;
    }
  }

  // Days active and longest streak.
  const days = Object.keys(minutesByDay).sort();
  let streak = 0;
  let best = 0;
  let prev = null;
  for (const d of days) {
    const t = Date.parse(`${d}T12:00:00`);
    streak = prev !== null && Math.round((t - prev) / DAY) === 1 ? streak + 1 : 1;
    best = Math.max(best, streak);
    prev = t;
  }
  const busiestDay = topEntries(minutesByDay, 1)[0] || null;
  const rushHour = minutesByHour.indexOf(Math.max(...minutesByHour));

  // Limit hits, deduped across sessions on the same account window.
  const limitHits = new Map();
  for (const s of sessions) {
    for (const h of s.limitHits) {
      const key = `${s.agent}:${h.type}:${h.resetsAt}`;
      if (!limitHits.has(key)) limitHits.set(key, { ...h, agent: s.agent });
    }
  }

  const longest = main
    .map((s) => ({ s, ms: sum(s.turns.map((t) => Math.min(t.end - t.start, MAX_TURN))) }))
    .sort((a, b) => b.ms - a.ms)[0];
  const biggest = [...main].sort((a, b) => totalTokens(b.tokens) - totalTokens(a.tokens))[0];

  const byAgent = {};
  const agentRow = (a) => (byAgent[a] ||= { sessions: 0, prompts: 0, tokens: 0, responses: 0, hours: 0 });
  for (const s of main) {
    const a = agentRow(s.agent);
    a.sessions++;
    a.prompts += s.prompts.length;
  }
  for (const s of sessions) {
    const a = agentRow(s.agent);
    a.tokens += totalTokens(s.tokens);
    a.responses += sum(Object.values(s.responses || {}));
  }
  for (const [a, h] of Object.entries(hoursByAgent)) agentRow(a).hours = h;

  // Models are compared by responses: every agent records those, while
  // Cursor keeps no token counts on disk.
  const totalResponses = sum(Object.values(responses));
  const makers = {};
  const tiers = { small: 0, mid: 0, large: 0 };
  for (const [m, n] of Object.entries(responses)) {
    makers[makerOf(m)] = (makers[makerOf(m)] || 0) + n;
    tiers[tierOf(m)] += n;
  }
  const share = (n, d) => (d ? n / d : 0);
  const toolTotal = sum(Object.values(tools));
  const toolShare = (re) => share(sum(Object.entries(tools).filter(([t]) => re.test(t)).map(([, n]) => n)), toolTotal);
  const minuteShare = (hours) => share(sum(hours.map((h) => minutesByHour[h])), sum(minutesByHour));

  const codexPeak = Math.max(-1, ...sessions.filter((s) => s.peakQuotaPct !== null).map((s) => s.peakQuotaPct));

  const stats = {
    window: { since, until, days: Math.round((until - since) / DAY) },
    sessions: main.length,
    subagentRuns: subs.length,
    automatedRuns: automated.length,
    automatedShare: totalTokens(tokens) ? automatedTokens / totalTokens(tokens) : 0,
    prompts: sum(main.map((s) => s.prompts.length)),
    tokens,
    totalTokens: totalTokens(tokens),
    subagentShare: totalTokens(tokens) ? subagentTokens / totalTokens(tokens) : 0,
    cacheShare: totalTokens(tokens) ? tokens.cacheRead / totalTokens(tokens) : 0,
    models: topEntries(models, 5),
    modelResponses: topEntries(responses, 6),
    responses: totalResponses,
    makers: topEntries(makers, 8),
    topModelShare: share(Math.max(0, ...Object.values(responses)), totalResponses),
    largeModelShare: share(tiers.large, totalResponses),
    smallModelShare: share(tiers.small, totalResponses),
    shellShare: toolShare(/^(Bash|exec_command|shell|local_shell|run_terminal_cmd|run_terminal_command(_v2)?)$/),
    browserShare: toolShare(/browser|playwright|chrome|puppeteer/i),
    weekendShare: share(weekendMs, workMs),
    nightShare: minuteShare([22, 23, 0, 1, 2, 3, 4]),
    morningShare: minuteShare([5, 6, 7, 8]),
    promptsPerHour: share(sum(main.map((s) => s.prompts.length)), workMs / HOUR),
    tools: topEntries(tools, 6),
    toolCalls: toolTotal,
    filesEdited: files.size,
    projects: topEntries(projects, 5),
    projectCount: Object.keys(projects).length,
    agentHours: workMs / HOUR,
    waitHours: waitMs / HOUR,
    waits,
    longWaits,
    peakParallel: peak,
    peakParallelAt: peakAt,
    daysActive: days.length,
    longestStreak: best,
    busiestDay: busiestDay && { day: busiestDay[0], hours: busiestDay[1] / 60 },
    minutesByDay,
    minutesByHour,
    rushHour: Math.max(...minutesByHour) > 0 ? rushHour : null,
    compactions: sum(sessions.map((s) => s.compactions)),
    limitHits: [...limitHits.values()].sort((a, b) => a.ts - b.ts),
    codexPeakQuotaPct: codexPeak >= 0 ? codexPeak : null,
    longestSession: longest && longest.ms > 0 && {
      title: longest.s.title, project: longest.s.project, agent: longest.s.agent, hours: longest.ms / HOUR,
    },
    biggestSession: biggest && {
      title: biggest.title, project: biggest.project, agent: biggest.agent, tokens: totalTokens(biggest.tokens),
    },
    byAgent,
  };
  stats.persona = pickPersonas(stats);
  return stats;
}

export function fmtHour(h) {
  if (h === null || h === undefined) return '-';
  const suffix = h < 12 ? 'am' : 'pm';
  const hr = h % 12 === 0 ? 12 : h % 12;
  return `${hr}${suffix}`;
}
