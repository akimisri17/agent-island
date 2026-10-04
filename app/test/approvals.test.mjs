import { test } from 'node:test';
import assert from 'node:assert/strict';
import { frictionRows } from '../ui/approvals.js';

const p = (o) => ({ project: '/w/app', name: 'Applications', asked: 41, tools: [['Bash', 38], ['Edit', 3]], commands: [['npm run', 20], ['docker compose', 9], ['git commit', 4]], medianWaitMs: 360000, affirmations: 7, suggestions: ['Bash(npm run:*)', 'Bash(docker compose:*)'], ...o });

test('rows: estimate, what asks, wait, allow button when there are suggestions', () => {
  const [r] = frictionRows([p()]);
  assert.equal(r.name, 'Applications');
  assert.equal(r.right, 'about 41×');
  assert.equal(r.line, 'npm run, docker compose, git commit · 6m median wait');
  assert.equal(r.sub, '7 "go ahead" replies');
  assert.equal(r.canAllow, true);
});

test('non-Bash tools, no wait, no replies, no suggestions', () => {
  const [r] = frictionRows([p({ commands: [], tools: [['Edit', 3], ['WebFetch', 2]], medianWaitMs: null, affirmations: 0, suggestions: [], asked: 5 })]);
  assert.equal(r.line, 'Edit, WebFetch');
  assert.equal(r.sub, null);
  assert.equal(r.canAllow, false);
});

test('top three projects only, and nothing when nothing asked', () => {
  const rows = frictionRows([p(), p({ name: 'b' }), p({ name: 'c' }), p({ name: 'd' })]);
  assert.deepEqual(rows.map((r) => r.name), ['Applications', 'b', 'c']);
  assert.deepEqual(frictionRows([]), []);
  assert.equal(frictionRows([p({ affirmations: 1 })])[0].sub, '1 "go ahead" reply');
});

test('note when shell scripts are at least half of asks', () => {
  const [r] = frictionRows([p({ asked: 98, commands: [['shell script', 60], ['npm run', 10]] })]);
  assert.equal(r.note, 'Mostly multi-line shell scripts the agent wrote — no rule can allow those.');
});

test('no note when shell scripts are a minority', () => {
  const [r] = frictionRows([p({ asked: 98, commands: [['shell script', 20], ['npm run', 40]] })]);
  assert.equal(r.note, null);
  assert.equal(frictionRows([p()])[0].note, null);
});

test('noisy command groups are dropped; falls back to tools when none remain', () => {
  const [r] = frictionRows([p({ commands: [['echo "---', 5], ['npm run', 4], ['cat $(x', 3], ['ls *', 2]] })]);
  assert.equal(r.line, 'npm run · 6m median wait');
  const [r2] = frictionRows([p({ commands: [['echo "---', 5], ['a`b', 2]] })]);
  assert.equal(r2.line, 'Bash, Edit · 6m median wait');
});
