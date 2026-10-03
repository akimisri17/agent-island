import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cutoffSection } from '../ui/cutoff.js';

const NOW = Date.parse('2026-10-04T12:00:00Z');
const M = 60_000;
const c = (o) => ({ sessionId: 's1', title: 'Repo board', project: 'agent-island', cwd: '/w/agent-island', kind: 'limit', limitType: 'five_hour', at: NOW - 90 * M, resetsAt: NOW - 3 * M, ...o });

test('limit cut-offs: heading says when the limit reset', () => {
  const s = cutoffSection([c()], { now: NOW, dismissed: new Set() });
  assert.equal(s.heading, 'Cut off by the limit · reset 3m ago');
  assert.deepEqual(s.rows, [{ id: 's1', cwd: '/w/agent-island', title: 'Repo board', line: 'agent-island · stopped by the 5-hour limit' }]);
});

test('interruptions and mixed lists', () => {
  const s = cutoffSection([c({ sessionId: 's2', kind: 'interrupted', limitType: null, resetsAt: null, title: null, project: 'erp-backend' })], { now: NOW, dismissed: new Set() });
  assert.equal(s.heading, 'Stopped mid-task');
  assert.deepEqual(s.rows[0], { id: 's2', cwd: '/w/agent-island', title: 'erp-backend', line: 'erp-backend · you interrupted it' });
  const mixed = cutoffSection([c(), c({ sessionId: 's2', kind: 'interrupted', resetsAt: null })], { now: NOW, dismissed: new Set() });
  assert.equal(mixed.heading, 'Cut off by the limit · reset 3m ago');
  assert.equal(mixed.rows.length, 2);
});

test('weekly limits and unknown types read plainly', () => {
  assert.match(cutoffSection([c({ limitType: 'seven_day' })], { now: NOW, dismissed: new Set() }).rows[0].line, /weekly limit$/);
  assert.match(cutoffSection([c({ limitType: 'opus_x' })], { now: NOW, dismissed: new Set() }).rows[0].line, /stopped by a usage limit$/);
});

test('dismissed sessions are hidden; nothing left means no section', () => {
  assert.equal(cutoffSection([c()], { now: NOW, dismissed: new Set(['s1']) }), null);
  assert.equal(cutoffSection([], { now: NOW, dismissed: new Set() }), null);
});
