# rvx Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a CLI tool that downloads and runs pre-built Rust crate binaries from crates.io — no Rust toolchain required.

**Architecture:** Five modules (cache, registry, resolve, download, exec) in a sequential pipeline. All HTTP is blocking via `reqwest::blocking`. Binary cached at `~/.rvx/bin/<crate>-<version>`, metadata at `~/.rvx/meta/<crate>-<version>.json`. CLI parsed via clap derive.

**Tech Stack:** Rust, clap, reqwest (blocking + rustls-tls), flate2, tar, xz2, zstd, zip, serde, serde_json, toml, sha2, dirs, tempfile

**Spec:** `docs/superpowers/specs/2026-03-10-rvx-design.md`

---

## Chunk 1: Project Scaffold + Cache Module

### Task 1: Project Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/error.rs`
- Create: `src/cli.rs`

- [ ] **Step 1: Initialize cargo project and add dependencies**

```bash
cd /Users/vaporif/Repos/rvx
cargo init
```

Then replace `Cargo.toml` with:

```toml
[package]
name = "rvx"
version = "0.1.0"
edition = "2021"
description = "uvx for Rust — download and run pre-built crate binaries"
license = "MIT"

[dependencies]
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", features = ["blocking", "rustls-tls"], default-features = false }
flate2 = "1"
tar = "0.4"
xz2 = "0.1"
zstd = "0.13"
zip = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
sha2 = "0.10"
dirs = "6"
tempfile = "3"
thiserror = "2"
fs2 = "0.4"
```

- [ ] **Step 2: Create error types**

Create `src/error.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Crate `{0}` not found on crates.io")]
    CrateNotFound(String),

    #[error("Version `{version}` not found for crate `{crate_name}`")]
    VersionNotFound { crate_name: String, version: String },

    #[error("No pre-built binary for `{target}`. The crate author needs to publish release binaries (e.g. via cargo-dist).")]
    NoBinaryForPlatform { target: String },

    #[error("Cannot resolve binary — crate has no repository URL and no binstall metadata")]
    NoRepositoryUrl,

    #[error("Binary `{0}` not found in archive")]
    BinaryNotFoundInArchive(String),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Not cached and no network available")]
    NotCachedNoNetwork,

    #[error("GitHub API rate limit exceeded. Set GITHUB_TOKEN env var for higher limits.")]
    GitHubRateLimit,

    #[error("Unsupported archive format: {0}")]
    UnsupportedArchiveFormat(String),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 3: Create CLI definition**

Create `src/cli.rs`:

```rust
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "rvx", about = "Download and run pre-built crate binaries")]
pub struct Cli {
    /// Crate name on crates.io, optionally with @version (e.g. mcp-server-qdrant@0.2.1)
    #[arg(required_unless_present_any = ["list", "clean"])]
    pub crate_spec: Option<String>,

    /// Arguments to pass through to the binary
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Binary name if it differs from crate name
    #[arg(long)]
    pub bin: Option<String>,

    /// Re-download even if cached
    #[arg(long)]
    pub update: bool,

    /// List cached binaries
    #[arg(long)]
    pub list: bool,

    /// Remove all cached binaries
    #[arg(long)]
    pub clean: bool,

    /// Suppress download output
    #[arg(long, short)]
    pub quiet: bool,
}

/// Parsed crate specification
#[derive(Debug, Clone)]
pub struct CrateSpec {
    pub name: String,
    pub version: Option<String>,
}

impl CrateSpec {
    pub fn parse(spec: &str) -> Self {
        if let Some((name, version)) = spec.split_once('@') {
            Self {
                name: name.to_string(),
                version: Some(version.to_string()),
            }
        } else {
            Self {
                name: spec.to_string(),
                version: None,
            }
        }
    }
}
```

- [ ] **Step 4: Create minimal main.rs**

Create `src/main.rs`:

```rust
mod cache;
mod cli;
mod download;
mod error;
mod exec;
mod registry;
mod resolve;
mod target;

use clap::Parser;
use cli::{Cli, CrateSpec};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> error::Result<()> {
    let cli = Cli::parse();

    if cli.list {
        cache::list()?;
        return Ok(());
    }

    if cli.clean {
        cache::clean()?;
        return Ok(());
    }

    let spec_str = cli.crate_spec.as_deref().expect("crate_spec required");
    let spec = CrateSpec::parse(spec_str);
    let bin_name = cli.bin.as_deref().unwrap_or(&spec.name);

    // Check cache first (unless --update)
    if !cli.update {
        if let Some(cached) = cache::find(&spec)? {
            exec::run(&cached, bin_name, &cli.args)?;
            unreachable!();
        }
    }

    // Resolve version and metadata from crates.io
    let crate_info = registry::fetch(&spec)?;

    // Check cache again with resolved version
    if !cli.update {
        let resolved_spec = CrateSpec {
            name: spec.name.clone(),
            version: Some(crate_info.version.clone()),
        };
        if let Some(cached) = cache::find(&resolved_spec)? {
            exec::run(&cached, bin_name, &cli.args)?;
            unreachable!();
        }
    }

    // Resolve download URL
    let artifact = resolve::resolve(&crate_info, bin_name, cli.quiet)?;

    // Download and extract
    let binary_path = download::download(&artifact, &crate_info, bin_name, cli.quiet)?;

    // Cache the binary
    let cached_path = cache::store(&binary_path, &crate_info, &artifact)?;

    // Exec
    exec::run(&cached_path, bin_name, &cli.args)?;
    unreachable!()
}
```

- [ ] **Step 5: Verify it compiles (with stub modules)**

Create stub files for each module so it compiles:

`src/target.rs`:
```rust
pub fn current_target() -> &'static str {
    env!("RVX_TARGET")
}

