use crate::cli::CrateSpec;
use crate::error::{Error, Result};
use serde::Deserialize;
use std::io::Read;

const CRATES_IO_API: &str = "https://crates.io/api/v1";

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
    #[allow(dead_code)] // parsed from Cargo.toml, reserved for future use
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
        .user_agent(crate::USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
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
        let exists = data.versions.iter().any(|ver| ver.num == *v && !ver.yanked);
        if !exists {
            return Err(Error::VersionNotFound {
                crate_name: spec.name.clone(),
                version: v.clone(),
            });
        }
        v.clone()
    } else {
        data.krate
            .max_stable_version
            .or_else(|| {
                data.versions
                    .iter()
                    .find(|v| !v.yanked)
                    .map(|v| v.num.clone())
            })
            .ok_or_else(|| {
                Error::Other(format!("No available version for crate `{}`", spec.name))
            })?
    };

    // Find the dl_path for this version
    let dl_path = data
        .versions
        .iter()
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
