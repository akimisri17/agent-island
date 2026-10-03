# Panel Redesign, Phase 1 (Shell and Design System) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the Agent Island panel on the approved design system (Silver palette, hairline rows, one icon family) with a bottom tab bar, a `⋯` menu in place of the gear and footer, and the Repo board moved out of Today into its own tab.

**Architecture:** The panel is static HTML/CSS/JS in `app/ui/`, loaded by Tauri with `withGlobalTauri`, calling Rust through `window.__TAURI__.core.invoke`. This phase changes only the front end and the window size. Pure formatting moves into small testable modules (`icons.js`, `repos.js`) tested with `node --test`; `panel.js` keeps the DOM wiring. A dev-only preview server (`npm run preview`) serves `app/ui/` with a stubbed `invoke` fed by real data from the existing Rust examples, so every screen can be checked in a browser.

**Tech Stack:** Vanilla JS (ES modules), CSS custom properties, Tauri 2, Rust examples (`dump`, `repos`), Node 22 `node:test`.

**Spec:** `docs/superpowers/specs/2026-10-04-panel-redesign-design.md`.

**Decisions for this phase (deviations from the spec, all deferred, none dropped):**
- Four tabs ship now: Waiting, Today, Repos, Wrapped. **Tasks** arrives with its data in phase 4, so no tab is ever empty by construction.
- Wrapped's agent/model filters stay in the panel as a quiet row; moving them into the report is deferred (the report has no filter UI yet).
- The translucent macOS window needs Tauri's `macOSPrivateApi`; deferred to its own small change. `--bg` is solid for now.
- Settings rows that need later phases (terminal picker, notify missed tasks, launch at login) are not added yet.
- The Repo board's copy buttons are removed. Pull, Delete and Terminal arrive in phase 2.

---

## Branch

Work on `feat/panel-shell`, created from `origin/dev` (which includes #24 and #25):

```bash
cd /Users/akhilmisri/Business/Products/agent-island
git fetch -q
git checkout -b feat/panel-shell origin/dev
```

Never commit to `main` or `dev` directly. No Claude attribution lines in commits.

## File structure

| File | Responsibility |
|---|---|
| `app/ui/icons.js` (new) | The one icon family: SVG symbol sprite, `icon(name)` helper, `ICONS` list |
| `app/ui/repos.js` (new) | `repoRow(status)` and `sortRepos(rows)`: Repo board formatting (moved out of `recap.js`) |
| `app/ui/recap.js` | Today recap only (repo code removed) |
| `app/ui/index.html` | Structure: header (title, back, `⋯`), menu, four views, note line, tab bar |
| `app/ui/panel.css` | Tokens and all styles, rewritten |
| `app/ui/panel.js` | DOM wiring, rewritten around the new structure |
| `app/test/icons.test.mjs` (new) | Icon helper tests |
| `app/test/repos.test.mjs` (new) | Repo row tests |
| `app/test/recap.test.mjs` | Old repo-card test removed |
| `app/scripts/preview.mjs` (new) | Dev-only server with stubbed `invoke` and real data |
| `app/package.json` | `preview` script |
| `app/src-tauri/tauri.conf.json` | Window 360 × 560 |

---

### Task 1: Icon family

**Files:**
- Create: `app/ui/icons.js`
- Test: `app/test/icons.test.mjs`

- [ ] **Step 1: Write the failing test**

Create `app/test/icons.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { icon, ICONS, sprite } from '../ui/icons.js';

test('icons: one family, every icon in the sprite, helper refers to it', () => {
  for (const name of ['clock', 'sun', 'branch', 'chart', 'more', 'back', 'arrow', 'copy', 'spark']) {
    assert.ok(ICONS.includes(name), name);
    assert.match(sprite(), new RegExp(`<symbol id="i-${name}" viewBox="0 0 24 24">`));
  }
  assert.equal(icon('more'), '<svg class="i" aria-hidden="true"><use href="#i-more"></use></svg>');
  assert.equal(icon('back', 's'), '<svg class="i s" aria-hidden="true"><use href="#i-back"></use></svg>');
  assert.throws(() => icon('gear'), /unknown icon/);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd app && node --test test/icons.test.mjs`
Expected: FAIL, `Cannot find module '../ui/icons.js'`.

- [ ] **Step 3: Write the implementation**

Create `app/ui/icons.js`:

```js
// The panel's one icon family: line icons on a 24-unit grid, drawn with the
// stroke set in CSS (.i), so every icon has the same weight. The sprite is
// added to the page once; icon() refers to it.

const PATHS = {
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  branch: '<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="7" r="2"/><path d="M6 7v10M18 9c0 5-7 4-11 8"/>',
  chart: '<path d="M5 20V11M12 20V5M19 20v-6"/>',
  more: '<circle cx="5" cy="12" r="1.3"/><circle cx="12" cy="12" r="1.3"/><circle cx="19" cy="12" r="1.3"/>',
  back: '<path d="M15 5l-7 7 7 7"/>',
  arrow: '<path d="M9 5l7 7-7 7"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>',
  spark: '<path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z"/>',
};

export const ICONS = Object.keys(PATHS);

export function sprite() {
  const symbols = ICONS.map((n) => `<symbol id="i-${n}" viewBox="0 0 24 24">${PATHS[n]}</symbol>`).join('');
  return `<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0" style="position:absolute" aria-hidden="true"><defs>${symbols}</defs></svg>`;
}