pub fn target_variants() -> Vec<String> {
    let primary = current_target().to_string();
    let mut variants = vec![primary.clone()];

    if primary.contains("linux") && primary.contains("gnu") {
        variants.push(primary.replace("gnu", "musl"));
    } else if primary.contains("linux") && primary.contains("musl") {
        variants.push(primary.replace("musl", "gnu"));
    }

    variants
}

pub fn binary_ext() -> &'static str {
    if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    }
}
```

Note: `RVX_TARGET` will be set via `build.rs`. Create `build.rs`:

```rust
fn main() {
    let target = std::env::var("TARGET").unwrap();
    println!("cargo:rustc-env=RVX_TARGET={target}");
}
```

Create empty stubs for `src/cache.rs`, `src/registry.rs`, `src/resolve.rs`, `src/download.rs`, `src/exec.rs` — just enough to compile.

Note: The `target.rs` stub should use `Vec<String>` (not `Vec<&'static str>`) to match the final implementation:

Run:
```bash
cargo check
```

Expected: compiles successfully.

- [ ] **Step 6: Commit scaffold**

```bash
git add -A && git commit -m "feat: project scaffold with CLI, error types, and module stubs"
```

---

### Task 2: Cache Module

**Files:**
- Create: `src/cache.rs`
- Create: `tests/cache_test.rs`

- [ ] **Step 1: Write cache tests**

Create `tests/cache_test.rs`:

```rust
use std::fs;
use tempfile::TempDir;

// We test the cache logic using the cache module's internal functions
// by setting the RVX_HOME env var to a temp directory

#[test]
fn test_cache_store_and_find() {
    let tmp = TempDir::new().unwrap();

    let bin_dir = tmp.path().join("bin");

    // Initially empty
    assert!(!bin_dir.exists());

    // Create a fake binary
    let fake_binary = tmp.path().join("fake-binary");
    fs::write(&fake_binary, b"#!/bin/sh\necho hello").unwrap();

    // Store it
    let meta = rvx::cache::CacheMeta {
        crate_name: "test-crate".to_string(),
        version: "1.0.0".to_string(),
        source_url: "https://example.com/test.tar.gz".to_string(),
        checksum: Some("abc123".to_string()),
        cached_at: "2026-01-01T00:00:00Z".to_string(),
    };

    let stored = rvx::cache::store_to(tmp.path(), &fake_binary, &meta).unwrap();
    assert!(stored.exists());
    assert_eq!(stored.file_name().unwrap(), "test-crate-1.0.0");

    // Find it
    let found = rvx::cache::find_in(tmp.path(), "test-crate", Some("1.0.0"));
    assert!(found.is_some());
    assert_eq!(found.unwrap(), stored);

    // Find without version returns the cached one
    let found = rvx::cache::find_in(tmp.path(), "test-crate", None);
    assert!(found.is_some());

    // Not found
    let found = rvx::cache::find_in(tmp.path(), "other-crate", None);
    assert!(found.is_none());
}

#[test]
fn test_cache_list() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    fs::write(bin_dir.join("tool-a-1.0.0"), b"binary").unwrap();
    fs::write(bin_dir.join("tool-b-2.3.1"), b"binary").unwrap();

    let entries = rvx::cache::list_in(tmp.path()).unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_cache_clean() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    let meta_dir = tmp.path().join("meta");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&meta_dir).unwrap();

    fs::write(bin_dir.join("tool-1.0.0"), b"binary").unwrap();
    fs::write(meta_dir.join("tool-1.0.0.json"), b"{}").unwrap();

    rvx::cache::clean_in(tmp.path()).unwrap();

    assert!(fs::read_dir(&bin_dir).unwrap().next().is_none());
    assert!(fs::read_dir(&meta_dir).unwrap().next().is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --test cache_test
```
Expected: FAIL — functions don't exist yet.

- [ ] **Step 3: Implement cache module**

Replace `src/cache.rs`:

