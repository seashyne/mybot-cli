---
title: คู่มือการใช้งาน
permalink: /manual/
---

# คู่มือการใช้งาน mybot cli

mybot คือ AI coding agent สำหรับ terminal ที่เขียนด้วย Rust — สตรีมคำตอบจากโมเดล,
เรียกใช้ tools กับโปรเจกต์, ถามก่อนทำสิ่งเสี่ยง, และจำ session ได้

คู่มือนี้เป็นภาษาไทย สำหรับเอกสารติดตั้งและ performance ดูเป็นภาษาอังกฤษได้ที่
[install.md](install.md) และ [performance.md](performance.md)

---

## 1. mybot ทำอะไรได้บ้าง

- **รองรับ 2 wire protocol**: Anthropic Messages API และ OpenAI-compatible Chat
  Completions (streaming SSE ทั้งคู่) ใช้ได้กับ Anthropic, OpenAI, MiniMax,
  DeepSeek, OpenRouter, Ollama, LM Studio และ endpoint ที่เข้ากันได้
- **Tools**: `read`, `write`, `edit`, `bash`, `grep`, `glob`, `todowrite`, `skill`,
  `memory`, `agent` และ tool จาก MCP server (`mcp__<server>__<tool>`)
- **Permissions**: โหมด `ask`, `auto`, `full` พร้อมกฎ allow/deny ที่บันทึกไว้
- **Sessions**: เก็บเป็น JSONL ใน `~/.mybot/sessions` ต่อด้วย `-c` หรือ `--session`
- **Context compaction**: `/compact` สรุปบทสนทนาแล้วแทน history ด้วย summary
- **MCP servers**: เชื่อมได้ทั้ง stdio และ streamable HTTP
- **Sub-agents**: agent definition เป็น markdown พร้อม tool subset ของตัวเอง
- **Teams**: กระดานงานร่วม, inbox และ teammate ที่ทำงานเบื้องหลังแล้วส่งรายงานกลับ
- **Memory**: บันทึกระยะยาวแยกตามโปรเจกต์ ใส่ index ใน system prompt อัตโนมัติ
- **Syntax highlighting**: ไฮไลต์โค้ดใน TUI
- **Version และ self-update**: `mybot version` และ `mybot update` ตรวจ checksum ก่อนติดตั้ง
- **Headless**: `mybot exec` พร้อม output แบบ `text`, `json`, `stream-json`

---

## 2. แพลตฟอร์มที่รองรับ

| แพลตฟอร์ม | สถานะ | หมายเหตุ |
| --- | --- | --- |
| Linux x86_64 | ทดสอบแล้ว | เครื่องที่ใช้พัฒนา |
| Linux aarch64 | ทดสอบผ่าน Docker/QEMU | Raspberry Pi OS 64-bit, เซิร์ฟเวอร์ ARM |
| macOS x86_64 / Apple Silicon | คาดว่ารองรับ | build จาก source; ยังไม่มีฮาร์ดแวร์ทดสอบใน repo นี้ |
| Windows 10/11 | best-effort | ต้องใช้ WSL2 หรือ Git Bash และมี `bash` |
| Android Termux (aarch64) | experimental | ยังไม่ทดสอบ |
| wasm / iOS | ไม่รองรับ | ต้องใช้ process และ terminal |

ข้อจำกัดที่ควรรู้:

- tool `bash` เรียก `bash -lc` ดังนั้นทุกแพลตฟอร์มต้องมี bash
- บน Windows การ kill process group ยังไม่ทำงาน (timeout จะหยุดแค่ process แม่)
- บน Linux/macOS ใช้ process group + `SIGKILL` เมื่อ timeout หรือกด cancel

---

## 3. การติดตั้ง

### 3.1 ติดตั้งด้วยคำสั่งเดียว (แนะนำ)

ดาวน์โหลด binary ที่ build ไว้แล้วสำหรับแพลตฟอร์มของคุณ:

```bash
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh | sh
```

ติดตั้งลง `~/.local/bin` และตรวจ checksum ให้อัตโนมัติ

ส่ง option ต่อท้ายด้วย `sh -s --`:

```bash
# เลือกที่ติดตั้ง
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh \
  | sh -s -- --prefix /usr/local/bin

# เลือกเวอร์ชัน
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh \
  | sh -s -- --version v0.2.0

# ถอนการติดตั้ง
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh \
  | sh -s -- --uninstall
```

### 3.2 ติดตั้งผ่าน npm

```bash
npm install -g mybot-cli
mybot
```

ตัว package จะดาวน์โหลด binary ให้ตอนรันครั้งแรก (ใช้ `npx mybot-cli` ได้ด้วย)
กำหนดเวอร์ชันด้วย `MYBOT_VERSION=v0.2.0`

### 3.3 สิ่งที่ต้องมี (สำหรับ build จาก source)

