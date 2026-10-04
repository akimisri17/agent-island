# Release notes

## Unreleased (since app-v0.1.0)

### New

- **Five tabs** along the bottom of the panel: Waiting, Today, Repos, Tasks and Wrapped. Keys 1 to 5 switch between them.
- **Repos tab:** one row per repository agents worked in today, with the branch and a plain-language status (*5 commits to pull*, *4 uncommitted · 2 to push*, *3 old branches*). Repositories that need you come first.
- **Repo actions** on the selected row:
  - **Pull**, fast-forward only, after a confirm sheet that shows the exact command.
  - **Delete N old branches**: local branches merged into the main branch or gone from the remote, after you confirm.
  - **Terminal** opens your chosen terminal app in the repository.
- **Recipes:** saved kick-off prompts per repository. Start opens a new Claude session with the prompt in your terminal. Prompts you've typed three or more times in a repository are suggested.
- **Cut off by the limit** on the Waiting tab: after a Claude limit resets, the sessions it stopped (and sessions you interrupted) are listed with **Resume**, which jumps to the session if it's open or runs `claude --resume` in a terminal, and **Hide**.
- **Tasks tab:** scheduled Claude tasks marked Missed, Failed, Stopped (stuck, often on an approval), Running, or when they last ran; dots for the last seven days; Run now and Last run; a red dot on the tab when one needs a look; an optional notification.
- **Where you approve the most** in Wrapped: per project, about how many tool calls had to ask, which commands, the median wait and "go ahead" replies. **Allow these…** suggests vetted build and test rules only, and writes them to `<project>/.claude/settings.local.json` after you confirm.
- **Settings:** terminal app picker and task notifications, next to the jump hotkey, limit notifications and Polish with Claude.

### Changed

- New quieter design (Silver), light and dark, in a 360×560 panel.
- The ⋯ menu (Refresh, Settings, Open full report, Quit) replaces the gear and the footer.

### Fixed

- The panel now opens over full-screen apps and on every Space.
- Resumed and forked Claude sessions are counted once, not once per file.
- Scheduled tasks no longer count as prompts you typed.
- Text injected into a session (shell command output, local commands, context blocks) no longer counts as a prompt.
