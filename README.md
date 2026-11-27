# mybot cli

A terminal AI coding agent written in Rust. It streams model responses, runs
tools against your working directory, asks before doing anything risky, and
remembers sessions.

```
mybot            # interactive TUI
mybot exec "explain this repo" --output-format text
```

## Features

- **Two wire protocols**: Anthropic Messages API (with ephemeral prompt caching)
  and OpenAI-compatible Chat Completions, both streaming SSE. Works with
  Anthropic, OpenAI, MiniMax, DeepSeek, OpenRouter, Ollama, and any compatible
  endpoint.
- **Tools**: `read`, `write`, `edit`, `bash`, `bg_start`, `bg_output`, `bg_stop`,
  `grep`, `glob`, `web_fetch`, `todowrite`, `skill`, `memory`, `agent`, plus
  `mcp__<server>__<tool>` for MCP servers.
  Reads are capped and line-numbered, edits are atomic and reject ambiguous
  matches, `web_fetch` blocks local/private IPs, `bg_*` manages background dev
  servers, and bash has OS sandboxing (`bwrap` with `--unshare-net` in strict
  mode / macOS `sandbox-exec`), timeouts, and process-group kills.
- **Lifecycle hooks**: `pre_tool_use`, `post_tool_use`, and `stop` shell hooks
  in `~/.mybot/hooks.toml` or `.mybot/hooks.toml` for policy checks and
  automated formatting/linting diagnostics.
- **Permissions**: `ask`, `auto`, and `full` modes with persisted allow/deny
  rules, a bash risk classifier, and path containment checks.
- **Sessions**: append-only JSONL ledgers in `~/.mybot/sessions`, with
  `--continue` and `--session <id>`.
- **Context compaction**: `/compact` (and automatic compaction at 82% context
  window) summarizes the conversation with the current model, replaces the
  history with the summary, and records the compaction in the session ledger.
- **Project context**: loads `CLAUDE.md` or `AGENTS.md` from the workspace root
  plus `~/.mybot/AGENTS.md`, capped at 32 KB.
- **Skills and plugins**: markdown instruction packs loaded on demand, plus
  plugins that bundle skills and slash commands.
- **MCP servers**: stdio and streamable HTTP transports, merged user and project
  config, with tools exposed as `mcp__<server>__<tool>`.
- **Sub-agents**: markdown agent definitions with their own context and an
  optional tool subset, invoked through the `agent` tool.
- **Teams**: a shared task board, inbox messaging, and background teammates that
  report back to the lead; the TUI shows live member status.
- **Project memory**: durable notes in `~/.mybot/memory`, indexed into the
  system prompt and managed with the `memory` tool or `/remember`.
- **Syntax highlighting** for fenced code blocks in the TUI.
- **First-run setup**: a guided screen picks a provider, stores the key, and
  chooses a model; `/model` switches at runtime.
- **Headless mode**: `text`, `json`, and `stream-json` output with stable exit
  codes for scripts and CI.
- **Version and self-update**: `mybot version` reports the build identity and
  `mybot update` installs the latest release with checksum verification.

## Platforms

Linux, macOS, and Windows (x86_64) are supported: Linux and macOS on x86_64 and
ARM64, and Windows x86_64 natively (or through WSL2 / Git Bash), while Android
Termux is experimental. See [docs/install.md](docs/install.md) for the full
matrix and cross-compilation instructions.

## Install

