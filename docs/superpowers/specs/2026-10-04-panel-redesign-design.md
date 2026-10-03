# Panel redesign: five tabs, quieter design, actions that run

Date: 2026-10-04. Status: approved in brainstorming; awaiting spec review.
Mockups: `.superpowers/brainstorm/*/content/screens-a.html` (layout) and `colors-quiet.html` (palette B, Silver).

## Why

The Today tab had grown to hold the recap, the Repo board and a link to Waiting. Copy-to-clipboard buttons ("Copy cleanup", "Copy gone cleanup") described git mechanics instead of outcomes. The gear icon was a hand-drawn path at a different stroke weight from everything else, and colour was used decoratively. The next features from `docs/internal/ROUTINE-2026-10.md` (resume after a limit, scheduled-task monitor, recipes, approval friction) had no place to go.

## Foundations

Every screen follows these.

**One question per tab.**

| Tab | Question |
|---|---|
| Waiting | Who needs me right now? |
| Today | What got done today? |
| Repos | Is my code in a good state? |
| Tasks | Did my automations run? |
| Wrapped | How am I working, over weeks? |

Anything that does not answer the tab's question goes elsewhere.

**Principles.**
- Hierarchy from type, not boxes: rows separated by hairlines; containers only for sheets and menus.
- Colour means state and appears only as small marks (status dots, tab dots, the limit line). Text is never coloured.
- Plain words, outcomes first ("26 commits to pull", "4 old branches", "Missed").
- Actions appear only where they apply; at most one primary and one secondary per row.
- Calm: tab badges are dots, not counts; one motion (sheets slide in, 180 ms ease-out); no spinner under 300 ms.
- Trust before doing: any action that changes something shows the exact command in a confirm sheet, then shows the result in the row.
- Native: system font, follows light/dark, rows at least 44 px, keyboard: `1`–`5` switch tabs, `⌘R` refresh, `⌘,` settings, `Esc` closes a sheet or hides the panel.
- Every tab has an empty state: one line on what appears there and why it is empty.
- One icon family: line icons, 1.6 px stroke, 18 px in the tab bar, 14 px inline. Inline SVG symbols, no icon font.

**Tokens (dark).**

| Token | Value | Use |
|---|---|---|
| `--bg` | `#141414` (translucent on macOS) | panel |
| `--ink` | `#E6E6E6` | names, numbers |
| `--meta` | `#8E8E8E` | secondary text |
| `--line` | `#212121` | hairlines |
| `--fill` | `#262626` | secondary buttons, meter track |
| `--primary` | `#E4E6EA` (Silver) | selected tab, primary buttons (text `#141414`) |
| `--needs` | `#D2AE6E` | needs-you dot, limit line |
| `--done` | `#90BBA4` | done/clean dot |
| `--failed` | `#C46F6F` | failed/missed dot |

Light mode mirrors: `--bg #F7F7F8`, `--ink #1C1C1E`, `--meta #6E6E73`, `--line #E5E5EA`, `--fill #ECECEF`, `--primary #1C1C1E` (text white); status colours darkened ~15% for contrast on white.

Type: three sizes, two weights. Title 17/650, body 13/400 with names at 13/600, meta 11.5/400. Branch names in the system monospace at 11. Times and counts use tabular figures.

Space: 4 px grid (4/8/12/16/24). 16 px side gutter. Radius 8 for controls, 12 for sheets. Panel 360 × 560.

## Structure

A bottom tab bar with five tabs (icon + label): Waiting, Today, Repos, Tasks, Wrapped. Each tab has a title at the top left and a `⋯` button at the top right. The footer (Refresh, Quit) and the gear are removed.

`⋯` menu: Refresh (⌘R), Settings… (⌘,), Open full report, "Updated N min ago" (disabled label), Quit (⌘Q).

Tab dots: Waiting shows a `--needs` dot when anything waits; Tasks shows a `--failed` dot when a run was missed or failed. The count is in the tab's own content, not on the dot.

## Tabs

**Waiting.** Limit line at the top (label, percentage and reset time in grey; 2 px line in `--needs`). Sessions that need you, oldest first: name, project, "Approve · 12m" or "Done · 40m". Clicking a row jumps to its window (as now). A "Cut off by the limit" section appears only after a Claude limit resets and lists sessions the limit stopped or that the person interrupted, each with **Resume**. Working and Idle collapse to one line that expands.

**Today.** Total ("5.9 h of agent work · 4 projects · 3 commits"), then one row per project: name, time on the right, what it was about underneath, commits/files when present. Agent names are dropped from rows. Bottom: **Copy standup** (primary) and **Polish** (secondary). Nothing else.

