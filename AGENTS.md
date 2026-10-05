# Agent guide

How to work in this repo. `CLAUDE.md` imports this file, so Claude Code, Codex and Cursor all read the same rules. Keep it under 150 lines; a test checks that, and that every path below exists.

## What this is

A local menu-bar app (macOS, Windows tray) that shows which coding-agent sessions are waiting on you, where your usage limits stand, and what your agents did. It reads logs already on the machine and sends nothing anywhere. See `README.md` for features.

## What this is not

- Not a way around a provider's limits. Read only what the person is already allowed to see.
- Not a cloud service, account, or telemetry of any kind.
- Not a window tiler, app switcher, or notch toy (music, files, clipboard).
- Not a prompt reader. Prompt text is never stored or shown.

## Layout

| Path | What |
|---|---|
| `wrapped/` | Node CLI and the source of truth for shared report code: `wrapped/src/stats.mjs`, `render.mjs`, `models.mjs`, `personas.mjs` |
| `wrapped/src/` | Node log readers: `claude.mjs`, `codex.mjs`, `cursor.mjs`, `antigravity.mjs` |
| `wrapped/test/fixtures/` | Sample logs shared by the Node and Rust tests |
| `app/src-tauri/src/` | Rust: log readers (`logs.rs`, `cursor.rs`, `antigravity.rs`, `foreign.rs`), live sessions (`live.rs`), limits, repos, tasks, recipes, approvals; Tauri commands in `lib.rs` |
| `app/src-tauri/examples/` | Dev tools that print real data, e.g. `dump.rs`, `live.rs`, `limits.rs` |
| `app/ui/` | Panel: `index.html`, `panel.js`, `panel.css`, one file per tab feature |
| `app/scripts/` | `sync-lib.mjs` (copies shared code), `preview.mjs`, `demo-data.mjs` |
| `docs/ARCHITECTURE.md` | How data flows, and the invariants below in full |
| `docs/RELEASE_NOTES.md` | Per-version notes |
| `docs/superpowers/` | Dated design specs and phase plans |
| `site/` | Project page (GitHub Pages from `main`) |

## Commands

```bash
cd wrapped && npm test                 # Node readers, stats, report
cd app && npm run sync && npm test     # panel logic (sync first: tests import ui/lib)
cd app/src-tauri && cargo test --lib   # Rust readers and features
cd app && DEMO=1 npm run preview       # panel in a browser on made-up data, no Rust needed
cd app && npm run dev                  # the real app
```

Run all three test suites before saying a change is done. CI (`.github/workflows/app.yml`) runs the same three on every PR.

Parity check after touching any reader, on real logs:

```bash
cd app/src-tauri && cargo run --release --example dump -- 30 > scan.json
```

Then compare with `node wrapped/bin/agent-wrapped.mjs --json`. Numbers must match.

## Invariants (stop and ask before breaking one)

1. **Nothing leaves the machine.** No network calls, no telemetry. The CSP blocks network. Only exception: the opt-in Polish button, which runs the user's own `claude -p`.
2. **Read-only unless the user clicks.** The only writes: Pull (`--ff-only`), deleting merged local branches, and Allow these… adding rules to a project's `.claude/settings.local.json`. Each shows what it will do and asks first. Never fetch in the background.
3. **Never write next to another app's files.** Other apps' SQLite is opened read-only/immutable; a test checks no files appear beside it.
4. **Never invent numbers.** If local data can't back a figure, don't show one (that's why there's no Claude percentage). Say "unknown" instead of guessing.
5. **Rust and Node agree.** Every reader exists in both and must give identical numbers on the same input. Change both, test both.
6. **Shared code lives in `wrapped/src/`.** `app/ui/lib/` is a generated copy (gitignored). Never edit it.
7. **Battery budget.** Full log scan at most once per 5 minutes, on open. The waiting count refreshes once a minute and must stay cheap (about 35 ms).
8. **No Accessibility, screen recording or keychain access.**

## Git and PRs

- Branch from `dev`. Feature branch → PR into `dev` (rebase-merge; `dev` needs linear history and a passing `test` check). Release: `dev` → `main` PR with a merge commit, then tag `app-v*`.
- Never commit to `main` or `dev` directly.
- Commit subjects: `Area: plain-language outcome`, sentence case, no `feat:` prefixes. Examples: `Repos: delete old branches after a confirm`, `Docs: agent guide`.
- No AI attribution in commits or PRs: no `Co-Authored-By` trailer, no "Generated with" line.
- User-visible change → add a line to `docs/RELEASE_NOTES.md` under the next version, in the same PR. Feature or command change → update `README.md` and `app/README.md` in the same PR.
- Screenshots and the site use demo data only (`DEMO=1`). Never commit a real report or real project names.

## Writing style (UI, docs, commits)

Plain words. Short sentences. Say what the user sees, not how it's built. Be honest about what is estimated.

## Bigger changes

New tab, new reader, or anything touching an invariant: write a short spec in `docs/superpowers/specs/YYYY-MM-DD-<topic>.md` (why, what changes, out of scope, testing), then a plan, then build. One PR per phase.

## Sessions

- **Start:** read this file and `docs/ARCHITECTURE.md`. Maintainers: the newest handoff is in `docs/internal/HANDOFF-*.md` in the primary checkout (private and gitignored, so git worktrees don't have it).
- **End of a long session:** write a handoff: state, the next jobs in order with files and tests, and what's later. Short is fine.
- **This file goes stale:** if anything here is wrong, fix it in the same PR. A wrong guide is worse than none.
