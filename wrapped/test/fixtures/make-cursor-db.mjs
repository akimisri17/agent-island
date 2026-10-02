// Builds cursor-state.vscdb, a tiny stand-in for Cursor's
// ~/Library/Application Support/Cursor/User/globalStorage/state.vscdb.
// Run: node test/fixtures/make-cursor-db.mjs
import { DatabaseSync } from 'node:sqlite';
import { rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const path = fileURLToPath(new URL('./cursor-state.vscdb', import.meta.url));
rmSync(path, { force: true });
const db = new DatabaseSync(path);
db.exec(`CREATE TABLE cursorDiskKV (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);
         CREATE TABLE composerHeaders (composerId TEXT PRIMARY KEY, isSubagent INTEGER);`);
const put = db.prepare('INSERT INTO cursorDiskKV (key, value) VALUES (?, ?)');
const t = (iso) => Date.parse(iso);
const header = (id, type, iso, doneIso) => ({ bubbleId: id, type, createdAt: iso, startedAtMs: t(iso), completedAtMs: t(doneIso ?? iso) });

put.run('composerData:k1', JSON.stringify({
  composerId: 'k1', name: 'Catalog upload', lastUpdatedAt: t('2026-09-12T08:30:00Z'),
  modelConfig: { modelName: 'grok-4.7' },
  workspaceIdentifier: { id: 'w1', uri: { fsPath: '/work/catalog' } },
  fullConversationHeadersOnly: [
    header('b1', 1, '2026-09-12T08:00:00Z'),
    header('b2', 2, '2026-09-12T08:01:00Z', '2026-09-12T08:04:00Z'),
    header('b3', 2, '2026-09-12T08:05:00Z'),
    header('b4', 1, '2026-09-12T08:20:00Z'),
    header('b5', 2, '2026-09-12T08:21:00Z', '2026-09-12T08:22:00Z'),
  ],
}));
put.run('bubbleId:k1:b2', JSON.stringify({ toolFormerData: { name: 'edit_file_v2', params: JSON.stringify({ relativeWorkspacePath: '/work/catalog/upload.ts' }) } }));
put.run('bubbleId:k1:b3', JSON.stringify({ toolFormerData: { name: 'run_terminal_command_v2', params: '{}' } }));
put.run('bubbleId:k1:b5', JSON.stringify({ tokenCount: { inputTokens: 0, outputTokens: 0 } }));
// A chat from before the window, and a draft with no messages: both skipped.
put.run('composerData:old', JSON.stringify({ composerId: 'old', lastUpdatedAt: t('2026-08-01T00:00:00Z'), fullConversationHeadersOnly: [header('o1', 1, '2026-08-01T00:00:00Z')] }));
put.run('composerData:draft', JSON.stringify({ composerId: 'draft', lastUpdatedAt: t('2026-09-20T00:00:00Z'), fullConversationHeadersOnly: [] }));
// An Auto-mode subagent: its type-1 messages are the parent agent, not a person.
put.run('composerData:sub', JSON.stringify({
  composerId: 'sub', lastUpdatedAt: t('2026-09-12T09:00:00Z'), modelConfig: { modelName: 'default' },
  fullConversationHeadersOnly: [header('s1', 1, '2026-09-12T08:10:00Z'), header('s2', 2, '2026-09-12T08:11:00Z')],
}));
db.prepare('INSERT INTO composerHeaders VALUES (?, ?)').run('sub', 1);
db.close();
console.log('wrote', path);
