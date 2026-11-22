#!/usr/bin/env node
// Thin wrapper so `npx mybot-cli` and a global `mybot` both work.
"use strict";

const { spawn, spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const binary = path.join(__dirname, process.platform === "win32" ? "mybot.exe" : "mybot");

// npm 11+ blocks install scripts by default, so the download also happens
// here on first run.
if (!fs.existsSync(binary)) {
  const installer = path.join(__dirname, "..", "scripts", "install.js");
  spawnSync(process.execPath, [installer], { stdio: "inherit" });
}

if (!fs.existsSync(binary)) {
  console.error(
    [
      "mybot: the native binary is missing.",
      "",
      "Install with the shell script instead:",
      "  curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh | sh",
      "",
      "Or build from source:",
      "  cargo install --git https://github.com/seashyne/mybot-cli",
    ].join("\n"),
  );
  process.exit(1);
}

const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });

child.on("error", (error) => {
  console.error(`mybot: cannot run ${binary}: ${error.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code === null ? 1 : code);
});