```rust
use crate::cli::CrateSpec;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct CacheMeta {
    pub crate_name: String,
    pub version: String,
    pub source_url: String,
    pub checksum: Option<String>,
    pub cached_at: String,
}

fn rvx_home() -> PathBuf {
    if let Ok(home) = std::env::var("RVX_HOME") {
        return PathBuf::from(home);
    }
    dirs::home_dir()
        .expect("could not determine home directory")
        .join(".rvx")
}

fn bin_dir(base: &Path) -> PathBuf {
    base.join("bin")
}

fn meta_dir(base: &Path) -> PathBuf {
    base.join("meta")
}

fn cache_key(name: &str, version: &str) -> String {
    format!("{name}-{version}")
}

/// Find a cached binary. If version is None, find any cached version.
pub fn find(spec: &CrateSpec) -> Result<Option<PathBuf>> {
    Ok(find_in(&rvx_home(), &spec.name, spec.version.as_deref()))
}

pub fn find_in(base: &Path, name: &str, version: Option<&str>) -> Option<PathBuf> {
    let bin = bin_dir(base);
    if !bin.exists() {
        return None;
    }

    if let Some(v) = version {
        let path = bin.join(cache_key(name, v));
        if path.exists() {
            return Some(path);
        }
        return None;
    }

    // No version specified — find any cached version for this crate
    let prefix = format!("{name}-");
    if let Ok(entries) = fs::read_dir(&bin) {
        for entry in entries.flatten() {
            let fname = entry.file_name();
            let fname = fname.to_string_lossy();
            if fname.starts_with(&prefix) {
                return Some(entry.path());
            }
        }
    }
    None
}

/// Store a binary in the cache. Returns the cached path.
pub fn store(binary_path: &Path, info: &crate::registry::CrateInfo, artifact: &crate::resolve::Artifact) -> Result<PathBuf> {
    let meta = CacheMeta {
        crate_name: info.name.clone(),
        version: info.version.clone(),
        source_url: artifact.url.clone(),
        checksum: artifact.checksum.clone(),
        cached_at: timestamp_now(),
    };
    store_to(&rvx_home(), binary_path, &meta)
}

pub fn store_to(base: &Path, binary_path: &Path, meta: &CacheMeta) -> Result<PathBuf> {
    let bin = bin_dir(base);
    let meta_path = meta_dir(base);
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(&meta_path)?;

    let key = cache_key(&meta.crate_name, &meta.version);
    let dest = bin.join(&key);

    fs::copy(binary_path, &dest)?;

    // Set executable permission on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
    }

    // Write metadata
    let meta_file = meta_path.join(format!("{key}.json"));
    let meta_json = serde_json::to_string_pretty(meta)?;
    fs::write(&meta_file, meta_json)?;

    Ok(dest)
}

/// List all cached binaries
pub fn list() -> Result<()> {
    let entries = list_in(&rvx_home())?;
    if entries.is_empty() {
        println!("No cached binaries.");
    } else {
        for entry in entries {
            println!("  {entry}");
        }
    }
    Ok(())
}

pub fn list_in(base: &Path) -> Result<Vec<String>> {
    let bin = bin_dir(base);
    let mut entries = Vec::new();

    if !bin.exists() {
        return Ok(entries);
    }

    for entry in fs::read_dir(&bin)? {
        let entry = entry?;
        entries.push(entry.file_name().to_string_lossy().to_string());
    }
    entries.sort();
    Ok(entries)
}

/// Remove all cached binaries
pub fn clean() -> Result<()> {
    clean_in(&rvx_home())
}

pub fn clean_in(base: &Path) -> Result<()> {
    let bin = bin_dir(base);
    let meta = meta_dir(base);

    if bin.exists() {
        for entry in fs::read_dir(&bin)? {
            fs::remove_file(entry?.path())?;
        }
    }

    if meta.exists() {
        for entry in fs::read_dir(&meta)? {
            fs::remove_file(entry?.path())?;
        }
    }

    Ok(())
}

fn timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
```

Also add `pub mod cache;` etc. to `src/main.rs` and add a `src/lib.rs`:

```rust
pub mod cache;
pub mod cli;
pub mod error;
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --test cache_test
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: implement cache module with store/find/list/clean"
```

---

## Chunk 2: Target Detection + Registry Module

### Task 3: Target Detection

**Files:**
- Modify: `src/target.rs`
- Create: `build.rs`

- [ ] **Step 1: Write target tests**

Add tests directly in `src/target.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_target_is_not_empty() {
        let t = current_target();
        assert!(!t.is_empty());
        // Should contain OS indicator
        assert!(
            t.contains("linux") || t.contains("darwin") || t.contains("windows"),
            "unexpected target: {t}"
        );
    }

    #[test]
    fn test_binary_ext() {
        let ext = binary_ext();
        if cfg!(target_os = "windows") {
            assert_eq!(ext, ".exe");
        } else {
            assert_eq!(ext, "");
        }
    }

    #[test]
    fn test_target_variants_includes_primary() {
        let variants = target_variants();
        assert!(!variants.is_empty());
        assert_eq!(variants[0], current_target());
    }
}
```

- [ ] **Step 2: Implement target module**

`build.rs`:
```rust
fn main() {
    let target = std::env::var("TARGET").unwrap();
    println!("cargo:rustc-env=RVX_TARGET={target}");
}
```

`src/target.rs`:
```rust
pub fn current_target() -> &'static str {
    env!("RVX_TARGET")
}

pub fn target_variants() -> Vec<String> {
    let primary = current_target().to_string();
    let mut variants = vec![primary.clone()];

    // Add musl/gnu fallback for Linux
    if primary.contains("linux") {
        if primary.contains("gnu") {
            variants.push(primary.replace("gnu", "musl"));
        } else if primary.contains("musl") {
            variants.push(primary.replace("musl", "gnu"));
        }
    }

    variants
}

pub fn binary_ext() -> &'static str {
    if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_target_is_not_empty() {
        let t = current_target();
        assert!(!t.is_empty());
        assert!(
            t.contains("linux") || t.contains("darwin") || t.contains("windows"),
            "unexpected target: {t}"
        );
    }

    #[test]
    fn test_binary_ext() {
        let ext = binary_ext();
        if cfg!(target_os = "windows") {
            assert_eq!(ext, ".exe");
        } else {
            assert_eq!(ext, "");
        }
    }

    #[test]
    fn test_target_variants_includes_primary() {
        let variants = target_variants();
        assert!(!variants.is_empty());
        assert_eq!(variants[0], current_target());
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test target::tests
```
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: compile-time target triple detection"
```

---

### Task 4: Registry Module

**Files:**
- Create: `src/registry.rs`
- Create: `tests/registry_test.rs`

- [ ] **Step 1: Define registry types and implement crates.io API client**

`src/registry.rs`:

```rust
use crate::error::{Error, Result};
use crate::cli::CrateSpec;
use serde::Deserialize;
use std::io::Read;

const CRATES_IO_API: &str = "https://crates.io/api/v1";
const USER_AGENT: &str = "rvx (https://github.com/user/rvx)";

