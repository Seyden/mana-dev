#!/usr/bin/env node
const fs = require("fs");
const path = require("path");

const newVersion = process.argv[2];
if (!newVersion || !/^\d+\.\d+\.\d+$/.test(newVersion)) {
  console.error("Usage: node scripts/bump-version.js <version>");
  console.error("Example: node scripts/bump-version.js 0.2.0");
  process.exit(1);
}

const packages = [
  ".",
  "npm/mana-dev",
  "npm/mana-dev-win32-x64",
  "npm/mana-dev-darwin-x64",
  "npm/mana-dev-darwin-arm64",
  "npm/mana-dev-linux-x64",
  "npm/mana-dev-linux-arm64",
];

const root = path.resolve(__dirname, "..");

for (const pkg of packages) {
  const pkgPath = path.join(root, pkg, "package.json");
  const json = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
  json.version = newVersion;
  if (json.optionalDependencies) {
    for (const key of Object.keys(json.optionalDependencies)) {
      json.optionalDependencies[key] = newVersion;
    }
  }
  fs.writeFileSync(pkgPath, JSON.stringify(json, null, 2) + "\n");
  console.log(`bumped ${json.name} → ${newVersion}`);
}
