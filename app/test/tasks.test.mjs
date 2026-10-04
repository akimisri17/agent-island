import { test } from 'node:test';
import assert from 'node:assert/strict';
import { taskView, tasksHaveProblems } from '../ui/tasks.js';

const H = 3_600_000;
const NOW = Date.parse('2026-10-04T12:00:00');
const last = (o) => ({ sessionId: 's', cwd: '/w', start: NOW - 4 * H, end: NOW - 4 * H + 3 * 60_000, outcome: 'done', ...o });
const row = (o) => ({ name: 'eod-digest', description: 'Daily digest', state: 'ok', last: last(), cadenceMs: 24 * H, days: ['ran', 'ran', 'ran', 'ran', 'ran', 'ran', 'ran'], ...o });

test('a task that ran: time and duration, daily, done dot', () => {
  const v = taskView(row(), NOW);
  assert.equal(v.dot, 'done');
  assert.match(v.right, / · 3m$/);
  assert.equal(v.line, 'Daily · last ran today');
  assert.deepEqual(v.actions, { runNow: false, lastRun: true });
  assert.deepEqual(v.days, ['ran', 'ran', 'ran', 'ran', 'ran', 'ran', 'ran']);
});

test('problems: missed, failed, stopped get a red dot and Run now', () => {
  for (const [state, right] of [['missed', 'Missed'], ['failed', 'Failed'], ['stopped', 'Stopped']]) {
    const v = taskView(row({ state, last: last({ start: NOW - 30 * H }) }), NOW);
    assert.equal(v.dot, 'failed');
    assert.equal(v.right, right);
    assert.equal(v.line, 'Daily · last ran yesterday');
    assert.equal(v.actions.runNow, true);
  }
  assert.equal(taskView(row({ state: 'stopped' }), NOW).note, 'It may be waiting for an approval in Claude.');
});

test('running, never ran, weekly and irregular', () => {
  assert.deepEqual(
    (({ dot, right }) => ({ dot, right }))(taskView(row({ state: 'running', last: last({ start: NOW - 4 * 60_000, outcome: 'running' }) }), NOW)),
    { dot: 'needs', right: 'Running · 4m' },
  );
  const never = taskView(row({ state: 'never', last: null, cadenceMs: null, days: Array(7).fill('none') }), NOW);
  assert.deepEqual([never.dot, never.right, never.line], ['', 'Never ran', 'Daily digest']);
  assert.deepEqual(never.actions, { runNow: true, lastRun: false });
  assert.equal(taskView(row({ cadenceMs: 7 * 24 * H, last: last({ start: NOW - 3 * 24 * H }) }), NOW).line, 'Weekly · last ran 3 days ago');
  assert.equal(taskView(row({ cadenceMs: 6 * H }), NOW).line, 'Every 6 h · last ran today');
  assert.equal(taskView(row({ cadenceMs: null }), NOW).line, 'Last ran today');
});

test('tab dot when any task has a problem', () => {
  assert.equal(tasksHaveProblems([row(), row({ state: 'running' })]), false);
  assert.equal(tasksHaveProblems([row(), row({ state: 'missed' })]), true);
});
