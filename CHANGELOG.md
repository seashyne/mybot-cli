# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-11-22

Fifth release: OS bash sandbox, diff previews, `/undo`, `web_fetch`, image
attachments, and native Windows builds.

### Added

- **OS-level bash sandbox** — `bash` now runs inside bubblewrap (`bwrap`) on
  Linux or Seatbelt (`sandbox-exec`) on macOS in `auto` (default) and `strict`
  modes, keeping the filesystem read-only outside the workspace and `/tmp` and
  hiding `~/.mybot` API keys; switch at runtime with `/sandbox [off|auto|strict]`.
- **Multimodal image attachments** — attach PNG/JPEG/WebP/GIF images via
  `/image <path|paste|clear>`, clipboard paste (`Ctrl+V` / `Alt+V`), or
  drag-and-drop paths; encoded for both Anthropic and OpenAI-compatible models.
- **Diff preview & `/undo` checkpoints** — permission prompts for `edit` and
  `write` now render color-coded diffs, and `/undo` (`/rewind`) reverts all
  file mutations from the previous turn.
- **`web_fetch` tool** — built-in public HTTP/HTTPS fetcher with HTML stripping
  and SSRF blocking for localhost, private, link-local, and CGNAT addresses.
- **Tool output expansion & cumulative tokens** — press `Ctrl+O` to toggle
  tool outputs in the transcript, and inspect cumulative session token totals
  in `/status`.
- **Native Windows (`x86_64-pc-windows-msvc`)** — added CI release binary/zip
  packaging, self-update support, npm wrapper resolution, PowerShell fallback,
  and `taskkill` process-tree cleanup.

### Security

- `config.toml` is now written with `0600` file permissions (`0700` parent
  directory on creation) on Unix.
- The npm `postinstall` script now verifies `sha256sums.txt` before finalizing
  the installed binary.

## [0.4.0] - 2026-11-22

Fourth release: welcome card, status bar cues, `@` mentions, and `/effort`.

### Added

- **`@` file & plugin mentions** — typing `@` in the composer opens an
  autocomplete popup for workspace files, plugins, and skills.
- **`/effort [off|low|medium|high]`** — switches reasoning effort at runtime,
  persists it to `config.toml`, and sends `reasoning_effort` on OpenAI-compatible
  requests.
- **`/changelog`** — displays release notes directly inside the TUI, with the
  latest highlights surfaced on the welcome card.
- **Model context windows** — added known context window sizes and expanded
  MiniMax catalog entries (`MiniMax-M2.5`, `MiniMax-M2.1`, `M3.1-Flash`).

### Changed

- **Welcome card & composer** — centered banner, rounded welcome card with
  `Ready` badge, tips, and What's new highlights, plus a hint bar, prompt
  gutter, and placeholder inside the composer.
- **Status bar** — now shows shortened working directory, risk-colored
  permission badge (`full access` in red), active effort level, and context
  usage percentage.

### Fixed

- Anthropic `message_delta` events without `input_tokens` no longer overwrite
  the session's input token count back to zero.

## [0.3.0] - 2026-11-22

Third release: a command palette, session resume, live model lists, and
runtime permission switching.

### Added

- **Command palette** — typing `/` in the composer opens a searchable list of
  every slash command with its usage and description. Matching covers names,
  aliases, and descriptions; `Tab` completes, `Enter` runs, and `Esc` closes.
  Commands that need an idle session are marked while a turn is running.
- **`/resume <id>`** — loads a previous session into the current window,
  replaying the last 20 messages as transcript entries without tool payloads.
  The id prefix from `/sessions` is enough.
- **Runtime permission switching** — `/permissions [ask|auto|full]` reports the
  current mode and switches it for the process, including sub-agents and
  teammates that share the engine, then persists it to `config.toml`.
- **`/tools`** — lists every registered tool with its execution mode and
  description, including MCP tools.
- **`/retry`** — sends the previous prompt again after a failed turn.
- **Live model lists** — the `/model` picker queries each configured provider
  (`/v1/models` or the Anthropic equivalent) in the background, merges the
  results above the curated catalog, and marks models the account cannot use.
  The picker gained a search field; typing an unknown id and pressing `Enter`
  still uses it verbatim.

### Changed

- Slash commands now come from one catalog (`src/tui/commands.rs`) that drives
  dispatch, `/help`, and the palette, so a command cannot exist in one place
  and be missing from another. The dispatcher was split into
  `slash/session.rs`, `slash/settings.rs`, and `slash/view.rs`.
- The model picker's `custom` row was replaced by the search field: any text
  that does not match a model is used as a model id.

### Fixed

- Model and completion URLs now recognise API roots other than `/v1`
  (`/v4`, `/openai`, `/v1beta/openai`), so Gemini and Z.ai style base URLs no
  longer end up with a duplicated segment.

## [0.2.0] - 2026-11-22

