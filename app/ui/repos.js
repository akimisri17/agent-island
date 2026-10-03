// The Repo board: one row per repository agents worked in today, from local
// git only. The status line says what you would do, in plain words; the dot
// says whether it needs you.

const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
const DOT_ORDER = { needs: 0, none: 1, done: 2 };

export function repoRow(r) {
  const unpushed = !r.upstream && !r.detached;
  const oldBranches = [
    ...(r.merged || []).map((name) => ({ name, reason: 'merged' })),
    ...(r.gone || []).map((name) => ({ name, reason: 'gone' })),
  ];
  const parts = [];
  if (r.changes) parts.push(`${r.changes} uncommitted`);
  if (unpushed) parts.push('not pushed yet');
  if (r.ahead) parts.push(`${r.ahead} to push`);
  if (r.behind) parts.push(plural(r.behind, 'commit') + ' to pull');
  if (r.stashes) parts.push(plural(r.stashes, 'stash', 'stashes'));
  if (r.worktrees.length) {
    const names = r.worktrees.map((w) => w.branch || w.path);
    parts.push(r.worktrees.length === 1 ? `1 worktree · ${names[0]}` : plural(r.worktrees.length, 'worktree'));
  }
  if (oldBranches.length) parts.push(plural(oldBranches.length, 'old branch', 'old branches'));

  const needs = !!(r.changes || r.ahead || r.behind || unpushed);
  return {
    name: r.project,
    path: r.path,
    branch: r.detached ? `detached at ${r.branch}` : r.branch,
    status: parts.length ? parts.join(' · ') : 'Clean',
    dot: needs ? 'needs' : parts.length ? 'none' : 'done',
    canPull: r.behind > 0 && !r.changes,
    oldBranches,
  };
}

export function sortRepos(rows) {
  return [...rows].sort((a, b) => DOT_ORDER[a.dot] - DOT_ORDER[b.dot] || a.name.localeCompare(b.name));
}
