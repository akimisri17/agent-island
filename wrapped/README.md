# agent-wrapped

The Wrapped view of Agent Island as a zero-install command: a report of what your coding agents did in the last 30 days, built from the data already on your machine. The desktop app in `../app` uses the same stats and report code. It writes one HTML file and opens it. Nothing is sent anywhere, and the page loads no fonts or scripts from the network.

```bash
node bin/agent-wrapped.mjs
```

Options: `--days 7`, `--out report.html`, `--json` (stats only), `--no-open`.

## What it reads

| Agent | Where |
|---|---|
| Claude Code | `~/.claude/projects/**/*.jsonl` (or `$CLAUDE_CONFIG_DIR/projects`) |
| Codex | `~/.codex/sessions/**/*.jsonl` (or `$CODEX_HOME/sessions`) |
| Cursor | `Cursor/User/globalStorage/state.vscdb`, read-only (needs Node 22.13+ for `node:sqlite`; skipped otherwise) |

Cursor stores no token counts locally, so models are compared by number of responses. Gemini, Copilot, and Qwen Code are not read yet.

## What the numbers mean

- **Agent-hours**: time from your prompt to the agent's last action in that turn, capped at 3 hours. Parallel sessions add up, so a day can exceed 24.
- **Waited on you**: time between an agent finishing and your next prompt in the same session. Gaps over an hour count as you being away.
- **Sessions you started**: sessions with at least one prompt you typed. Plugin- or script-driven runs (for example claude-mem observers) and subagents count toward tokens only.
- **Personas**: each scores how far past its threshold your data is; the strongest is the title, the next two are badges. Thresholds are guesses until real cards show the spread.
- **Usage limits**: Claude `quotaLimits` with status `rejected`, and Codex `rate_limit_reached_type`, deduplicated per reset window.

The share card shows numbers only. No project names, prompts, or paths.

## Why this exists

It is the cheapest test of the Agent Island hook: do people find their own agent data surprising enough to share? Watch for shares, stars, and "can it also show me…" issues.

```bash
npm test
```
