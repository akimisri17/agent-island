// Made-up sessions for screenshots and `agent-wrapped --demo`. Deterministic,
// so the README images do not change between runs. No real data involved.
import { newSession } from './lines.mjs';

const MIN = 60_000;
const DAY = 86_400_000;

function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const PROJECTS = ['storefront', 'billing-api', 'mobile-app', 'infra', 'docs-site', 'data-pipeline', 'auth-service', 'design-system'];
const TITLES = ['Fix checkout race', 'Add invoice export', 'Migrate to new router', 'Speed up CI', 'Write onboarding docs', 'Backfill events table', 'Rotate session keys', 'Dark mode tokens'];
const AGENTS = [
  { agent: 'claude', weight: 0.62, models: [['claude-opus-5', 0.45], ['claude-sonnet-5', 0.5], ['claude-haiku-4-5', 0.05]], tools: ['Bash', 'Edit', 'Read', 'Grep', 'Write', 'mcp__browser__navigate'] },
  { agent: 'codex', weight: 0.2, models: [['gpt-5.5', 1]], tools: ['exec_command', 'apply_patch'] },
  { agent: 'cursor', weight: 0.18, models: [['grok-4.7', 0.7], ['cursor-auto', 0.3]], tools: ['edit_file_v2', 'read_file_v2', 'run_terminal_command_v2', 'ripgrep_raw_search'] },
];

const pick = (r, items) => items[Math.floor(r() * items.length)];
const weighted = (r, pairs) => {
  let x = r();
  for (const [v, w] of pairs) if ((x -= w) <= 0) return v;
  return pairs[pairs.length - 1][0];
};

export function demoSessions({ since, until }) {
  const r = rng(42);
  const sessions = [];
  let n = 0;
  for (let day = since; day < until; day += DAY) {
    const d = new Date(day);
    const weekend = d.getDay() === 0 || d.getDay() === 6;
    const count = weekend ? Math.floor(r() * 3) : 4 + Math.floor(r() * 6);
    for (let i = 0; i < count; i++) {
      const spec = weighted(r, AGENTS.map((a) => [a, a.weight]));
      const s = newSession(spec.agent, `demo-${n++}`, 'demo');
      const p = Math.floor(r() * PROJECTS.length);
      s.project = PROJECTS[p];
      s.title = TITLES[(p + i) % TITLES.length];
      // Mostly afternoon and evening, some late nights.
      const startHour = weighted(r, [[9, 0.15], [11, 0.15], [14, 0.25], [17, 0.25], [20, 0.12], [23, 0.08]]);
      let t = new Date(d.getFullYear(), d.getMonth(), d.getDate(), startHour, Math.floor(r() * 60)).getTime();
      const turns = 2 + Math.floor(r() * 9);
      for (let k = 0; k < turns; k++) {
        const work = (2 + r() * 25) * MIN;
        s.prompts.push(t);
        s.turns.push({ start: t, end: t + work });
        const model = weighted(r, spec.models);
        const steps = 3 + Math.floor(r() * 20);
        s.responses[model] = (s.responses[model] || 0) + steps;
        const out = spec.agent === 'cursor' ? 0 : steps * (300 + Math.floor(r() * 900));
        s.models[model] = (s.models[model] || 0) + out;
        if (spec.agent !== 'cursor') {
          s.tokens.output += out;
          s.tokens.cacheRead += steps * (40_000 + Math.floor(r() * 80_000));
          s.tokens.cacheWrite += steps * 2_000;
          s.tokens.input += steps * 20;
        }
        for (let j = 0; j < steps; j++) {
          const tool = pick(r, spec.tools);
          s.tools[tool] = (s.tools[tool] || 0) + 1;
          if (/Edit|Write|apply_patch|edit_file/.test(tool)) s.filesEdited.add(`${s.project}/src/file-${Math.floor(r() * 40)}.ts`);
        }
        // The gap before the next prompt: often quick, sometimes the person is elsewhere.
        t += work + weighted(r, [[1, 0.45], [6, 0.3], [18, 0.17], [45, 0.08]]) * MIN;
      }
      s.start = s.turns[0].start;
      s.end = s.turns[s.turns.length - 1].end;
      if (spec.agent === 'claude' && r() < 0.04) {
        s.limitHits.push({ ts: s.end, type: r() < 0.7 ? 'five_hour' : 'seven_day', resetsAt: s.end + 3 * 3_600_000 });
      }
      s.compactions = r() < 0.15 ? 1 : 0;
      sessions.push(s);
    }
  }
  return sessions;
}
