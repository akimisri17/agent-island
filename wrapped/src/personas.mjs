// Personas for the report title and share card.
//
// Each persona scores how far past its threshold the data is (score >= 1 means
// it applies). The strongest is the title; the next two are badges. Scores are
// capped so one huge number cannot hide everything else, and ties keep list
// order, which puts rarer personas first.
//
// Thresholds are set so a score of 2 reads as exceptional. They are guesses
// until shared cards show the real spread; recalibrate them then.

const CAP = 3;
const pct = (x) => `${Math.round(x * 100)}%`;
const minHours = (st, n) => st.agentHours >= n;

export const PERSONAS = [
  {
    name: 'The Conductor',
    score: (st) => st.peakParallel / 3,
    line: (st) => `Ran ${st.peakParallel} agents at once.`,
  },
  {
    name: 'The Polyglot',
    score: (st) => st.makers.filter(([, n]) => n / st.responses >= 0.05).length / 3,
    line: (st) => `Worked with ${st.makers.filter(([, n]) => n / st.responses >= 0.05).map(([m]) => m).join(', ')} models.`,
  },
  {
    name: 'The Absent Boss',
    score: (st) => (minHours(st, 5) ? st.waitHours / st.agentHours / 0.4 : 0),
    line: (st) => `Agents waited ${Math.round(st.waitHours)} hours for you.`,
  },
  {
    name: 'The Limit Tester',
    score: (st) => st.limitHits.length / 6,
    line: (st) => `Hit the usage wall ${st.limitHits.length} times.`,
  },
  {
    name: 'The Delegator',
    score: (st) => st.subagentShare / 0.4,
    line: (st) => `${pct(st.subagentShare)} of tokens went to subagents.`,
  },
  {
    name: 'The Night Shift',
    score: (st) => (minHours(st, 5) ? st.nightShare / 0.3 : 0),
    line: (st) => `${pct(st.nightShare)} of agent work happened after 10pm.`,
  },
  {
    name: 'The Early Bird',
    score: (st) => (minHours(st, 5) ? st.morningShare / 0.3 : 0),
    line: (st) => `${pct(st.morningShare)} of agent work happened before 9am.`,
  },
  {
    name: 'The Weekend Warrior',
    score: (st) => (minHours(st, 5) ? st.weekendShare / 0.35 : 0),
    line: (st) => `${pct(st.weekendShare)} of agent-hours were on weekends.`,
  },
  {
    name: 'The Marathoner',
    score: (st) => (st.longestSession ? st.longestSession.hours / 8 : 0),
    line: (st) => `One session ran ${st.longestSession.hours.toFixed(1)} agent-hours.`,
  },
  {
    name: 'The Big Spender',
    score: (st) => st.largeModelShare / 0.6,
    line: (st) => `${pct(st.largeModelShare)} of responses came from top-tier models.`,
  },
  {
    name: 'The Penny Pincher',
    score: (st) => st.smallModelShare / 0.5,
    line: (st) => `${pct(st.smallModelShare)} of responses came from small, fast models.`,
  },
  {
    name: 'The Loyalist',
    score: (st) => (st.responses >= 200 ? st.topModelShare / 0.95 : 0),
    line: (st) => `${pct(st.topModelShare)} of responses from one model.`,
  },
  {
    name: 'The Shell Jockey',
    score: (st) => st.shellShare / 0.6,
    line: (st) => `${pct(st.shellShare)} of tool calls were shell commands.`,
  },
  {
    name: 'The Browser Pilot',
    score: (st) => st.browserShare / 0.2,
    line: (st) => `${pct(st.browserShare)} of tool calls drove a browser.`,
  },
  {
    name: 'The Micromanager',
    score: (st) => (minHours(st, 5) ? st.promptsPerHour / 12 : 0),
    line: (st) => `${st.promptsPerHour.toFixed(0)} prompts per agent-hour.`,
  },
  {
    name: 'The Hands-Off',
    score: (st) => (minHours(st, 5) && st.promptsPerHour > 0 ? 2 / st.promptsPerHour : 0),
    line: (st) => `One prompt every ${Math.round(60 / st.promptsPerHour)} agent-minutes.`,
  },
  {
    name: 'The Context Hoarder',
    score: (st) => (st.sessions >= 5 ? st.compactions / st.sessions / 1.5 : 0),
    line: (st) => `${st.compactions} context compactions across ${st.sessions} sessions.`,
  },
  {
    name: 'The Streaker',
    score: (st) => st.longestStreak / 21,
    line: (st) => `${st.longestStreak} days in a row with agents at work.`,
  },
  {
    name: 'The Explorer',
    score: (st) => st.projectCount / 25,
    line: (st) => `Agents worked across ${st.projectCount} projects.`,
  },
];

export function pickPersonas(st) {
  if (st.sessions === 0) {
    return { name: 'The Newcomer', line: 'No agent sessions in this window yet.', badges: [] };
  }
  const scored = PERSONAS
    .map((p, i) => ({ p, i, score: Math.min(CAP, Number(p.score(st)) || 0) }))
    .filter((x) => x.score >= 1)
    .sort((a, b) => b.score - a.score || a.i - b.i);
  if (!scored.length) {
    return { name: 'The Builder', line: `${st.filesEdited} files touched across ${st.projectCount} projects.`, badges: [] };
  }
  const [top, ...rest] = scored;
  return {
    name: top.p.name,
    line: top.p.line(st),
    badges: rest.slice(0, 2).map((x) => ({ name: x.p.name, line: x.p.line(st) })),
  };
}
