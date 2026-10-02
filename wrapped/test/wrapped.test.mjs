import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { parseClaudeFile } from '../src/claude.mjs';
import { parseCodexFile } from '../src/codex.mjs';
import { computeStats } from '../src/stats.mjs';
import { renderHtml } from '../src/render.mjs';
import { basename } from '../src/lines.mjs';

const fx = (name) => fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url));
const since = Date.parse('2026-09-01T00:00:00Z');
const until = Date.parse('2026-10-01T00:00:00Z');

test('claude: prompts, tokens deduped by message id, edits, limits, compaction', async () => {
  const s = await parseClaudeFile(fx('claude-session.jsonl'), { since });
  assert.equal(s.title, 'Fix login bug');
  assert.equal(s.project, 'shop');
  // tool results and task notifications are not human prompts
  assert.equal(s.prompts.length, 2);
  assert.deepEqual(s.tokens, { input: 15, cacheRead: 3000, cacheWrite: 100, output: 130 });
  assert.deepEqual(s.models, { 'claude-sonnet-5': 50, 'claude-opus-5': 80 });
  assert.deepEqual([...s.filesEdited].sort(), ['/work/shop/login.test.ts', '/work/shop/login.ts']);
  assert.equal(s.limitHits.length, 1);
  assert.equal(s.compactions, 1);
  assert.equal(s.turns.length, 2);
  assert.equal(s.turns[0].end - s.turns[0].start, 3 * 60_000);
});

test('claude: entries before the window are ignored', async () => {
  const s = await parseClaudeFile(fx('claude-session.jsonl'), { since: Date.parse('2026-09-10T10:10:00Z') });
  assert.equal(s.prompts.length, 1);
});

test('codex: cumulative tokens, patch files, quota peak, limit hit', async () => {
  const s = await parseCodexFile(fx('codex-rollout.jsonl'), { since });
  assert.equal(s.id, 'c1');
  assert.equal(s.project, 'api');
  assert.equal(s.prompts.length, 2);
  assert.deepEqual(s.tokens, { input: 1500, cacheRead: 4500, cacheWrite: 0, output: 350 });
  assert.deepEqual(s.models, { 'gpt-5.5': 350 });
  assert.deepEqual([...s.filesEdited].sort(), ['/work/api/list.ts', '/work/api/page.ts']);
  assert.equal(s.peakQuotaPct, 100);
  assert.equal(s.limitHits.length, 1);
  assert.equal(s.tools.apply_patch, 1);
});

test('stats: wait time, parallel peak, automated sessions excluded from "yours"', async () => {
  const a = await parseClaudeFile(fx('claude-session.jsonl'), { since });
  const b = await parseCodexFile(fx('codex-rollout.jsonl'), { since });
  const automated = { ...(await parseClaudeFile(fx('claude-session.jsonl'), { since })), prompts: [], turns: [], id: 'auto' };
  const st = computeStats([a, b, automated], { since, until });
  assert.equal(st.sessions, 2);
  assert.equal(st.automatedRuns, 1);
  // claude: 10:03 -> 10:13 wait; codex: 09:05 -> 09:07 wait
  assert.equal(Math.round(st.waitHours * 60), 12);
  assert.equal(st.waits, 2);
  assert.equal(st.longWaits, 1);
  assert.equal(st.peakParallel, 1);
  assert.equal(st.limitHits.length, 2);
  assert.ok(st.persona.name);
});

test('render: escapes project names and titles', async () => {
  const s = await parseClaudeFile(fx('claude-session.jsonl'), { since });
  s.project = '<script>alert(1)</script>';
  s.title = '"><img src=x>';
  const html = renderHtml(computeStats([s], { since, until }), { files: { claude: 1, codex: 0 }, bytes: 1, seconds: 0.1 });
  assert.ok(!html.includes('<script>alert(1)'));
  assert.ok(!html.includes('"><img src=x>'));
});

test('basename handles Windows and POSIX paths', () => {
  assert.equal(basename('C:\\work\\shop'), 'shop');
  assert.equal(basename('/work/shop/'), 'shop');
});

test('cursor: chats, turns, responses by model, tools, edits, subagents', async () => {
  const { parseCursorDb } = await import('../src/cursor.mjs');
  const sessions = await parseCursorDb(fx('cursor-state.vscdb'), { since });
  assert.deepEqual(sessions.map((s) => s.id).sort(), ['k1', 'sub']);
  const k = sessions.find((s) => s.id === 'k1');
  assert.equal(k.agent, 'cursor');
  assert.equal(k.title, 'Catalog upload');
  assert.equal(k.project, 'catalog');
  assert.equal(k.prompts.length, 2);
  assert.deepEqual(k.turns.map((t) => (t.end - t.start) / 60_000), [5, 2]);
  assert.deepEqual(k.responses, { 'grok-4.7': 3 });
  assert.deepEqual(k.tools, { edit_file_v2: 1, run_terminal_command_v2: 1 });
  assert.deepEqual([...k.filesEdited], ['/work/catalog/upload.ts']);
  assert.equal(k.tokens.output, 0);
  const sub = sessions.find((s) => s.id === 'sub');
  assert.equal(sub.isSubagent, true);
  assert.equal(sub.prompts.length, 0);
  assert.deepEqual(sub.responses, { 'cursor-auto': 1 });
});

test('models: maker and tier from the name', async () => {
  const { makerOf, tierOf } = await import('../src/models.mjs');
  assert.equal(makerOf('claude-opus-5'), 'Anthropic');
  assert.equal(makerOf('gpt-5.5'), 'OpenAI');
  assert.equal(makerOf('openrouter/x-ai/grok-4'), 'xAI');
  assert.equal(makerOf('qwen3-coder-plus'), 'Alibaba');
  assert.equal(makerOf('cursor-auto'), 'Cursor');
  assert.equal(makerOf('mystery-model'), 'Other');
  assert.equal(tierOf('claude-haiku-4-5'), 'small');
  assert.equal(tierOf('claude-opus-5'), 'large');
  assert.equal(tierOf('claude-sonnet-5'), 'mid');
});