#[derive(Debug, Clone)]
pub struct CrateInfo {
    pub name: String,
    pub version: String,
    pub repository: Option<String>,
    pub binstall: Option<BinstallMeta>,
}

#[derive(Debug, Clone)]
pub struct BinstallMeta {
    pub pkg_url: String,
    pub pkg_fmt: Option<String>,
    pub bin_dir: Option<String>,
}

#[derive(Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    krate: CratesIoCrate,
    versions: Vec<CratesIoVersion>,
}

#[derive(Deserialize)]
struct CratesIoCrate {
    name: String,
    repository: Option<String>,
    max_stable_version: Option<String>,
}

#[derive(Deserialize)]
struct CratesIoVersion {
    num: String,
    dl_path: String,
    yanked: bool,
}

#[derive(Deserialize)]
struct CratesIoVersionResponse {
    version: CratesIoVersionDetail,
}

#[derive(Deserialize)]
struct CratesIoVersionDetail {
    num: String,
    dl_path: String,
    yanked: bool,
}

#[derive(Deserialize)]
struct CargoToml {
    package: Option<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    metadata: Option<CargoMetadata>,
}

#[derive(Deserialize)]
struct CargoMetadata {
    binstall: Option<CargoMetadataBinstall>,
}

#[derive(Deserialize)]
struct CargoMetadataBinstall {
    #[serde(rename = "pkg-url")]
    pkg_url: Option<String>,
    #[serde(rename = "pkg-fmt")]
    pkg_fmt: Option<String>,
    #[serde(rename = "bin-dir")]
    bin_dir: Option<String>,
}

fn http_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .build()?)
}

/// Fetch crate info from crates.io, including binstall metadata from .crate tarball
pub fn fetch(spec: &CrateSpec) -> Result<CrateInfo> {
    let client = http_client()?;

    // Fetch crate metadata
    let url = format!("{CRATES_IO_API}/crates/{}", spec.name);
    let resp = client.get(&url).send()?;

    if resp.status().as_u16() == 404 {
        return Err(Error::CrateNotFound(spec.name.clone()));
    }
    let resp = resp.error_for_status()?;
    let data: CratesIoResponse = resp.json()?;

    // Determine version
    let version = if let Some(ref v) = spec.version {
        // Verify requested version exists
        let exists = data.versions.iter().any(|ver| ver.num == *v && !ver.yanked);
        if !exists {
            return Err(Error::VersionNotFound {
                crate_name: spec.name.clone(),
                version: v.clone(),
            });
        }
        v.clone()
    } else {
        // Use max_stable_version or first non-yanked
        data.krate.max_stable_version
            .or_else(|| {
                data.versions.iter()
                    .find(|v| !v.yanked)
                    .map(|v| v.num.clone())
            })
            .ok_or_else(|| Error::Other(format!("No available version for crate `{}`", spec.name)))?
    };

    // Find the dl_path for this version
    let dl_path = data.versions.iter()
        .find(|v| v.num == version)
        .map(|v| v.dl_path.clone())
        .unwrap_or_else(|| format!("/api/v1/crates/{}/{}/download", spec.name, version));

    // Download .crate tarball to extract Cargo.toml for binstall metadata
    let binstall = fetch_binstall_metadata(&client, &dl_path)?;

    Ok(CrateInfo {
        name: data.krate.name,
        version,
        repository: data.krate.repository,
        binstall,
    })
}

fn fetch_binstall_metadata(
    client: &reqwest::blocking::Client,
    dl_path: &str,
) -> Result<Option<BinstallMeta>> {
    let url = if dl_path.starts_with("http") {
        dl_path.to_string()
    } else {
        format!("https://crates.io{dl_path}")
    };

    let resp = client.get(&url).send()?;
    let bytes = resp.bytes()?;

    // .crate files are gzipped tarballs
    let gz = flate2::read::GzDecoder::new(&bytes[..]);
    let mut archive = tar::Archive::new(gz);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path.ends_with("Cargo.toml") {
            // Check it's the root Cargo.toml (first path component is the crate dir)
            let components: Vec<_> = path.components().collect();
            if components.len() == 2 {
                let mut contents = String::new();
                entry.read_to_string(&mut contents)?;

                let cargo_toml: CargoToml = toml::from_str(&contents)?;
                if let Some(pkg) = cargo_toml.package {
                    if let Some(meta) = pkg.metadata {
                        if let Some(binstall) = meta.binstall {
                            if let Some(pkg_url) = binstall.pkg_url {
                                return Ok(Some(BinstallMeta {
                                    pkg_url,
                                    pkg_fmt: binstall.pkg_fmt,
                                    bin_dir: binstall.bin_dir,
                                }));
                            }
                        }
                    }
                }
                break;
            }
        }
    }

    Ok(None)
}
```

- [ ] **Step 2: Write integration test for registry (live API)**

Create `tests/registry_test.rs`:

```rust
//! Integration tests that hit the real crates.io API.
//! Run with: cargo test --test registry_test -- --ignored

#[test]
#[ignore] // requires network
fn test_fetch_known_crate() {
    let spec = rvx::cli::CrateSpec {
        name: "ripgrep".to_string(),
        version: None,
    };
    let info = rvx::registry::fetch(&spec).unwrap();
    assert_eq!(info.name, "ripgrep");
    assert!(!info.version.is_empty());
    assert!(info.repository.is_some());
}

#[test]
#[ignore]
fn test_fetch_nonexistent_crate() {
    let spec = rvx::cli::CrateSpec {
        name: "this-crate-definitely-does-not-exist-rvx-test".to_string(),
        version: None,
    };
    let result = rvx::registry::fetch(&spec);
    assert!(result.is_err());
}

