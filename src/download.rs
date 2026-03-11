use crate::error::{Error, Result};
use crate::resolve::{ArchiveFormat, Artifact};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

pub fn download(artifact: &Artifact, bin_name: &str, quiet: bool) -> Result<PathBuf> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(crate::USER_AGENT)
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
    let stable_path = stable_dir.keep();
    let final_path = stable_path.join(&binary_name);
    fs::copy(&binary_path, &final_path)?;
    crate::set_executable(&final_path)?;

    if !quiet {
        eprintln!("Extracted binary: {}", final_path.display());
    }

    Ok(final_path)
}

fn extract_archive(data: &[u8], dest: &Path, format: &ArchiveFormat) -> Result<()> {
    match format {
        ArchiveFormat::TarGz => {
            let gz = flate2::read::GzDecoder::new(data);
            let archive = tar::Archive::new(gz);
            safe_unpack_tar(archive, dest)?;
        }
        ArchiveFormat::TarXz => {
            let xz = xz2::read::XzDecoder::new(data);
            let archive = tar::Archive::new(xz);
            safe_unpack_tar(archive, dest)?;
        }
        ArchiveFormat::TarZst => {
            let zst = zstd::stream::read::Decoder::new(data)?;
            let archive = tar::Archive::new(zst);
            safe_unpack_tar(archive, dest)?;
        }
        ArchiveFormat::Zip => {
            safe_extract_zip(data, dest)?;
        }
    }
    Ok(())
}

fn safe_unpack_tar<R: std::io::Read>(mut archive: tar::Archive<R>, dest: &Path) -> Result<()> {
    let dest = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();

        // Reject any path with ".." components
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(Error::UnsafePath(path.display().to_string()));
        }

        let full_path = dest.join(&path);
        // Ensure resolved path is within destination
        let resolved = if full_path.exists() {
            full_path.canonicalize()?
        } else {
            // For new files, canonicalize the parent
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
                parent
                    .canonicalize()?
                    .join(full_path.file_name().unwrap_or_default())
            } else {
                full_path.clone()
            }
        };

        if !resolved.starts_with(&dest) {
            return Err(Error::UnsafePath(path.display().to_string()));
        }

        entry.unpack(&resolved)?;
    }
    Ok(())
}

fn safe_extract_zip(data: &[u8], dest: &Path) -> Result<()> {
    let cursor = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)?;
    let dest = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let Some(enclosed_name) = file.enclosed_name() else {
            return Err(Error::UnsafePath(file.name().to_string()));
        };

        let out_path = dest.join(enclosed_name);

        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&out_path)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

fn find_binary(dir: &Path, binary_name: &str) -> Result<PathBuf> {
    find_binary_recursive(dir, binary_name)
        .ok_or_else(|| Error::BinaryNotFoundInArchive(binary_name.to_string()))
}

fn find_binary_recursive(dir: &Path, binary_name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_binary_recursive(&path, binary_name) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some(binary_name) {
            return Some(path);
        }
    }
    None
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
    fn test_tar_path_traversal_rejected() {
        // Build a malicious tar manually since tar::Builder rejects ".." in paths
        let mut tar_builder = tar::Builder::new(Vec::new());

        let content = b"malicious";
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_entry_type(tar::EntryType::Regular);
        // Write the path directly into the header bytes to bypass validation
        {
            let path_bytes = b"../../etc/evil";
            let header_bytes = header.as_mut_bytes();
            header_bytes[..path_bytes.len()].copy_from_slice(path_bytes);
            header_bytes[path_bytes.len()] = 0;
        }
        header.set_cksum();
        tar_builder.append(&header, &content[..]).unwrap();

        let tar_data = tar_builder.into_inner().unwrap();

        let mut gz_encoder =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz_encoder.write_all(&tar_data).unwrap();
        let gz_data = gz_encoder.finish().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let result = extract_archive(&gz_data, tmp.path(), &ArchiveFormat::TarGz);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsafe path"));
    }

    #[test]
    fn test_zip_extraction_works() {
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        zip_writer.start_file("mybin", options).unwrap();
        zip_writer.write_all(b"binary content").unwrap();
        let cursor = zip_writer.finish().unwrap();
        let zip_data = cursor.into_inner();

        let tmp = tempfile::tempdir().unwrap();
        extract_archive(&zip_data, tmp.path(), &ArchiveFormat::Zip).unwrap();

        let found = find_binary(tmp.path(), "mybin").unwrap();
        assert!(found.exists());
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
