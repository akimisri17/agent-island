import { test } from 'node:test';
import assert from 'node:assert/strict';
import { repoRow, sortRepos } from '../ui/repos.js';

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