One line, downloads the prebuilt binary for your platform:

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh | sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.ps1 | iex
```

Or with npm:

```bash
npm install -g mybot-cli
```

From source (Rust 1.85+, edition 2024):

```bash
git clone https://github.com/seashyne/mybot-cli.git
cd mybot-cli
./install.sh            # builds and installs to ~/.local/bin
mybot --help
```

`install.sh` accepts `--prefix DIR`, `--headless` (no TUI), `--uninstall`,
`--version TAG` and `--from-source` / `--from-release`. Or install into
`~/.cargo/bin`:

```bash
cargo install --path .
```

The TUI is a default feature. For a headless-only build:

```bash
cargo build --release --no-default-features
```

## Version and updates

`mybot --version` prints the package version and the git commit the binary was
built from:

```bash
$ mybot --version
mybot 0.5.0 (66cdc54)
```

`mybot version` adds the target triple, build profile, binary path, and config
directory; `--json` makes it machine-readable.

`mybot update` replaces the running binary with the newest GitHub release:

```bash
mybot update --check          # only report whether a newer release exists
mybot update                  # download, verify, and install it
mybot update --tag v0.5.0     # install a specific release
mybot update --force          # reinstall even when the version is unchanged
```

The updater resolves the release through the GitHub API, downloads the
`mybot-<target>` asset plus `sha256sums.txt`, and refuses to install without a
matching checksum. It probes the downloaded binary with `--version`, then
renames it over the current executable; the rename is atomic on Unix, so a
running session keeps working and picks up the new binary next start.

Notes:

- A published release is required; a git tag alone has no downloadable assets.
- npm installs are managed by npm — run `npm update -g mybot-cli` instead
  (`--force` overrides the refusal).
- If the binary lives in a root-owned directory, run the update with the
  permissions needed to write there.
- Set `MYBOT_GITHUB_TOKEN` to avoid GitHub API rate limits.
- Supported targets: Linux x86_64/aarch64 and macOS x86_64/arm64.

## Quick start

Run `mybot` with no configuration and a setup screen walks you through picking a
provider, pasting a key and choosing a model:

```bash
mybot
```

Presets are included for Anthropic, OpenAI, Google Gemini, DeepSeek, OpenRouter,
Groq, Mistral, xAI, Moonshot, Qwen, Z.ai, MiniMax, LM Studio, Ollama and any
custom OpenAI-compatible endpoint.

Or set a key through the environment:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
mybot
```

Switch models at any time from inside the TUI with `/model` — the picker lists
all 14 preset providers plus your configured providers, and replaces them with
the live model list each API returns. Selecting a provider that needs an API key
prefills `/key <provider> ` in the composer so you can paste your key and
connect without leaving the TUI:

```text
/key anthropic sk-ant-...
/key openai sk-...
/key deepseek sk-...
/key minimax <key>
/key lmstudio http://127.0.0.1:1234/v1
```

OpenAI-compatible providers can also be added without editing files:

```bash
mybot provider add local \
  --kind openai \
  --base-url http://localhost:11434/v1 \
  --model qwen2.5-coder

mybot -m local/qwen2.5-coder
```

### Local models (LM Studio, Ollama, llama.cpp)

Any OpenAI-compatible local server works and needs no API key when the base URL
is on localhost. With `model = "auto"`, mybot asks `/v1/models` which chat model
is loaded (embedding-only models are skipped).

LM Studio:

```bash
~/.lmstudio/bin/lms server start      # note the port it prints (often 1234)
# load a chat model in the LM Studio UI, then:
mybot provider add lmstudio --kind openai \
  --base-url http://127.0.0.1:1234/v1 --model auto
mybot
```

Ollama:

```bash
ollama serve
mybot provider add ollama --kind openai \
  --base-url http://127.0.0.1:11434/v1 --model auto
mybot -m ollama/auto
```

Check what is configured:

```bash
mybot provider list
mybot provider test anthropic
```

## Configuration

`~/.mybot/config.toml` (override the root with `MYBOT_HOME`):

```toml
default_provider = "anthropic"

[providers.anthropic]
kind = "anthropic"
base_url = "https://api.anthropic.com"
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4-5"
max_tokens = 8192
# thinking_budget = 4096
# max_retries = 2          # retries after 429, 5xx, or a connection error
# retry_base_delay_ms = 500
# retry_max_delay_ms = 8000

[providers.openai]
kind = "openai"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
model = "gpt-4o-mini"

[permissions]
mode = "auto"   # ask | auto | full

[[permissions.rules]]
tool = "bash"
matcher = "command"
pattern = "cargo test*"
behavior = "allow"
```

Precedence: CLI flags > environment (`MYBOT_PROVIDER`, `MYBOT_MODEL`) > config.
A missing key exits with code 3 and names the environment variable to set.
Localhost base URLs are allowed without a key.

### Retries

Requests that fail before the stream starts are retried up to `max_retries`
times (default 2) with exponential backoff from `retry_base_delay_ms`, capped at
`retry_max_delay_ms`. A server `Retry-After` header overrides the backoff.
Only 429, 5xx, and connection errors are retried — 4xx client errors and
authentication failures fail immediately. Retries never happen once streaming
has begun, so output is never duplicated.

```bash
mybot exec "refactor this" --output-format stream-json | jq -c 'select(.type=="retry")'
```

In the TUI a retry shows up as a transient status message; `exec` prints
`[retry 1/2 in 0.5s] …` to stderr.

## Usage

