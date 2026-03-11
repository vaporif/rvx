pub mod cache;
pub mod cli;
pub mod download;
pub mod error;
pub mod exec;
pub mod registry;
pub mod resolve;
pub mod target;

pub const USER_AGENT: &str = "rvx (https://github.com/user/rvx)";

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
