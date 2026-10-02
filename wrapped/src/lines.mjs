import { createReadStream } from 'node:fs';
import { createInterface } from 'node:readline';

// Yields parsed JSON objects from a JSONL file. Bad lines are skipped:
// logs are written while sessions run, so the last line is often partial.
export async function* readJsonl(path) {
  const rl = createInterface({
    input: createReadStream(path, { highWaterMark: 1 << 20 }),
    crlfDelay: Infinity,
  });
  for await (const line of rl) {
    if (!line) continue;
    try {
      yield JSON.parse(line);
    } catch {
      // partial or corrupt line
    }
  }
}

export function newSession(agent, id, file) {
  return {
    agent,
    id,
    file,
    project: null,
    cwd: null, // working directory, for finding the repository
    title: null,
    start: null,
    end: null,
    prompts: [],
    turns: [],
    tokens: { input: 0, cacheRead: 0, cacheWrite: 0, output: 0 },
    models: {}, // output tokens per model
    responses: {}, // agent responses per model: the one measure every agent records
    tools: {},
    filesEdited: new Set(),
    compactions: 0,
    limitHits: [],
    peakQuotaPct: null,
    isSubagent: false,
  };
}

// Turn tracking shared by both adapters. A human prompt opens a turn; any
// agent activity extends it. The gap between a turn's end and the next prompt
// is time the agent sat finished, waiting on the person.
export function createTurnTracker(session) {
  let open = null;
  return {
    prompt(ts) {
      session.prompts.push(ts);
      if (open) session.turns.push(open);
      open = { start: ts, end: ts };
      touch(session, ts);
    },
    activity(ts) {
      touch(session, ts);
      if (open && ts > open.end) open.end = ts;
    },
    finish() {
      if (open) session.turns.push(open);
      open = null;
    },
  };
}

function touch(session, ts) {
  if (session.start === null || ts < session.start) session.start = ts;
  if (session.end === null || ts > session.end) session.end = ts;
}

export function addModel(session, model, outputTokens) {
  if (!model || model === '<synthetic>') return;
  session.models[model] = (session.models[model] || 0) + outputTokens;
}

export function addResponse(session, model) {
  if (!model || model === '<synthetic>') return;
  session.responses[model] = (session.responses[model] || 0) + 1;
}

export function addTool(session, name) {
  if (!name) return;
  session.tools[name] = (session.tools[name] || 0) + 1;
}

// The project a working directory belongs to. Git worktrees count toward
// their repository: /repo/.worktrees/feature-x and /repo/.claude/worktrees/y
// are both "repo".
export function projectOf(cwd) {
  if (!cwd) return null;
  const parts = cwd.split(/[\\/]/).filter(Boolean);
  const wt = parts.findIndex((p) => p === '.worktrees' || p === 'worktrees');
  if (wt > 0) {
    const repo = parts.slice(0, wt).filter((p) => p !== '.claude');
    return repo[repo.length - 1] || null;
  }
  return parts[parts.length - 1] || null;
}

// Splits on both separators: logs written on Windows carry paths like C:\work\shop.
export function basename(p) {
  if (!p) return null;
  const parts = p.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || p;
}
