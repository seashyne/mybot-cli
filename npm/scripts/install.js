#!/usr/bin/env node
// Downloads the matching prebuilt mybot binary from GitHub Releases.
"use strict";

const fs = require("node:fs");
const http = require("node:http");
const https = require("node:https");
const path = require("node:path");

const REPO = "seashyne/mybot-cli";
const pkg = require("../package.json");
const TARGET = `${__dirname}/../bin`;

function target() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "linux" && arch === "x64") return "x86_64-unknown-linux-gnu";
  if (platform === "linux" && arch === "arm64") return "aarch64-unknown-linux-gnu";
  if (platform === "darwin" && arch === "x64") return "x86_64-apple-darwin";
  if (platform === "darwin" && arch === "arm64") return "aarch64-apple-darwin";
  return null;
}

function download(url, destination, redirects = 0) {
  return new Promise((resolve, reject) => {
    if (redirects > 5) {
      reject(new Error("too many redirects"));
      return;
    }

    const client = url.startsWith("http:") ? http : https;
    client
      .get(url, { headers: { "user-agent": `mybot-cli/${pkg.version}` } }, (response) => {
        const status = response.statusCode || 0;

        if (status >= 300 && status < 400 && response.headers.location) {
          response.resume();
          download(response.headers.location, destination, redirects + 1)
            .then(resolve)
            .catch(reject);
          return;
        }

        if (status !== 200) {
          response.resume();
          reject(new Error(`HTTP ${status} for ${url}`));
          return;
        }

        const file = fs.createWriteStream(destination, { mode: 0o755 });
        response.pipe(file);
        file.on("finish", () => file.close(() => resolve()));
        file.on("error", reject);
      })
      .on("error", reject);
  });
}

async function main() {
  const triple = target();
  if (!triple) {
    console.warn(
      `mybot-cli: no prebuilt binary for ${process.platform}/${process.arch}.\n` +
        "Install from source instead: cargo install --git https://github.com/" +
        REPO,
    );
    return;
  }

  const version = process.env.MYBOT_VERSION;
  const base =
    process.env.MYBOT_BASE_URL ||
    (version
      ? `https://github.com/${REPO}/releases/download/${version}`
      : `https://github.com/${REPO}/releases/latest/download`);
  const url = `${base}/mybot-${triple}`;
  const destination = path.join(TARGET, process.platform === "win32" ? "mybot.exe" : "mybot");

  fs.mkdirSync(TARGET, { recursive: true });

  console.log(`mybot-cli: downloading ${triple}…`);
  try {
    await download(url, destination);
    fs.chmodSync(destination, 0o755);
    console.log(`mybot-cli: installed ${destination}`);
  } catch (error) {
    fs.rmSync(destination, { force: true });
    console.warn(
      `mybot-cli: download failed (${error.message}).\n` +
        "The npm package is installed but the binary is missing.\n" +
        "Install it with the shell script instead:\n" +
        "  curl -fsSL https://raw.githubusercontent.com/" +
        REPO +
        "/main/install.sh | sh",
    );
  }
}

main();
