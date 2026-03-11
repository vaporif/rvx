# rvx

Download and run pre-built Rust crate binaries. No Rust required.

## Build & Test

- `cargo build` — build
- `cargo test` — run unit tests
- `cargo test -- --ignored` — run integration/e2e tests (requires network)
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — format check

## Architecture

Five modules in sequential pipeline: cache -> registry -> resolve -> download -> exec.

- `src/cache.rs` — `~/.rvx/` cache management
- `src/registry.rs` — crates.io API client + .crate tarball parsing for binstall metadata
- `src/resolve.rs` — binstall template rendering + GitHub release fallback
- `src/download.rs` — archive download, checksum verification, extraction
- `src/exec.rs` — Unix exec() process replacement
- `src/target.rs` — compile-time target triple detection
- `src/cli.rs` — clap derive CLI definition
- `src/error.rs` — error types via thiserror

## Conventions

- No async — all HTTP is `reqwest::blocking`
- `RVX_HOME` env var overrides default `~/.rvx/` cache location (useful for testing)
- Integration tests use `#[ignore]` — run explicitly with `-- --ignored`
- Conventional commits: `feat:`, `fix:`, `test:`, `docs:`
