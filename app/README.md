# Agent Island desktop app

The agent-island product: a menu-bar app (macOS) and tray app (Windows) that watches your coding agents locally. One app, several views on the same local session data.

| View | Status |
|---|---|
| **Wrapped**: what your agents did in the last 7 or 30 days, persona and badges, full HTML report | built |
| **Waiting**: one hotkey to the session that has waited longest | next |
| **Coach**: forecast before the weekly limit, with one recommended move | planned |
| **Recap**: end-of-day standup summary, written by your own `claude -p` | planned |

The `wrapped/` CLI at the repo root is the same Wrapped view without installing anything (`npx` later). It shares this app's stats and report code. It is a way in, not a second product.

Nothing runs in the background. Logs are read when the panel opens, and again after it has been closed for 5 minutes or more. No network access.

## How it fits together

| Part | Where | Job |
|---|---|---|
| Log readers | `src-tauri/src/logs.rs`, `cursor.rs` | Rust ports of `wrapped/src/claude.mjs`, `codex.mjs`, `cursor.mjs`. Reads about 6 GB in about 4 s |
| Stats, personas, report | `ui/lib/` (copied from `wrapped/src/`) | The same code the CLI uses. Do not edit the copies |
| Panel | `ui/index.html`, `panel.js`, `panel.css` | The popover |
| Shell | `src-tauri/src/lib.rs` | Tray icon, popover window, `scan` and `open_report` commands |

`scripts/sync-lib.mjs` copies the shared JS before every dev run and build.

## Agents read

| Agent | Source | Tokens |
|---|---|---|
| Claude Code | `~/.claude/projects/**/*.jsonl` | yes |
| Codex | `~/.codex/sessions/**/*.jsonl` | yes |
| Cursor | `Cursor/User/globalStorage/state.vscdb` (SQLite, read-only) | not stored locally |

Models are compared by number of responses, since Cursor has no local token counts. Gemini, Copilot, and Qwen Code are next.

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
