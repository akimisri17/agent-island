// The app's webview runs the same stats and report code as the CLI.
// Copy it in before each dev run or build so there is one source of truth.
import { copyFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const src = new URL('../../wrapped/src/', import.meta.url);
const dest = new URL('../ui/lib/', import.meta.url);
mkdirSync(dest, { recursive: true });
for (const f of ['stats.mjs', 'render.mjs', 'models.mjs', 'personas.mjs']) copyFileSync(new URL(f, src), new URL(f, dest));
console.log(`synced shared report code -> ${fileURLToPath(dest)}`);
