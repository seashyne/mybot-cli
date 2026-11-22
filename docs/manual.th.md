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
- **Tools**: `read`, `write`, `edit`, `bash`, `grep`, `glob`, `todowrite`
- **Permissions**: โหมด `ask`, `auto`, `full` พร้อมกฎ allow/deny ที่บันทึกไว้
- **Sessions**: เก็บเป็น JSONL ใน `~/.mybot/sessions` ต่อด้วย `-c` หรือ `--session`
- **Context compaction**: `/compact` สรุปบทสนทนาแล้วแทน history ด้วย summary
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

### 3.1 สิ่งที่ต้องมี

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

### 3.2 ติดตั้งจาก source

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
```

หรือติดตั้งเข้า `~/.cargo/bin` ด้วย cargo:

```bash
cargo install --path .
```

หรือติดตั้งจาก Git โดยตรง:

```bash
cargo install --git https://github.com/seashyne/mybot-cli.git
```

### 3.3 build แบบไม่มี TUI (headless only)

เหมาะกับเซิร์ฟเวอร์หรือ CI ที่ไม่ต้องการ ratatui/crossterm:

```bash
cargo build --release --no-default-features
```

### 3.4 ติดตั้งบน ARM (Raspberry Pi / เซิร์ฟเวอร์ ARM64)

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

---

## 4. เริ่มต้นใช้งาน

### 4.1 ตั้ง API key

```bash
export ANTHROPIC_API_KEY=sk-ant-...
mybot
```

หรือใช้ OpenAI:

```bash
export OPENAI_API_KEY=sk-...
mybot -m openai/gpt-4o-mini
```

### 4.2 เปิด TUI

```bash
mybot                  # เปิดหน้าจอสนทนา
mybot "explain this repo"   # เปิดพร้อม prompt แรก
mybot -c               # ต่อ session ล่าสุดในโฟลเดอร์นี้
```

### 4.3 ใช้แบบ headless

```bash
mybot exec "explain this repo" --output-format text
mybot exec - < prompt.txt
mybot exec "refactor this" --output-format json
```

### 4.4 ใช้โมเดล local (LM Studio / Ollama)

เพิ่ม provider ครั้งเดียว:

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

### Slash commands

| คำสั่ง | การทำงาน |
| --- | --- |
| `/help` | แสดงคำสั่งและปุ่มลัด |
| `/new` หรือ `/clear` | เริ่ม session ใหม่ |
| `/model` | แสดง provider/model ปัจจุบัน |
| `/sessions` | แสดง session ล่าสุด 10 รายการ |
| `/status` | แสดง provider, model, permission, session, tokens |
| `/compact` | สรุปบทสนทนาและแทน history ด้วย summary |
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

- `read` ให้เลขบรรทัดและตัดบรรทัดที่ยาวเกิน 2,000 ตัวอักษร
- `edit` ปฏิเสธถ้า match หลายที่ เพื่อไม่แก้ผิดตำแหน่ง
- `bash` รวม stdout/stderr, บอก exit code และเวลาเมื่อเกิน 1 วินาที
- output ที่ยาวเกิน 22 KB จะถูกเก็บหัว+ท้าย และบันทึกไฟล์เต็มไว้ให้

---

## 9. Permissions

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

## 10. Headless mode

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

## 11. ประสิทธิภาพ

สรุปจาก [performance.md](performance.md) บน Intel i5-13400F:

- startup `mybot --help`: median 1.26 ms
- memory สูงสุดตอนเริ่ม: ~5.9 MiB
- binary ขนาด 14.2 MiB (strip เหลือ 11.0 MiB)
- SSE decode: ~480 MiB/s
- exact edit match ในไฟล์ 100 KB: ~9.8 µs

ตัวเลขจริงขึ้นกับโมเดล, เครือข่าย, ขนาด history และเครื่องที่ใช้

---

## 12. แก้ปัญหา

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

## 13. License

MIT — ดู [LICENSE](../LICENSE)