// size: '' for 18 px (tab bar, header), 's' for 14 px inline.
export function icon(name, size = '') {
  if (!PATHS[name]) throw new Error(`unknown icon: ${name}`);
  return `<svg class="${size ? `i ${size}` : 'i'}" aria-hidden="true"><use href="#i-${name}"></use></svg>`;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd app && node --test test/icons.test.mjs`
Expected: PASS (1 test).

- [ ] **Step 5: Commit**

```bash
git add app/ui/icons.js app/test/icons.test.mjs
git commit -m "Panel: one icon family as an SVG sprite"
```

---

### Task 2: Repo rows in plain words

Replaces `repoCard` (and its copy commands) with `repoRow`: a status dot, one plain-language status line, and old branches combined.

**Files:**
- Create: `app/ui/repos.js`
- Create: `app/test/repos.test.mjs`
- Modify: `app/ui/recap.js` (delete everything from the line `// The Repo board: one card per repository agents worked in today, from local` to the end of the file)
- Modify: `app/test/recap.test.mjs` (delete the whole `test('repo card: facts, and commands to copy only when they are safe', …)` block at the end of the file)

- [ ] **Step 1: Write the failing test**

Create `app/test/repos.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { repoRow, sortRepos } from '../ui/repos.js';

const base = {
  project: 'shop', path: '/w/shop', branch: 'main', detached: false, upstream: 'origin/main',
  ahead: 0, behind: 0, changes: 0, stashes: 0, worktrees: [], defaultBranch: 'main', merged: [], gone: [],
};

test('repo row: clean', () => {
  const r = repoRow(base);
  assert.equal(r.status, 'Clean');
  assert.equal(r.dot, 'done');
  assert.equal(r.canPull, false);
  assert.deepEqual(r.oldBranches, []);
});

test('repo row: plain words, needs-you dot, pull only without local changes', () => {
  const r = repoRow({ ...base, behind: 26 });
  assert.equal(r.status, '26 commits to pull');
  assert.equal(r.dot, 'needs');
  assert.equal(r.canPull, true);

  const busy = repoRow({ ...base, changes: 30, upstream: null, branch: 'docs/x' });
  assert.equal(busy.status, '30 uncommitted · not pushed yet');
  assert.equal(busy.dot, 'needs');
  assert.equal(repoRow({ ...base, behind: 1, changes: 1 }).canPull, false);
});

test('repo row: merged and gone branches are one "old branches" count', () => {
  const r = repoRow({ ...base, ahead: 2, stashes: 1, merged: ['feat/old-cart'], gone: ['feat/squashed'] });
  assert.equal(r.status, '2 to push · 1 stash · 2 old branches');
  assert.deepEqual(r.oldBranches, [
    { name: 'feat/old-cart', reason: 'merged' },
    { name: 'feat/squashed', reason: 'gone' },
  ]);
});

test('repo row: only housekeeping left is a quiet dot', () => {
  const r = repoRow({ ...base, worktrees: [{ path: '/w/shop/.worktrees/x', branch: 'hotfix/tax' }] });
  assert.equal(r.status, '1 worktree · hotfix/tax');
  assert.equal(r.dot, 'none');
});

test('repo row: detached HEAD is named, not flagged as unpushed', () => {
  const r = repoRow({ ...base, branch: 'abc1234', detached: true, upstream: null });
  assert.equal(r.branch, 'detached at abc1234');
  assert.equal(r.status, 'Clean');
});

test('sortRepos: needs you first, then quiet, then clean; by name within', () => {
  const rows = [
    repoRow({ ...base, project: 'b' }),
    repoRow({ ...base, project: 'z', changes: 1 }),
    repoRow({ ...base, project: 'a', stashes: 1 }),
    repoRow({ ...base, project: 'c', behind: 2 }),
  ];
  assert.deepEqual(sortRepos(rows).map((r) => r.name), ['c', 'z', 'a', 'b']);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd app && node --test test/repos.test.mjs`
Expected: FAIL, `Cannot find module '../ui/repos.js'`.

- [ ] **Step 3: Write the implementation**

Create `app/ui/repos.js`:

```js
// The Repo board: one row per repository agents worked in today, from local
// git only. The status line says what you would do, in plain words; the dot
// says whether it needs you.

const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
const DOT_ORDER = { needs: 0, none: 1, done: 2 };

export function repoRow(r) {
  const unpushed = !r.upstream && !r.detached;
  const oldBranches = [
    ...(r.merged || []).map((name) => ({ name, reason: 'merged' })),
    ...(r.gone || []).map((name) => ({ name, reason: 'gone' })),
  ];
  const parts = [];
  if (r.changes) parts.push(`${r.changes} uncommitted`);
  if (unpushed) parts.push('not pushed yet');
  if (r.ahead) parts.push(`${r.ahead} to push`);
  if (r.behind) parts.push(plural(r.behind, 'commit') + ' to pull');
  if (r.stashes) parts.push(plural(r.stashes, 'stash', 'stashes'));
  if (r.worktrees.length) {
    const names = r.worktrees.map((w) => w.branch || w.path);
    parts.push(r.worktrees.length === 1 ? `1 worktree · ${names[0]}` : plural(r.worktrees.length, 'worktree'));
  }
  if (oldBranches.length) parts.push(plural(oldBranches.length, 'old branch', 'old branches'));

  const needs = !!(r.changes || r.ahead || r.behind || unpushed);
  return {
    name: r.project,
    path: r.path,
    branch: r.detached ? `detached at ${r.branch}` : r.branch,
    status: parts.length ? parts.join(' · ') : 'Clean',
    dot: needs ? 'needs' : parts.length ? 'none' : 'done',
    canPull: r.behind > 0 && !r.changes,
    oldBranches,
  };
}

export function sortRepos(rows) {
  return [...rows].sort((a, b) => DOT_ORDER[a.dot] - DOT_ORDER[b.dot] || a.name.localeCompare(b.name));
}
```

Then delete the old Repo board code from `app/ui/recap.js`: everything from the comment line `// The Repo board: one card per repository agents worked in today, from local` to the end of the file (the `quote` and `plural` helpers and `repoCard`). The file now ends with the closing `}` of `standupText`.

Then delete the last test in `app/test/recap.test.mjs`, the block starting `test('repo card: facts, and commands to copy only when they are safe', async () => {` through its closing `});`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd app && npm test`
Expected: PASS, all tests (recap tests unchanged, plus icons and repos). `grep -n repoCard ui/*.js test/*.mjs` prints only `ui/panel.js` lines (fixed in Task 5).

- [ ] **Step 5: Commit**

```bash
git add app/ui/repos.js app/test/repos.test.mjs app/ui/recap.js app/test/recap.test.mjs
git commit -m "Repo board: plain-language status, one old-branches count"
```

---

### Task 3: Tokens and styles

**Files:**
- Modify: `app/ui/panel.css` (replace the whole file)
- Modify: `app/src-tauri/tauri.conf.json` (window size)

- [ ] **Step 1: Replace `app/ui/panel.css` with:**

```css
/* Tokens. Colour means state and appears only as small marks; text is ink or
   meta, never coloured. Light is the default; dark mirrors it. */
:root {
  --bg: #f7f7f8; --ink: #1c1c1e; --meta: #6e6e73; --line: #e5e5ea; --fill: #ececef;
  --primary: #1c1c1e; --on-primary: #ffffff;
  --needs: #b08a45; --done: #6d9a82; --failed: #a95252;
  --r-control: 8px; --r-sheet: 12px;
  color-scheme: light dark;
}
@media (prefers-color-scheme: dark) {
  :root { --bg: #141414; --ink: #e6e6e6; --meta: #8e8e8e; --line: #212121; --fill: #262626;
    --primary: #e4e6ea; --on-primary: #141414;
    --needs: #d2ae6e; --done: #90bba4; --failed: #c46f6f; }
}

* { box-sizing: border-box; }
html, body { height: 100%; }
body {
  margin: 0; background: var(--bg); color: var(--ink); overflow: hidden;
  font: 13px/1.4 -apple-system, BlinkMacSystemFont, "Segoe UI Variable", "Segoe UI", sans-serif;
  display: flex; flex-direction: column;
  -webkit-user-select: none; user-select: none; cursor: default;
}
button { font: inherit; color: inherit; cursor: pointer; }
[hidden] { display: none !important; }
code, .mono { font: 11px ui-monospace, SFMono-Regular, Menlo, monospace; color: var(--meta); }
.num { font-variant-numeric: tabular-nums; }

/* Icons: one family, one stroke. */
.i { width: 18px; height: 18px; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; flex: none; }
.i.s { width: 14px; height: 14px; }

/* Header: title on the left, back when in Settings, ⋯ on the right. */
.top { display: flex; align-items: center; gap: 4px; padding: 14px 12px 4px 16px; flex: none; }
.title { flex: 1; margin: 0; font-size: 17px; font-weight: 650; letter-spacing: -.01em; }
.icon { border: 0; background: none; color: var(--meta); padding: 4px; border-radius: 6px; display: flex; }
.icon:hover, .icon[aria-expanded="true"] { color: var(--ink); background: var(--fill); }
#back { margin-left: -6px; }

/* ⋯ menu */
.menu { position: absolute; right: 10px; top: 42px; z-index: 10; min-width: 190px; padding: 4px;
  background: var(--bg); border: 1px solid var(--line); border-radius: 10px; box-shadow: 0 10px 28px rgba(0,0,0,.28); }
.menu button { display: flex; justify-content: space-between; width: 100%; border: 0; background: none; padding: 6px 9px; border-radius: 6px; text-align: left; }
.menu button:hover, .menu button:focus-visible { background: var(--fill); outline: none; }
.menu kbd { font: inherit; color: var(--meta); }
.menu hr { border: 0; border-top: 1px solid var(--line); margin: 4px 0; }
.menu-note { color: var(--meta); font-size: 11.5px; padding: 3px 9px; }

/* Views */
.view { flex: 1; min-height: 0; display: flex; flex-direction: column; overflow-y: auto; }
.view::-webkit-scrollbar { width: 0; }
.sub { margin: 0; padding: 0 16px 8px; color: var(--meta); font-size: 11.5px; }
.sub b { color: var(--ink); font-weight: 600; }
.note { margin: 0; padding: 6px 16px; color: var(--meta); font-size: 11.5px; border-top: 1px solid var(--line); flex: none; }

/* Rows: hairline separated, no boxes. */
.rows { list-style: none; margin: 0; padding: 0; }
.rows li { display: grid; grid-template-columns: minmax(0, 1fr) auto; column-gap: 8px; row-gap: 2px; align-items: baseline;
  padding: 9px 16px; border-top: 1px solid var(--line); min-height: 44px; }
.rows li[tabindex] { cursor: pointer; }
.rows li[tabindex]:hover, .rows li[tabindex]:focus-visible { background: var(--fill); outline: none; }
.rows .name { font-weight: 600; display: flex; align-items: center; gap: 7px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rows .right { color: var(--meta); font-size: 11.5px; white-space: nowrap; }
.rows .right.strong { color: var(--ink); font-weight: 600; font-size: 13px; }
.rows .line { grid-column: 1 / -1; color: var(--meta); font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rows .dotted .line { padding-left: 14px; }
.dot { width: 7px; height: 7px; border-radius: 50%; flex: none; background: var(--line); }
.dot.needs { background: var(--needs); }
.dot.done { background: var(--done); }
.dot.failed { background: var(--failed); }
.dot.ring { background: none; box-shadow: inset 0 0 0 1.4px var(--meta); }
.sec { margin: 0; padding: 14px 16px 6px; font-size: 10.5px; font-weight: 650; letter-spacing: .06em; text-transform: uppercase; color: var(--meta); }

/* Collapsed groups (Working, Idle) */
.group summary { list-style: none; color: var(--meta); font-size: 11.5px; padding: 10px 16px; border-top: 1px solid var(--line); cursor: pointer; display: flex; align-items: center; gap: 4px; }
.group summary::-webkit-details-marker { display: none; }
.group summary .i { transition: transform .15s; }
.group[open] summary .i { transform: rotate(90deg); }
.group .rows li { opacity: .7; }

/* Limits: a label, a grey status, a 2 px line. */
.limits { padding: 2px 16px 10px; display: flex; flex-direction: column; gap: 5px; flex: none; }
.limit-row { display: flex; justify-content: space-between; gap: 8px; align-items: baseline; font-size: 11.5px; color: var(--meta); }
.limit-name { white-space: nowrap; }
.limit-status { text-align: right; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.limits.limited .limit-status, .limits.high .limit-status { color: var(--ink); }
.meter { height: 2px; background: var(--fill); border-radius: 1px; overflow: hidden; }
.meter b { display: block; height: 100%; background: var(--needs); }
.hits { display: flex; gap: 3px; align-items: center; flex-wrap: wrap; }
.hits i { width: 6px; height: 6px; border-radius: 50%; box-shadow: inset 0 0 0 1px var(--meta); }
.hits i.on { background: var(--needs); box-shadow: none; }
.hits span { color: var(--meta); font-size: 11px; margin-left: 4px; }
.advice { font-size: 11.5px; color: var(--meta); }

/* Empty states: what will appear here, and why it is empty now. */
.empty { margin: auto 0; padding: 24px 28px; text-align: center; display: flex; flex-direction: column; align-items: center; gap: 6px; }
.empty .i { width: 26px; height: 26px; color: var(--meta); }
.empty b { font-size: 14px; }
.empty span { color: var(--meta); font-size: 12px; line-height: 1.5; }

/* Buttons */
.actions { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; padding: 10px 16px 12px; margin-top: auto; flex: none; }
.btn { display: inline-flex; justify-content: center; align-items: center; gap: 6px; border: 0; border-radius: var(--r-control); padding: 7px 10px; font-weight: 650; font-size: 12px; background: var(--fill); color: var(--ink); }
.btn.primary { background: var(--primary); color: var(--on-primary); }
.btn:disabled { opacity: .45; cursor: default; }
.link { border: 0; background: none; color: var(--meta); font-size: 11.5px; padding: 0; display: inline-flex; align-items: center; gap: 2px; }
.link:hover { color: var(--ink); }
.polished { margin: 0 16px; white-space: pre-wrap; font: inherit; font-size: 12px; border-top: 1px solid var(--line); padding: 10px 0 0; max-height: 140px; overflow-y: auto; user-select: text; -webkit-user-select: text; flex: none; }

/* Wrapped */
.controls { display: grid; grid-template-columns: auto minmax(0, 1fr) minmax(0, 1fr); gap: 6px; align-items: center; padding: 0 16px 10px; flex: none; }
.seg { display: flex; background: var(--fill); border-radius: 7px; padding: 2px; }
.seg button { border: 0; background: transparent; padding: 2px 8px; border-radius: 5px; color: var(--meta); font-size: 11.5px; }
.seg button[aria-selected="true"] { background: var(--bg); color: var(--ink); }
.controls select { font: inherit; font-size: 11.5px; color: var(--ink); background: var(--fill); border: 0; border-radius: 7px; padding: 4px 6px; min-width: 0; }
.persona { padding: 0 16px; font-size: 15px; font-weight: 650; }
.persona-line { padding: 2px 16px 0; color: var(--meta); font-size: 11.5px; }
.badges { display: flex; flex-wrap: wrap; gap: 4px 10px; padding: 4px 16px 0; color: var(--meta); font-size: 11.5px; }
.badges:empty { display: none; }
.tiles { display: grid; grid-template-columns: 1fr 1fr; margin-top: 12px; border-top: 1px solid var(--line); flex: none; }
.tile { padding: 10px 16px; border-bottom: 1px solid var(--line); }
.tile:nth-child(odd) { border-right: 1px solid var(--line); }
.tile .n { font-size: 17px; font-weight: 650; font-variant-numeric: tabular-nums; }
.tile .cap { color: var(--meta); font-size: 11px; }
.chart { padding: 12px 16px 0; flex: none; }
.spark { width: 100%; height: 36px; display: block; }
.spark rect { fill: var(--meta); }
.spark rect.zero { opacity: .25; }
.axis { display: flex; justify-content: space-between; color: var(--meta); font-size: 10.5px; margin-top: 3px; }
.meta { padding: 8px 16px 0; color: var(--meta); font-size: 11.5px; }
.report-row { margin-top: auto; padding: 10px 16px; border-top: 1px solid var(--line); }

/* Settings */
.set-row { display: flex; justify-content: space-between; align-items: center; gap: 12px; padding: 10px 16px; border-top: 1px solid var(--line); min-height: 44px; }
.set-row b { display: block; font-weight: 600; }
.set-row small { display: block; color: var(--meta); font-size: 11.5px; margin-top: 2px; }
.set-row input[type="checkbox"] { width: 16px; height: 16px; accent-color: var(--primary); flex: none; }
.key { font: 11.5px ui-monospace, monospace; min-width: 60px; padding: 4px 9px; border-radius: 6px; border: 0; background: var(--fill); color: var(--ink); flex: none; }
.key.recording { box-shadow: inset 0 0 0 1.4px var(--needs); }
.set-msg { margin: 0; padding: 0 16px 8px; font-size: 11.5px; color: var(--meta); }
.set-msg:empty { display: none; }

/* Tab bar */
.tabbar { display: flex; justify-content: space-around; padding: 7px 6px 9px; border-top: 1px solid var(--line); flex: none; }
.tabbar button { border: 0; background: none; display: flex; flex-direction: column; align-items: center; gap: 3px; width: 64px; padding: 2px 0;
  font-size: 10px; font-weight: 600; color: var(--meta); position: relative; border-radius: 6px; }
.tabbar button[aria-selected="true"] { color: var(--ink); }
.tabbar button:focus-visible { outline: 1.5px solid var(--meta); outline-offset: 1px; }
.tdot { position: absolute; top: 1px; left: calc(50% + 8px); width: 6px; height: 6px; border-radius: 50%; background: var(--needs); }
```

- [ ] **Step 2: Set the window size**

In `app/src-tauri/tauri.conf.json`, change the panel window's size:

```json
        "width": 360,
        "height": 560,
```

- [ ] **Step 3: Commit**

```bash
git add app/ui/panel.css app/src-tauri/tauri.conf.json
git commit -m "Panel: Silver tokens, hairline rows, tab bar styles; window 360x560"
```

(The panel is visually broken between this commit and Task 5; that is expected.)

---

### Task 4: Page structure

**Files:**
- Modify: `app/ui/index.html` (replace the whole file)

- [ ] **Step 1: Replace `app/ui/index.html` with:**

```html
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Agent Island</title>
<link rel="stylesheet" href="panel.css">
<script type="module" src="panel.js"></script>
</head>
<body>
<header class="top">
  <button class="icon" id="back" aria-label="Back" hidden></button>
  <h1 class="title" id="title">Waiting</h1>
  <button class="icon" id="more" aria-label="More" aria-haspopup="menu" aria-expanded="false" aria-controls="menu"></button>
</header>

<div class="menu" id="menu" role="menu" hidden>
  <button role="menuitem" id="m-refresh">Refresh<kbd>⌘R</kbd></button>
  <button role="menuitem" id="m-settings">Settings…<kbd>⌘,</kbd></button>
  <hr>
  <button role="menuitem" id="m-report">Open full report</button>
  <hr>
  <div class="menu-note" id="m-updated"></div>
  <button role="menuitem" id="m-quit">Quit<kbd>⌘Q</kbd></button>
</div>

<section class="view" id="view-waiting" aria-live="polite">
  <p class="sub" id="waiting-sub"></p>
  <div class="limits" id="limits" hidden></div>
  <ul class="rows" id="live"></ul>
  <div class="empty" id="live-empty" hidden>
    <span data-icon="clock"></span>
    <b>Nothing is waiting on you</b>
    <span>Claude Code, Codex and Cursor sessions show up here when they finish or need approval.</span>
  </div>
  <details class="group" id="working-group" hidden><summary><span data-icon="arrow" data-size="s"></span>Working <span id="working-count"></span></summary><ul class="rows" id="working"></ul></details>
  <details class="group" id="idle-group" hidden><summary><span data-icon="arrow" data-size="s"></span>Idle <span id="idle-count"></span></summary><ul class="rows" id="idle"></ul></details>
</section>

<section class="view" id="view-today" hidden>
  <p class="sub" id="today-sub">Reading today's logs…</p>
  <ul class="rows" id="today-list"></ul>
  <div class="empty" id="today-empty" hidden>
    <span data-icon="sun"></span>
    <b>No agent work yet today</b>
    <span>Each project you work on with an agent today gets a line here, ready to copy into a standup.</span>
  </div>
  <pre class="polished" id="polished" hidden></pre>
  <div class="actions">
    <button class="btn primary" id="copy-recap" disabled></button>
    <button class="btn" id="polish" disabled></button>
  </div>
</section>

<section class="view" id="view-repos" hidden>
  <p class="sub">As of each repo's last fetch</p>
  <ul class="rows" id="repos"></ul>
  <div class="empty" id="repos-empty" hidden>
    <span data-icon="branch"></span>
    <b>No repositories yet today</b>
    <span>Every git repository an agent works in today shows up here with its branch and what needs doing.</span>
  </div>
</section>

<section class="view" id="view-wrapped" hidden>
  <div class="controls">
    <div class="seg" role="tablist" aria-label="Range">
      <button role="tab" data-days="7" aria-selected="true">7d</button>
      <button role="tab" data-days="30" aria-selected="false">30d</button>
    </div>
    <select id="agent" aria-label="Agent"><option>All agents</option></select>
    <select id="maker" aria-label="Model maker"><option>All models</option></select>
  </div>
  <div class="persona" id="persona">Reading your agent logs…</div>
  <div class="persona-line" id="line"></div>
  <div class="badges" id="badges"></div>
  <div class="tiles">
    <div class="tile"><div class="n" id="hours">–</div><div class="cap">agent-hours</div></div>
    <div class="tile"><div class="n" id="waited">–</div><div class="cap">hours waited on you</div></div>
    <div class="tile"><div class="n" id="limit-hits">–</div><div class="cap">limit hits</div></div>
    <div class="tile"><div class="n" id="parallel">–</div><div class="cap">agents at once, peak</div></div>
  </div>
  <div class="chart">
    <svg id="spark" class="spark" viewBox="0 0 300 36" preserveAspectRatio="none" aria-label="Agent-hours per day"></svg>
    <div class="axis"><span id="axis-from"></span><span id="axis-to"></span></div>
  </div>
  <div class="meta" id="meta"></div>
  <div class="report-row"><button class="link" id="report" disabled>Full report <span data-icon="arrow" data-size="s"></span></button></div>
</section>

<section class="view" id="view-settings" hidden>
  <div class="set-row">
    <span><b>Jump hotkey</b><small>Brings forward the session that has waited longest.</small></span>
    <button class="key" id="hotkey" title="Click, then press a new shortcut">–</button>
  </div>
  <p class="set-msg" id="hotkey-msg" aria-live="polite"></p>
  <label class="set-row">
    <span><b>Limit notifications</b><small>Once per window when you get close to a limit, and when a Claude limit resets. <button class="link" id="test-notify">Send a test</button></small></span>
    <input type="checkbox" id="notify">
  </label>
  <p class="set-msg" id="notify-msg" aria-live="polite"></p>
  <label class="set-row">
    <span><b>Polish recap with Claude</b><small>Lets Today send session titles, project names and file names to Claude through your own <code>claude</code> command. It uses your plan's usage.</small></span>
    <input type="checkbox" id="recap-claude">
  </label>
  <p class="set-msg" id="polish-msg" aria-live="polite"></p>
</section>

<p class="note" id="note" hidden></p>

<nav class="tabbar" role="tablist" aria-label="View" id="tabbar">
  <button role="tab" data-view="waiting" aria-selected="true"><span data-icon="clock"></span>Waiting<span class="tdot" id="wdot" hidden></span></button>
  <button role="tab" data-view="today" aria-selected="false"><span data-icon="sun"></span>Today</button>
  <button role="tab" data-view="repos" aria-selected="false"><span data-icon="branch"></span>Repos</button>
  <button role="tab" data-view="wrapped" aria-selected="false"><span data-icon="chart"></span>Wrapped</button>
</nav>
</body>
</html>
```

`<span data-icon="…">` placeholders are replaced with SVG by `panel.js` at start (Task 5), so the markup stays free of path data. The CSP already allows inline styles and `'self'` scripts; nothing new is loaded.

- [ ] **Step 2: Commit**

```bash
git add app/ui/index.html
git commit -m "Panel: header with menu, four views, bottom tab bar"
```

---

### Task 5: Wiring

**Files:**
- Modify: `app/ui/panel.js` (replace the whole file)

- [ ] **Step 1: Replace `app/ui/panel.js` with:**

```js
// The panel. Rust reads the logs; the same stats and report code as the CLI
// (synced into ./lib by scripts/sync-lib.mjs) turns them into numbers.
import { computeStats, filterSessions, filterOptions } from './lib/stats.mjs';
import { renderHtml, fmtNum, AGENT_NAMES } from './lib/render.mjs';
import { buildRecap } from './recap.js';
import { repoRow, sortRepos } from './repos.js';
import { icon, sprite } from './icons.js';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const STALE_MS = 5 * 60_000; // re-read logs when the panel opens after this long
const TABS = ['waiting', 'today', 'repos', 'wrapped'];
const TITLES = { waiting: 'Waiting', today: 'Today', repos: 'Repos', wrapped: 'Wrapped', settings: 'Settings' };
const $ = (id) => document.getElementById(id);
const cache = new Map(); // days -> { scan, at }: raw sessions, so filters need no re-read
let days = 7;
let busy = false;
const filter = { agent: null, maker: null };
let current = null; // { stats, meta } as shown, for the full report
let view = 'waiting';
let lastView = 'waiting';
const updatedAt = {}; // view -> ms of its last successful load
let prefs = { hotkey: 'ctrl+alt+KeyJ', notifyLimits: true, recapWithClaude: false };
const IS_MAC = /Mac/.test(navigator.platform);

// Icons: one sprite, then fill every placeholder.
document.body.insertAdjacentHTML('afterbegin', sprite());
for (const ph of document.querySelectorAll('[data-icon]')) ph.outerHTML = icon(ph.dataset.icon, ph.dataset.size || '');
$('more').innerHTML = icon('more');
$('back').innerHTML = icon('back');

// "ctrl+alt+KeyJ" -> "⌃⌥J" on macOS, "Ctrl+Alt+J" elsewhere.
function hotkeyLabel(acc) {
  const mac = { ctrl: '⌃', control: '⌃', alt: '⌥', option: '⌥', shift: '⇧', super: '⌘', cmd: '⌘', command: '⌘', meta: '⌘' };
  const win = { ctrl: 'Ctrl', control: 'Ctrl', alt: 'Alt', option: 'Alt', shift: 'Shift', super: 'Win', cmd: 'Win', command: 'Win', meta: 'Win' };
  const parts = acc.split('+');
  const key = parts.pop().replace(/^Key/, '').replace(/^Digit/, '');
  const mods = parts.map((m) => (IS_MAC ? mac : win)[m.toLowerCase()] || m);
  return IS_MAC ? mods.join('') + key : [...mods, key].join('+');
}

try {
  Object.assign(filter, JSON.parse(localStorage.getItem('filter') || '{}'));
  const saved = localStorage.getItem('view');
  view = TABS.includes(saved) ? saved : 'waiting';
} catch {
  // storage unavailable: start unfiltered, on Waiting
}

const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

// One line at the bottom for errors; empty and hidden otherwise.
function note(text = '') {
  $('note').textContent = text;
  $('note').hidden = !text;
}

function ago(ts) {
  if (!ts) return '';
  const m = Math.round((Date.now() - ts) / 60_000);
  if (m < 1) return 'Updated just now';
  if (m < 60) return `Updated ${m} min ago`;
  return `Updated ${Math.round(m / 60)} h ago`;
}

// --- Waiting ---

const AGENT_LABEL = { claude: 'Claude Code', codex: 'Codex', cursor: 'Cursor', antigravity: 'Antigravity' };
// From the log alone, a pending approval and a long-running tool look the
// same; after 30 minutes the backend calls it idle (stopped mid-tool).
const STATE_LABEL = { waiting: 'Done', approval: 'Approve', idle: 'Stopped', working: 'Working' };
const HOST_LABEL = { Claude: 'Claude app', 'Visual Studio Code': 'VS Code', iTerm: 'iTerm', iTerm2: 'iTerm', WindowsTerminal: 'Terminal' };

// Compact ages: "now", "47m", "17h", "2d".
function waitedFor(ms) {
  const m = Math.floor(ms / 60_000);
  if (m < 1) return 'now';
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h}h`;
  return `${Math.floor(h / 24)}d`;
}

function liveRow(s) {
  const li = el('li', 'dotted');
  const name = el('span', 'name');
  const needs = s.state === 'waiting' || s.state === 'approval';
  name.append(el('span', needs ? 'dot needs' : 'dot'), el('span', '', s.title || s.project || 'Untitled session'));
  const right = el('span', 'right num', s.state === 'working' ? STATE_LABEL.working : `${STATE_LABEL[s.state]} · ${waitedFor(Date.now() - s.since)}`);
  right.title = s.state === 'working' ? '' : `Since ${new Date(s.since).toLocaleString()}`;
  const where = (s.host && (HOST_LABEL[s.host] || s.host)) || AGENT_LABEL[s.agent] || s.agent;
  const line = el('span', 'line', [s.title && s.project, where, s.unread && 'unread'].filter(Boolean).join(' · '));
  li.append(name, right, line);
  if (needs) {
    li.tabIndex = 0;
    li.title = `Open in ${s.host || 'its app'}`;
    const go = async () => {
      try {
        await invoke('jump', { sessionId: s.sessionId });
        window.__TAURI__.window.getCurrentWindow().hide();
      } catch (e) {
        note(String(e));
      }
    };
    li.addEventListener('click', go);
    li.addEventListener('keydown', (e) => e.key === 'Enter' && go());
  }
  return li;
}

let lastLive = [];
async function loadLive() {
  let sessions;
  try {
    sessions = await invoke('live');
  } catch (e) {
    if (view === 'waiting') note(`Could not list sessions: ${e}`);
    return;
  }
  lastLive = sessions;
  updatedAt.waiting = Date.now();
  const needs = sessions.filter((s) => s.state === 'waiting' || s.state === 'approval');
  const idle = sessions.filter((s) => s.state === 'idle');
  const working = sessions.filter((s) => s.state === 'working');
  $('wdot').hidden = needs.length === 0;
  $('waiting-sub').textContent = needs.length
    ? `${plural(needs.length, 'session')} waiting · ${hotkeyLabel(prefs.hotkey)} jumps to the longest`
    : `${hotkeyLabel(prefs.hotkey)} jumps to the longest wait`;
  $('live').replaceChildren(...needs.map(liveRow));
  $('live-empty').hidden = needs.length > 0;
  // Idle and working sessions are folded away: they are not asking for you.
  for (const [name, list] of [['idle', idle], ['working', working]]) {
    $(name).replaceChildren(...list.map(liveRow));
    $(`${name}-count`).textContent = String(list.length);
    $(`${name}-group`).hidden = list.length === 0;
  }
}

const clock = (t) => new Date(t).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
const windowName = (m) => (m === 300 ? '5-hour' : m === 10080 ? 'weekly' : m ? `${Math.round(m / 60)}-hour` : '');

let limitsAt = 0;
async function loadLimits(force = false) {
  if (!force && Date.now() - limitsAt < 60_000) return;
  limitsAt = Date.now();
  let l;
  try {
    l = await invoke('limits');
  } catch {
    return; // limits are a nice-to-have; the session list still works
  }
  const box = $('limits');
  const rows = [];
  let tone = '';
  const c = l.claude;
  if (c) {
    tone = c.risk;
    const status =
      c.risk === 'limited' ? `Limited · resets ${clock(c.limitedUntil)}`
      : c.windowResets ? `resets about ${clock(c.windowResets)}`
      : 'no usage in 5 hours';
    const row = el('div', 'limit-row');
    row.append(el('span', 'limit-name', 'Claude · 5-hour limit'), el('span', 'limit-status', status));
    rows.push(row);
    // The dots only matter once this window has passed some earlier hit.
    if (c.pastHits > 0 && c.passed > 0 && c.risk !== 'limited') {
      // One dot per past limit hit; filled once this window has passed the
      // usage that came before it. No percentage: the limit is shared with
      // claude.ai, which leaves no local trace.
      const hits = el('div', 'hits');
      hits.title = 'Each dot is a past time you hit the 5-hour limit. Filled: this window has already used more than you had used then.';
      for (let i = 0; i < c.pastHits; i++) hits.append(el('i', i < c.passed ? 'on' : ''));
      hits.append(el('span', '', `past ${c.passed} of your ${c.pastHits} limit hits`));
      rows.push(hits);
    }
    if (c.advice) rows.push(el('div', 'advice', c.advice));
  }
  for (const w of l.codex || []) {
    const row = el('div', 'limit-row');
    const detail = [`${Math.round(w.usedPercent)}%`, w.minutesToFull && `full in ~${waitedFor(w.minutesToFull * 60_000)}`, w.resetsAt && `resets ${clock(w.resetsAt)}`].filter(Boolean).join(' · ');
    row.append(el('span', 'limit-name', `Codex · ${windowName(w.windowMinutes)}`.trim()), el('span', 'limit-status', detail));
    const meter = el('div', 'meter');
    const fill = el('b');
    fill.style.width = `${Math.min(100, w.usedPercent)}%`;
    meter.append(fill);
    rows.push(row, meter);
    if (w.usedPercent >= 80 && tone !== 'limited') tone = 'high';
  }
  box.className = `limits ${tone}`;
  box.replaceChildren(...rows);
  box.hidden = rows.length === 0;
}

// --- Navigation ---

function setView(v) {
  closeMenu();
  note();
  view = v;
  if (v !== 'settings') {
    lastView = v;
    try {
      localStorage.setItem('view', v);
    } catch {
      // not remembered across launches
    }
  }
  for (const b of document.querySelectorAll('#tabbar button')) b.setAttribute('aria-selected', String(b.dataset.view === v));
  for (const name of [...TABS, 'settings']) $(`view-${name}`).hidden = v !== name;
  $('title').textContent = TITLES[v];
  $('back').hidden = v !== 'settings';
  $('more').hidden = v === 'settings';
  if (v === 'settings') {
    for (const id of ['hotkey-msg', 'notify-msg', 'polish-msg']) $(id).textContent = '';
    return showSettings();
  }
  if (v === 'waiting') return loadLive(), loadLimits();
  if (v === 'today' || v === 'repos') return loadToday();
  return load();
}

function refresh() {
  if (view === 'waiting') return loadLive(), loadLimits(true);
  if (view === 'today' || view === 'repos') return loadToday(true);
  if (view === 'wrapped') return load(true);
}

function openMenu() {
  $('m-updated').textContent = ago(updatedAt[view]);
  $('m-updated').hidden = !updatedAt[view];
  $('menu').hidden = false;
  $('more').setAttribute('aria-expanded', 'true');
  $('m-refresh').focus();
}
function closeMenu() {
  $('menu').hidden = true;
  $('more').setAttribute('aria-expanded', 'false');
}

$('more').addEventListener('click', (e) => {
  e.stopPropagation();
  $('menu').hidden ? openMenu() : closeMenu();
});
document.addEventListener('click', (e) => {
  if (!$('menu').hidden && !$('menu').contains(e.target)) closeMenu();
});
$('m-refresh').addEventListener('click', () => (closeMenu(), refresh()));
$('m-settings').addEventListener('click', () => setView('settings'));
$('m-report').addEventListener('click', () => (closeMenu(), openReport()));
$('m-quit').addEventListener('click', () => invoke('quit'));
$('back').addEventListener('click', () => setView(lastView));
for (const b of document.querySelectorAll('#tabbar button')) b.addEventListener('click', () => setView(b.dataset.view));

let recordingHotkey = false;
document.addEventListener('keydown', (e) => {
  if (recordingHotkey) return;
  const mod = IS_MAC ? e.metaKey : e.ctrlKey;
  if (mod && e.key === 'r') return e.preventDefault(), refresh();
  if (mod && e.key === ',') return e.preventDefault(), setView('settings');
  if (mod && e.key === 'q') return e.preventDefault(), invoke('quit');
  if (e.key === 'Escape') {
    if (!$('menu').hidden) return closeMenu();
    if (view === 'settings') return setView(lastView);
    return window.__TAURI__.window.getCurrentWindow().hide();
  }
  const n = Number(e.key);
  if (!mod && !e.altKey && n >= 1 && n <= TABS.length && !(e.target instanceof HTMLInputElement)) setView(TABS[n - 1]);
});

// --- Wrapped ---

const hours = (h) => (h >= 10 || h === 0 ? Math.round(h).toString() : h.toFixed(1));

// One bar per day, oldest on the left. Days with no agent work get a faint
// stub so the time axis stays readable.
function spark(st) {
  const svg = $('spark');
  const n = st.window.days;
  const days = [];
  for (let i = 0; i < n; i++) {
    const d = new Date(st.window.until - (n - 1 - i) * 86_400_000);
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    days.push({ d, h: (st.minutesByDay[key] || 0) / 60 });
  }
  const max = Math.max(0.5, ...days.map((x) => x.h));
  const bw = 300 / n;
  const gap = n > 14 ? 1.5 : 4;
  svg.replaceChildren(...days.map(({ d, h }, i) => {
    const r = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
    const bh = h > 0 ? Math.max(3, (h / max) * 34) : 2;
    r.setAttribute('x', (i * bw + gap / 2).toFixed(1));
    r.setAttribute('y', (36 - bh).toFixed(1));
    r.setAttribute('width', Math.max(1, bw - gap).toFixed(1));
    r.setAttribute('height', bh.toFixed(1));
    r.setAttribute('rx', '1.5');
    if (h === 0) r.setAttribute('class', 'zero');
    const t = document.createElementNS('http://www.w3.org/2000/svg', 'title');
    t.textContent = `${d.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' })}: ${hours(h)} agent-hours`;
    r.append(t);
    return r;
  }));
  const fmt = (d) => d.toLocaleDateString([], { month: 'short', day: 'numeric' });
  $('axis-from').textContent = fmt(days[0].d);
  $('axis-to').textContent = 'today';
}

function fillSelect(sel, all, values, label, selected) {
  sel.replaceChildren(new Option(all, ''), ...values.map((v) => new Option(label(v), v)));
  sel.value = values.includes(selected) ? selected : '';
  sel.disabled = values.length < 2 && !selected;
}

// Applies the agent and maker filters to the cached scan and redraws.
function render() {
  const hit = cache.get(days);
  if (!hit) return;
  const { scan } = hit;
  const agents = filterOptions(scan.sessions).agents;
  if (filter.agent && !agents.includes(filter.agent)) filter.agent = null;
  const byAgent = filterSessions(scan.sessions, { agent: filter.agent });
  const makers = filterOptions(byAgent).makers;
  if (filter.maker && !makers.includes(filter.maker)) filter.maker = null;
  fillSelect($('agent'), 'All agents', agents, (a) => AGENT_NAMES[a] || a, filter.agent);
  fillSelect($('maker'), 'All models', makers, (m) => `${m} models`, filter.maker);

  const stats = computeStats(filterSessions(byAgent, { maker: filter.maker }), { since: scan.since, until: scan.until });
  const meta = { files: scan.files, bytes: scan.bytes, seconds: scan.seconds, filter: { ...filter } };
  current = { stats, meta };
  show({ stats, meta });
}

function show({ stats: st, meta }) {
  if (Object.values(meta.files).every((n) => !n)) {
    $('persona').textContent = 'No agent logs yet';
    $('line').textContent = `Nothing from Claude Code, Codex, Cursor, or Antigravity in the last ${days} days.`;
    $('badges').replaceChildren();
    $('report').disabled = true;
  } else if (st.sessions === 0) {
    $('persona').textContent = 'Nothing for this filter';
    $('line').textContent = 'No sessions you started match it in this range.';
    $('badges').replaceChildren();
    $('report').disabled = true;
  } else {
    $('persona').textContent = st.persona.name;
    $('line').textContent = st.persona.line;
    $('badges').replaceChildren(...(st.persona.badges || []).map((b) => {
      const s = el('span', '', b.name.replace(/^The /, ''));
      s.title = b.line;
      return s;
    }));
    $('report').disabled = false;
  }
  $('hours').textContent = hours(st.agentHours);
  $('waited').textContent = hours(st.waitHours);
  $('limit-hits').textContent = String(st.limitHits.length);
  $('parallel').textContent = String(st.peakParallel);
  // Cursor keeps no token counts locally: say so rather than show "0 tokens".
  const tokens = st.totalTokens ? `${fmtNum(st.totalTokens)} tokens` : 'tokens not recorded';
  $('meta').textContent = `${fmtNum(st.sessions)} sessions · ${fmtNum(st.filesEdited)} files · ${tokens}`;
  spark(st);
}

async function load(force = false) {
  const hit = cache.get(days);
  if (hit && !force && Date.now() - hit.at < STALE_MS) return render();
  if (hit) render();
  if (busy) return;
  busy = true;
  try {
    const scan = await invoke('scan', { days });
    cache.set(days, { scan, at: Date.now() });
    updatedAt.wrapped = Date.now();
    render();
  } catch (e) {
    note(`Could not read logs: ${e}`);
  } finally {
    busy = false;
  }
}

async function openReport() {
  if (!current) await load(true);
  if (!current) return;
  try {
    await invoke('open_report', { html: renderHtml(current.stats, current.meta) });
  } catch (e) {
    note(`Could not open report: ${e}`);
  }
}

for (const b of document.querySelectorAll('.seg button')) {
  b.addEventListener('click', () => {
    days = Number(b.dataset.days);
    for (const o of document.querySelectorAll('.seg button')) o.setAttribute('aria-selected', String(o === b));
    load();
  });
}
for (const key of ['agent', 'maker']) {
  $(key).addEventListener('change', () => {
    filter[key] = $(key).value || null;
    if (key === 'agent') filter.maker = null; // makers depend on the agent
    try {
      localStorage.setItem('filter', JSON.stringify(filter));
    } catch {
      // not remembered across launches; still applied now
    }
    render();
  });
}
$('report').addEventListener('click', openReport);

// --- Today and Repos (one read of today's logs feeds both) ---

let recap = null;
let polishedText = null;
let todayAt = 0;
let repos = [];

async function loadToday(force = false) {
  if (!force && recap && Date.now() - todayAt < 60_000) return renderToday(), renderRepos();
  try {
    const [scan] = await Promise.all([invoke('scan_today'), loadLive()]);
    const cwds = [...new Set(scan.sessions.map((s) => s.cwd).filter(Boolean))];
    // Git runs in parallel with itself, and alongside the commit lookup.
    const [commits, repoList] = await Promise.all([
      invoke('recap_commits', { cwds, since: scan.since }).catch(() => []),
      invoke('repo_status', { cwds }).catch(() => []),
    ]);
    repos = sortRepos(repoList.map(repoRow));
    recap = buildRecap({ sessions: scan.sessions, commits, live: lastLive });
    todayAt = Date.now();
    updatedAt.today = updatedAt.repos = todayAt;
    polishedText = null;
    renderToday();
    renderRepos();
  } catch (e) {
    $('today-sub').textContent = 'Could not read today’s logs';
    note(String(e));
  }
}

function renderToday() {
  const r = recap;
  const t = r.totals;
  const sub = $('today-sub');
  if (r.projects.length) {
    sub.replaceChildren(el('b', '', t.time), ` of agent work · ${plural(t.projects, 'project')} · ${plural(t.commits, 'commit')}`);
  } else {
    sub.textContent = '';
  }
  $('today-empty').hidden = r.projects.length > 0;
  $('today-list').replaceChildren(
    ...r.projects.map((p) => {
      const li = el('li');
      li.append(el('span', 'name', p.project), el('span', 'right strong num', p.time));
      if (p.titles.length) li.append(el('span', 'line', p.titles.join(' · ')));
      const counts = [p.commits.length && plural(p.commits.length, 'commit'), p.files && plural(p.files, 'file')].filter(Boolean);
      if (counts.length) li.append(el('span', 'line', counts.join(' · ')));
      return li;
    }),
  );
  $('polished').hidden = !polishedText;
  $('polished').textContent = polishedText || '';
  const copy = $('copy-recap');
  copy.disabled = !r.projects.length;
  copy.innerHTML = `${icon('copy', 's')}<span>${polishedText ? 'Copy polished' : 'Copy standup'}</span>`;
  const polish = $('polish');
  polish.disabled = !r.projects.length;
  polish.innerHTML = `${icon('spark', 's')}<span>Polish</span>`;
  polish.title = prefs.recapWithClaude ? 'Rewrites the recap with your own claude command' : 'Turn on "Polish recap with Claude" in Settings';
}

function renderRepos() {
  $('repos-empty').hidden = repos.length > 0;
  $('repos').replaceChildren(
    ...repos.map((r) => {
      const li = el('li', 'dotted');
      const name = el('span', 'name');
      name.append(el('span', `dot ${r.dot}`), el('span', '', r.name));
      const branch = el('span', 'right mono', r.branch);
      branch.title = r.path;
      li.append(name, branch, el('span', 'line', r.status));
      return li;
    }),
  );
}

$('copy-recap').addEventListener('click', async () => {
  const label = $('copy-recap').querySelector('span');
  try {
    await navigator.clipboard.writeText(polishedText || recap.text);
    label.textContent = 'Copied';
  } catch {
    label.textContent = 'Copy failed';
  }
  setTimeout(renderToday, 1500);
});

$('polish').addEventListener('click', async () => {
  if (!prefs.recapWithClaude) {
    setView('settings');
    lastView = 'today';
    $('polish-msg').textContent = 'Turn this on to polish the recap.';
    return;
  }
  const btn = $('polish');
  btn.disabled = true;
  btn.querySelector('span').textContent = 'Asking Claude…';
  try {
    polishedText = await invoke('polish_recap', { text: recap.text });
  } catch (e) {
    note(String(e));
  } finally {
    renderToday();
  }
});

// --- Settings ---

function showSettings() {
  $('hotkey').textContent = hotkeyLabel(prefs.hotkey);
  $('notify').checked = prefs.notifyLimits;
  $('recap-claude').checked = prefs.recapWithClaude;
}

async function saveSettings(next) {
  try {
    prefs = await invoke('set_settings', { next: { ...prefs, ...next } });
    $('hotkey-msg').textContent = '';
    return true;
  } catch (e) {
    $('hotkey-msg').textContent = String(e);
    return false;
  } finally {
    showSettings();
  }
}

$('notify').addEventListener('change', (e) => saveSettings({ notifyLimits: e.target.checked }));
$('test-notify').addEventListener('click', async (e) => {
  e.preventDefault();
  try {
    await invoke('test_notification');
    $('notify-msg').textContent = '';
  } catch (err) {
    $('notify-msg').textContent = `Could not notify: ${err}`;
  }
});
$('recap-claude').addEventListener('change', (e) => saveSettings({ recapWithClaude: e.target.checked }));

// Click the key, then press a shortcut. Needs a modifier unless it is F1–F24.
$('hotkey').addEventListener('click', () => {
  const btn = $('hotkey');
  btn.classList.add('recording');
  btn.textContent = 'Press keys…';
  $('hotkey-msg').textContent = '';
  recordingHotkey = true;
  const onKey = async (e) => {
    e.preventDefault();
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return; // wait for the real key
    document.removeEventListener('keydown', onKey, true);
    btn.classList.remove('recording');
    recordingHotkey = false;
    if (e.key === 'Escape') return showSettings();
    const mods = [e.ctrlKey && 'ctrl', e.altKey && 'alt', e.shiftKey && 'shift', e.metaKey && 'super'].filter(Boolean);
    if (!mods.length && !/^F\d{1,2}$/.test(e.code)) {
      $('hotkey-msg').textContent = 'Use at least one modifier key, such as ⌃, ⌥, ⇧ or ⌘.';
      return showSettings();
    }
    await saveSettings({ hotkey: [...mods, e.code].join('+') });
  };
  document.addEventListener('keydown', onKey, true);
});

listen('panel-shown', () => {
  loadLive(); // cheap: keeps the Waiting dot current on every tab
  if (view === 'waiting') loadLimits();
  if (view === 'wrapped') load();
  if (view === 'today' || view === 'repos') loadToday();
});
listen('open-report', async () => {
  if (!cache.get(days)) await load(true);
  openReport();
});
invoke('get_settings')
  .then((p) => (prefs = p))
  .catch(() => {})
  .finally(() => setView(view));
```

- [ ] **Step 2: Check nothing still refers to removed elements**

Run:
```bash
cd app && grep -nE "repoCard|wcount|today-wait|repos-head|#gear|'gear'|'updated'|'refresh'|'quit'" ui/panel.js ui/index.html
```
Expected: no output.

- [ ] **Step 3: Run the tests**

Run: `cd app && npm test`
Expected: PASS, all tests.

- [ ] **Step 4: Commit**

```bash
git add app/ui/panel.js
git commit -m "Panel: tab bar, ⋯ menu, Repos tab, quieter rows; keyboard 1-4, ⌘R, ⌘,"
```

---

### Task 6: Browser preview

A dev-only server so every screen can be checked without building the app. It is not part of `ui/`, so it never ships.

**Files:**
- Create: `app/scripts/preview.mjs`
- Modify: `app/package.json` (add a script)

- [ ] **Step 1: Create `app/scripts/preview.mjs`:**

```js
// Dev-only: serves app/ui in a browser with window.__TAURI__ stubbed, fed by
// real data from the Rust examples (today's sessions and repo status). Never
// shipped: the stub is injected by this server, not stored in ui/.
//   npm run preview        -> http://localhost:5174
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const ui = fileURLToPath(new URL('../ui/', import.meta.url));
const tauri = fileURLToPath(new URL('../src-tauri/', import.meta.url));
const run = (args) => execFileSync('cargo', ['run', '-q', '--release', '--example', ...args], { cwd: tauri, maxBuffer: 1 << 30 }).toString();

console.log('Reading today’s logs and repos with the Rust examples…');
const scan = JSON.parse(run(['dump', '--', '1']));
const cwds = [...new Set(scan.sessions.map((s) => s.cwd).filter(Boolean))];
const repos = JSON.parse(run(['repos', '--', ...cwds]));
const data = { scan, repos };

const stub = `
const data = ${JSON.stringify(data)};
const handlers = {
  scan_today: () => data.scan, scan: () => data.scan, repo_status: () => data.repos,
  recap_commits: () => [], live: () => [], limits: () => ({}),
  get_settings: () => ({ hotkey: 'ctrl+alt+KeyJ', notifyLimits: true, recapWithClaude: false }),
  set_settings: ({ next }) => next, open_report: () => null, jump: () => null, quit: () => null,
};
window.__TAURI__ = {
  core: { invoke: async (cmd, args) => { if (!handlers[cmd]) throw new Error(cmd + ' is not stubbed'); return handlers[cmd](args || {}); } },
  event: { listen: async () => () => {} },
  window: { getCurrentWindow: () => ({ hide() {} }) },
};`;

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css' };
createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^\/+/, '') || 'index.html';
  if (path === '__stub.js') return res.writeHead(200, { 'content-type': 'text/javascript' }).end(stub);
  try {
    let body = await readFile(join(ui, path));
    if (path === 'index.html') {
      body = body.toString()
        .replace('<script type="module"', '<script src="__stub.js"></script>\n<script type="module"')
        .replace('</head>', '<style>body{width:360px;height:560px;margin:24px auto;outline:1px solid #8884}</style></head>');
    }
    res.writeHead(200, { 'content-type': TYPES[extname(path)] || 'application/octet-stream' }).end(body);
  } catch {
    res.writeHead(404).end();
  }
}).listen(5174, () => console.log('Preview at http://localhost:5174'));
```

- [ ] **Step 2: Add the script**

In `app/package.json`, add to `"scripts"` (after `"sync"`):

```json
    "preview": "node scripts/sync-lib.mjs && node scripts/preview.mjs",