- Rust 1.85 ขึ้นไป (โปรเจกต์ใช้ edition 2024)
- C toolchain, CMake และ Perl (สำหรับ `aws-lc-sys` ที่ rustls ใช้)
- `git`
- bash

Ubuntu/Debian:

```bash
sudo apt-get update
sudo apt-get install -y build-essential cmake perl pkg-config git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 3.4 ติดตั้งจาก source

```bash
git clone https://github.com/seashyne/mybot-cli.git
cd mybot-cli
./install.sh          # build แล้วติดตั้งเข้า ~/.local/bin
mybot --help
```

`install.sh` รับ option เพิ่มเติม:

```bash
./install.sh --prefix /usr/local/bin   # เปลี่ยนที่ติดตั้ง
./install.sh --headless                # build แบบไม่มี TUI
./install.sh --uninstall               # ถอนการติดตั้ง
./install.sh --from-release            # ดาวน์โหลด release แทนการ build
```

หรือติดตั้งเข้า `~/.cargo/bin` ด้วย cargo:

```bash
cargo install --path .
```

หรือติดตั้งจาก Git โดยตรง:

```bash
cargo install --git https://github.com/seashyne/mybot-cli.git
```

### 3.5 build แบบไม่มี TUI (headless only)

เหมาะกับเซิร์ฟเวอร์หรือ CI ที่ไม่ต้องการ ratatui/crossterm:

```bash
cargo build --release --no-default-features
```

### 3.6 ติดตั้งบน ARM (Raspberry Pi / เซิร์ฟเวอร์ ARM64)

วิธีที่ง่ายที่สุดคือ build บนอุปกรณ์นั้นโดยตรง:

```bash
sudo apt-get install -y build-essential cmake perl pkg-config git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://github.com/seashyne/mybot-cli.git
cd mybot-cli
cargo build --release
```

เครื่อง RAM น้อยให้ลด parallel jobs:

```bash
cargo build --release -j2
```

Cross-compile จาก x86_64 ไป aarch64:

```bash
rustup target add aarch64-unknown-linux-gnu
sudo apt-get install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu cmake

export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++

cargo build --release --target aarch64-unknown-linux-gnu
```

หรือใช้ Docker (ต้องมี QEMU binfmt):

```bash
# ครั้งแรกเท่านั้น
docker run --privileged --rm tonistiigi/binfmt --install arm64

docker run --rm --platform linux/arm64 \
  -v "$PWD":/work -v "$PWD/out":/out -w /work \
  -e CARGO_TARGET_DIR=/tmp/target rust:1-bookworm \
  sh -c "cargo build --release && cp /tmp/target/release/mybot /out/mybot-linux-arm64"
```

รายละเอียดเพิ่มเติมอยู่ใน [install.md](install.md)

### 3.7 อัปเดตตัวเอง

ตรวจสอบและอัปเดต binary ที่ติดตั้งอยู่:

```bash
mybot version            # เวอร์ชัน, commit, target, path
mybot update --check     # ดูว่ามี release ใหม่ไหม
mybot update             # ดาวน์โหลดและติดตั้งทับ binary เดิม
mybot update --tag v0.2.0
mybot update --force     # ติดตั้งซ้ำแม้เวอร์ชันเดียวกัน
```

`mybot update` จะ:

1. ถาม GitHub Releases ล่าสุด (หรือ tag ที่ระบุ)
2. ดาวน์โหลด asset `mybot-<target>` และ `sha256sums.txt`
3. ตรวจ sha256 และปฏิเสธถ้าไม่มี checksum
4. รัน binary ใหม่ด้วย `--version` เพื่อยืนยันว่าใช้งานได้
5. rename ทับ binary เดิมแบบ atomic

ข้อควรรู้:

- ต้องมี release ที่ publish แล้ว; การมีแค่ tag ไม่มี asset ให้ดาวน์โหลด
- ถ้าติดตั้งผ่าน npm ให้ใช้ `npm update -g mybot-cli`; `mybot update` จะปฏิเสธ
  เว้นแต่ใส่ `--force`
- ถ้า binary อยู่ใน `/usr/local/bin` ต้องรันด้วยสิทธิ์ที่เขียนโฟลเดอร์นั้นได้
- ตั้ง `MYBOT_GITHUB_TOKEN` เพื่อหลีกเลี่ยง rate limit ของ GitHub API
- รองรับ Linux x86_64/aarch64 และ macOS x86_64/arm64

---

## 4. เริ่มต้นใช้งาน

### 4.1 setup ครั้งแรก (หน้าจอแนะนำ)

รัน `mybot` ครั้งแรกเมื่อยังไม่มี provider ที่ใช้ได้ — จะขึ้นหน้าจอ setup ให้เลือกทีละขั้น

```
╭ mybot setup ─────────────────────────────────────────╮
│ step 1 of 3                                          │
│ Choose a provider                                    │
│ ▸ Anthropic · Claude          api.anthropic.com      │
│   OpenAI · GPT                api.openai.com         │
│   Google Gemini               generativelanguage...  │
│   DeepSeek                    api.deepseek.com       │
│   OpenRouter                  one key, many models   │
│   ...                                                │
│ ↑↓ move · Enter select · Esc quit                    │
╰──────────────────────────────────────────────────────╯
```

1. เลือก provider (↑↓ แล้ว Enter)
2. วาง API key (ถ้าเป็น cloud) — บันทึกลง `config.toml` ให้อัตโนมัติ
3. ใส่ชื่อ model (มีค่าเริ่มต้นให้ แก้ได้)

Esc ย้อนกลับทีละขั้น หรือกด Esc ที่ขั้นแรกเพื่อออก

### 4.2 provider ที่มี preset ให้เลือก

| Provider | Base URL | Model เริ่มต้น |
| --- | --- | --- |
| Anthropic · Claude | api.anthropic.com | claude-sonnet-4-5 |
| OpenAI · GPT | api.openai.com | gpt-4o-mini |
| Google Gemini | generativelanguage.googleapis.com | gemini-2.5-flash |
| DeepSeek | api.deepseek.com | deepseek-chat |
| OpenRouter | openrouter.ai | openrouter/auto |
| Groq | api.groq.com | llama-3.3-70b-versatile |
| Mistral | api.mistral.ai | mistral-large-latest |
| xAI · Grok | api.x.ai | grok-2-latest |
| Moonshot · Kimi | api.moonshot.cn | moonshot-v1-8k |
| Alibaba · Qwen | dashscope | qwen-max |
| Z.ai · GLM | api.z.ai | glm-4.6 |
| MiniMax | api.minimax.io | MiniMax-M2 |
| LM Studio | 127.0.0.1:1234 | auto |
| Ollama | 127.0.0.1:11434 | auto |
| Custom | ระบุเอง | ระบุเอง |

ชื่อ model เป็นเพียงค่าเริ่มต้น — พิมพ์ชื่ออื่นทับได้เสมอถ้า account เข้าถึงได้

### 4.3 ตั้ง API key ผ่าน environment

ถ้าไม่อยากเก็บ key ในไฟล์ ใช้ env แทนได้:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
mybot
```

