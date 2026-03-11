# rvx Design Spec

`uvx` for Rust — download and run pre-built crate binaries. No Rust required.

## Problem

MCP servers distributed via `npx`/`uvx` drag in runtimes, burning RAM and startup time. Rust MCP servers are static binaries but have no easy install path — `cargo install` needs a toolchain, `cargo binstall` doesn't run binaries.

## Solution

`rvx <crate[@version]> [args...]` downloads a pre-built binary from crates.io and runs it. First run downloads (~seconds), subsequent runs exec from cache instantly.

## Architecture

Five modules, sequential flow: resolve → download → exec. No async runtime.

### Modules

**`cache`** — Manages `~/.rvx/`. Check/store/list/clean binaries.

```
~/.rvx/
  bin/
    <crate>-<version>          (executable)
  meta/
    <crate>-<version>.json     (source url, timestamp, checksum, crate version)
```

**`registry`** — Queries crates.io API + downloads `.crate` tarball to extract `Cargo.toml` for binstall metadata.

- `GET /api/v1/crates/{name}` — latest version, repository URL
- `GET /api/v1/crates/{name}/{version}` — specific version info
- Download `.crate` tarball, extract `Cargo.toml`, parse `[package.metadata.binstall]`

**`resolve`** — Finds the download URL.

1. Render binstall `pkg-url` template with version + target triple
2. Fallback: query GitHub Releases API, match artifact naming patterns
3. Return download URL + expected checksum if available

**`download`** — Fetches and extracts the binary.

- Download archive to temp dir
- Verify SHA256 checksum if available
- Extract from `.tar.gz`, `.tar.xz`, `.tar.zst`, `.zip`
- Search recursively for binary name in extracted contents
- Set executable bit, move to cache

**`exec`** — Replaces current process with cached binary.

- Unix: `CommandExt::exec` (replaces process, inherits stdio)
- Windows: spawn child, forward exit code

### Target Triple Detection

Compile-time via `cfg` attributes. Supported:

| OS + Arch | Triple |
|---|---|
| Linux x86_64 | `x86_64-unknown-linux-gnu` / `musl` |
| Linux aarch64 | `aarch64-unknown-linux-gnu` |
| macOS x86_64 | `x86_64-apple-darwin` |
| macOS aarch64 | `aarch64-apple-darwin` |
| Windows x86_64 | `x86_64-pc-windows-msvc` |

Try multiple variants (gnu/musl) if first doesn't match.

### Release Artifact Discovery

**Primary: binstall metadata** from crate's `Cargo.toml`:

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }.{ archive-format }"
pkg-fmt = "tgz"
bin-dir = "{ bin }{ binary-ext }"
```

**Fallback: GitHub Releases pattern matching** — common naming patterns:

```
<crate>-<version>-<target>.tar.gz
<crate>-v<version>-<target>.tar.gz
<name>-<version>-<target>.<ext>
```

## CLI Interface

```
rvx [OPTIONS] <CRATE[@VERSION]> [ARGS...]
```

| Flag | Description | Default |
|---|---|---|
| `--bin <name>` | Binary name if differs from crate | Crate name |
| `--update` | Re-download even if cached | false |
| `--list` | List cached binaries | — |
| `--clean` | Remove all cached binaries | — |
| `--quiet` | Suppress download output | false |

## Trust Model

All resolution flows through crates.io as trust anchor. No arbitrary sources. Checksum verification when available. Same trust model as `cargo install`.

## Cache Behavior

- Unpinned first run: resolve latest, download, cache, exec
- Unpinned cached: exec immediately, no network
- Pinned: download specific version, cache, exec
- `--update`: re-resolve/re-download

## Dependencies

- `reqwest` (blocking, rustls-tls) — HTTP
- `flate2` + `tar` — .tar.gz
- `xz2` — .tar.xz
- `zstd` — .tar.zst
- `zip` — .zip
- `serde` + `serde_json` — API parsing
- `toml` — Cargo.toml parsing
- `clap` (derive) — CLI
- `dirs` — home directory
- `sha2` — checksums
- `tempfile` — temp extraction dir

## Non-Goals

No compilation fallback. No arbitrary sources. No project-scoping. No auto-update. No package management. No runtime.

## Edge Cases

- No binary for platform → clear error pointing to cargo-dist
- Binary name differs → `--bin` flag
- GitHub rate limit → suggest `GITHUB_TOKEN`, binstall path avoids API
- Concurrent launches → file lock on cache entry
- Nested archive dirs → recursive binary search
- No repo URL + no binstall → error