test('personas: strongest is the title, next two are badges', async () => {
  const { pickPersonas } = await import('../src/personas.mjs');
  const base = {
    sessions: 10, agentHours: 50, waitHours: 5, peakParallel: 1, limitHits: [], subagentShare: 0,
    nightShare: 0, morningShare: 0, weekendShare: 0, longestSession: { hours: 1 }, largeModelShare: 0,
    smallModelShare: 0, topModelShare: 0.5, responses: 100, makers: [['Anthropic', 100]], shellShare: 0,
    browserShare: 0, promptsPerHour: 6, compactions: 0, longestStreak: 3, projectCount: 2, filesEdited: 9,
  };
  assert.equal(pickPersonas(base).name, 'The Builder');
  assert.equal(pickPersonas({ ...base, sessions: 0 }).name, 'The Newcomer');
  const p = pickPersonas({ ...base, peakParallel: 9, limitHits: Array(12).fill({}), longestStreak: 22, projectCount: 40 });
  assert.equal(p.name, 'The Conductor'); // 9/3 = 3, capped
  // Limit Tester 12/6 = 2, Explorer 40/25 = 1.6, Streaker 22/21 = 1.05 misses the cut
  assert.deepEqual(p.badges.map((b) => b.name), ['The Limit Tester', 'The Explorer']);
  const poly = pickPersonas({ ...base, makers: [['Anthropic', 50], ['xAI', 30], ['OpenAI', 20]] });
  assert.equal(poly.name, 'The Polyglot');
  assert.match(poly.line, /Anthropic, xAI, OpenAI/);
});

test('filters: by agent, by main model maker, and the options offered', async () => {
  const { filterSessions, filterOptions, primaryMaker } = await import('../src/stats.mjs');
  const a = await parseClaudeFile(fx('claude-session.jsonl'), { since });
  const b = await parseCodexFile(fx('codex-rollout.jsonl'), { since });
  const { parseCursorDb } = await import('../src/cursor.mjs');
  const c = await parseCursorDb(fx('cursor-state.vscdb'), { since });
  const all = [a, b, ...c];
  assert.equal(primaryMaker(a), 'Anthropic');
  assert.equal(primaryMaker(b), 'OpenAI');
  assert.equal(primaryMaker(c.find((s) => s.id === 'k1')), 'xAI');
  assert.deepEqual(filterSessions(all, { agent: 'codex' }).map((s) => s.id), ['c1']);
  assert.deepEqual(filterSessions(all, { maker: 'xAI' }).map((s) => s.id), ['k1']);
  assert.deepEqual(filterSessions(all, { agent: 'cursor', maker: 'Cursor' }).map((s) => s.id), ['sub']);
  assert.equal(filterSessions(all, {}).length, all.length);
  const opts = filterOptions(all);
  assert.deepEqual([...opts.agents].sort(), ['claude', 'codex', 'cursor']);
  assert.deepEqual([...opts.makers].sort(), ['Anthropic', 'Cursor', 'OpenAI', 'xAI']);
});

test('projectOf: worktrees count toward their repository', async () => {
  const { projectOf } = await import('../src/lines.mjs');
  assert.equal(projectOf('/u/me/shop'), 'shop');
  assert.equal(projectOf('/u/me/shop/.worktrees/WO-20260927-005'), 'shop');
  assert.equal(projectOf('/u/me/erp/worktrees/dashboards'), 'erp');
  assert.equal(projectOf('/u/me/site/.claude/worktrees/fix-nav'), 'site');
  assert.equal(projectOf('C:\\work\\api\\.worktrees\\b'), 'api');
  assert.equal(projectOf(null), null);
});

test('antigravity: prompt, turn, tools, edits, models from protobuf blobs', async () => {
  const { parseAntigravityDb } = await import('../src/antigravity.mjs');
  const s = await parseAntigravityDb(fx('antigravity-conv.db'), { since });
  assert.equal(s.agent, 'antigravity');
  assert.equal(s.id, 'antigravity-conv');
  assert.equal(s.project, 'forge'); // worktree counts toward its repo
  assert.equal(s.prompts.length, 1);
  assert.equal(s.turns.length, 1);
  assert.equal((s.turns[0].end - s.turns[0].start) / 1000, 120);
  assert.deepEqual(s.tools, { run_command: 1, write_to_file: 1 });
  assert.deepEqual([...s.filesEdited], ['/work/forge/notes.md']);
  assert.deepEqual(s.responses, { 'gemini-pro-default': 2, 'gemini-3.8-flash': 1 });
  assert.equal(s.tokens.output, 0);
});

test('foreign SQLite: reading a WAL database creates no files next to it', async () => {
  const { mkdtempSync, copyFileSync, readdirSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const { DatabaseSync } = await import('node:sqlite');
  const { parseCursorDb } = await import('../src/cursor.mjs');
  const dir = mkdtempSync(join(tmpdir(), 'agent-wrapped-'));
  const db = join(dir, 'state.vscdb');
  copyFileSync(fx('cursor-state.vscdb'), db);
  const w = new DatabaseSync(db);
  w.exec('PRAGMA journal_mode=WAL');
  w.close(); // the owning app has quit: no -wal on disk
  assert.deepEqual(readdirSync(dir), ['state.vscdb']);
  const sessions = await parseCursorDb(db, { since });
  assert.equal(sessions.length, 2);
  assert.deepEqual(readdirSync(dir), ['state.vscdb']);
});
