# mybot cli

A terminal AI coding agent written in Rust. It streams model responses, runs
tools against your working directory, asks before doing anything risky, and
remembers sessions.

```
mybot            # interactive TUI
mybot exec "explain this repo" --output-format text
```

## Features

- **Two wire protocols**: Anthropic Messages API and OpenAI-compatible Chat
  Completions, both streaming SSE. Works with Anthropic, OpenAI, MiniMax,
  DeepSeek, OpenRouter, Ollama, and any compatible endpoint.
- **Tools**: `read`, `write`, `edit`, `bash`, `grep`, `glob`, `todowrite`.
  Reads are capped and line-numbered, edits are atomic and reject ambiguous
  matches, bash has timeouts and process-group kills.
- **Permissions**: `ask`, `auto`, and `full` modes with persisted allow/deny
  rules, a bash risk classifier, and path containment checks.
- **Sessions**: append-only JSONL ledgers in `~/.mybot/sessions`, with
  `--continue` and `--session <id>`.
- **Context compaction**: `/compact` summarizes the conversation with the
  current model, replaces the history with the summary, and records the
  compaction in the session ledger so resumed sessions stay small.
- **Project context**: loads `CLAUDE.md` or `AGENTS.md` from the workspace root
  plus `~/.mybot/AGENTS.md`, capped at 32 KB.
- **Headless mode**: `text`, `json`, and `stream-json` output with stable exit
  codes for scripts and CI.

Not implemented yet: MCP servers, plugins, sub-agents, syntax highlighting.

## Platforms

Linux and macOS on x86_64 and ARM64 are the primary targets. Windows works
through WSL2 or Git Bash (the `bash` tool requires `bash`), and Android Termux
is experimental. See [docs/install.md](docs/install.md) for the full matrix and
cross-compilation instructions.

## Install

Requires Rust 1.85+ (edition 2024).

```bash
git clone https://github.com/seashyne/mybot-cli.git
cd mybot-cli
./install.sh            # builds and installs to ~/.local/bin
mybot --help
```

`install.sh` also accepts `--prefix DIR`, `--headless` (no TUI) and
`--uninstall`. Or install into `~/.cargo/bin`:

```bash
cargo install --path .
```

The TUI is a default feature. For a headless-only build:

```bash
cargo build --release --no-default-features
```

## Quick start

Set an API key and run:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
mybot
```

OpenAI-compatible providers can be added without editing files:

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

Slash commands: `/help`, `/new`, `/model`, `/sessions`, `/status`, `/init`,
`/compact`, `/quit`.

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
src/tools/            read, write, edit, bash, grep, glob, todowrite
src/permissions/      modes, rules, bash classifier, path checks
src/session/          JSONL session ledger
src/prompt/           system prompt assembly and project context
src/headless/         non-interactive runner and output formats
src/tui/              ratatui UI (feature "tui")
```

The agent loop is shared by the TUI and headless mode: both consume the same
`AgentEvent` stream, so tool execution and permission handling have exactly one
implementation.

## Tests

```bash
cargo test              # unit + integration, no API key required
cargo clippy --all-targets
```

Integration tests run a fake Anthropic-compatible SSE server, so the full
pipeline (request shape, streaming parse, tool loop, session replay, retries,
exit codes) is covered offline.

## Documentation

- [Installation and platform support](docs/install.md)
- [คู่มือผู้ใช้ภาษาไทย](docs/manual.th.md)
- [Performance benchmarks](docs/performance.md)

## License

MIT. See [LICENSE](LICENSE).
