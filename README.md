<p align="center">
  <img src="app/src-tauri/icons/128x128.png" width="88" alt="Agent Island icon">
</p>

<h1 align="center">Agent Island</h1>

<p align="center">
  <b>See what your coding agents are doing, and what they cost you. Locally.</b><br>
  A menu-bar app for people who run Claude Code, Codex, and Cursor all day.
</p>

<p align="center">
  <img alt="macOS 12+" src="https://img.shields.io/badge/macOS-12%2B-111111?style=flat-square&logo=apple&logoColor=white">
  <img alt="Windows 10/11" src="https://img.shields.io/badge/Windows-10%2F11-0078D4?style=flat-square&logo=windows&logoColor=white">
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-core-CE422B?style=flat-square&logo=rust&logoColor=white">
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-2f2d28?style=flat-square"></a>
  <a href="https://github.com/akimisri17/agent-island/actions/workflows/app.yml"><img alt="Build" src="https://img.shields.io/github/actions/workflow/status/akimisri17/agent-island/app.yml?style=flat-square&label=build"></a>
</p>

<p align="center">
  <img src="docs/media/panel-light.png" width="300" alt="Agent Island menu-bar panel: persona, agent-hours, hours agents waited on you, limit hits, peak parallel agents">
  &nbsp;&nbsp;
  <img src="docs/media/panel-dark.png" width="300" alt="The same panel in dark mode">
</p>

<p align="center"><sub>Screenshots use made-up demo data (<code>--demo</code>).</sub></p>

## Why

If you run several agents at once, two things happen every day:

1. **An agent finishes or stops to ask you something, and you don't notice.** It sits there until you happen to look.
2. **You hit a usage limit in the middle of a task,** with no warning beforehand.

Usage meters show how much is left right now. Agent Island tracks the sessions themselves: how much work your agents did, how long they sat waiting for you, and where your limits went. It does this across agents, from data already on your machine. Nothing is sent anywhere.

## What you get today: Wrapped

Click the menu-bar icon for a quick panel. Open the full report for the rest.

- **A persona and two badges** picked from your own data: *The Conductor* (many agents at once), *The Absent Boss* (agents waiting on you), *The Polyglot* (several model makers), and 16 more.
- **Agent-hours**: how long your agents actually worked.
- **Hours your agents waited on you**: finished work sitting idle until your next prompt.
- **Usage-limit hits**, with when each limit reset.
- **Peak parallel agents**, busiest days, and the hour your agents work most.
- **An agents table**: sessions, prompts, hours, and responses for each agent.
- **Models grouped by maker** (Anthropic, OpenAI, xAI, Google, …).
- **Where the tokens went**: by project, by model, cache re-reads, subagents, and context compactions.
- **Filters**: one agent (Claude Code, Codex, Cursor) or one model maker (Anthropic, OpenAI, xAI, …) at a time, in the panel and the report.
- **A share card**: numbers only, with no project names, prompts, or paths.

<p align="center">
  <img src="docs/media/share-card.png" width="600" alt="Share card: I am The Conductor, with agent-hours, sessions, files touched, peak parallel agents, hours waited, and limit hits">
</p>

<details>
<summary><b>Full report preview</b></summary>
<br>
<p align="center"><img src="docs/media/report.png" width="720" alt="Full HTML report with persona, badges, headline numbers, waiting time, and daily chart"></p>
</details>

## Supported agents

| Agent | What it reads | Tokens | Status |
|---|---|---|---|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | ✅ | supported |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | ✅ | supported |
| **Cursor** | Cursor's local chat database `state.vscdb`, opened read-only | ✗ (Cursor doesn't store them on disk) | supported |
| Gemini CLI / Antigravity | `~/.gemini` | | next |
| GitHub Copilot CLI | `~/.copilot` | | next |
| Qwen Code, OpenCode | local session files | | planned |

Any model these agents call shows up automatically: Claude, GPT, Grok, Gemini, Qwen, DeepSeek, and so on. Browser chats (chatgpt.com, grok.com, claude.ai) keep no history on your machine, so they aren't included.

## Install

### Download