หรือใช้ OpenAI:

```bash
export OPENAI_API_KEY=sk-...
mybot -m openai/gpt-4o-mini
```

### 4.4 เปิด TUI

```bash
mybot                  # เปิดหน้าจอสนทนา
mybot "explain this repo"   # เปิดพร้อม prompt แรก
mybot -c               # ต่อ session ล่าสุดในโฟลเดอร์นี้
mybot version          # ดูเวอร์ชันและ build info
mybot update --check   # เช็กว่ามี release ใหม่ไหม
```

### 4.5 เปลี่ยน model ระหว่างใช้งาน

พิมพ์ `/model` ใน TUI — จะเปิดรายการ provider/model ทั้งหมด แล้วถาม API ของแต่ละ
provider เพื่อเติม model ที่มีจริงในบัญชีให้อัตโนมัติ

```
╭ switch model ───────────────────────────────────────╮
│ search type to filter…        (18 models · loading…) │
│                                                      │
│ ▸ anthropic   claude-sonnet-4-5        current       │
│   anthropic   claude-opus-4-1                        │
│   lmstudio    qwen2.5-0.5b-instruct    needs key     │
│   openai      gpt-4o-mini              needs key     │
│ ↑↓ move · Enter apply · Esc close · type to filter   │
╰──────────────────────────────────────────────────────╯
```

- พิมพ์เพื่อกรองได้ทันที เช่น `sonnet`, `deepseek/`, `gpt`
- รายการที่ API ตอบกลับมาจะถูกเติมไว้บนสุด และเก็บ model ที่ตั้งไว้ใน config ไว้ด้วย
- ถ้าพิมพ์ชื่อ model ที่ไม่มีในรายการแล้วกด Enter จะใช้ค่านั้นเป็น model id ตรง ๆ
- provider ที่ยังไม่ได้ใส่ key จะขึ้น `needs key` แต่ยังเลือกได้ (จะ error ตอนส่งจริง)
- เลือกแล้ว Enter → สลับทันทีโดยไม่ต้องออกจากโปรแกรม
- การเปลี่ยนจะถูกบันทึกเป็น `default_provider` ใน `config.toml`
- บทสนทนายังอยู่ครบ สลับ model กลางบทสนทนาได้

### 4.6 ใช้แบบ headless

```bash
mybot exec "explain this repo" --output-format text
mybot exec - < prompt.txt
mybot exec "refactor this" --output-format json
```

### 4.7 ใช้โมเดล local (LM Studio / Ollama)

เลือกจากหน้าจอ setup ได้เลย หรือเพิ่มผ่านคำสั่ง:

```bash
# LM Studio (port ปกติ 1234)
mybot provider add lmstudio --kind openai \
  --base-url http://127.0.0.1:1234/v1 --model auto

# Ollama
mybot provider add ollama --kind openai \
  --base-url http://127.0.0.1:11434/v1 --model auto
```

จากนั้น:

```bash
mybot -m lmstudio/auto
mybot -m ollama/auto
```

`model = "auto"` จะถาม `/v1/models` ว่ามี chat model ตัวไหนโหลดอยู่
(ข้าม embedding-only model) ถ้าหาไม่เจอให้ระบุชื่อ model ตรงๆ

---

## 5. การตั้งค่า

ไฟล์หลัก: `~/.mybot/config.toml`
เปลี่ยน root ได้ด้วย `MYBOT_HOME`

```toml
default_provider = "anthropic"

[providers.anthropic]
kind = "anthropic"
base_url = "https://api.anthropic.com"
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4-5"
max_tokens = 8192
# thinking_budget = 4096
# max_retries = 2
# retry_base_delay_ms = 500
# retry_max_delay_ms = 8000

[providers.openai]
kind = "openai"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
model = "gpt-4o-mini"

[permissions]
mode = "auto"

[[permissions.rules]]
tool = "bash"
matcher = "command"
pattern = "cargo test*"
behavior = "allow"
```

ลำดับความสำคัญ: CLI flags > environment (`MYBOT_PROVIDER`, `MYBOT_MODEL`) > config

ถ้าไม่มี API key จะจบด้วย exit code 3 และบอกชื่อ environment variable ที่ต้องตั้ง
ส่วน base URL ที่เป็น localhost ไม่ต้องใช้ key

### คำสั่งจัดการ provider

```bash
mybot provider list
mybot provider add local --kind openai --base-url http://localhost:11434/v1 --model qwen2.5-coder
mybot provider use local
mybot provider test local
mybot provider remove local
```

### Retries

คำขอที่ล้มเหลวก่อนเริ่ม stream จะลองใหม่สูงสุด `max_retries` ครั้ง (ค่าเริ่มต้น 2)
ด้วย exponential backoff จาก `retry_base_delay_ms` ถึง `retry_max_delay_ms`
ถ้า server ส่ง `Retry-After` มา จะใช้ค่านั้นแทน

ลองใหม่เฉพาะ 429, 5xx และ connection error — 4xx อื่นและ auth error จะ fail ทันที
และจะไม่ retry หลังเริ่ม stream แล้ว เพื่อไม่ให้ข้อความซ้ำ

---

## 6. TUI

### ปุ่มลัด

| ปุ่ม | การทำงาน |
| --- | --- |
| `Enter` | ส่งข้อความ |
| `Shift+Enter` / `Ctrl+J` | ขึ้นบรรทัดใหม่ |
| `Esc` | ยกเลิก turn ที่กำลังทำงาน หรือล้างช่องพิมพ์ |
| `Ctrl+C` | ยกเลิก turn; ถ้าไม่ได้ทำงานอยู่จะออก |
| `Ctrl+D` | ออกเมื่อช่องพิมพ์ว่าง |
| `Up` / `Down` | เรียก history เมื่อช่องว่าง หรือเลื่อน cursor |
| `PgUp` / `PgDn` | เลื่อน transcript |
| `a` / `A` / `d` | อนุญาตครั้งเดียว / อนุญาตตลอด / ปฏิเสธ (ตอนถาม permission) |
| `/` | เปิด command palette (พิมพ์ต่อเพื่อกรอง) |

### Command palette

พิมพ์ `/` ในช่องพิมพ์ จะมีรายการคำสั่งขึ้นมาเหนือช่องพิมพ์ พร้อมคำอธิบาย

- พิมพ์ต่อเพื่อกรอง — จับทั้งชื่อ, alias และคำอธิบาย
- `↑` / `↓` เลื่อน, `Tab` เติมชื่อคำสั่ง, `Enter` รัน, `Esc` ปิด
- คำสั่งที่แก้ session (`/new`, `/resume`, `/compact`, `/retry`) จะบอกว่า
  "waits for the turn" ถ้ามี turn กำลังทำงานอยู่
- รายการเดียวกับ `/help` เพราะอ่านจาก catalog เดียวกัน

### Slash commands

