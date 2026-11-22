---
layout: default
title: mybot cli
permalink: /
---

<section class="hero">
  <h1 class="hero-title">mybot cli</h1>
  <p class="hero-tagline">AI coding agent ใน terminal เขียนด้วย Rust</p>
  <p class="hero-sub">เร็ว เบา เป็น binary เดียว ใช้ได้ทั้งโมเดล cloud และโมเดล local</p>

  <div class="hero-actions">
    <a class="btn btn-primary" href="{{ '/install/' | relative_url }}">ติดตั้ง</a>
    <a class="btn" href="{{ '/manual/' | relative_url }}">คู่มือการใช้งาน</a>
    <a class="btn" href="https://github.com/seashyne/mybot-cli">GitHub</a>
  </div>

  <pre class="hero-code"><code>curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh \
  | sh</code></pre>
</section>

## ทำอะไรได้บ้าง

<div class="features">
  <div class="feature">
    <h3>TUI แบบ interactive</h3>
    <p>พิมพ์คุยกับโมเดล ดู tool call แบบเรียลไทม์ กด Esc เพื่อยกเลิกงานที่ค้างอยู่</p>
  </div>
  <div class="feature">
    <h3>Headless / CI</h3>
    <p><code>mybot exec</code> รับ prompt เดียวจบ รองรับ <code>--output-format json</code> และ exit code ที่ชัดเจน</p>
  </div>
  <div class="feature">
    <h3>Providers หลายเจ้า</h3>
    <p>Anthropic Messages API และ OpenAI-compatible ทุกเจ้า รวมถึง LM Studio และ Ollama ในเครื่อง</p>
  </div>
  <div class="feature">
    <h3>Tools ในตัว</h3>
    <p>อ่าน/เขียนไฟล์ แก้ไขแบบ exact และ fuzzy ค้นหา และรัน bash ภายใต้ระบบ permission</p>
  </div>
  <div class="feature">
    <h3>Sessions</h3>
    <p>ทุกบทสนทนาเก็บเป็น JSONL ต่อได้ด้วย <code>--continue</code> หรือ <code>--session</code></p>
  </div>
  <div class="feature">
    <h3>Context compaction</h3>
    <p><code>/compact</code> ย่อประวัติทั้งบทสนทนาเป็นสรุปสั้นๆ แล้วทำงานต่อได้ในหน้าต่างใหม่</p>
  </div>
  <div class="feature">
    <h3>Skills</h3>
    <p>ชุดคำสั่ง markdown ที่โมเดลโหลดเองเมื่อต้องทำงานเฉพาะทาง</p>
  </div>
  <div class="feature">
    <h3>Plugins</h3>
    <p>รวม skill และ slash command ไว้แจกจ่าย ติดตั้งด้วยคำสั่งเดียว</p>
  </div>
  <div class="feature">
    <h3>Setup ครั้งแรก</h3>
    <p>หน้าจอเลือก provider → ใส่ key → เลือก model แล้วเริ่มได้เลย</p>
  </div>
</div>

## เริ่มใช้งาน

รัน `mybot` ครั้งแรก จะมีหน้าจอ setup ให้เลือก provider → ใส่ key → เลือก model
แล้วเข้า TUI ทันที ไม่ต้องแก้ไฟล์ config เอง

```bash
mybot
```

## Providers

มี preset ให้เลือกในหน้าจอ setup:

<div class="providers">
  <span>Anthropic · Claude</span>
  <span>OpenAI · GPT</span>
  <span>Google Gemini</span>
  <span>DeepSeek</span>
  <span>OpenRouter</span>
  <span>Groq</span>
  <span>Mistral</span>
  <span>xAI · Grok</span>
  <span>Moonshot · Kimi</span>
  <span>Alibaba · Qwen</span>
  <span>Z.ai · GLM</span>
  <span>MiniMax</span>
  <span>LM Studio</span>
  <span>Ollama</span>
  <span>Custom endpoint</span>
</div>

สลับ model ระหว่างใช้งานได้ด้วย `/model` — เลือกจากรายการหรือพิมพ์ชื่อเอง
แล้วระบบบันทึกเป็นค่า default ให้

## ติดตั้ง

คำสั่งเดียว — ดาวน์โหลด binary ที่ build ไว้แล้ว:

```bash
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh | sh
```

หรือผ่าน npm:

```bash
npm install -g mybot-cli
```

หรือ build เองจาก source:

```bash
git clone https://github.com/seashyne/mybot-cli.git
cd mybot-cli && ./install.sh
```

จากนั้นตั้ง provider และเริ่มใช้งาน:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
mybot "อธิบายโครงสร้างโปรเจกต์นี้ให้หน่อย"
```

รายละเอียดทั้งหมด — ARM64, cross-compile, headless-only, Windows/WSL — อยู่ใน
[คู่มือติดตั้ง]({{ '/install/' | relative_url }})

## Skills และ Plugins

Skill คือไฟล์ markdown ที่บอกวิธีทำงานเฉพาะทาง โมเดลเห็นรายการทั้งหมดและโหลดเมื่อจำเป็น

```bash
mybot skill list
mybot skill new code-review --scope user
mybot skill show code-review
```

Plugin รวม skill และ slash command ไว้ด้วยกัน:

```
<plugin>/
  plugin.toml
  skills/<name>/SKILL.md
  commands/<name>.md     # กลายเป็น /<name> ใน TUI
```

```bash
mybot plugin add examples/plugins/code-review --project
mybot plugin list
```

ใน TUI: `/skills` ดูรายการ · `/skill <name>` เรียกใช้ · `/<command>` จาก plugin

## เอกสาร

| หน้า | เนื้อหา |
| --- | --- |
| [คู่มือติดตั้ง]({{ '/install/' | relative_url }}) | requirements, แพลตฟอร์มที่รองรับ, ARM64, Docker, update/uninstall |
| [คู่มือการใช้งาน (ไทย)]({{ '/manual/' | relative_url }}) | ทุกคำสั่ง, TUI keys, tools, permissions, sessions, การแก้ปัญหา |
| [Performance]({{ '/performance/' | relative_url }}) | benchmark, startup time, memory, วิธีวัดซ้ำ |

## แพลตฟอร์ม

| OS | สถานะ |
| --- | --- |
| Linux x86_64 | รองรับเต็ม (ทดสอบแล้ว) |
| Linux ARM64 | รองรับเต็ม (build + test ผ่าน Docker/QEMU) |
| macOS (Intel / Apple Silicon) | ควรใช้งานได้ — ยังไม่ได้ทดสอบบนเครื่องจริง |
| Windows | ใช้ผ่าน WSL2 หรือ Git Bash |
| Android (Termux) | ยังไม่รองรับ |

## License

MIT — ดูรายละเอียดใน [LICENSE](https://github.com/seashyne/mybot-cli/blob/main/LICENSE)