#[test]
#[ignore]
fn test_fetch_with_binstall_metadata() {
    // cargo-binstall itself has binstall metadata
    let spec = rvx::cli::CrateSpec {
        name: "cargo-binstall".to_string(),
        version: None,
    };
    let info = rvx::registry::fetch(&spec).unwrap();
    assert!(info.binstall.is_some(), "cargo-binstall should have binstall metadata");
    let binstall = info.binstall.unwrap();
    assert!(!binstall.pkg_url.is_empty());
}
```

- [ ] **Step 3: Add registry to lib.rs exports**

Update `src/lib.rs`:
```rust
pub mod cache;
pub mod cli;
pub mod error;
pub mod registry;
pub mod target;
```

- [ ] **Step 4: Run unit tests**

```bash
cargo test
```
Expected: PASS (unit tests only, integration tests are `#[ignore]`)

- [ ] **Step 5: Run integration tests (requires network)**

```bash
cargo test --test registry_test -- --ignored
```
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: registry module for crates.io API and binstall metadata"
```

---

## Chunk 3: Resolve + Download Modules

### Task 5: Resolve Module

**Files:**
- Create: `src/resolve.rs`

- [ ] **Step 1: Implement binstall template rendering and GitHub release fallback**

`src/resolve.rs`:

```rust
use crate::error::{Error, Result};
use crate::registry::{BinstallMeta, CrateInfo};
use crate::target;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Artifact {
    pub url: String,
    pub checksum: Option<String>,
    pub format: ArchiveFormat,
}

#[derive(Debug, Clone)]
pub enum ArchiveFormat {
    TarGz,
    TarXz,
    TarZst,
    Zip,
}

impl ArchiveFormat {
    pub fn from_filename(name: &str) -> Option<Self> {
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Some(Self::TarGz)
        } else if name.ends_with(".tar.xz") {
            Some(Self::TarXz)
        } else if name.ends_with(".tar.zst") || name.ends_with(".tar.zstd") {
            Some(Self::TarZst)
        } else if name.ends_with(".zip") {
            Some(Self::Zip)
        } else {
            None
        }
    }
}

/// Resolve the download URL for a crate's pre-built binary
pub fn resolve(info: &CrateInfo, bin_name: &str, quiet: bool) -> Result<Artifact> {
    let mut client_builder = reqwest::blocking::Client::builder()
        .user_agent("rvx");

    // Add GitHub token if available
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
            headers.insert(reqwest::header::AUTHORIZATION, val);
        }
    }
    let client = client_builder.default_headers(headers).build()?;

    // Try binstall metadata first
    if let Some(ref binstall) = info.binstall {
        for target_triple in target::target_variants() {
            if let Some(artifact) = try_binstall(&client, info, binstall, bin_name, &target_triple)? {
                if !quiet {
                    eprintln!("Resolved via binstall metadata: {}", artifact.url);
                }
                return Ok(artifact);
            }
        }
    }

    // Fallback: GitHub releases
    let repo_url = info.repository.as_deref()
        .ok_or(Error::NoRepositoryUrl)?;

    let (owner, repo) = parse_github_url(repo_url)
        .ok_or_else(|| Error::Other(format!("Unsupported repository URL: {repo_url}")))?;

    for target_triple in target::target_variants() {
        if let Some(artifact) = try_github_release(&client, &owner, &repo, info, bin_name, &target_triple, quiet)? {
            if !quiet {
                eprintln!("Resolved via GitHub releases: {}", artifact.url);
            }
            return Ok(artifact);
        }
    }

    Err(Error::NoBinaryForPlatform {
        target: target::current_target().to_string(),
    })
}

fn try_binstall(
    client: &reqwest::blocking::Client,
    info: &CrateInfo,
    binstall: &BinstallMeta,
    bin_name: &str,
    target_triple: &str,
) -> Result<Option<Artifact>> {
    let url = render_binstall_template(
        &binstall.pkg_url,
        info,
        bin_name,
        target_triple,
    );

    // HEAD request to check if URL exists
    match client.head(&url).send() {
        Ok(resp) if resp.status().is_success() => {
            let format = binstall.pkg_fmt.as_deref()
                .and_then(|f| match f {
                    "tgz" | "tar.gz" => Some(ArchiveFormat::TarGz),
                    "txz" | "tar.xz" => Some(ArchiveFormat::TarXz),
                    "tar.zst" | "tar.zstd" => Some(ArchiveFormat::TarZst),
                    "zip" => Some(ArchiveFormat::Zip),
                    _ => None,
                })
                .or_else(|| ArchiveFormat::from_filename(&url))
                .unwrap_or(ArchiveFormat::TarGz);

            // Try to find checksum
            let checksum = try_fetch_checksum(&client, &url);

            Ok(Some(Artifact { url, checksum, format }))
        }
        _ => Ok(None),
    }
}

fn render_binstall_template(
    template: &str,
    info: &CrateInfo,
    bin_name: &str,
    target_triple: &str,
) -> String {
    let repo = info.repository.as_deref().unwrap_or("");

    // Normalize repo URL: remove trailing .git and slashes
    let repo = repo.trim_end_matches('/').trim_end_matches(".git");

    let archive_format = if target_triple.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };

    template
        .replace("{ repo }", repo)
        .replace("{repo}", repo)
        .replace("{ version }", &info.version)
        .replace("{version}", &info.version)
        .replace("{ name }", &info.name)
        .replace("{name}", &info.name)
        .replace("{ target }", target_triple)
        .replace("{target}", target_triple)
        .replace("{ bin }", bin_name)
        .replace("{bin}", bin_name)
        .replace("{ archive-format }", archive_format)
        .replace("{archive-format}", archive_format)
        .replace("{ binary-ext }", target::binary_ext())
        .replace("{binary-ext}", target::binary_ext())
}

