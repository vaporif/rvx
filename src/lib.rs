pub mod cache;
pub mod cli;
pub mod download;
pub mod error;
pub mod exec;
pub mod registry;
pub mod resolve;
pub mod target;

pub const USER_AGENT: &str = "rvx (https://github.com/vaporif/rvx)";

/// Maximum artifact download size (500 MB)
pub const MAX_ARTIFACT_SIZE: u64 = 500 * 1024 * 1024;
/// Maximum .crate tarball size (50 MB)
pub const MAX_CRATE_TARBALL_SIZE: u64 = 50 * 1024 * 1024;

/// Read a response body with a size limit, streaming into a Vec.
pub fn read_response_with_limit(
    resp: reqwest::blocking::Response,
    max_bytes: u64,
) -> error::Result<Vec<u8>> {
    use std::io::Read;

    // Check Content-Length header first if available
    if let Some(len) = resp.content_length() {
        if len > max_bytes {
            return Err(error::Error::DownloadTooLarge {
                max_mb: max_bytes / (1024 * 1024),
            });
        }
    }

    let mut buf = Vec::new();
    let mut reader = resp.take(max_bytes + 1);
    reader.read_to_end(&mut buf)?;

    if buf.len() as u64 > max_bytes {
        return Err(error::Error::DownloadTooLarge {
            max_mb: max_bytes / (1024 * 1024),
        });
    }

    Ok(buf)
}

/// Set executable permission (0o755) on Unix, no-op on other platforms.
pub fn set_executable(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    let _ = path;
    Ok(())
}