| คำสั่ง | การทำงาน |
| --- | --- |
| `/help` | แสดงคำสั่งและปุ่มลัด |
| `/new` หรือ `/clear` | เริ่ม session ใหม่ |
| `/model [provider/model]` | เปิด picker เลือก model (กรองได้, ดึงรายการจริงจาก API) |
| `/permissions [ask\|auto\|full]` | ดูหรือเปลี่ยน permission mode (บันทึกทันที) |
| `/tools` | แสดง tools ที่โมเดลเรียกได้ พร้อมโหมดการทำงาน |
| `/context` | แสดงขนาด system prompt, บทสนทนา, tokens |
| `/sessions` | แสดง session ล่าสุด 10 รายการ |
| `/resume <id>` | โหลด session เก่าเข้ามาในหน้าต่างนี้ (ใส่แค่ prefix ก็ได้) |
| `/status` | แสดง provider, model, permission, session, tokens |
| `/version` | แสดงเวอร์ชัน, commit, target และ path ของ binary |
| `/compact` | สรุปบทสนทนาและแทน history ด้วย summary |
| `/retry` | ส่ง prompt ล่าสุดซ้ำ |
| `/stop` หรือ `/cancel` | ยกเลิก turn ที่กำลังทำงาน |
| `/export [path]` | เขียน transcript เป็นไฟล์ markdown |
| `/config` | แสดง path ของ config, providers และ rules |
| `/doctor` | ตรวจ config และ integrations |
| `/keys` หรือ `/hotkeys` | แสดงปุ่มลัด |
| `/skills` | แสดง skills และ plugin commands |
| `/skill <name>` | โหลด skill แล้วเริ่ม turn ทันที |
| `/agents` | แสดง sub-agent definitions |
| `/memory` | แสดงรายการบันทึก; `/memory show <name>`, `/memory delete <name>` |
| `/remember <text>` | บันทึกโน้ตโปรเจกต์อย่างเร็ว |
| `/mcp` | แสดง MCP servers และ tools ที่เชื่อมได้ |
| `/team` | แสดงกระดานงานและสมาชิกในทีม |
| `/init` | สร้าง `AGENTS.md` ในโฟลเดอร์ปัจจุบัน |
| `/quit` หรือ `/exit` | ออก |

### `/compact` ทำงานอย่างไร

1. ส่งบทสนทนาทั้งหมด (แบบ flatten เป็น transcript เดียว) ให้โมเดลปัจจุบันสรุป
2. แทน history ในหน่วยความจำด้วยข้อความ summary เดียว
3. เขียน entry `compact` ลง session ledger
4. แสดงเส้นแบ่งใน transcript: `── compacted N messages → summary · ~X → ~Y tokens ──`

สิ่งที่ควรรู้:

- ใช้โมเดลเพิ่มหนึ่งครั้ง (มีค่าใช้จ่ายตาม provider)
- กด `Esc` ระหว่างทำงานเพื่อยกเลิกได้; ถ้ายกเลิกหรือ error history เดิมจะไม่เปลี่ยน
- ถ้า summary ชน output limit จะยังใช้ summary นั้นและติดป้าย `(summary truncated)`
- บทสนทนายาวมากจะถูกตัดก่อนสรุป โดยเก็บข้อความแรกและช่วงท้ายล่าสุด
- เมื่อ resume ด้วย `-c` หรือ `--session` จะเห็นเฉพาะ summary ไม่ใช่ประวัติเก่าทั้งหมด
- transcript เดิมยังแสดงอยู่ในหน้าจอ แต่โมเดลจะไม่เห็นแล้วหลัง compact

---

## 7. Sessions

- เก็บใน `~/.mybot/sessions/<id>.jsonl` (override ด้วย `MYBOT_HOME`)
- เป็น append-only ledger: header + ข้อความแต่ละรายการ
- `mybot -c` ต่อ session ล่าสุดที่ cwd ตรงกัน
- `mybot --session <id>` เปิด session ที่ระบุ (รองรับ id แบบย่อ)
- `/sessions` ใน TUI ดูรายการล่าสุด
- `/new` สร้าง ledger ใหม่
- `/compact` เขียน entry `compact`; ตอน replay จะล้างข้อความก่อนหน้าและเริ่มจาก summary

---

## 8. Tools

| Tool | หน้าที่ | ขีดจำกัดโดยประมาณ |
| --- | --- | --- |
| `read` | อ่านไฟล์พร้อมเลขบรรทัด | 2,000 บรรทัด หรือ 50 KB ต่อครั้ง |
| `write` | สร้าง/เขียนทับไฟล์ทั้งไฟล์ | สร้าง parent directory ให้ |
| `edit` | แทนข้อความแบบ exact match | ต้อง match ไม่ซ้ำ; มี fuzzy fallback |
| `bash` | รันคำสั่งใน project | timeout 120s (สูงสุด 300s) |
| `grep` | ค้นหาเนื้อหาไฟล์ | 100 matches (สูงสุด 1,000), 16 KB output |
| `glob` | หาไฟล์ตาม pattern | 200 files (สูงสุด 2,000) |
| `todowrite` | จัดการ todo list | แสดงใน TUI |
| `skill` | โหลด skill ตามชื่อ | ตามเนื้อหาใน SKILL.md |
| `memory` | อ่าน/เขียนบันทึกโปรเจกต์ | list/read/write/delete |
| `agent` | มอบงานให้ sub-agent | สูงสุด 24 model turns |
| `mcp__*` | tool จาก MCP server | ตามที่ server กำหนด |
| `task_create` / `task_list` / `task_update` | กระดานงานของทีม | ใช้ร่วมกับ teammate |
| `spawn_teammate` | เปิด teammate ทำงานเบื้องหลัง | สูงสุด 32 model turns |
| `send_message` / `check_messages` | กล่องข้อความในทีม | `wait_secs` 0-300 |