Second release: MCP servers, project memory, sub-agents, teams, syntax
highlighting, and self-update.

### Added

- **Version reporting** — `mybot --version` now includes the git commit, and
  `mybot version [--json]` prints the target triple, build profile, binary
  path, and config directory. The TUI welcome screen and `/status` show the
  version, and `/version` prints the full report.
- **Self-update** — `mybot update` resolves the latest GitHub release (or
  `--tag <tag>`), downloads the `mybot-<target>` asset and `sha256sums.txt`,
  verifies the checksum, probes the new binary with `--version`, and replaces
  the current executable atomically. `--check` only reports whether an update
  exists; `--force` reinstalls the same version. npm-managed binaries are
  refused unless forced, and insecure download URLs are rejected.
- **Release artifacts** — tagged builds publish raw binaries and `.tar.gz`
  archives for Linux and macOS on x86_64 and arm64, plus `sha256sums.txt` that
  `mybot update` verifies before installing.
- **MCP servers** — connect to Model Context Protocol servers over stdio or
  streamable HTTP. Configuration is merged from `~/.mybot/mcp.toml` and
  `<project>/.mybot/mcp.toml` (project entries win), and every remote tool is
  exposed as `mcp__<server>__<tool>`. Added `mybot mcp list`, `mybot mcp test
  <name>`, and the `/mcp` command. A server that fails to start is reported as
  a warning instead of aborting the session.
- **Project memory** — durable markdown notes per project under
  `~/.mybot/memory/<project>-<hash>/`, with a generated `MEMORY.md` index
  injected into the system prompt (capped at 8 KB). Added the `memory` tool
  (`list`, `read`, `write`, `delete`), `mybot memory list|show|delete|path`,
  `/memory`, and `/remember`.
- **Sub-agents** — markdown agent definitions in `~/.mybot/agents/*.md` or
  `<project>/.mybot/agents/*.md`, with an optional tool subset. The model
  delegates through the `agent` tool; each sub-agent has its own context and a
  24-turn cap. Added `/agents`.
- **Teams** — a shared task board, inboxes, and background teammates that run
  their own agent loop with the same provider and permission engine. Added
  `task_create`, `task_list`, `task_update`, `send_message`, `check_messages`,
  and `spawn_teammate`; teammates update the board and report to the lead. The
  TUI shows live teammate status in a `team` panel, and `/team` prints the full
  board.
- **Syntax highlighting** — fenced code blocks in the TUI are highlighted with
  syntect using the base16-ocean.dark theme.
- **Permission rules for MCP tools** — `mcp__*` calls can be allowed with a
  persisted tool-level rule; `memory` and the team tools are auto-allowed in
  `auto` mode.

### Changed

- `build.rs` records the git commit, commit date, and target triple at build
  time so version output identifies the exact binary, and re-runs when git refs
  move so the embedded commit never goes stale.
- The `Tool` trait now returns dynamic `&str` names and descriptions so MCP
  tools can be registered at runtime.
- The TUI and headless runner now share the same MCP, sub-agent, and memory
  wiring, and both consume the same `AgentEvent` stream; the TUI additionally
  handles the new `AgentEvent::Team` snapshot.
- Oversized modules were split into directories (`commands/`, `mcp/`,
  `providers/*/`, `agent/runner/`, `tui/app/`, `tui/render/`, `tui/setup/`,
  `team/tools/`), and `tests/structure.rs` now fails if any file in `src/`,
  `tests/`, or `benches/` exceeds 400 lines.
- The system prompt gained `Memory` and `Team` sections and documents the new
  tools.
- README, the Thai manual, and the docs index describe MCP, memory, sub-agents,
  teams, and the line limit.

### Fixed

- The release workflow requested the retired `macos-13` runner labels, so both
  macOS jobs stayed queued and no release was published. Builds now use
  `macos-15-intel` for x86_64 and `macos-15` for arm64, and the `publish` job
  creates the GitHub release with the checksums.

## [0.1.0] - 2026-11-22

First tagged release.

### Added

- Interactive TUI (ratatui) and headless `mybot exec` with `text`, `json`, and
  `stream-json` output.
- Anthropic Messages and OpenAI-compatible streaming providers, with provider
  presets, runtime model switching, retries, and first-run setup.
- Tools `read`, `write`, `edit`, `bash`, `grep`, `glob`, `todowrite`, and
  `skill`; permission modes `ask`, `auto`, and `full` with persisted rules.
- JSONL session ledgers, `--continue`/`--session`, and `/compact`.
- Skills and plugins, project context loading, an installer script, an npm
  package, and a documentation site.

[Unreleased]: https://github.com/seashyne/mybot-cli/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/seashyne/mybot-cli/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/seashyne/mybot-cli/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/seashyne/mybot-cli/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/seashyne/mybot-cli/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/seashyne/mybot-cli/releases/tag/v0.1.0
