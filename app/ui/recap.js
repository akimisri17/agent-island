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
    .filter((p) => p.ms >= 60_000 || p.commits.length) // under a minute is noise
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
    .filter((s) => s.state === 'waiting' || s.state === 'approval')
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

  return { totals, projects, waiting, next, text: standupText(projects, next) };
}

// What gets pasted into a standup: Done / In progress / Next, in the usual
// shape. Done comes from commit messages, the one record of finished work;
// In progress from session titles. Agent names, minutes and file counts stay
// in the panel, and so does the private "waiting on me" queue. Empty
// sections are left out.
function standupText(projects, next) {
  const tidy = (t) => t.charAt(0).toUpperCase() + t.slice(1).replace(/\.$/, '');
  const done = projects.flatMap((p) => p.commits.slice(0, 5).map((c) => `- ${p.project}: ${tidy(c)}`));
  const doing = projects
    .filter((p) => p.ms >= 60_000)
    .map((p) => {
      const titles = p.titles.filter((t) => t.toLowerCase() !== p.project.toLowerCase()).map(tidy);
      return titles.length ? `- ${p.project}: ${titles.join('; ')}` : `- ${p.project}`;
    });
  const sections = [];
  if (done.length) sections.push(['Done', ...done].join('\n'));
  if (doing.length) sections.push(['In progress', ...doing].join('\n'));
  if (next) sections.push(`Next\n- ${next.project && next.project !== next.title ? `${next.project}: ` : ''}${tidy(next.title)}`);
  return sections.length ? sections.join('\n\n') : 'No agent work today.';
}

// The Repo board: one card per repository agents worked in today, from local
// git only. Commands are offered to copy, never run.
const quote = (p) => (/^[\w@%+=:,./-]+$/.test(p) ? p : `'${p.replace(/'/g, `'\\''`)}'`);
const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

export function repoCard(r) {
  const facts = [];
  if (r.changes) facts.push(plural(r.changes, 'uncommitted change'));
  if (!r.upstream && !r.detached) facts.push('no upstream');
  if (r.ahead) facts.push(`${r.ahead} to push`);
  if (r.behind) facts.push(`${r.behind} to pull`);
  if (r.stashes) facts.push(plural(r.stashes, 'stash', 'stashes'));
  if (r.worktrees.length) facts.push(plural(r.worktrees.length, 'worktree'));
  if (r.merged.length) facts.push(`${plural(r.merged.length, 'merged branch', 'merged branches')} to delete`);
  const gone = r.gone || [];
  if (gone.length) facts.push(`${plural(gone.length, 'branch', 'branches')} gone from remote`);
  const git = `git -C ${quote(r.path)}`;
  const commands = [];
  if (r.behind && !r.changes) commands.push({ label: 'Copy pull', cmd: `${git} pull --ff-only` });
  if (r.merged.length) commands.push({ label: 'Copy cleanup', cmd: `${git} branch -d ${r.merged.map(quote).join(' ')}` });
  // Squash and rebase merges leave these unmerged as far as git can tell, so
  // deleting them needs -D: the command names each branch for a last look.
  if (gone.length) commands.push({ label: 'Copy gone cleanup', cmd: `${git} branch -D ${gone.map(quote).join(' ')}` });
  return {
    name: r.project,
    branch: r.detached ? `detached at ${r.branch}` : r.branch,
    facts: facts.length ? facts : ['clean, up to date with last fetch'],
    attention: !!(r.changes || r.ahead || r.behind || r.merged.length || gone.length),
    worktrees: r.worktrees.map((w) => w.branch || w.path),
    commands,
  };
}