fn try_fetch_checksum(client: &reqwest::blocking::Client, artifact_url: &str) -> Option<String> {
    // Try common checksum file patterns
    for suffix in &[".sha256", ".sha256sum", ".SHA256SUM"] {
        let checksum_url = format!("{artifact_url}{suffix}");
        if let Ok(resp) = client.get(&checksum_url).send() {
            if resp.status().is_success() {
                if let Ok(text) = resp.text() {
                    // Format: either just the hash, or "hash  filename"
                    let hash = text.split_whitespace().next().unwrap_or("").trim();
                    if hash.len() == 64 {
                        // SHA256 is 64 hex chars
                        return Some(hash.to_string());
                    }
                }
            }
        }
    }
    None
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

fn try_github_release(
    client: &reqwest::blocking::Client,
    owner: &str,
    repo: &str,
    info: &CrateInfo,
    bin_name: &str,
    target_triple: &str,
    quiet: bool,
) -> Result<Option<Artifact>> {
    // Try common tag patterns
    let tag_patterns = [
        format!("v{}", info.version),
        info.version.clone(),
        format!("{}-v{}", info.name, info.version),
        format!("{}-{}", info.name, info.version),
    ];

    for tag in &tag_patterns {
        let url = format!(
            "https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}"
        );

        let resp = match client.get(&url).send() {
            Ok(r) => r,
            Err(_) => continue,
        };

        if resp.status().as_u16() == 403 {
            return Err(Error::GitHubRateLimit);
        }

        if !resp.status().is_success() {
            continue;
        }

        let release: GitHubRelease = match resp.json() {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Try to find matching artifact
        if let Some(artifact) = find_matching_asset(&release.assets, &info.name, bin_name, &info.version, target_triple, &client) {
            return Ok(Some(artifact));
        }
    }

    Ok(None)
}

fn find_matching_asset(
    assets: &[GitHubAsset],
    crate_name: &str,
    bin_name: &str,
    version: &str,
    target_triple: &str,
    client: &reqwest::blocking::Client,
) -> Option<Artifact> {
    // Filter to assets that contain the target triple
    let candidates: Vec<_> = assets.iter()
        .filter(|a| {
            let name = a.name.to_lowercase();
            name.contains(&target_triple.to_lowercase())
                && ArchiveFormat::from_filename(&name).is_some()
        })
        .collect();

    // Prefer assets that also match the crate/bin name
    let best = candidates.iter()
        .find(|a| {
            let name = a.name.to_lowercase();
            name.contains(&crate_name.to_lowercase()) || name.contains(&bin_name.to_lowercase())
        })
        .or(candidates.first())?;

    let format = ArchiveFormat::from_filename(&best.name)?;

    // Try to find checksum from other assets
    let checksum = find_checksum_in_assets(assets, &best.name, client);

    Some(Artifact {
        url: best.browser_download_url.clone(),
        checksum,
        format,
    })
}

fn find_checksum_in_assets(
    assets: &[GitHubAsset],
    artifact_name: &str,
    client: &reqwest::blocking::Client,
) -> Option<String> {
    // Look for checksum files in release assets
    let checksum_patterns = ["SHA256SUMS", "checksums.txt", "sha256sums.txt"];

    for asset in assets {
        let is_checksum = checksum_patterns.iter().any(|p| asset.name.contains(p))
            || asset.name.ends_with(".sha256")
            || asset.name.ends_with(".sha256sum");

        if !is_checksum {
            continue;
        }

        if let Ok(resp) = client.get(&asset.browser_download_url).send() {
            if let Ok(text) = resp.text() {
                // Look for a line containing our artifact name
                for line in text.lines() {
                    if line.contains(artifact_name) {
                        let hash = line.split_whitespace().next().unwrap_or("");
                        if hash.len() == 64 {
                            return Some(hash.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn parse_github_url(url: &str) -> Option<(String, String)> {
    // Parse GitHub URL to extract owner/repo
    let url = url.trim_end_matches('/').trim_end_matches(".git");

    // Handle https://github.com/owner/repo
    if let Some(path) = url.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = path.splitn(3, '/').collect();
        if parts.len() >= 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    // Handle git@github.com:owner/repo
    if let Some(path) = url.strip_prefix("git@github.com:") {
        let parts: Vec<&str> = path.splitn(3, '/').collect();
        if parts.len() >= 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_url() {
        let (owner, repo) = parse_github_url("https://github.com/qdrant/mcp-server-qdrant").unwrap();
        assert_eq!(owner, "qdrant");
        assert_eq!(repo, "mcp-server-qdrant");

        let (owner, repo) = parse_github_url("https://github.com/owner/repo.git").unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");

        assert!(parse_github_url("https://gitlab.com/user/repo").is_none());
    }

    #[test]
    fn test_render_binstall_template() {
        let info = CrateInfo {
            name: "my-tool".to_string(),
            version: "1.2.3".to_string(),
            repository: Some("https://github.com/user/my-tool".to_string()),
            binstall: None,
        };

        let result = render_binstall_template(
            "{ repo }/releases/download/v{ version }/{ name }-{ target }.{ archive-format }",
            &info,
            "my-tool",
            "x86_64-apple-darwin",
        );

        assert_eq!(
            result,
            "https://github.com/user/my-tool/releases/download/v1.2.3/my-tool-x86_64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn test_archive_format_from_filename() {
        assert!(matches!(ArchiveFormat::from_filename("foo.tar.gz"), Some(ArchiveFormat::TarGz)));
        assert!(matches!(ArchiveFormat::from_filename("foo.tgz"), Some(ArchiveFormat::TarGz)));
        assert!(matches!(ArchiveFormat::from_filename("foo.tar.xz"), Some(ArchiveFormat::TarXz)));
        assert!(matches!(ArchiveFormat::from_filename("foo.tar.zst"), Some(ArchiveFormat::TarZst)));
        assert!(matches!(ArchiveFormat::from_filename("foo.zip"), Some(ArchiveFormat::Zip)));
        assert!(ArchiveFormat::from_filename("foo.bin").is_none());
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test resolve::tests
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat: resolve module with binstall and GitHub release fallback"
```

---

### Task 6: Download Module

**Files:**
- Create: `src/download.rs`

- [ ] **Step 1: Implement download and archive extraction**

`src/download.rs`:

```rust
use crate::error::{Error, Result};
use crate::registry::CrateInfo;
use crate::resolve::{ArchiveFormat, Artifact};
use crate::target;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};

/// Download artifact, verify checksum, extract binary, return path to extracted binary
pub fn download(
    artifact: &Artifact,
    info: &CrateInfo,
    bin_name: &str,
    quiet: bool,
) -> Result<PathBuf> {
    if !quiet {
        eprintln!("Downloading {}...", artifact.url);
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent("rvx")
        .build()?;

    let resp = client.get(&artifact.url).send()?.error_for_status()?;
    let bytes = resp.bytes()?;

    // Verify checksum if available
    if let Some(ref expected) = artifact.checksum {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = format!("{:x}", hasher.finalize());
        if actual != *expected {
            return Err(Error::ChecksumMismatch {
                expected: expected.clone(),
                actual,
            });
        }
        if !quiet {
            eprintln!("Checksum verified.");
        }
    }

    // Extract to temp dir
    let tmp_dir = tempfile::tempdir()?;
    extract_archive(bytes.as_ref(), &artifact.format, tmp_dir.path())?;

    // Find the binary in extracted contents
    let binary_name = format!("{bin_name}{}", target::binary_ext());
    let binary_path = find_binary(tmp_dir.path(), &binary_name)?;

    // Copy to a stable temp location (tempdir will be cleaned up)
    let output = tmp_dir.path().join(format!("_output_{binary_name}"));
    fs::copy(&binary_path, &output)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&output, fs::Permissions::from_mode(0o755))?;
    }

    if !quiet {
        eprintln!("Extracted {binary_name}.");
    }

    // Leak the tempdir so it's not deleted (we need the file until cache::store)
    let tmp_path = tmp_dir.into_path();
    Ok(tmp_path.join(format!("_output_{binary_name}")))
}

fn extract_archive(bytes: &[u8], format: &ArchiveFormat, dest: &Path) -> Result<()> {
    match format {
        ArchiveFormat::TarGz => {
            let gz = flate2::read::GzDecoder::new(bytes);
            let mut archive = tar::Archive::new(gz);
            archive.unpack(dest)?;
        }
        ArchiveFormat::TarXz => {
            let xz = xz2::read::XzDecoder::new(bytes);
            let mut archive = tar::Archive::new(xz);
            archive.unpack(dest)?;
        }
        ArchiveFormat::TarZst => {
            let zst = zstd::stream::read::Decoder::new(bytes)?;
            let mut archive = tar::Archive::new(zst);
            archive.unpack(dest)?;
        }
        ArchiveFormat::Zip => {
            // zip crate needs a Read + Seek, so write to cursor
            let cursor = io::Cursor::new(bytes);
            let mut archive = zip::ZipArchive::new(cursor)
                .map_err(|e| Error::Other(format!("Failed to read zip: {e}")))?;
            archive.extract(dest)
                .map_err(|e| Error::Other(format!("Failed to extract zip: {e}")))?;
        }
    }
    Ok(())
}

/// Recursively search for a binary with the given name
fn find_binary(dir: &Path, name: &str) -> Result<PathBuf> {
    for entry in walkdir(dir)? {
        if let Some(fname) = entry.file_name() {
            if fname.to_string_lossy() == name {
                return Ok(entry);
            }
        }
    }
    Err(Error::BinaryNotFoundInArchive(name.to_string()))
}

fn walkdir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    if !dir.is_dir() {
        return Ok(results);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            results.extend(walkdir(&path)?);
        } else {
            results.push(path);
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_extract_tar_gz() {
        // Create a tar.gz in memory with a fake binary
        let tmp = tempfile::tempdir().unwrap();
        let mut builder = tar::Builder::new(Vec::new());

        let data = b"fake binary content";
        let mut header = tar::Header::new_gnu();
        header.set_path("my-tool/my-tool").unwrap();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append(&header, &data[..]).unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        // Gzip it
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        let gz_bytes = encoder.finish().unwrap();

        let dest = tmp.path().join("extracted");
        fs::create_dir(&dest).unwrap();
        extract_archive(&gz_bytes, &ArchiveFormat::TarGz, &dest).unwrap();

        let found = find_binary(&dest, "my-tool").unwrap();
        assert!(found.exists());
    }

    #[test]
    fn test_find_binary_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let result = find_binary(tmp.path(), "nonexistent");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test download::tests
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat: download module with archive extraction and checksum verification"
```

---

## Chunk 4: Exec Module + Integration + Polish

### Task 7: Exec Module

**Files:**
- Create: `src/exec.rs`

- [ ] **Step 1: Implement exec with Unix process replacement**

`src/exec.rs`:

```rust
use crate::error::Result;
use std::path::Path;

/// Replace current process with the cached binary (Unix) or spawn child (Windows)
pub fn run(binary_path: &Path, _bin_name: &str, args: &[String]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(binary_path)
            .args(args)
            .exec();
        // exec() only returns if it fails
        Err(crate::error::Error::Io(err))
    }

    #[cfg(windows)]
    {
        let status = std::process::Command::new(binary_path)
            .args(args)
            .status()?;
        std::process::exit(status.code().unwrap_or(1));
    }

    #[cfg(not(any(unix, windows)))]
    {
        Err(crate::error::Error::Other("Unsupported platform".to_string()))
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add -A && git commit -m "feat: exec module with Unix process replacement"
```

---

### Task 8: Wire Everything Together in main.rs

**Files:**
- Modify: `src/main.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Finalize main.rs**

Ensure `src/main.rs` has the complete flow as defined in Task 1 Step 4, with all modules properly imported and working together. Verify all module stubs are replaced with real implementations.

Update `src/lib.rs` to export all public modules:

```rust
pub mod cache;
pub mod cli;
pub mod download;
pub mod error;
pub mod exec;
pub mod registry;
pub mod resolve;
pub mod target;
```

- [ ] **Step 2: Build and check for compilation errors**

```bash
cargo build
```
Expected: compiles successfully

- [ ] **Step 3: Run all unit tests**

```bash
cargo test
```
Expected: all tests pass

- [ ] **Step 4: Run clippy**

```bash
cargo clippy -- -D warnings
```
Expected: no warnings

- [ ] **Step 5: Run fmt**

```bash
cargo fmt --check
```
Expected: no formatting issues

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: wire all modules together in main"
```

---

### Task 9: End-to-End Integration Test

**Files:**
- Create: `tests/e2e_test.rs`

- [ ] **Step 1: Write e2e test that downloads and caches a real crate binary**

Create `tests/e2e_test.rs`:

```rust
//! End-to-end tests. Requires network.
//! Run with: cargo test --test e2e_test -- --ignored

use std::process::Command;

#[test]
#[ignore]
fn test_rvx_list_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rvx"))
        .env("RVX_HOME", tmp.path())
        .args(["--list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("No cached binaries"));
}

#[test]
#[ignore]
fn test_rvx_clean_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rvx"))
        .env("RVX_HOME", tmp.path())
        .args(["--clean"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
#[ignore]
fn test_rvx_download_and_run() {
    // Use a small, well-known crate that publishes binaries
    // ripgrep is a good candidate - it has binstall metadata and GitHub releases
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rvx"))
        .env("RVX_HOME", tmp.path())
        .args(["ripgrep", "--version"])
        .output()
        .unwrap();

    assert!(output.status.success(), "rvx failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("ripgrep"), "Expected ripgrep version output, got: {stdout}");

    // Verify it's cached
    let list_output = Command::new(env!("CARGO_BIN_EXE_rvx"))
        .env("RVX_HOME", tmp.path())
        .args(["--list"])
        .output()
        .unwrap();
    let list_stdout = String::from_utf8(list_output.stdout).unwrap();
    assert!(list_stdout.contains("ripgrep"), "Expected ripgrep in cache list");
}

#[test]
#[ignore]
fn test_rvx_nonexistent_crate() {
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rvx"))
        .env("RVX_HOME", tmp.path())
        .args(["this-crate-definitely-does-not-exist-rvx-test"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("not found"));
}
```

- [ ] **Step 2: Run e2e tests**

```bash
cargo test --test e2e_test -- --ignored
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "test: add end-to-end integration tests"
```

---

### Task 10: File Locking for Concurrent Access

**Files:**
- Modify: `src/download.rs` or `src/main.rs`

- [ ] **Step 1: Add file lock during download to prevent concurrent race conditions**

In `src/main.rs`, wrap the download section with a file lock and double-check cache:

```rust
// In the run() function, before the download step:
use fs2::FileExt;

let lock_path = cache::lock_path(&crate_info.name, &crate_info.version);
std::fs::create_dir_all(lock_path.parent().unwrap())?;
let lock_file = std::fs::File::create(&lock_path)?;
lock_file.lock_exclusive()?; // blocks until lock is acquired

// Double-check cache after acquiring lock (another process may have downloaded while we waited)
let resolved_spec = CrateSpec {
    name: spec.name.clone(),
    version: Some(crate_info.version.clone()),
};
if let Some(cached) = cache::find(&resolved_spec)? {
    lock_file.unlock()?;
    exec::run(&cached, bin_name, &cli.args)?;
    unreachable!();
}

// ... download, extract, store ...

// After storing in cache:
lock_file.unlock()?;
```

Add to `src/cache.rs`:
```rust
pub fn lock_path(name: &str, version: &str) -> PathBuf {
    rvx_home().join("locks").join(format!("{name}-{version}.lock"))
}
```

- [ ] **Step 2: Commit**

```bash
git add -A && git commit -m "feat: file locking for concurrent download safety"
```

---

### Task 11: CLAUDE.md and Final Polish

**Files:**
- Create: `CLAUDE.md`

- [ ] **Step 1: Create CLAUDE.md with project conventions**

```markdown
# rvx

Download and run pre-built Rust crate binaries. No Rust required.

## Build & Test

- `cargo build` — build
- `cargo test` — run unit tests
- `cargo test -- --ignored` — run integration/e2e tests (requires network)
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — format check

## Architecture

Five modules in sequential pipeline: cache → registry → resolve → download → exec.

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
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md && git commit -m "docs: add CLAUDE.md with project conventions"
```

- [ ] **Step 3: Run full test suite and clippy one final time**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
```
Expected: all clean

---
