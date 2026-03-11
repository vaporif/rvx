use clap::Parser;
use fs2::FileExt;
use rvx::cache;
use rvx::cli::{Cli, CrateSpec};
use rvx::download;
use rvx::error;
use rvx::exec;
use rvx::registry;
use rvx::resolve;

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

    let spec_str = cli
        .crate_spec
        .as_deref()
        .ok_or_else(|| error::Error::Other("crate_spec is required".to_string()))?;
    let spec: CrateSpec = spec_str.parse().unwrap();
    let bin_name = cli.bin.as_deref().unwrap_or(&spec.name);

    // Check cache first (unless --update)
    if !cli.update {
        if let Some(cached) = cache::find(&spec)? {
            return exec::run(&cached, &cli.args).map(|inf| match inf {});
        }
    }

    // Resolve version and metadata from crates.io
    let crate_info = registry::fetch(&spec)?;

    let resolved_spec = CrateSpec {
        name: spec.name.clone(),
        version: Some(crate_info.version.clone()),
    };

    // Check cache again with resolved version
    if !cli.update {
        if let Some(cached) = cache::find(&resolved_spec)? {
            return exec::run(&cached, &cli.args).map(|inf| match inf {});
        }
    }

    // Acquire file lock for concurrent download safety
    // Lock is released when `_lock_file` is dropped (or process exits/execs)
    let lock_path = cache::lock_path(&crate_info.name, &crate_info.version);
    std::fs::create_dir_all(lock_path.parent().unwrap())?;
    let _lock_file = {
        let f = std::fs::File::create(&lock_path)?;
        f.lock_exclusive()?;
        f
    };

    // Double-check cache after acquiring lock (another process may have downloaded)
    if !cli.update {
        if let Some(cached) = cache::find(&resolved_spec)? {
            return exec::run(&cached, &cli.args).map(|inf| match inf {});
        }
    }

    // Resolve download URL
    let artifact = resolve::resolve(&crate_info, bin_name, cli.quiet)?;

    // Download and extract
    let binary_path = download::download(&artifact, bin_name, cli.quiet)?;

    // Cache the binary
    let cached_path = cache::store(&binary_path, &crate_info, &artifact)?;

    // Clean up temp download directory
    if let Some(parent) = binary_path.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }

    // Exec
    exec::run(&cached_path, &cli.args).map(|inf| match inf {})
}
