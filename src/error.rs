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

    #[error("GitHub API rate limit exceeded. Set GITHUB_TOKEN env var for higher limits.")]
    GitHubRateLimit,

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Unsafe path in archive: {0}")]
    UnsafePath(String),

    #[error("Download exceeds maximum size of {max_mb}MB")]
    DownloadTooLarge { max_mb: u64 },

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