```

- [ ] **Step 3: Run it**

Run: `cd app && npm run preview` (leave running).
Expected: `Preview at http://localhost:5174` after the Rust examples finish (first run compiles, about a minute).

- [ ] **Step 4: Commit**

```bash
git add app/scripts/preview.mjs app/package.json
git commit -m "Preview: npm run preview serves the panel with real data in a browser"
```

---

### Task 7: Visual check against the spec

No code unless something is wrong. Use the browser at `http://localhost:5174` (360 × 560 frame). Check each item; fix anything that fails in the file named, re-run `npm test`, and commit the fix with a message saying what was wrong.

- [ ] **Step 1: Every tab, dark mode**

For Waiting, Today, Repos, Wrapped (click the tab bar, then press `1`–`4`):
- Title at top left, `⋯` at top right, no gear, no footer.
- Rows separated by hairlines, no bordered cards.
- Text is ink or grey only; colour appears only in dots and the 2 px limit line.
- Empty states show on Waiting (the stub returns no live sessions) with icon, bold line, grey sentence.
- Repos: rows sorted needs-you first; branch in monospace on the right; status in plain words ("26 commits to pull", "Clean"); nothing truncated except long names with an ellipsis.
- Today: no repo section, no "waiting on you" link; Copy standup (primary, light) and Polish (secondary) at the bottom.
- Wrapped: persona in ink (not coloured); four numbers in a 2 × 2 hairline grid; "Full report →" at the bottom.

