#!/usr/bin/env node
import { writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { resolve } from 'node:path';
import { scan, defaultRoots } from '../src/scan.mjs';
import { computeStats, filterSessions, filterOptions } from '../src/stats.mjs';
import { renderHtml } from '../src/render.mjs';
import { demoSessions } from '../src/demo.mjs';

const HELP = `agent-wrapped: a local report of what your coding agents did.

Reads Claude Code (~/.claude/projects), Codex (~/.codex/sessions), and Cursor
(its local chat database) on this machine and writes one HTML file. Nothing is
sent anywhere.

Usage: agent-wrapped [--days 30] [--out agent-wrapped.html] [--json] [--no-open] [--demo]
                     [--agent claude|codex|cursor] [--maker Anthropic|OpenAI|xAI|...]

  --demo    made-up data, to see the report without any agent logs
  --agent   only sessions from one agent
  --maker   only sessions whose main model is from one maker
`;

function parseArgs(argv) {
  const opts = { days: 30, out: 'agent-wrapped.html', json: false, open: true, demo: false, agent: null, maker: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '-h' || a === '--help') opts.help = true;
    else if (a === '--days') opts.days = Number(argv[++i]);
    else if (a === '--out') opts.out = argv[++i];
    else if (a === '--json') opts.json = true;
    else if (a === '--no-open') opts.open = false;
    else if (a === '--demo') opts.demo = true;
    else if (a === '--agent') opts.agent = argv[++i]?.toLowerCase();
    else if (a === '--maker') opts.maker = argv[++i];
    else throw new Error(`Unknown option: ${a}`);
  }
  if (!Number.isFinite(opts.days) || opts.days <= 0) throw new Error('--days must be a positive number');
  return opts;
}

// node:sqlite (used for Cursor) prints an ExperimentalWarning on some Node
// versions. It is noise for a CLI user; let every other warning through.
const emitWarning = process.emitWarning.bind(process);
process.emitWarning = (warning, ...rest) => {
  if (String(warning?.message ?? warning).includes('SQLite')) return;
  emitWarning(warning, ...rest);
};

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) return void process.stdout.write(HELP);

  const until = Date.now();
  const since = until - opts.days * 86_400_000;
  const roots = defaultRoots();
  const t0 = Date.now();
  const tty = process.stderr.isTTY;
  const result = opts.demo ? demoResult(since, until) : await scan({
    since,
    roots,
    onProgress: (done, total) => {
      if (tty && total) process.stderr.write(`\rReading agent logs… ${Math.round((done / total) * 100)}%`);
    },
  });
  if (tty) process.stderr.write('\r\x1b[K');
  const seconds = (Date.now() - t0) / 1000;
  const options = filterOptions(result.sessions);
  if (opts.agent && !options.agents.includes(opts.agent)) throw new Error(`no ${opts.agent} sessions in this window (found: ${options.agents.join(', ') || 'none'})`);
  if (opts.maker) {
    const match = options.makers.find((m) => m.toLowerCase() === opts.maker.toLowerCase());
    if (!match) throw new Error(`no sessions with ${opts.maker} models (found: ${options.makers.join(', ') || 'none'})`);
    opts.maker = match;
  }
  const filter = { agent: opts.agent, maker: opts.maker };
  const stats = computeStats(filterSessions(result.sessions, filter), { since, until });

  if (opts.json) {
    const { minutesByDay, minutesByHour, ...rest } = stats;
    process.stdout.write(JSON.stringify({ ...rest, files: result.files, bytes: result.bytes, seconds }, null, 2) + '\n');
    return;
  }
  if (result.files.claude + result.files.codex + result.files.cursor === 0) {
    process.stderr.write(`No agent logs from the last ${opts.days} days in ${roots.claude} or ${roots.codex}.\n`);
    process.exitCode = 1;
    return;
  }

  const out = resolve(opts.out);
  await writeFile(out, renderHtml(stats, { files: result.files, bytes: result.bytes, seconds, filter }));
  process.stderr.write(`${stats.persona.name}. ${stats.persona.line}\nWrote ${out}\n`);
  if (opts.open) openFile(out);
}

function demoResult(since, until) {
  return { sessions: demoSessions({ since, until }), files: { claude: 412, codex: 96, cursor: 1 }, bytes: 2.4e9 };
}

function openFile(path) {
  if (process.platform === 'darwin') execFile('open', [path]);
  else if (process.platform === 'win32') execFile('cmd', ['/c', 'start', '', path]);
  else execFile('xdg-open', [path], () => {});
}

main().catch((err) => {
  process.stderr.write(`agent-wrapped: ${err.message}\n`);
  process.exitCode = 1;
});
