import { test } from 'node:test';
import assert from 'node:assert/strict';
import { repoRow, sortRepos, pullSheet, branchSheet, resultLine } from '../ui/repos.js';

const base = {
  project: 'shop', path: '/w/shop', branch: 'main', detached: false, upstream: 'origin/main',
  ahead: 0, behind: 0, changes: 0, stashes: 0, worktrees: [], defaultBranch: 'main', merged: [], gone: [],
};

test('repo row: clean', () => {
  const r = repoRow(base);
  assert.equal(r.status, 'Clean');
  assert.equal(r.dot, 'done');
  assert.equal(r.canPull, false);
  assert.deepEqual(r.oldBranches, []);
});

test('repo row: plain words, needs-you dot, pull only without local changes', () => {
  const r = repoRow({ ...base, behind: 26 });
  assert.equal(r.status, '26 commits to pull');
  assert.equal(r.dot, 'needs');
  assert.equal(r.canPull, true);

  const busy = repoRow({ ...base, changes: 30, upstream: null, branch: 'docs/x' });
  assert.equal(busy.status, '30 uncommitted · not pushed yet');
  assert.equal(busy.dot, 'needs');
  assert.equal(repoRow({ ...base, behind: 1, changes: 1 }).canPull, false);
});

test('repo row: merged and gone branches are one "old branches" count', () => {
  const r = repoRow({ ...base, ahead: 2, stashes: 1, merged: ['feat/old-cart'], gone: ['feat/squashed'] });
  assert.equal(r.status, '2 to push · 1 stash · 2 old branches');
  assert.deepEqual(r.oldBranches, [
    { name: 'feat/old-cart', reason: 'merged' },
    { name: 'feat/squashed', reason: 'gone' },
  ]);
});

test('repo row: only housekeeping left is a quiet dot', () => {
  const r = repoRow({ ...base, worktrees: [{ path: '/w/shop/.worktrees/x', branch: 'hotfix/tax' }] });
  assert.equal(r.status, '1 worktree · hotfix/tax');
  assert.equal(r.dot, 'none');
});

test('repo row: detached HEAD is named, not flagged as unpushed', () => {
  const r = repoRow({ ...base, branch: 'abc1234', detached: true, upstream: null });
  assert.equal(r.branch, 'detached at abc1234');
  assert.equal(r.status, 'Clean');
});

test('sortRepos: needs you first, then quiet, then clean; by name within', () => {
  const rows = [
    repoRow({ ...base, project: 'b' }),
    repoRow({ ...base, project: 'z', changes: 1 }),
    repoRow({ ...base, project: 'a', stashes: 1 }),
    repoRow({ ...base, project: 'c', behind: 2 }),
  ];
  assert.deepEqual(sortRepos(rows).map((r) => r.name), ['c', 'z', 'a', 'b']);
});

test('pull sheet: exact command, plain title', () => {
  const s = pullSheet(repoRow({ ...base, project: 'crm', branch: 'dev', behind: 26 }));
  assert.deepEqual(s, {
    title: 'Pull 26 commits into dev?',
    command: 'git pull --ff-only',
    note: 'Only fast-forwards. Stops if it would need a merge.',
    ok: 'Pull',
  });
  assert.equal(pullSheet(repoRow({ ...base, behind: 1 })).title, 'Pull 1 commit into main?');
});

test('branch sheet: why each branch is old, one delete button', () => {
  const s = branchSheet(repoRow({ ...base, project: 'shop', merged: ['feat/a'], gone: ['fix/b', 'fix/c'] }));
  assert.equal(s.title, '3 old branches in shop');
  assert.match(s.note, /Only your local copies are deleted/);
  assert.deepEqual(s.items, [
    { name: 'feat/a', why: 'merged' },
    { name: 'fix/b', why: 'gone from remote' },
    { name: 'fix/c', why: 'gone from remote' },
  ]);
  assert.equal(s.ok, 'Delete 3');
  assert.equal(s.warning, 'Gone branches are force-deleted. If you committed to one after its pull request was merged, those commits are deleted too.');
});

test('branch sheet: no warning when only merged branches', () => {
  const s = branchSheet(repoRow({ ...base, merged: ['feat/a', 'feat/b'] }));
  assert.equal(s.warning, null);
});

test('result lines after an action', () => {
  assert.deepEqual(resultLine('pull', 26), { text: 'Pulled 26 commits', ok: true });
  assert.deepEqual(resultLine('pull', 1), { text: 'Pulled 1 commit', ok: true });
  assert.deepEqual(resultLine('delete', ['a', 'b']), { text: 'Deleted 2 old branches', ok: true });
  assert.deepEqual(resultLine('delete', []), { text: 'Nothing deleted: the list changed. Refresh and try again.', ok: false });
  assert.deepEqual(resultLine('error', 'Not possible to fast-forward, aborting.'), { text: 'Not possible to fast-forward, aborting.', ok: false });
});

test('rows carry the behind count for the pull sheet', () => {
  assert.equal(repoRow({ ...base, behind: 4 }).behind, 4);
});
