import { projectOf, readJsonl, newSession, createTurnTracker, addModel, addResponse, addTool, basename } from './lines.mjs';

const PATCH_FILE = /^\*\*\* (?:Update|Add) File: (.+)$/gm;

// Parses one Codex rollout log (~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl).
export async function parseCodexFile(path, { since } = {}) {
  const s = newSession('codex', basename(path).replace(/\.jsonl$/, ''), path);
  const turns = createTurnTracker(s);
  let model = null;
  let lastTotal = null;
  const limitKeys = new Set();

  for await (const d of readJsonl(path)) {
    const p = d.payload || {};
    if (d.type === 'session_meta') {
      if (p.id) s.id = p.id;
      if (p.cwd) s.project = projectOf(p.cwd);
      continue;
    }
    if (d.type === 'turn_context' && p.model) model = p.model;
    if (!d.timestamp) continue;
    const ts = Date.parse(d.timestamp);
    if (Number.isNaN(ts) || ts < since) continue;

    if (d.type === 'compacted') s.compactions++;

    if (d.type === 'event_msg') {
      if (p.type === 'user_message') turns.prompt(ts);
      else turns.activity(ts);
      if (p.type === 'token_count') {
        if (p.info?.total_token_usage) lastTotal = p.info.total_token_usage;
        const rl = p.rate_limits;
        const pct = Math.max(rl?.primary?.used_percent ?? -1, rl?.secondary?.used_percent ?? -1);
        if (pct >= 0) s.peakQuotaPct = Math.max(s.peakQuotaPct ?? 0, pct);
        if (rl?.rate_limit_reached_type) {
          const key = `${rl.rate_limit_reached_type}:${rl.primary?.resets_at}`;
          if (!limitKeys.has(key)) {
            limitKeys.add(key);
            s.limitHits.push({ ts, type: rl.rate_limit_reached_type, resetsAt: (rl.primary?.resets_at ?? 0) * 1000 });
          }
        }
      }
      continue;
    }

    if (d.type === 'response_item') {
      turns.activity(ts);
      if (p.type === 'message' && p.role === 'assistant') addResponse(s, model);
      if (p.type === 'function_call' || p.type === 'custom_tool_call') {
        addTool(s, p.name);
        if (p.name === 'apply_patch' && typeof p.input === 'string') {
          for (const m of p.input.matchAll(PATCH_FILE)) s.filesEdited.add(m[1].trim());
        }
      }
    }
  }
  turns.finish();

  if (lastTotal) {
    const cached = lastTotal.cached_input_tokens || 0;
    s.tokens.input += Math.max(0, (lastTotal.input_tokens || 0) - cached);
    s.tokens.cacheRead += cached;
    s.tokens.output += lastTotal.output_tokens || 0;
    addModel(s, model, lastTotal.output_tokens || 0);
  }
  return s;
}
