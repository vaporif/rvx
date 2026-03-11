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
    format!("{name}@{version}")
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
        return path.exists().then_some(path);
    }

    // No version specified — find any cached version for this crate
    let prefix = format!("{name}@");
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
pub fn store(
    binary_path: &Path,
    info: &crate::registry::CrateInfo,
    artifact: &crate::resolve::Artifact,
) -> Result<PathBuf> {
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

pub fn lock_path(name: &str, version: &str) -> PathBuf {
    rvx_home()
        .join("locks")
        .join(format!("{name}-{version}.lock"))
}

fn timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
