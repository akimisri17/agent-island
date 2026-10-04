// Wrapped: where the person approves the most. Counts are estimates: logs
// record tool calls and the permission mode, not the approvals themselves.

function wait(ms) {
  const m = Math.round(ms / 60_000);
  return m < 1 ? `${Math.max(1, Math.round(ms / 1000))}s` : m < 60 ? `${m}m` : `${Math.round(m / 60)}h`;
}

const NOISY = /["'`$()*]/;

export function frictionRows(projects) {
  return projects.slice(0, 3).map((p) => {
    const groups = p.commands.map(([g]) => g).filter((g) => !NOISY.test(g));
    const what = groups.length ? groups.slice(0, 3) : p.tools.slice(0, 3).map(([t]) => t);
    const line = [what.join(', '), p.medianWaitMs != null && `${wait(p.medianWaitMs)} median wait`].filter(Boolean).join(' · ');
    const scripts = (p.commands.find(([g]) => g === 'shell script') || [, 0])[1];
    return {
      project: p.project,
      name: p.name,
      right: `about ${p.asked}×`,
      line,
      sub: p.affirmations ? `${p.affirmations} "go ahead" ${p.affirmations === 1 ? 'reply' : 'replies'}` : null,
      note: p.asked > 0 && scripts * 2 >= p.asked ? 'Mostly multi-line shell scripts the agent wrote — no rule can allow those.' : null,
      canAllow: p.suggestions.length > 0,
      suggestions: p.suggestions,
    };
  });
}
