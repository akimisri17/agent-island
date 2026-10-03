// The "Cut off" section of the Waiting tab: sessions a usage limit stopped
// (shown once the limit has reset) or that the person interrupted, and that
// nobody went back to.

const LIMIT_NAME = { five_hour: '5-hour limit', seven_day: 'weekly limit', seven_day_opus: 'weekly Opus limit' };

function ago(ms) {
  const m = Math.max(1, Math.round(ms / 60_000));
  if (m < 60) return `${m}m ago`;
  return `${Math.round(m / 60)}h ago`;
}

export function cutoffSection(list, { now, dismissed }) {
  const shown = list.filter((c) => !dismissed.has(c.sessionId));
  if (!shown.length) return null;
  const resets = shown.filter((c) => c.kind === 'limit' && c.resetsAt).map((c) => c.resetsAt);
  const heading = resets.length ? `Cut off by the limit · reset ${ago(now - Math.max(...resets))}` : 'Stopped mid-task';
  const rows = shown.map((c) => {
    const why = c.kind === 'limit'
      ? (LIMIT_NAME[c.limitType] ? `stopped by the ${LIMIT_NAME[c.limitType]}` : 'stopped by a usage limit')
      : 'you interrupted it';
    const title = c.title || c.project || 'Untitled session';
    return { id: c.sessionId, cwd: c.cwd, title, line: [c.project, why].filter(Boolean).join(' · ') };
  });
  return { heading, rows };
}
