# Agent Island desktop app

The agent-island product: a menu-bar app (macOS) and tray app (Windows) that watches your coding agents locally. One app, several views on the same local session data.

| View | Status |
|---|---|
| **Wrapped**: what your agents did in the last 7 or 30 days, persona and badges, full HTML report | built |
| **Waiting**: ⌃⌥J to the session that has waited longest, count in the menu bar | built (macOS jump) |
| **Limits**: exact reset when limited, this window against your past limit hits, official Codex percentages, one move | built |
| **Today**: daily recap per project with today's commits; copy for standup; optional polish by your own `claude -p` | built |
| **Repos**: branch and plain-language status per repository, needs-you first; Pull (fast-forward only, confirmed), delete old branches (merged or gone from remote), open a terminal | built |
| **Recipes**: saved kick-off prompts per repository; Start opens a new Claude session in your terminal; prompts typed 3+ times are suggested | built |
| **Resume**: sessions cut off by a Claude limit (or interrupted) listed on Waiting after the reset; Resume jumps to them or runs `claude --resume` | built |
| **Tasks**: scheduled Claude tasks, missed / failed / stopped / running, last seven days, Run now, optional notification | built |
| **Approvals**: in Wrapped, where tool calls had to ask per project (estimate), median wait; Allow these… writes vetted build/test rules to `<project>/.claude/settings.local.json` after you confirm | built |

The `wrapped/` CLI at the repo root is the same Wrapped view without installing anything (`npx` later). It shares this app's stats and report code. It is a way in, not a second product.

Wrapped reads logs when the panel opens, at most once every 5 minutes. The waiting count refreshes once a minute (about 35 ms). No network access.

To see live sessions from a terminal: `cd src-tauri && cargo run --example live`.

To work on the panel in a browser: `npm run preview` (http://localhost:5174) serves `ui/` with the Tauri calls stubbed and fed by your real logs through the Rust examples. `DEMO=1 npm run preview` uses made-up data instead (`scripts/demo-data.mjs`) and needs no Rust; the README screenshots come from it. URL options: `?view=repos` opens a tab, `?shot=1` drops the frame, `?scroll=bottom`, `?click=<selector>`.

## How it fits together

| Part | Where | Job |
|---|---|---|
| Log readers | `src-tauri/src/logs.rs`, `cursor.rs`, `antigravity.rs`, `foreign.rs` | Rust ports of `wrapped/src/claude.mjs`, `codex.mjs`, `cursor.mjs`, `antigravity.mjs`, `sqlite.mjs`, `protobuf.mjs`. Reads about 6 GB in about 4 s |
| Stats, personas, report | `ui/lib/` (copied from `wrapped/src/`) | The same code the CLI uses. Do not edit the copies |
| Panel | `ui/index.html`, `panel.js`, `panel.css` | The popover |
| Live sessions | `src-tauri/src/live.rs` | Running `claude`/`codex` processes → session log → state (finished, tool or approval, working); Cursor chats from its database while Cursor runs; jump via AppleScript (Terminal, iTerm), `open -a` (editors, other apps) |
| Limits | `src-tauri/src/limits.rs` | Claude: usage per message (a cost-like weighting) and logged limit hits; Codex: official `rate_limits`. `cargo run --release --example limits` |
| Recap | `ui/recap.js`, `src-tauri/src/recap.rs` | Today's sessions grouped by project (tested in `test/recap.test.mjs`); git commits by the repo's own author; `claude -p --no-session-persistence --setting-sources project --tools ""` from a temp folder |
| Shell | `src-tauri/src/lib.rs` | Tray icon and badge, popover window, global hotkey, `scan`, `live`, `jump`, `open_report` commands |

`scripts/sync-lib.mjs` copies the shared JS before every dev run and build.

## Agents read

| Agent | Source | Tokens |
|---|---|---|
| Claude Code | `~/.claude/projects/**/*.jsonl` | yes |
| Codex | `~/.codex/sessions/**/*.jsonl` | yes |
| Cursor | `Cursor/User/globalStorage/state.vscdb` (SQLite, read-only) | not stored locally |
| Antigravity CLI | `~/.gemini/antigravity-cli/conversations/*.db` (SQLite + protobuf, read-only) | not reliably readable |

Models are compared by number of responses, since Cursor and Antigravity have no usable token counts. Other apps' SQLite files are opened immutable unless the owner app is live, so nothing is written next to them. Gemini CLI, Copilot, and Qwen Code are next.

## Run and build

Needs Node 20+ and Rust (`rustup`).

```bash
npm install
npm run dev
npm run build -- --bundles app,dmg
cd src-tauri && cargo test --lib
```

The parser tests use the same fixtures as the CLI tests (`wrapped/test/fixtures`).

To check the Rust parser against the JS one on real logs:

```bash
cd src-tauri && cargo run --release --example dump -- 30 > scan.json
```

Windows installers (`.msi`, `.exe`) are built by `.github/workflows/app.yml` on a Windows runner. Run it from the Actions tab, or push a tag like `app-v0.1.0`.

## Before strangers can install it

- **macOS:** sign with an Apple Developer ID ($99/yr) and notarize. Unsigned builds are blocked by Gatekeeper. Right-click → Open works for testing.
- **Windows:** sign with a code-signing certificate (for example Azure Trusted Signing). Unsigned builds show a SmartScreen warning.
- Both are configured as `tauri-action` secrets in the workflow when ready.