```bash
mybot [PROMPT]                 # interactive TUI, optional first prompt
mybot exec "<prompt>"          # one turn, headless
mybot exec - < prompt.txt      # read the prompt from stdin
mybot -c                       # continue the newest session in this directory
mybot --session <id>           # resume a specific session
mybot init [dir]               # create AGENTS.md
mybot version [--json]         # version, commit, target, and paths
mybot update [--check]         # install the latest release in place
```

Useful flags: `-m/--model provider/model`, `--permission ask|auto|full`,
`--system-prompt[-file]`, `--append-system-prompt[-file]`, `--cwd`.

`exec` flags: `--output-format text|json|stream-json`, `-o/--output-last-message`,
`--timeout <seconds>`, `--max-steps <n>`.

Exit codes: `0` success, `2` usage, `3` config/auth, `4` runtime, `6` timeout,
`7` step limit, `130` cancelled.

### TUI keys

| Key | Action |
| --- | --- |
| `Enter` | send |
| `Shift+Enter` / `Ctrl+J` | newline |
| `Esc` | cancel the running turn, or clear the input |
| `Ctrl+C` | cancel the turn; when idle, quit |
| `Ctrl+D` | quit when the input is empty |
| `Up` / `Down` | history when empty, cursor movement otherwise |
| `PgUp` / `PgDn` | scroll the transcript |
| `a` / `A` / `d` | allow once / always allow / deny (permission modal) |
| `Ctrl+O` | expand or collapse tool outputs in the transcript |
| `Ctrl+Y` | copy the last assistant reply (`/copy code` for last code block) |
| `Ctrl+V` / `/image` | attach a clipboard screenshot or image file |
| `@` | mention a workspace file, plugin, or skill |
| `/` | open the command palette |

Typing `/` opens a searchable command palette above the composer; `Tab`
completes the highlighted command, `Enter` runs it, and `Esc` closes it.
Typing `@` opens an autocomplete popup for workspace files, plugins, and skills.

Slash commands: `/help`, `/changelog`, `/new`, `/model`, `/key` (`/login`),
`/effort`, `/permissions`, `/sandbox`, `/image`, `/test` (`/check`),
`/diff` (`/git`), `/tools`, `/context`, `/sessions`, `/resume`, `/status`,
`/version`, `/compact`, `/retry`, `/undo`, `/stop`, `/export`, `/copy`,
`/find`, `/config`, `/doctor`, `/keys`, `/init`, `/skills`, `/skill`,
`/agents`, `/memory`, `/remember`, `/mcp`, `/team`, `/quit`.

### Compacting a session

`/compact` asks the current model for a handoff summary of the conversation,
then replaces the in-memory history with that summary. The session ledger gets a
`compact` entry, so `-c` and `--session` resume from the summary instead of
replaying every earlier turn. The previous transcript stays visible in the TUI
with a compaction boundary line.

Compaction needs one extra model call. Press `Esc` while it runs to cancel; a
failed or cancelled compaction leaves the history untouched. If the summary hits
the output limit it is still used, and the boundary line marks it as truncated.
Very long conversations are flattened into a bounded transcript before
summarization, keeping the first request and the most recent turns.

## Skills and plugins

A **skill** is a markdown instruction pack the model can load on demand:

```
~/.mybot/skills/<name>/SKILL.md          # available in every project
<project>/.mybot/skills/<name>/SKILL.md  # project-only, wins on a name clash
```

```markdown
---
name: rust-review
description: Review Rust code for correctness and idioms
---
# Rust review
1. Read `git diff`
2. Check error handling and ownership
3. Report findings as `path:line`
```

The model sees every skill name and description in its system prompt and calls
the `skill` tool to load the body. From the TUI, `/skills` lists them and
`/skill rust-review` loads one immediately.

A **plugin** bundles skills and slash commands so they can be shared:

```
<plugin>/
  plugin.toml          name, version, description
  skills/<name>/SKILL.md
  commands/<name>.md   becomes /<name>, $ARGUMENTS is substituted
```

```bash
mybot skill list
mybot skill new my-skill --scope user
mybot plugin list
mybot plugin add ./examples/plugins/code-review --project
mybot plugin command review "focus on src/"
```

The bundled example in `examples/plugins/code-review/` adds a `/review`
command and a `security-review` skill.

## MCP servers

MCP servers extend the tool set over the Model Context Protocol. Configure them
in `~/.mybot/mcp.toml` or `<project>/.mybot/mcp.toml` (project entries win):

