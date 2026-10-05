// The agent guide is only useful while it's true. These checks fail the build
// when AGENTS.md or docs/ARCHITECTURE.md point at files that no longer exist,
// when CLAUDE.md stops importing AGENTS.md, or when the guide grows too long.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const root = new URL('../../', import.meta.url);
const read = (p) => readFileSync(new URL(p, root), 'utf8');

// Backticked repo paths with a slash, e.g. `app/ui/panel.js` or `wrapped/test/fixtures/`.
// Bare file names, commands, globs, home paths and private or generated folders are skipped.
function repoPaths(md) {
  const spans = [...md.matchAll(/`([^`\n]+)`/g)].map((m) => m[1]);
  return spans.filter(
    (s) =>
      /^[\w.-]+(\/[\w.-]+)+\/?$/.test(s) &&
      !s.startsWith('docs/internal') &&
      !s.startsWith('app/ui/lib') &&
      !s.startsWith('.claude/')
  );
}

for (const doc of ['AGENTS.md', 'docs/ARCHITECTURE.md']) {
  test(`${doc}: every path it names exists`, () => {
    const paths = repoPaths(read(doc));
    assert.ok(paths.length > 5, `expected ${doc} to name repo paths`);
    const missing = paths.filter((p) => !existsSync(new URL(p, root)));
    assert.deepEqual(missing, [], `${doc} names paths that don't exist; fix the doc in the same PR`);
  });
}

test('CLAUDE.md imports AGENTS.md and nothing else', () => {
  assert.equal(read('CLAUDE.md').trim(), '@AGENTS.md');
});

test('AGENTS.md stays short enough to be read every session', () => {
  const lines = read('AGENTS.md').split('\n').length;
  assert.ok(lines <= 150, `AGENTS.md is ${lines} lines; keep it under 150 and move detail to docs/ARCHITECTURE.md`);
});
