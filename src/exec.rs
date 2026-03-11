use crate::error::Result;
use std::path::Path;

/// Replace current process with the cached binary (Unix) or spawn child (Windows)
pub fn run(binary_path: &Path, _bin_name: &str, args: &[String]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(binary_path).args(args).exec();
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
        Err(crate::error::Error::Other(
            "Unsupported platform".to_string(),
        ))
    }
}
