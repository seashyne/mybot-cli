# mybot-cli

Terminal AI coding agent written in Rust. This package downloads the native
binary for your platform during `postinstall`; there is no JavaScript runtime
overhead when you run it.

```bash
npm install -g mybot-cli
mybot
```

Or without installing:

```bash
npx mybot-cli --help
```

First run opens a setup screen: pick a provider (Anthropic, OpenAI, Gemini,
DeepSeek, OpenRouter, Groq, Mistral, xAI, Moonshot, Qwen, Z.ai, MiniMax,
LM Studio, Ollama or a custom endpoint), paste a key, choose a model.

Switch models later with `/model` inside the TUI.

## Platforms

| OS | Architecture | Binary |
| --- | --- | --- |
| Linux | x86_64 | `x86_64-unknown-linux-gnu` |
| Linux | arm64 | `aarch64-unknown-linux-gnu` |
| macOS | x86_64 | `x86_64-apple-darwin` |
| macOS | arm64 | `aarch64-apple-darwin` |

Windows is not supported directly — use WSL2.

## Pin a version

```bash
MYBOT_VERSION=v0.1.0 npm install -g mybot-cli
```

## Alternative installs

```bash
# shell script, downloads the latest release
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh | sh

# from source
cargo install --git https://github.com/seashyne/mybot-cli.git
```

## Links

- Source: https://github.com/seashyne/mybot-cli
- Docs: https://seashyne.github.io/mybot-cli/
- License: MIT
