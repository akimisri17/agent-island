import { projectOf, readJsonl, newSession, createTurnTracker, addModel, addResponse, addTool, basename } from './lines.mjs';

const EDIT_TOOLS = new Set(['Edit', 'Write', 'MultiEdit', 'NotebookEdit']);
const NOT_HUMAN_PREFIXES = [
  '<local-command',
  '<task-notification',
  '<scheduled-task',
  '[SYSTEM',
  '<system-reminder',
  'This session is being continued from a previous conversation',
];

// Parses one Claude Code session log (~/.claude/projects/<dir>/<id>.jsonl).
// Subagent logs live under <id>/subagents/ and only contribute tokens and tools.
export async function parseClaudeFile(path, { since, isSubagent = false } = {}) {
  return buildClaudeSession(await readClaudeFile(path, { since, isSubagent }));
}

// Reads a log into a list of events, one per counted line. Resumed and forked
// sessions start a new file that copies earlier lines with the same uuid and
// message id, so the scan reads every file first and then builds sessions
// oldest first, skipping lines an earlier file already counted.
export async function readClaudeFile(path, { since, isSubagent = false } = {}) {
  const f = { path, isSubagent, title: null, cwd: null, first: null, events: [] };
  for await (const d of readJsonl(path)) {
    if (d.type === 'custom-title' && d.customTitle) f.title = d.customTitle;
    if (d.type === 'agent-name' && d.agentName && !f.title) f.title = d.agentName;
    if (!d.timestamp) continue;
    const ts = Date.parse(d.timestamp);
    if (Number.isNaN(ts)) continue;
    if (f.first === null || ts < f.first) f.first = ts;
    if (ts < since) continue;
    if (d.cwd && !f.cwd) f.cwd = d.cwd;

    const e = { uuid: d.uuid ?? null, ts, kind: null, compact: !!d.isCompactSummary, limit: null };
    if (d.quotaLimits?.status === 'rejected') e.limit = { type: d.quotaLimits.rateLimitType, resetsAt: d.quotaLimits.resetsAt };
    if (d.type === 'assistant' && d.message) {
      const m = d.message;
      e.kind = 'assistant';
      e.msgId = m.id ?? null;
      e.model = m.model;
      e.usage = m.usage ?? null;
      e.tools = [];
      for (const c of Array.isArray(m.content) ? m.content : []) {
        if (c.type === 'tool_use') e.tools.push({ name: c.name, file: c.input?.file_path || c.input?.notebook_path || null });
      }
    } else if (d.type === 'user') {
      e.kind = !isSubagent && !d.isSidechain && isHumanPrompt(d) ? 'prompt' : 'activity';
    } else if (!e.compact && !e.limit) continue;
    f.events.push(e);
  }
  return f;
}

// Builds the session record from a read file. `seen` holds line uuids and
// message ids already counted by earlier files; it is updated in place.
export function buildClaudeSession(f, seen = { uuids: new Set(), messages: new Set() }) {
  const id = basename(f.path).replace(/\.jsonl$/, '');
  const s = newSession('claude', id, f.path);
  s.isSubagent = f.isSubagent;
  s.title = f.title;
  if (f.cwd) {
    s.project = projectOf(f.cwd);
    s.cwd = f.cwd;
  }
  const turns = createTurnTracker(s);
  const usageById = new Map();
  const limitKeys = new Set();

  for (const e of f.events) {
    if (e.uuid) {
      if (seen.uuids.has(e.uuid)) continue;
      seen.uuids.add(e.uuid);
    }
    if (e.limit) {
      const key = `${e.limit.type}:${e.limit.resetsAt}`;
      if (!limitKeys.has(key)) {
        limitKeys.add(key);
        s.limitHits.push({ ts: e.ts, type: e.limit.type, resetsAt: e.limit.resetsAt * 1000 });
      }
    }
    if (e.compact) s.compactions++;
    if (e.kind === 'assistant') {
      turns.activity(e.ts);
      if (e.msgId && e.usage && !seen.messages.has(e.msgId)) usageById.set(e.msgId, { model: e.model, usage: e.usage });
      for (const t of e.tools) {
        addTool(s, t.name);
        if (EDIT_TOOLS.has(t.name) && t.file) s.filesEdited.add(t.file);
      }
    } else if (e.kind === 'prompt') turns.prompt(e.ts);
    else if (e.kind === 'activity') turns.activity(e.ts);
  }
  turns.finish();

  for (const [msgId, { model, usage }] of usageById) {
    seen.messages.add(msgId);
    s.tokens.input += usage.input_tokens || 0;
    s.tokens.cacheRead += usage.cache_read_input_tokens || 0;
    s.tokens.cacheWrite += usage.cache_creation_input_tokens || 0;
    s.tokens.output += usage.output_tokens || 0;
    addModel(s, model, usage.output_tokens || 0);
    addResponse(s, model);
  }
  return s;
}

// Builds sessions from many read files, oldest first, so a resumed or forked
// copy never counts a line twice. Ties (a fork copies the first timestamp
// too) go to the file created first, then to the path.
export function buildClaudeSessions(files) {
  const order = files
    .filter((f) => f.first !== null)
    .sort((a, b) => a.first - b.first || (a.born ?? 0) - (b.born ?? 0) || (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  const seen = { uuids: new Set(), messages: new Set() };
  return order.map((f) => buildClaudeSession(f, seen));
}

function isHumanPrompt(d) {
  if (d.toolUseResult !== undefined || d.isMeta) return false;
  const text = promptText(d.message?.content);
  // Scheduled tasks are logged as human but nobody typed them.
  if (text?.trimStart().startsWith('<scheduled-task')) return false;
  if (d.origin) return d.origin.kind === 'human';
  // Programmatic runs (Agent SDK, `claude -p` from scripts and plugins such as
  // claude-mem) are not a person typing.
  if (typeof d.entrypoint === 'string' && d.entrypoint.startsWith('sdk')) return false;
  // Older logs have no origin field: fall back to the message shape.
  if (Array.isArray(d.message?.content) && d.message.content.some((c) => c.type === 'tool_result')) return false;
  if (text === null) return false;
  return !NOT_HUMAN_PREFIXES.some((p) => text.trimStart().startsWith(p));
}

function promptText(content) {
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) return content.find((c) => c.type === 'text')?.text ?? null;
  return null;
}
