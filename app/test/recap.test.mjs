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
  assert.match(r.text, /^Today \(.+\): 1\.8 h of agent work across 2 projects, 2 commits\./);
  assert.match(r.text, /- shop \(Claude Code, Cursor, 1\.5 h\): Fix checkout race; Add test\. 2 files, 2 commits\./);
  assert.match(r.text, /Waiting on me: Migrate router \(mobile\); infra\./);
  assert.match(r.text, /Next: pick up "Migrate router"\./);
});

test('a quiet day', () => {
  const r = buildRecap({ sessions: [], now: NOW });
  assert.equal(r.projects.length, 0);
  assert.equal(r.next, null);
  assert.match(r.text, /0 min of agent work across 0 projects, 0 commits/);
});