- [ ] **Step 2: Menu and keyboard**

- `⋯` opens the menu with Refresh, Settings…, Open full report, an "Updated …" line, Quit; clicking outside or `Esc` closes it.
- `⌘,` opens Settings: back chevron + "Settings" title, `⋯` hidden; `Esc` or back returns to the previous tab.
- `⌘R` refreshes without reloading the page.

- [ ] **Step 3: Light mode**

Switch the OS (or the browser's emulated colour scheme) to light. Background `#F7F7F8`, ink near-black, primary button dark with white text, dots still visible.

- [ ] **Step 4: Run all suites**

Run:
```bash
cd app && npm test && cd src-tauri && cargo test --lib && cd ../../wrapped && npm test
```
Expected: all pass (app: icons, repos, recap; Rust 40; wrapped 16).

---

### Task 8: PR into dev

- [ ] **Step 1: Check the branch, push, open the PR**

```bash
cd /Users/akhilmisri/Business/Products/agent-island
git branch --show-current   # must print feat/panel-shell
git push -u origin feat/panel-shell
gh pr create --base dev --title "Panel redesign, phase 1: tab bar, menu, Repos tab, Silver design" --body "Phase 1 of docs/superpowers/specs/2026-10-04-panel-redesign-design.md.

- Silver tokens (light and dark), hairline rows, colour only as small state marks, one icon family.
- Bottom tab bar: Waiting, Today, Repos, Wrapped (Tasks comes with its data in phase 4).
- ⋯ menu replaces the gear and footer: Refresh, Settings, Open full report, Quit. Keyboard: 1-4, ⌘R, ⌘,, Esc.
- Repos moves out of Today into its own tab; status in plain words; merged and gone branches are one \"old branches\" count. Copy buttons removed; real actions come in phase 2.
- Empty states for Waiting, Today, Repos.
- Window 360 × 560.
- \`npm run preview\`: the panel in a browser with real data and a stubbed invoke (dev only).

Deferred, per plan: translucent window, Wrapped filters into the report, new settings rows."
```

The user merges the PR.

---

## Self-review notes

- Spec coverage for phase 1: tokens (Task 3), icon family (Task 1), tab bar and `⋯` menu (Tasks 4–5), Settings with Back (Tasks 4–5), empty states (Task 4), Repos in its own tab with plain status and combined old branches (Tasks 2, 5), Today recap-only (Task 5), Wrapped restyle (Tasks 3–5), keyboard (Task 5), 360 × 560 (Task 3), browser preview check in light and dark (Tasks 6–7). Deferred items are listed at the top with reasons.
- Names used across tasks: `icon`, `sprite`, `ICONS` (Task 1); `repoRow`, `sortRepos`, fields `name`, `path`, `branch`, `status`, `dot`, `canPull`, `oldBranches` (Task 2); element ids in Task 4 match every `$('…')` in Task 5.
