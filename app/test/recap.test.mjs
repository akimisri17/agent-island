import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildRecap } from '../ui/recap.js';

const H = 3_600_000;
const NOW = Date.parse('2026-10-02T18:00:00Z');
const session = (o) => ({ agent: 'claude', isSubagent: false, prompts: [1], turns: [], filesEdited: [], title: null, end: NOW, ...o });

test('groups today by project, adds commits, names what is waiting', () => {
  const r = buildRecap({
    now: NOW,
    sessions: [
      session({ project: 'shop', title: 'Fix checkout race', turns: [{ start: 0, end: H }], filesEdited: ['a.ts', 'b.ts'] }),
      session({ project: 'shop', agent: 'cursor', title: 'Add test', turns: [{ start: 0, end: H / 2 }], filesEdited: ['b.ts'], end: NOW - H }),
      session({ project: 'api', turns: [{ start: 0, end: H / 4 }] }),
      session({ project: 'bot', prompts: [], turns: [{ start: 0, end: H }] }), // automated: left out
      session({ project: 'bot', isSubagent: true, turns: [{ start: 0, end: H }] }),
    ],
    commits: [{ project: 'shop', commits: [{ subject: 'Fix race in cart lock', ts: 2 }, { subject: 'Add test', ts: 1 }] }, { project: 'elsewhere', commits: [{ subject: 'x', ts: 1 }] }],
    live: [
      { state: 'waiting', title: 'Migrate router', project: 'mobile', since: NOW - 2 * H },
      { state: 'working', title: 'Busy', project: 'x', since: NOW },
      { state: 'approval', title: null, project: 'infra', since: NOW - H },
    ],
  });
  assert.deepEqual(r.projects.map((p) => p.project), ['shop', 'api']);
  const shop = r.projects[0];
  assert.equal(shop.time, '1.5 h');
  assert.deepEqual(shop.agents, ['Claude Code', 'Cursor']);
  assert.deepEqual(shop.titles, ['Fix checkout race', 'Add test']);
  assert.equal(shop.files, 2);
  assert.deepEqual(shop.commits, ['Fix race in cart lock', 'Add test']);
  assert.equal(r.projects[1].time, '15 min');
  assert.deepEqual(r.totals, { time: '1.8 h', projects: 2, agents: 2, commits: 2 });
  assert.deepEqual(r.waiting.map((w) => w.title), ['Migrate router', 'infra']);
  assert.equal(r.next.title, 'Migrate router');
  // Done / In progress / Next: no agent names, minutes, file counts, or the
  // private waiting queue in the pasted text.
  assert.equal(r.text, [
    'Done',
    '- shop: Fix race in cart lock',
    '- shop: Add test',
    '',
    'In progress',
    '- shop: Fix checkout race; Add test',
    '- api',
    '',
    'Next',
    '- mobile: Migrate router',
  ].join('\n'));
  assert.doesNotMatch(r.text, /Claude Code|Cursor|min|files?\b|Waiting/);
});

test('a quiet day', () => {
  const r = buildRecap({ sessions: [], now: NOW });
  assert.equal(r.projects.length, 0);
  assert.equal(r.next, null);
  assert.equal(r.text, 'No agent work today.');
});

test('idle sessions are not "waiting", and sub-minute projects are left out', () => {
  const r = buildRecap({
    now: NOW,
    sessions: [
      session({ project: 'real', turns: [{ start: 0, end: 5 * 60_000 }] }),
      session({ project: 'blip', turns: [{ start: 0, end: 20_000 }] }),
    ],
    live: [
      { state: 'idle', title: 'Stuck', since: NOW - 17 * H },
      { state: 'waiting', title: 'Done', since: NOW - H },
    ],
  });
  assert.deepEqual(r.projects.map((p) => p.project), ['real']);
  assert.deepEqual(r.waiting.map((w) => w.title), ['Done']);
});

test('standup text: sections without content are left out', () => {
  const H2 = 3_600_000;
  const r = buildRecap({ now: NOW, sessions: [session({ project: 'site', title: 'site', turns: [{ start: 0, end: H2 }] })] });
  assert.equal(r.text, 'In progress\n- site'); // no Done (no commits), no Next (nothing waiting); title equal to project dropped
});

test('repo card: facts, and commands to copy only when they are safe', async () => {
  const { repoCard } = await import('../ui/recap.js');
  const base = { project: 'shop', path: '/w/my shop', branch: 'main', detached: false, upstream: 'origin/main', ahead: 0, behind: 0, changes: 0, stashes: 0, worktrees: [], merged: [] };
  assert.deepEqual(repoCard(base).facts, ['clean, up to date with last fetch']);
  assert.equal(repoCard(base).attention, false);
  const c = repoCard({ ...base, behind: 2, stashes: 1, merged: ['feat/a', 'fix/b'], worktrees: [{ path: '/w/x', branch: 'feat/c' }] });
  assert.deepEqual(c.facts, ['2 to pull', '1 stash', '1 worktree', '2 merged branches to delete']);
  assert.deepEqual(c.commands.map((x) => x.cmd), ["git -C '/w/my shop' pull --ff-only", "git -C '/w/my shop' branch -d feat/a fix/b"]);
  assert.deepEqual(c.worktrees, ['feat/c']);
  // Never suggest a pull over uncommitted work.
  assert.deepEqual(repoCard({ ...base, behind: 1, changes: 3 }).commands, []);
  assert.equal(repoCard({ ...base, branch: 'abc1234', detached: true, upstream: null }).branch, 'detached at abc1234');
});