- `read` ให้เลขบรรทัดและตัดบรรทัดที่ยาวเกิน 2,000 ตัวอักษร
- `edit` ปฏิเสธถ้า match หลายที่ เพื่อไม่แก้ผิดตำแหน่ง
- `bash` รวม stdout/stderr, บอก exit code และเวลาเมื่อเกิน 1 วินาที
- output ที่ยาวเกิน 22 KB จะถูกเก็บหัว+ท้าย และบันทึกไฟล์เต็มไว้ให้

---

## 9. Memory (ความจำโปรเจกต์)

mybot เก็บบันทึกระยะยาวแยกตามโปรเจกต์ไว้ที่
`~/.mybot/memory/<project>-<hash>/` เพื่อให้ session ใหม่จำ decisions และ
ข้อจำกัดของโปรเจกต์ได้โดยไม่ต้องเล่าใหม่

- หนึ่งบันทึก = หนึ่งไฟล์ `.md` พร้อม frontmatter: `name`, `title`,
  `description`, `type`, `updated`
- `type` เป็นหนึ่งใน `user`, `feedback`, `project`, `reference`
- `MEMORY.md` คือ index ที่ถูกใส่ใน system prompt อัตโนมัติ (ไม่เกิน 8 KB)
- โมเดลอ่าน/เขียนผ่าน tool `memory` (action: `list`, `read`, `write`, `delete`)
- ใน TUI ใช้ `/memory` ดูรายการ, `/memory show <name>`, `/memory delete <name>`
  และ `/remember <ข้อความ>` สำหรับบันทึกเร็ว

ข้อควรระวัง: อย่าเก็บ API key, token, รหัสผ่าน หรือข้อมูลส่วนตัวลงใน memory
เพราะเนื้อหาจะถูกส่งให้โมเดลทุกครั้งที่เริ่ม session

---

## 10. MCP servers

mybot เชื่อมต่อ MCP (Model Context Protocol) ได้สองแบบ:

- **stdio**: spawn process แล้วคุย JSON-RPC ทาง stdin/stdout
- **streamable HTTP**: POST ไปที่ URL พร้อม session header

ตำแหน่ง config (โหลดทั้งสองที่; ชื่อซ้ำให้ project ชนะ):

```
~/.mybot/mcp.toml          # ใช้ทุกโปรเจกต์
<project>/.mybot/mcp.toml  # เฉพาะโปรเจกต์นี้
```

```toml
[servers.files]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
env = { DEBUG = "0" }
timeout_secs = 60

[servers.docs]
url = "https://mcp.example.com/mcp"
headers = { Authorization = "Bearer ..." }
```

- ตั้ง `command` หรือ `url` อย่างใดอย่างหนึ่งเท่านั้น
- tool ที่ได้จะชื่อ `mcp__<server>__<tool>` และใช้กฎ permission ได้เหมือน tool ปกติ
- `mybot mcp list` แสดง config และ tool ที่เชื่อมได้จริง
- `mybot mcp test <name>` ทดสอบ server เดียว
- `/mcp` ใน TUI แสดงสถานะ
- server ที่เริ่มไม่สำเร็จจะแสดง warning และข้ามไป ไม่ทำให้ session ล้ม

---

## 11. Sub-agents และทีม

### 11.1 Sub-agent

Sub-agent คือ agent ย่อยที่มี context แยกและ tool subset ของตัวเอง เหมาะกับงาน
สำรวจหรือรีวิวแคบ ๆ ที่ไม่ควรกิน context หลัก

```
~/.mybot/agents/<name>.md          # ใช้ทุกโปรเจกต์
<project>/.mybot/agents/<name>.md  # เฉพาะโปรเจกต์ (ชนะถ้าชื่อซ้ำ)
```

```markdown
---
name: reviewer
description: Review a diff for correctness
tools: read, grep, glob
---
You are a careful reviewer. Report findings as path:line.
```

- โมเดลเรียกผ่าน tool `agent` พร้อม `prompt` และ `subagent_type` (ถ้ามี)
- sub-agent ทำงานสูงสุด 24 model turns และตอบกลับเป็นรายงานพร้อม tool trace
- sub-agent ถามผู้ใช้ไม่ได้; ถ้าติดปัญหาจะหยุดและอธิบาย
- `/agents` แสดง definition ที่โหลดไว้

### 11.2 ทีม (teammates)

เมื่องานแบ่งเป็นส่วนอิสระได้ ตัวหลัก (lead) เปิด teammate ทำงานเบื้องหลังได้
โดยใช้กระดานงานและ inbox ร่วมกัน

