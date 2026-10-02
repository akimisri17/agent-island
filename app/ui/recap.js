// The daily recap: what each project got from agents today, what is
// unfinished, and a plain-text version to paste into a standup.

const MAX_TURN = 3 * 3_600_000; // same cap as agent-hours in Wrapped
const AGENTS = { claude: 'Claude Code', codex: 'Codex', cursor: 'Cursor', antigravity: 'Antigravity' };

const hours = (ms) => {
  const h = ms / 3_600_000;
  return h >= 10 ? `${Math.round(h)} h` : h >= 1 ? `${h.toFixed(1)} h` : `${Math.round(ms / 60_000)} min`;
};

export function waitedFor(ms) {
  const m = Math.floor(ms / 60_000);
  if (m < 60) return `${Math.max(1, m)} min`;
  const h = Math.floor(m / 60);
  return h < 48 ? `${h} h` : `${Math.floor(h / 24)} d`;
}

export function buildRecap({ sessions, commits = [], live = [], now = Date.now() }) {
  const mine = sessions.filter((s) => !s.isSubagent && s.prompts.length > 0);
  const byProject = new Map();
  for (const s of mine) {
    const key = s.project || 'other';
    const p = byProject.get(key) || { project: key, ms: 0, agents: new Set(), titles: [], files: new Set(), commits: [], last: 0 };
    for (const t of s.turns) p.ms += Math.min(t.end - t.start, MAX_TURN);
    p.agents.add(AGENTS[s.agent] || s.agent);
    if (s.title && !p.titles.some((x) => x.title === s.title)) p.titles.push({ title: s.title, end: s.end });
    for (const f of s.filesEdited || []) p.files.add(f);
    p.last = Math.max(p.last, s.end || 0);
    byProject.set(key, p);
  }
  for (const repo of commits) {
    const p = byProject.get(repo.project);
    if (p) p.commits.push(...repo.commits);
  }
  const projects = [...byProject.values()]
    .filter((p) => p.ms > 0 || p.commits.length)
    .sort((a, b) => b.ms - a.ms)
    .map((p) => ({
      project: p.project,
      time: hours(p.ms),
      ms: p.ms,
      agents: [...p.agents],
      titles: p.titles.sort((a, b) => b.end - a.end).slice(0, 3).map((t) => t.title),
      files: p.files.size,
      commits: p.commits.sort((a, b) => b.ts - a.ts).map((c) => c.subject),
    }));

  const waiting = live
    .filter((s) => s.state !== 'working')
    .sort((a, b) => a.since - b.since)
    .map((s) => ({ title: s.title || s.project || 'Untitled session', project: s.project, waited: waitedFor(now - s.since), approval: s.state === 'approval' }));
  const totalMs = projects.reduce((n, p) => n + p.ms, 0);
  const commitCount = projects.reduce((n, p) => n + p.commits.length, 0);
  const totals = {
    time: hours(totalMs),
    projects: projects.length,
    agents: new Set(projects.flatMap((p) => p.agents)).size,
    commits: commitCount,
  };
  const next = waiting[0] || null;

  const date = new Date(now).toLocaleDateString('en-US', { weekday: 'short', day: 'numeric', month: 'short' });
  const lines = [`Today (${date}): ${totals.time} of agent work across ${plural(totals.projects, 'project')}, ${plural(commitCount, 'commit')}.`];
  for (const p of projects) {
    const bits = [p.titles.length ? p.titles.join('; ') : 'agent work'];
    const counts = [p.files && plural(p.files, 'file'), p.commits.length && plural(p.commits.length, 'commit')].filter(Boolean).join(', ');
    if (counts) bits.push(counts);
    lines.push(`- ${p.project} (${p.agents.join(', ')}, ${p.time}): ${bits.join('. ')}.`);
    for (const c of p.commits.slice(0, 3)) lines.push(`    - ${c}`);
  }
  if (waiting.length) lines.push(`Waiting on me: ${waiting.map((w) => `${w.title}${w.project && w.project !== w.title ? ` (${w.project})` : ''}`).join('; ')}.`);
  if (next) lines.push(`Next: pick up "${next.title}".`);
  return { totals, projects, waiting, next, text: lines.join('\n') };
}

function plural(n, word) {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}
