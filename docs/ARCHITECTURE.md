# How Agent Island works

For people and agents changing the code. Features are in `README.md`; per-part detail is in `app/README.md`. This file covers how data moves and the rules that hold the system together.

## Two programs, one set of numbers

- **`wrapped/`** is a Node CLI. It reads agent logs and prints or writes the Wrapped report.
- **`app/`** is the Tauri app. Rust reads the logs; the panel (plain JS in `app/ui/`) draws them.

The report logic (`stats.mjs`, `personas.mjs`, `render.mjs`, `models.mjs`) is written once in `wrapped/src/`. `app/scripts/sync-lib.mjs` copies it into `app/ui/lib/` before every dev run, build, preview and CI test. The copy is gitignored and never edited.

The log readers exist twice, in Node (`wrapped/src/*.mjs`) and in Rust (`app/src-tauri/src/logs.rs`, `cursor.rs`, `antigravity.rs`, `foreign.rs`). Both are tested on the same fixtures in `wrapped/test/fixtures/` and must give identical numbers on real logs (`cargo run --release --example dump`).

## End-to-end path

```
~/.claude/projects   ~/.codex/sessions   Cursor state.vscdb   ~/.gemini (Antigravity)
          \                 |                  |                  /
        Rust readers (app)  ·  Node readers (CLI)        read-only, nothing written
                            |
         one record per session: prompts, turns, models, tools, files, limit hits
                            |
          stats.mjs → personas.mjs → render.mjs   (shared)
                /                              \
      menu-bar panel (app/ui)          HTML report + share card
```

1. **Scan.** The panel calls the `scan` command when it opens, at most once every 5 minutes (`STALE_MS` in `app/ui/panel.js`). Rust walks the log folders and returns session records.
2. **Stats.** The panel runs the shared `stats.mjs` on those records: turns, agent-hours (capped at 3 h per turn), time waiting on you, limit hits, tokens.
3. **Live.** Separately, `live` runs about once a minute: list running `claude` and `codex` processes, match each to its session log by working folder or `--resume` id, read only the end of that log, and decide finished / approval-or-long-tool / idle / working. Cursor chats come from its database while Cursor runs. This drives the Waiting tab and the menu-bar count.
4. **Jump.** `jump` brings the session forward: AppleScript for Terminal and iTerm tabs, `open -a` for editors and other apps.
5. **Everything else** is a Tauri command in `app/src-tauri/src/lib.rs` backed by its own module: `limits`, `scan_today` + `recap_commits` (Today), `repo_status` + `repo_pull` + `repo_delete_branches` (Repos), `recipes_for` + `start_recipe` (Recipes), `cut_off` + `resume_session` (Resume), `tasks_board` + `run_task` (Tasks), `approvals` + `allow_rules` (Approvals), `polish_recap` (opt-in Polish).

## State

- **Source of truth:** the agents' own log files and databases. Agent Island owns none of it and never changes it.
- **Ours:** `settings.json` (hotkey, terminal app, notifications, Polish on/off) and `recipes.json` in the app config folder, plus the panel's own storage for hidden cut-off rows (last 200).
- **In memory only:** scan results and short caches (Claude limit data 5 min, task board 1 min).
- **Never stored:** prompt text, credentials, anything from the network.

## Where the code is heavy

- `app/ui/panel.js` (about 1,100 lines): all tabs' rendering and refresh timing. Read the refresh guards before adding a new fetch.
- `app/src-tauri/src/logs.rs` and `live.rs` (about 800 and 750 lines): log parsing and live-state rules. Formats are undocumented and change between agent releases; keep parsing tolerant and add a fixture for every new shape.
- `app/src-tauri/src/lib.rs`: the command list and window/tray code, including the macOS panel conversion.
- Dedupe rules: resumed and forked Claude sessions copy earlier lines; count each prompt and message id once (fixtures in `wrapped/test/fixtures/fork/`). Scheduled tasks and injected text are not human prompts.

## Invariants

The short list is in `AGENTS.md`. The reasons:

1. **Nothing leaves the machine.** The app's CSP (`app/src-tauri/tauri.conf.json`) allows only IPC. The trust promise is the product; one network call breaks it.
2. **Read-only unless the user clicks.** Three confirmed writes only: Pull (fast-forward only, never a merge), deleting local branches that are merged or gone from the remote, and allow-rules into `<project>/.claude/settings.local.json`. No background `git fetch`.
3. **Never write next to another app's files.** `foreign.rs` opens other apps' SQLite with `immutable=1` unless the owner app is live; a test checks that no `-wal`/`-shm` files appear.
4. **Never invent numbers.** Claude's logs record only the moment a limit is hit, and the 5-hour limit is shared with chats that leave no trace, so there's no Claude percentage. Estimates are labelled as estimates.
5. **Rust and Node agree.** A reader change lands in both, with a fixture, in the same PR.
6. **One source for shared code:** `wrapped/src/`.
7. **Cheap when idle.** Full scan (about 6 GB in about 4 s) only on open, at most every 5 minutes; live refresh about 35 ms a minute.
8. **No extra permissions.** No Accessibility, screen recording or keychain. Jumping to a Terminal/iTerm tab uses that app's automation permission only.

## Adding an agent

1. Add a fixture of its real log shape to `wrapped/test/fixtures/` (scrub prompt text).
2. Write the Node reader in `wrapped/src/` with tests, then the Rust port with tests on the same fixture.
3. Wire it into the scan in both, then run the parity check on real logs.
4. Update the agents table in `README.md` and `app/README.md`, and add a release note.