Installers for macOS (Apple silicon and Intel) and Windows are built by [GitHub Actions](https://github.com/akimisri17/agent-island/actions/workflows/app.yml) and will be published on the [Releases](https://github.com/akimisri17/agent-island/releases) page.

**The builds are not signed yet.** The first time you open it:

- **macOS:** right-click **Agent Island.app** → **Open** → **Open**. Or run:
  ```bash
  xattr -dr com.apple.quarantine "/Applications/Agent Island.app"
  ```
- **Windows:** on the SmartScreen prompt, click **More info** → **Run anyway**.

### Try it without installing

The same report, as a command (Node 20+, no dependencies):

```bash
git clone https://github.com/akimisri17/agent-island.git
cd agent-island
node wrapped/bin/agent-wrapped.mjs          # your last 30 days
node wrapped/bin/agent-wrapped.mjs --demo   # made-up data, no logs needed
```

Options: `--days 7`, `--agent cursor`, `--maker xAI`, `--out report.html`, `--json`, `--no-open`. An `npx agent-wrapped` package is coming.

### Build from source

Needs Node 20+ and Rust ([rustup](https://rustup.rs)).

```bash
cd app
npm install
npm run dev                          # run it
npm run build -- --bundles app,dmg   # macOS .app and .dmg
```

## Privacy

- **Nothing leaves your machine.** No account, no telemetry, no analytics, no server.
- **Read-only.** It reads agent logs and Cursor's database without changing them, and it never touches agent settings.
- **Not your prompts.** It counts messages and tools and reads timestamps and model names. Prompt text is never stored or shown. The report does show project names and the session titles your agents generate, blurred until you choose to reveal them.
- **No network.** The app's content security policy blocks network requests, and the HTML report loads no fonts or scripts from anywhere.
- **No screen recording, no Accessibility permission, no keychain access.**
- **Share safely.** The share card has numbers only. The report blurs project names until you untick the box.

## How it works

```
 ~/.claude/projects   ~/.codex/sessions   Cursor state.vscdb
          \                  |                  /
           Rust log readers (app)  ·  Node log readers (CLI)
                             |
            one record per session: prompts, turns,
            models, tools, files, limit hits
                             |
          stats.mjs  →  personas.mjs  →  render.mjs   (shared)
                 /                           \
       menu-bar panel                  HTML report + share card
```

- **Turns:** each prompt you type starts a turn, and the agent's activity extends it. Time from the end of a turn to your next prompt counts as *waiting on you*. Gaps over an hour count as you being away.
- **Agent-hours** are capped at 3 hours per turn, so a forgotten loop doesn't inflate them. Parallel sessions add up.
- **Sessions you started** are ones with at least one prompt you typed. Plugin-driven and subagent runs count toward tokens only.
- **Battery:** nothing runs in the background. Logs are read when the app starts and when you open the panel, at most once every 5 minutes. Reading about 6 GB of logs takes about 4 seconds.

The Rust readers and the Node readers are tested against the same sample files, and they give identical numbers on real logs.

## The honest caveat

None of these agents publish a stable format for their local logs. Agent Island reads what each tool writes for itself, and that can change in any release. When a format changes, a reader may undercount or skip a source. It should never invent numbers. Cursor is the most fragile: it's an internal database, and it has no token counts.

Windows installers build in CI, but the app hasn't been run on a real Windows machine yet. Reports from Windows users are very welcome.

Personas are judged against thresholds that are guesses for now. They'll be tuned once there's real data from more people.

## Roadmap

- [x] **Wrapped**: report, persona and badges, share card, menu-bar panel
- [x] Claude Code, Codex, and Cursor
- [ ] Gemini CLI / Antigravity, Copilot CLI, Qwen Code
- [ ] **Waiting**: one hotkey to jump to the session that has waited longest, in whatever terminal it's in
- [ ] **Limit coach**: warn before the weekly limit, with one suggested move (pause a session, switch model, wait for the reset)
- [ ] **Daily recap**: an end-of-day standup summary, written by your own installed `claude -p`
- [ ] Signed releases, Homebrew, `npx agent-wrapped`

## Repository layout

| Path | What |
|---|---|
| `app/` | The desktop app (Tauri): Rust readers, menu-bar panel. See [`app/README.md`](app/README.md) |
| `wrapped/` | Node readers, stats, personas, report, and the CLI. The app copies the shared code in at build time. See [`wrapped/README.md`](wrapped/README.md) |
| `docs/media/` | README images, made with `--demo` |
| `.github/workflows/app.yml` | Tests, then macOS and Windows installers |

## Contributing

Issues are the roadmap. The most useful ones:

- **A missing agent.** Say which tool you use and where it stores sessions.
- **A number that looks wrong.** Include the output of `node wrapped/bin/agent-wrapped.mjs --json` and what you expected. It has no prompt text, but it does list project names and session titles, so edit those out first.
- **A persona that doesn't fit you.**

Before sending a PR, run both test suites:

```bash
cd wrapped && npm test
cd app/src-tauri && cargo test --lib
```

## License

[MIT](LICENSE) © 2026 Akhil Misri
