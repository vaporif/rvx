use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "rvx", about = "Download and run pre-built crate binaries")]
pub struct Cli {
    /// Crate name on crates.io, optionally with @version (e.g. mcp-server-qdrant@0.2.1)
    #[arg(required_unless_present_any = ["list", "clean"])]
    pub crate_spec: Option<String>,

    /// Arguments to pass through to the binary
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Binary name if it differs from crate name
    #[arg(long)]
    pub bin: Option<String>,

    /// Re-download even if cached
    #[arg(long)]
    pub update: bool,

    /// List cached binaries
    #[arg(long)]
    pub list: bool,

    /// Remove all cached binaries
    #[arg(long)]
    pub clean: bool,

    /// Suppress download output
    #[arg(long, short)]
    pub quiet: bool,
}

/// Parsed crate specification
#[derive(Debug, Clone)]
pub struct CrateSpec {
    pub name: String,
    pub version: Option<String>,
}

impl std::str::FromStr for CrateSpec {
    type Err = std::convert::Infallible;

    fn from_str(spec: &str) -> std::result::Result<Self, Self::Err> {
        Ok(if let Some((name, version)) = spec.split_once('@') {
            Self {
                name: name.to_string(),
                version: Some(version.to_string()),
            }
        } else {
            Self {
                name: spec.to_string(),
                version: None,
            }
        })
    }
}
