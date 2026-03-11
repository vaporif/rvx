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
use fs2::FileExt;

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

    // Acquire file lock for concurrent download safety
    let lock_path = cache::lock_path(&crate_info.name, &crate_info.version);
    std::fs::create_dir_all(lock_path.parent().unwrap())?;
    let lock_file = std::fs::File::create(&lock_path)?;
    lock_file.lock_exclusive()?;

    // Double-check cache after acquiring lock (another process may have downloaded)
    if !cli.update {
        let resolved_spec = CrateSpec {
            name: spec.name.clone(),
            version: Some(crate_info.version.clone()),
        };
        if let Some(cached) = cache::find(&resolved_spec)? {
            lock_file.unlock()?;
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

    // Clean up temp download directory
    if let Some(parent) = binary_path.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }

    lock_file.unlock()?;

    // Exec
    exec::run(&cached_path, bin_name, &cli.args)?;
    unreachable!()
}
