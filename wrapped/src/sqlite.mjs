import { existsSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

// Opens another app's SQLite database without writing next to it.
//
// A plain read-only open of a WAL-mode database creates "-wal" and "-shm"
// files beside it when they do not exist yet. So: if the owning app is live
// (its -wal file exists), open read-only to see its latest writes; otherwise
// open as immutable, which writes nothing. Returns null when unavailable.
export async function openForeignDb(path) {
  let DatabaseSync;
  try {
    ({ DatabaseSync } = await import('node:sqlite'));
  } catch {
    return null; // Node without SQLite support
  }
  try {
    if (existsSync(`${path}-wal`)) return new DatabaseSync(path, { readOnly: true });
    return new DatabaseSync(`${pathToFileURL(path).href}?immutable=1`, { readOnly: true });
  } catch {
    return null;
  }
}