```toml
[servers.files]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]

[servers.docs]
url = "https://mcp.example.com/mcp"
headers = { Authorization = "Bearer ..." }
timeout_secs = 60
```

`command` selects a stdio server, `url` selects streamable HTTP. Every tool is
registered as `mcp__<server>__<tool>`, so it can be allowed or denied with the
usual permission rules. `mybot mcp list` shows the config and live tools;
`mybot mcp test <name>` connects to one server. A server that fails to start is
reported as a warning and does not abort the session.

## Memory

Project notes live in `~/.mybot/memory/<project>-<hash>/` as markdown files with
frontmatter (`type` is one of `user`, `feedback`, `project`, `reference`). The
generated `MEMORY.md` index is injected into the system prompt (capped at 8 KB),
so the model knows what exists before it reads an entry with the `memory` tool.

```
/memory                 # list entries
/memory show <name>     # print one entry
/memory delete <name>   # remove one entry
/remember <text>        # save a quick project note
```

## Sub-agents and teams

Define reusable sub-agents as markdown files in `~/.mybot/agents/<name>.md` or
`<project>/.mybot/agents/<name>.md`:

```markdown
---
name: reviewer
description: Review a diff for correctness
tools: read, grep, glob
---
You are a careful reviewer. Report findings as path:line.
```

The model calls the `agent` tool with a prompt and an optional `subagent_type`;
the sub-agent has its own context, a 24-step cap, and only the listed tools.
`/agents` lists what is installed.

For larger jobs the lead can run a team. Teammates work in the background with
the same provider and permission engine, share a task board, and send reports to
the lead's inbox:

- `task_create`, `task_list`, `task_update` — the shared board
- `spawn_teammate` — start a teammate with a name, role, prompt, and optional tool subset
- `send_message`, `check_messages` — inboxes (`check_messages` can wait for teammates to finish)

The TUI shows running teammates above the composer, and `/team` prints the full
board. Each teammate is a separate model conversation, so it costs tokens; keep
teams for work that actually splits into independent parts.

## Permissions

| Mode | Behaviour |
| --- | --- |
| `ask` | every tool call asks |
| `auto` | read-only commands and in-project edits run; anything else asks |
| `full` | everything runs, except hard-denied destructive commands |

Hard-denied commands (`rm -rf /`, `rm -rf ~`, `mkfs`, `dd ... of=/dev/...`)
are refused in every mode. Headless runs cannot prompt, so anything that needs
approval is denied.

Choosing "always allow" appends a rule to `config.toml`.

## Architecture

```
src/cli.rs            clap surface and exit codes
src/config/           config.toml, provider resolution, data paths
src/protocol/         canonical Message/Block/ToolResult model
src/providers/        SSE decoder, Anthropic, OpenAI, mock provider
src/agent/            turn loop, delta assembler, events
src/tools/            read, write, edit, bash, grep, glob, todowrite, memory
src/mcp/              MCP config, stdio/HTTP transports, tool bridge
src/memory.rs         project memory store and prompt index
src/subagents/        agent definitions and the `agent` tool
src/team/             task board, inboxes, background teammates
src/version.rs        build identity from Cargo and git
src/update/           release lookup, checksum verification, self-replace
src/permissions/      modes, rules, bash classifier, path checks
src/session/          JSONL session ledger
src/prompt/           system prompt assembly and project context
src/headless/         non-interactive runner and output formats
src/tui/              ratatui UI (feature "tui")
```

The agent loop is shared by the TUI and headless mode: both consume the same
`AgentEvent` stream, so tool execution and permission handling have exactly one
implementation. Sub-agents and teammates reuse that loop instead of duplicating
it.

Every source file in `src/`, `tests/`, and `benches/` is limited to 400 lines;
`tests/structure.rs` enforces the rule.

## Tests

```bash
cargo test              # unit + integration, no API key required
cargo clippy --all-targets
```

Integration tests run a fake Anthropic-compatible SSE server, so the full
pipeline (request shape, streaming parse, tool loop, session replay, retries,
exit codes) is covered offline.

## Documentation

Read the rendered site at **https://seashyne.github.io/mybot-cli/** or browse the source:

- [Installation and platform support](docs/install.md)
- [คู่มือผู้ใช้ภาษาไทย](docs/manual.th.md)
- [Performance benchmarks](docs/performance.md)

## License

MIT. See [LICENSE](LICENSE).