**Repos.** The Repo board (already built in #24/#25) as its own tab. Subtitle "As of each repo's last fetch". Rows sorted needs-you first: dot, name, branch (mono), one status line ("30 uncommitted · not pushed yet", "2 to push · 1 stash · 4 old branches", "Clean"). The selected or hovered row shows its actions: **Pull** (only when behind and nothing is uncommitted), **Terminal**, **Recipes**. "4 old branches" is a link to the old-branches sheet. Merged and gone-upstream branches are combined into one "old branches" count.

**Tasks.** Scheduled Claude tasks, problems first. Row: dot, task name, state on the right ("Missed", "Running · 4m", "6:00 pm · 3m"), schedule line, seven day-dots (filled ran, ringed missed, grey before the task existed). Missed or failed rows offer **Run now** and **Last run**.

**Wrapped.** 7d/30d control at the top right. Persona line. Four numbers in a 2×2 grid divided by hairlines. "Where you approve the most": one row per project with approval count, the tools that caused them, average wait, and **Allow these…** on the top row. "Full report →" opens the report as now. Agent and model filters move into the report.

**Settings** (from `⋯`, with Back): jump hotkey, terminal app (auto-detected, with a picker), notify near limits, notify missed tasks, polish with Claude, launch at login.

## Actions

Two kinds, chosen per action:

| Action | Runs | How |
|---|---|---|
| Pull | in the app | confirm sheet with `git pull --ff-only`; result line in the row |
| Delete old branches | in the app | sheet lists branches with reason ("merged", "gone from GitHub"); **Delete N** runs `git branch -d` for merged and `git branch -D` for gone; local only |
| Terminal | new terminal tab | opens a tab in the repo folder |
| Resume | new terminal tab | `claude --resume <session id>` in the session's folder |
| Recipe Start | new terminal tab | `claude "<prompt>"` in the repo folder |
| Run now (task) | new terminal tab | runs the scheduled task's prompt with `claude` in its folder |
| Allow these… | in the app | sheet shows the exact permission rules and the settings file they go into; writes only on confirm |

In-app git runs on a background thread, never with a shell, with `GIT_TERMINAL_PROMPT=0`. Any non-zero exit shows git's first error line in the row in `--meta` with a `--failed` dot; nothing is retried.

Terminal tabs: macOS supports Terminal, iTerm2, Ghostty and Warp via their scripting interfaces (AppleScript / URL schemes); the first use triggers the macOS Automation prompt once. Windows uses Windows Terminal (`wt -d <dir>`). If no supported terminal is found, the action falls back to copying the command and says so.

## New data

- **Cut off by the limit:** a Claude session whose last user-visible event is a limit rejection (`quotaLimits.status = rejected`) or an interruption marker, with no human prompt after it, and whose limit has since reset. Read from the existing logs scan.
- **Scheduled tasks:** task names from `~/.claude/scheduled-tasks/*/SKILL.md`; runs from session logs whose first prompt starts `<scheduled-task name="…">` (already detected for the counting fix). A run "failed" when its session ends with an error or a limit rejection. Expected cadence is inferred from past run times (median gap); a run is "Missed" when the time since the last run exceeds 1.5 × that gap.
- **Recipes:** stored in the app's config dir as JSON, per repository path. Suggestions: human prompts repeated 3+ times in the same repository (normalised whitespace), first 80 characters shown.
- **Approval friction:** per session, tool calls that were followed by a permission prompt and a human reply; count, tool names and wait per project. The exact log shape is confirmed during planning before this is built.

## Phasing

Each phase is its own plan and PR into `dev`.

1. **Shell and design system:** tokens, icon set, bottom tab bar, `⋯` menu, settings screen, empty states; move Repos into its tab; restyle Waiting, Today, Wrapped to the new rows. No new data.
2. **Repo actions:** Pull, old-branches sheet with Delete, Terminal; terminal launcher with Settings picker.
3. **Resume after the limit.**
4. **Tasks tab.**
5. **Recipes.**
6. **Approval friction.**

## Testing

- Rust unit tests for every parser and git action against temporary repositories (as `repos.rs` does now), including failure paths (pull that would need a merge, delete of a branch checked out elsewhere).
- JS tests for every row formatter (plain-language status lines, tab dots, sort order).
- Visual check of each tab in the browser preview with a stubbed `invoke`, at 360 × 560, in light and dark.
- Rust and Node readers keep giving identical numbers.

## Out of scope

Fetching from remotes, pushing, committing or switching branches from the app; editing recipes from anywhere but the app; any network call other than what `claude` itself makes.
