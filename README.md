# rvx

Like [uvx](https://docs.astral.sh/uv/guides/tools/) but for Rust — download and run pre-built crate binaries without a Rust toolchain.

## Usage

```sh
rvx ripgrep -- --version
rvx bat@0.24.0 -- README.md
rvx --bin rg ripgrep -- -i pattern
```

### Cache Management

```sh
rvx --list              # show cached binaries
rvx --clean             # clear cache
rvx --update ripgrep    # force re-download
```

### Options

| Flag | Description |
|------|-------------|
| `--bin <name>` | Binary name if it differs from the crate name |
| `--update` | Re-download even if cached |
| `--list` | List cached binaries |
| `--clean` | Remove all cached binaries |
| `--quiet`, `-q` | Suppress download output |

## How It Works

1. Check local cache (`~/.rvx`)
2. Query crates.io for version and [binstall](https://github.com/cargo-bins/cargo-binstall) metadata
3. Resolve binary from binstall template or GitHub releases
4. Download, verify SHA256 checksum, extract
5. Cache and exec (Unix `exec()` replaces current process)

Supports `.tar.gz`, `.tar.xz`, `.tar.zst`, and `.zip` archives.

## Install

**From source:**

```sh
cargo install --path .
```

## Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `GITHUB_TOKEN` | — | GitHub API token (avoids rate limits) |
| `RVX_HOME` | `~/.rvx` | Override cache directory |

## Development

```sh
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt
```

## License

MIT
