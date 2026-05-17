#!/usr/bin/env node
"use strict";

const { execFileSync } = require("child_process");
const path = require("path");
const os = require("os");

const PLATFORM_PACKAGES = {
  "win32-x64":    ["@mana-app/dev-win32-x64",   "mana-dev.exe"],
  "darwin-x64":   ["@mana-app/dev-darwin-x64",  "mana-dev"],
  "darwin-arm64": ["@mana-app/dev-darwin-arm64", "mana-dev"],
  "linux-x64":    ["@mana-app/dev-linux-x64",   "mana-dev"],
  "linux-arm64":  ["@mana-app/dev-linux-arm64",  "mana-dev"],
};

const key = `${os.platform()}-${os.arch()}`;
const entry = PLATFORM_PACKAGES[key];

if (!entry) {
  process.stderr.write(`[mana-dev] Unsupported platform: ${key}\n`);
  process.exit(1);
}

const [pkg, binName] = entry;

let pkgDir;
try {
  pkgDir = path.dirname(require.resolve(`${pkg}/package.json`, { paths: [__dirname] }));
} catch {
  process.stderr.write(`[mana-dev] Could not find platform package ${pkg}.\n`);
  process.stderr.write(`           Try: npm install @mana-app/dev\n`);
  process.exit(1);
}

const bin = path.join(pkgDir, binName);

try {
  execFileSync(bin, process.argv.slice(2), { stdio: "inherit" });
} catch (e) {
  process.exit(e.status ?? 1);
}