| Tool | หน้าที่ |
| --- | --- |
| `task_create` | เพิ่มงานลงกระดาน |
| `task_list` | ดูกระดานทั้งหมด |
| `task_update` | เปลี่ยน status (`pending`, `in_progress`, `completed`) หรือ owner |
| `spawn_teammate` | เปิด teammate พร้อมชื่อ, role, prompt และ tool subset |
| `send_message` | ส่งข้อความถึง `lead` หรือ teammate คนอื่น |
| `check_messages` | อ่าน inbox; ใส่ `wait_secs` เพื่อรอ teammate ทุกคนเสร็จก่อน |

ลำดับการทำงานโดยทั่วไป:

1. lead สร้าง task แล้ว assign owner
2. lead เรียก `spawn_teammate` หนึ่งคนต่องาน
3. teammate อัปเดตกระดานเป็น `in_progress` → ทำงาน → `completed`
4. teammate ส่งรายงานเข้า inbox ของ lead
5. lead เรียก `check_messages` (อาจใส่ `wait_secs`) แล้วสรุปให้ผู้ใช้

ระหว่างที่ทีมทำงาน TUI จะแสดงแผง `team` เหนือช่องพิมพ์ พร้อมสถานะและ tool
ล่าสุดของแต่ละคน และ `/team` พิมพ์กระดานเต็ม

ข้อควรระวัง: teammate แต่ละคนคือบทสนทนาโมเดลแยก จึงมีค่าใช้จ่ายเพิ่ม ควรใช้
เมื่องานแยกส่วนได้จริงและมีประโยชน์จากการทำพร้อมกัน

---

## 12. Skills และ Plugins

### 9.1 Skill คืออะไร

Skill คือชุดคำสั่ง (markdown) ที่โมเดลโหลดมาใช้เมื่อต้องทำงานเฉพาะทาง
เช่น รีวิวโค้ด, เขียน migration, ตรวจ security — ไม่ต้องพิมพ์ instruction ซ้ำทุกครั้ง

โครงสร้าง:

```
~/.mybot/skills/<name>/SKILL.md          # ใช้ได้ทุกโปรเจกต์
<project>/.mybot/skills/<name>/SKILL.md  # เฉพาะโปรเจกต์นี้ (ชนะถ้าชื่อซ้ำ)
```

ตัวอย่าง `SKILL.md`:

```markdown
---
name: rust-review
description: Review Rust code for correctness and idioms
---
# Rust review

1. อ่าน `git diff`
2. ตรวจ error handling และ ownership
3. รายงานเป็น `path:line` พร้อมข้อเสนอ
```

- `name` ต้องเป็นตัวพิมพ์เล็ก ตัวเลข `-` หรือ `_`
- `description` คือสิ่งที่โมเดลเห็นใน system prompt ใช้บอกว่าเมื่อไรควรโหลด
- เนื้อหาหลัง `---` คือคำสั่งเต็มที่โมเดลจะได้รับ

### 9.2 ใช้ skill

โมเดลจะเห็นรายการ skill ทั้งหมดและเรียก tool `skill` เองเมื่อจำเป็น
หรือสั่งตรงๆ จาก TUI:

```
/skills            # ดูรายการทั้งหมด
/skill rust-review # โหลด skill แล้วเริ่มงานทันที
```

จาก command line:

```bash
mybot skill list
mybot skill show rust-review
mybot skill new my-skill            # สร้างโครงในโปรเจกต์
mybot skill new my-skill --scope user  # สร้างใน ~/.mybot/skills
```

### 9.3 Plugin

Plugin คือโฟลเดอร์ที่รวม skill และ slash command ไว้ด้วยกัน แจกจ่ายได้

```
~/.mybot/plugins/<plugin>/          # user-wide
<project>/.mybot/plugins/<plugin>/  # project (ชนะถ้าชื่อซ้ำ)
  plugin.toml
  skills/<name>/SKILL.md
  commands/<name>.md
```

`plugin.toml`:

```toml
name = "code-review"
version = "0.1.0"
description = "Review changes for correctness and security"
```

`commands/<name>.md` กลายเป็น slash command ใน TUI — เนื้อหาไฟล์คือ prompt
และ `$ARGUMENTS` จะถูกแทนด้วยข้อความที่พิมพ์ตามหลัง:

```markdown
---
description: Review the current diff
---
Review the current changes and report issues with `path:line`.

$ARGUMENTS
```

### 9.4 จัดการ plugin

```bash
mybot plugin list
mybot plugin add ./path/to/plugin            # ติดตั้งลง ~/.mybot/plugins
mybot plugin add ./path/to/plugin --project  # ติดตั้งลง .mybot/plugins
mybot plugin remove code-review
mybot plugin command review "focus on src/"  # ดู prompt ที่จะส่ง
```

ใน TUI ใช้ `/review` ได้ทันทีหลังติดตั้ง (ดูรายการด้วย `/skills`)

### 9.5 ตัวอย่างที่มากับ repo

