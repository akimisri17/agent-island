// Builds antigravity-conv.db, a tiny stand-in for an Antigravity CLI
// conversation (~/.gemini/antigravity-cli/conversations/<id>.db).
// Run: node test/fixtures/make-antigravity-db.mjs
import { DatabaseSync } from 'node:sqlite';
import { rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// Minimal protobuf encoder for the fixture.
const varint = (n) => { const out = []; while (n >= 128) { out.push((n % 128) | 128); n = Math.floor(n / 128); } out.push(n); return out; };
const field = (no, wire, payload) => [...varint(no * 8 + wire), ...payload];
const int = (no, n) => field(no, 0, varint(n));
const bytes = (no, b) => field(no, 2, [...varint(b.length), ...b]);
const str = (no, s) => bytes(no, [...Buffer.from(s)]);
const msg = (no, ...parts) => bytes(no, parts.flat());
const ts = (no, iso) => { const ms = Date.parse(iso); return msg(no, int(1, Math.floor(ms / 1000)), int(2, (ms % 1000) * 1e6)); };
const buf = (...parts) => Buffer.from(parts.flat());

const path = fileURLToPath(new URL('./antigravity-conv.db', import.meta.url));
rmSync(path, { force: true });
const db = new DatabaseSync(path);
db.exec(`CREATE TABLE steps (idx integer PRIMARY KEY, step_type integer NOT NULL DEFAULT 0, metadata blob);
         CREATE TABLE gen_metadata (idx integer PRIMARY KEY, data blob);`);
const step = db.prepare('INSERT INTO steps VALUES (?, ?, ?)');
const tool = (name, args) => msg(4, str(1, 'call_1'), str(2, name), str(3, JSON.stringify(args)));
step.run(0, 14, buf(ts(1, '2026-09-14T10:00:00Z'), int(3, 4)));
step.run(1, 15, buf(ts(1, '2026-09-14T10:00:05Z'), ts(8, '2026-09-14T10:00:20Z')));
step.run(2, 132, buf(ts(1, '2026-09-14T10:00:21Z'), tool('run_command', { CommandLine: 'ls', Cwd: '/work/forge/.worktrees/WO-1' }), ts(8, '2026-09-14T10:00:22Z')));
step.run(3, 15, buf(ts(1, '2026-09-14T10:00:23Z'), ts(8, '2026-09-14T10:01:00Z')));
step.run(4, 132, buf(ts(1, '2026-09-14T10:01:01Z'), tool('write_to_file', { TargetFile: '/work/forge/notes.md' })));
step.run(5, 15, buf(ts(1, '2026-09-14T10:01:02Z'), ts(8, '2026-09-14T10:02:00Z')));
step.run(6, 9, Buffer.from([0xff, 0xff])); // malformed blob: skipped
const gen = db.prepare('INSERT INTO gen_metadata VALUES (?, ?)');
gen.run(0, buf(msg(1, str(19, 'gemini-pro-default'))));
gen.run(1, buf(msg(1, str(19, 'gemini-pro-default'))));
gen.run(2, buf(msg(1, str(19, 'gemini-3.8-flash'))));
gen.run(3, buf(msg(1, str(19, 'MODEL_PLACEHOLDER_M16')))); // placeholder: ignored
db.close();
console.log('wrote', path);
