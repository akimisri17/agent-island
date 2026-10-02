# agent-island

A free, local utility for people who run coding agents all day. It watches Claude Code, Codex, and Cursor sessions on your machine, tells you which one is waiting on you, and warns you before an account runs out. Nothing leaves the laptop.

One product, one app (`app/`). Its first view is **Wrapped**: what your agents did in the last 7 or 30 days, built from the logs already on disk. The same view runs without installing anything, as a command:

```bash
node wrapped/bin/agent-wrapped.mjs
```

## What's here

| Path | Role |
|---|---|
| `app/` | The Agent Island app: menu bar (macOS), tray (Windows). Tauri. See `app/README.md` |
| `wrapped/` | The Wrapped view as a zero-install command, plus the stats and report code the app shares. Node 20+ |
| `docs/internal/` | Strategy docs (one-pager, research, market, plans). Private, gitignored, its own repo |
| `reference/coucou/` | Shallow local clone for reading. Gitignored. Do not commit it |

## Coucou

The session-bus ideas come from studying [Coucou](https://github.com/Louis-CFM/coucou) (pinned at `341b86a`, 2026-10-01). Its code is MIT. The name Coucou, the Mochi character, the icon, the sounds, and the media are reserved by its author (`LICENSE-ASSETS.md`), so nothing here uses them.

Refresh the study copy:

```bash
rm -rf reference/coucou && git clone --depth 1 https://github.com/Louis-CFM/coucou.git reference/coucou
```

## Privacy

Generated reports (`agent-wrapped*.html`) contain your own project names and usage. They are gitignored. Do not commit one.
