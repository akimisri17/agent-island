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
    behind: r.behind,
    oldBranches,
  };
}

export function sortRepos(rows) {
  return [...rows].sort((a, b) => DOT_ORDER[a.dot] - DOT_ORDER[b.dot] || a.name.localeCompare(b.name));
}

// What the confirm sheets say. The command is shown exactly as it will run.
export function pullSheet(row) {
  return {
    title: `Pull ${plural(row.behind, 'commit')} into ${row.branch}?`,
    command: 'git pull --ff-only',
    note: 'Only fast-forwards. Stops if it would need a merge.',
    ok: 'Pull',
  };
}

export function branchSheet(row) {
  const n = row.oldBranches.length;
  const anyGone = row.oldBranches.some((b) => b.reason === 'gone');
  return {
    title: `${plural(n, 'old branch', 'old branches')} in ${row.name}`,
    note: 'Their work is already in the main branch, or their pull request was merged and the branch was deleted on the remote. Only your local copies are deleted.',
    items: row.oldBranches.map((b) => ({ name: b.name, why: b.reason === 'merged' ? 'merged' : 'gone from remote' })),
    warning: anyGone
      ? 'Gone branches are force-deleted. If you committed to one after its pull request was merged, those commits are deleted too.'
      : null,
    ok: `Delete ${n}`,
  };
}

// The line a row shows after an action, until the next refresh.
export function resultLine(kind, value) {
  if (kind === 'pull') return { text: `Pulled ${plural(value, 'commit')}`, ok: true };
  if (kind === 'delete') {
    return value.length
      ? { text: `Deleted ${plural(value.length, 'old branch', 'old branches')}`, ok: true }
      : { text: 'Nothing deleted: the list changed. Refresh and try again.', ok: false };
  }
  return { text: String(value), ok: false };
}
