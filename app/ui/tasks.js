// The Tasks tab: one row per scheduled Claude task. Problems (missed, failed,
// stopped) get a red dot and Run now; the dots are the last seven days.

const H = 3_600_000;
const PROBLEM = new Set(['missed', 'failed', 'stopped']);
const RIGHT = { missed: 'Missed', failed: 'Failed', stopped: 'Stopped', never: 'Never ran' };
const NOTE = {
  stopped: 'It may be waiting for an approval in Claude.',
  failed: 'Its last run ended with an error or a usage limit.',
  missed: "It hasn't run when it usually does.",
};

function dur(ms) {
  const m = Math.max(1, Math.round(ms / 60_000));
  return m < 60 ? `${m}m` : `${Math.round(m / 60)}h`;
}

function startOfDay(t) {
  const d = new Date(t);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function dayWord(t, now) {
  const days = Math.round((startOfDay(now) - startOfDay(t)) / (24 * H));
  return days <= 0 ? 'today' : days === 1 ? 'yesterday' : `${days} days ago`;
}

function cadenceWord(ms) {
  if (!ms) return '';
  if (ms >= 20 * H && ms <= 28 * H) return 'Daily';
  if (ms >= 6 * 24 * H && ms <= 8 * 24 * H) return 'Weekly';
  return `Every ${Math.max(1, Math.round(ms / H))} h`;
}

const clock = (t) => new Date(t).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });

export function taskView(r, now) {
  const dot = PROBLEM.has(r.state) ? 'failed' : r.state === 'running' ? 'needs' : r.state === 'ok' ? 'done' : '';
  let right = RIGHT[r.state];
  if (r.state === 'running') right = `Running · ${dur(now - r.last.start)}`;
  if (r.state === 'ok') right = `${clock(r.last.start)} · ${dur(r.last.end - r.last.start)}`;
  let line;
  if (!r.last) line = r.description || '';
  else {
    const c = cadenceWord(r.cadenceMs);
    line = c ? `${c} · last ran ${dayWord(r.last.start, now)}` : `Last ran ${dayWord(r.last.start, now)}`;
  }
  return {
    name: r.name,
    dot,
    right,
    line,
    note: NOTE[r.state] || null,
    days: r.days,
    actions: { runNow: PROBLEM.has(r.state) || r.state === 'never', lastRun: !!r.last },
  };
}

export function tasksHaveProblems(rows) {
  return rows.some((r) => PROBLEM.has(r.state));
}
