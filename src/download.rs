use crate::error::{Error, Result};
use crate::registry::CrateInfo;
use crate::resolve::{ArchiveFormat, Artifact};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const USER_AGENT: &str = "rvx (https://github.com/user/rvx)";

pub fn download(
    artifact: &Artifact,
    _info: &CrateInfo,
    bin_name: &str,
    quiet: bool,
) -> Result<PathBuf> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    if !quiet {
        eprintln!("Downloading {}", artifact.url);
    }

    let resp = client.get(&artifact.url).send()?.error_for_status()?;
    let bytes = resp.bytes()?;

    // Verify checksum if available
    if let Some(ref expected) = artifact.checksum {
        let mut hasher = Sha256::new();
        hasher.update(bytes.as_ref());
        let actual = format!("{:x}", hasher.finalize());
        if actual != *expected {
            return Err(Error::ChecksumMismatch {
                expected: expected.clone(),
                actual,
            });
        }
        if !quiet {
            eprintln!("Checksum verified");
        }
    }

    // Extract to temp dir
    let extract_dir = tempfile::tempdir()?;
    extract_archive(bytes.as_ref(), extract_dir.path(), &artifact.format)?;

    // Find the binary
    let binary_name = if cfg!(target_os = "windows") {
        format!("{bin_name}.exe")
    } else {
        bin_name.to_string()
    };

    let binary_path = find_binary(extract_dir.path(), &binary_name)?;

    // Copy binary to a stable temp location that outlives this function
    let stable_dir = tempfile::tempdir()?;
    let dest = stable_dir.path().join(&binary_name);
    fs::copy(&binary_path, &dest)?;

    // Make binary executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
    }

    // Keep the temp dir alive by leaking it
    let stable_path = stable_dir.keep();
    let final_path = stable_path.join(&binary_name);

    if !quiet {
        eprintln!("Extracted binary: {}", final_path.display());
    }

    Ok(final_path)
}

fn extract_archive(data: &[u8], dest: &Path, format: &ArchiveFormat) -> Result<()> {
    match format {
        ArchiveFormat::TarGz => {
            let gz = flate2::read::GzDecoder::new(data);
            let mut archive = tar::Archive::new(gz);
            archive.unpack(dest)?;
        }
        ArchiveFormat::TarXz => {
            let xz = xz2::read::XzDecoder::new(data);
            let mut archive = tar::Archive::new(xz);
            archive.unpack(dest)?;
        }
        ArchiveFormat::TarZst => {
            let zst = zstd::stream::read::Decoder::new(data)?;
            let mut archive = tar::Archive::new(zst);
            archive.unpack(dest)?;
        }
        ArchiveFormat::Zip => {
            let cursor = Cursor::new(data);
            let mut archive = zip::ZipArchive::new(cursor)?;
            archive.extract(dest)?;
        }
    }
    Ok(())
}

fn find_binary(dir: &Path, binary_name: &str) -> Result<PathBuf> {
    let entries = walkdir(dir)?;

    for entry in &entries {
        if let Some(name) = entry.file_name().and_then(|n| n.to_str()) {
            if name == binary_name {
                return Ok(entry.clone());
            }
        }
    }

    Err(Error::BinaryNotFoundInArchive(binary_name.to_string()))
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
    use std::io::{Read, Write};

    #[test]
    fn test_extract_tar_gz() {
        // Create a tar.gz in memory containing a fake binary
        let mut tar_builder = tar::Builder::new(Vec::new());

        let binary_content = b"#!/bin/sh\necho hello";
        let mut header = tar::Header::new_gnu();
        header.set_path("mybin").unwrap();
        header.set_size(binary_content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar_builder.append(&header, &binary_content[..]).unwrap();

        let tar_data = tar_builder.into_inner().unwrap();

        // Gzip it
        let mut gz_encoder =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz_encoder.write_all(&tar_data).unwrap();
        let gz_data = gz_encoder.finish().unwrap();

        // Extract
        let tmp = tempfile::tempdir().unwrap();
        extract_archive(&gz_data, tmp.path(), &ArchiveFormat::TarGz).unwrap();

        // Find binary
        let found = find_binary(tmp.path(), "mybin").unwrap();
        assert!(found.exists());

        let mut content = String::new();
        std::fs::File::open(&found)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert!(content.contains("echo hello"));
    }

    #[test]
    fn test_find_binary_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let result = find_binary(tmp.path(), "nonexistent");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("nonexistent"),
            "Error should mention binary name: {err}"
        );
    }
}
