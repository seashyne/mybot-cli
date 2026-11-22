# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Unreleased work on top of `v0.1.0`.

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
  time so version output identifies the exact binary.
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

[Unreleased]: https://github.com/seashyne/mybot-cli/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/seashyne/mybot-cli/releases/tag/v0.1.0
