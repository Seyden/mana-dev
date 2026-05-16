# mana-dev

Build toolchain for [Mana](https://github.com/Mana-iOS/) source packages. Compiles TypeScript sources into distributable `.mana` bundles and exposes an HTTP server that the Mana app can point to during development — serving your sources and receiving log output in real time.

## Installation

```bash
npm install -D @mana-app/dev
```

No additional runtime dependencies. The correct native binary for your platform is installed automatically.

## Usage

### Build

Compile all sources in `src/` and output `.mana` files to `dist/`:

```bash
mana-dev build
```

### Watch

Rebuild automatically on file changes and serve via HTTP:

```bash
mana-dev serve --watch
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `SOURCE` | `src/` | Source directory containing TypeScript files |
| `--output`, `-o` | `dist/` | Output directory for `.mana` files |
| `--watch`, `-w` | `false` | Rebuild on file changes |
| `--port`, `-p` | `8080` | HTTP port (serve command only) |

CLI flags override config file values.

## Project config

Add a `"mana"` key to your `package.json` — no extra config file needed:

```json
{
  "name": "my-sources",
  "repositoryName": "My Source Repository",
  "thumbnail": "https://example.com/thumbnail.png",
  "mana": {
    "src": "src/",
    "out": "dist/",
    "target": "es2020",
    "minify": true,
    "platform": "browser"
  }
}
```

Or use a standalone `mana.json` at the project root (takes priority over `package.json`):

```json
{
  "src": "src/",
  "out": "dist/",
  "target": "es2020",
  "minify": true,
  "platform": "browser",
  "repositoryName": "My Source Repository",
  "thumbnail": "https://example.com/thumbnail.png"
}
```

### Config priority (highest → lowest)

1. CLI flags (`--output`, `SOURCE` positional arg)
2. `mana.json`
3. `package.json` `"mana"` key
4. Built-in defaults

## Output

After a successful build, the output directory contains:

```
dist/
├── sources/
│   ├── SourceName.mana      ← bundled + minified JS per source
│   └── ...
└── sources.json             ← repository index
```

`sources.json` example:

```json
{
  "repositoryName": "My Source Repository",
  "thumbnail": "https://example.com/thumbnail.png",
  "sources": [
    {
      "name": "SourceName",
      "environment": "source",
      "intents": 12345,
      "hash": "a1b2c3d4..."
    }
  ]
}
```

## Writing a source

See the [@mana-app/types](https://github.com/Mana-iOS/mana-types) repository for full documentation on writing sources and trackers, available interfaces, and required methods.

```bash
npm install -D @mana-app/types
```

## Development (contributing)

### Prerequisites

- [Rust](https://rustup.rs/) stable
- [Go](https://go.dev/) 1.21+

### Build from source

```bash
# Build the Rust binary (mana-dev)
cargo build --release --bin mana-dev

# Build the Go watcher (mana-watcher)
cd watcher && go build -o mana-watcher .
```

### Project layout

```
src/                    Rust source (mana-dev binary)
watcher/                Go source (mana-watcher — esbuild wrapper)
runtime/                JS files embedded into the Rust binary at compile time
  emulator.js
  target_processor.js
npm/                    npm package manifests
  mana-dev/             Main published package
  mana-dev-win32-x64/   Platform-specific binary packages
  mana-dev-darwin-arm64/
  mana-dev-linux-x64/
  ...
.github/workflows/      CI — builds all platforms and publishes on git tag
```

### Releasing

Push a version tag to trigger the CI release workflow:

```bash
git tag v1.0.0
git push origin v1.0.0
```

CI builds `mana-dev` and `mana-watcher` for all supported platforms, copies the binaries into the platform npm packages, and publishes everything to npm.

Requires an `NPM_TOKEN` secret configured in the repository settings.

## Supported platforms

| Platform | Package |
|----------|---------|
| Windows x64 | `@mana-app/dev-win32-x64` |
| macOS x64 | `@mana-app/dev-darwin-x64` |
| macOS ARM64 (Apple Silicon) | `@mana-app/dev-darwin-arm64` |
| Linux x64 | `@mana-app/dev-linux-x64` |
| Linux ARM64 | `@mana-app/dev-linux-arm64` |
