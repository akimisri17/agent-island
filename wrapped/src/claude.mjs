import { projectOf, readJsonl, newSession, createTurnTracker, addModel, addResponse, addTool, basename } from './lines.mjs';

const EDIT_TOOLS = new Set(['Edit', 'Write', 'MultiEdit', 'NotebookEdit']);
const NOT_HUMAN_PREFIXES = ['<local-command', '<task-notification', '[SYSTEM', '<system-reminder', 'This session is being continued from a previous conversation'];

// Parses one Claude Code session log (~/.claude/projects/<dir>/<id>.jsonl).
// Subagent logs live under <id>/subagents/ and only contribute tokens and tools.
export async function parseClaudeFile(path, { since, isSubagent = false } = {}) {
  const id = basename(path).replace(/\.jsonl$/, '');
  const s = newSession('claude', id, path);
  s.isSubagent = isSubagent;
  const turns = createTurnTracker(s);
  const usageById = new Map();
  const limitKeys = new Set();

  for await (const d of readJsonl(path)) {
    if (d.type === 'custom-title' && d.customTitle) s.title = d.customTitle;
    if (d.type === 'agent-name' && d.agentName && !s.title) s.title = d.agentName;
    if (!d.timestamp) continue;
    const ts = Date.parse(d.timestamp);
    if (Number.isNaN(ts) || ts < since) continue;
    if (d.cwd && !s.project) {
      s.project = projectOf(d.cwd);
      s.cwd = d.cwd;
    }

    if (d.quotaLimits?.status === 'rejected') {
      const key = `${d.quotaLimits.rateLimitType}:${d.quotaLimits.resetsAt}`;
      if (!limitKeys.has(key)) {
        limitKeys.add(key);
        s.limitHits.push({ ts, type: d.quotaLimits.rateLimitType, resetsAt: d.quotaLimits.resetsAt * 1000 });
      }
    }
    if (d.isCompactSummary) s.compactions++;

    if (d.type === 'assistant' && d.message) {
      const m = d.message;
      turns.activity(ts);
      if (m.id && m.usage) usageById.set(m.id, { model: m.model, usage: m.usage });
      for (const c of Array.isArray(m.content) ? m.content : []) {
        if (c.type !== 'tool_use') continue;
        addTool(s, c.name);
        const file = c.input?.file_path || c.input?.notebook_path;
        if (EDIT_TOOLS.has(c.name) && file) s.filesEdited.add(file);
      }
    } else if (d.type === 'user') {
      if (!isSubagent && !d.isSidechain && isHumanPrompt(d)) turns.prompt(ts);
      else turns.activity(ts);
    }
  }
  turns.finish();

  for (const { model, usage } of usageById.values()) {
    s.tokens.input += usage.input_tokens || 0;
    s.tokens.cacheRead += usage.cache_read_input_tokens || 0;
    s.tokens.cacheWrite += usage.cache_creation_input_tokens || 0;
    s.tokens.output += usage.output_tokens || 0;
    addModel(s, model, usage.output_tokens || 0);
    addResponse(s, model);
  }
  return s;
}

function isHumanPrompt(d) {
  if (d.toolUseResult !== undefined || d.isMeta) return false;
  if (d.origin) return d.origin.kind === 'human';
  // Programmatic runs (Agent SDK, `claude -p` from scripts and plugins such as
  // claude-mem) are not a person typing.
  if (typeof d.entrypoint === 'string' && d.entrypoint.startsWith('sdk')) return false;
  // Older logs have no origin field: fall back to the message shape.
  const content = d.message?.content;
  let text = null;
  if (typeof content === 'string') text = content;
  else if (Array.isArray(content)) {
    if (content.some((c) => c.type === 'tool_result')) return false;
    text = content.find((c) => c.type === 'text')?.text ?? null;
  }
  if (text === null) return false;
  return !NOT_HUMAN_PREFIXES.some((p) => text.trimStart().startsWith(p));
}