`examples/plugins/code-review/` มี plugin ตัวอย่างพร้อม:
- command `/review`
- skill `security-review`

ลองได้เลย:

```bash
mybot plugin add examples/plugins/code-review --project
```

## 13. Permissions

| โหมด | พฤติกรรม |
| --- | --- |
| `ask` | ถามทุก tool call |
| `auto` | คำสั่งอ่านอย่างเดียวและแก้ไฟล์ในโปรเจกต์ทำได้; ที่เหลือถาม |
| `full` | ทำทุกอย่าง ยกเว้นคำสั่งทำลายที่ hard-deny |

คำสั่งที่ถูกปฏิเสธเสมอทุกโหมด: `rm -rf /`, `rm -rf ~`, `mkfs`, `dd ... of=/dev/...`

กฎใน config:

```toml
[[permissions.rules]]
tool = "bash"
matcher = "command"   # tool | command | path
pattern = "cargo test*"
behavior = "allow"    # allow | deny | ask
```

การเลือก "always allow" ใน TUI จะเพิ่มกฎลง `config.toml`

โหมด headless ไม่สามารถถามได้ ดังนั้น tool call ที่ต้องอนุมัติจะถูกปฏิเสธ
ยกเว้นถูก allow ด้วยโหมดหรือกฎ

---

## 14. Headless mode

```bash
mybot exec "<prompt>"
mybot exec - < prompt.txt
mybot exec "<prompt>" --output-format json
mybot exec "<prompt>" --output-format stream-json
```

Flags:

- `--output-format text|json|stream-json`
- `-o/--output-last-message <file>` เขียนข้อความสุดท้ายลงไฟล์
- `--timeout <seconds>` (ค่าเริ่มต้น 600)
- `--max-steps <n>` จำกัดจำนวน model turns
- `-m/--model provider/model`
- `--permission ask|auto|full`
- `--system-prompt`, `--system-prompt-file`
- `--append-system-prompt`, `--append-system-prompt-file`
- `--cwd <dir>`
- `-c/--continue`, `--session <id>`

Exit codes:

| Code | ความหมาย |
| --- | --- |
| 0 | สำเร็จ |
| 2 | usage ผิด |
| 3 | config/auth |
| 4 | runtime |
| 6 | timeout |
| 7 | เกิน step limit |
| 130 | ถูกยกเลิก |

ตัวอย่าง CI:

```bash
mybot exec "run the tests and summarize failures" \
  --permission auto \
  --output-format json \
  --timeout 300
```

ดู retry events ใน stream-json:

```bash
mybot exec "refactor this" --output-format stream-json \
  | jq -c 'select(.type=="retry")'
```

---

## 15. ประสิทธิภาพ

สรุปจาก [performance.md](performance.md) บน Intel i5-13400F:

- startup `mybot --help`: median 1.26 ms
- memory สูงสุดตอนเริ่ม: ~5.9 MiB
- binary ขนาด 14.2 MiB (strip เหลือ 11.0 MiB)
- SSE decode: ~480 MiB/s
- exact edit match ในไฟล์ 100 KB: ~9.8 µs

ตัวเลขจริงขึ้นกับโมเดล, เครือข่าย, ขนาด history และเครื่องที่ใช้

---

## 16. แก้ปัญหา

**exit code 3 — missing API key**
ตั้ง environment variable ตามชื่อที่ error บอก หรือเพิ่ม `api_key` ใน config
สำหรับ localhost ไม่ต้องใช้ key

**429 / 5xx บ่อย**
mybot retry ให้ 2 ครั้งโดยอัตโนมัติ เพิ่ม `max_retries` ใน config ได้
ถ้า provider ส่ง `Retry-After` จะรอตามนั้น

**local model ใช้ไม่ได้**
ตรวจว่า server เปิดอยู่และเป็น OpenAI-compatible จริง
ลอง `curl http://127.0.0.1:1234/v1/models`
ถ้า `model = "auto"` หา chat model ไม่เจอ ให้ระบุชื่อ model ตรงๆ

**คำสั่ง bash ค้าง**
tool มี timeout 120s (สูงสุด 300s) และ `Esc` ยกเลิกได้
บน Windows การ kill process ลูกอาจไม่ทำงาน

**session ไม่พบ**
ใช้ `mybot -c` ในโฟลเดอร์เดิม หรือระบุ id จาก `/sessions` / ไฟล์ใน `~/.mybot/sessions`

**build บน ARM ไม่ผ่าน**
ตรวจว่ามี `cmake`, C compiler และ perl ครบ
ถ้าเป็นปัญหา GLIBC ให้ build บน distro รุ่นเดียวกันกับเครื่องปลายทาง หรือใช้ musl target

**Windows: bash not found**
ใช้ WSL2 หรือติดตั้ง Git Bash และเพิ่มเข้า PATH

---

## 17. License

MIT — ดู [LICENSE](../LICENSE)
