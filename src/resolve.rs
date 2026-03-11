use crate::error::{Error, Result};
use crate::registry::CrateInfo;
use crate::target;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;

#[derive(Debug)]
pub struct Artifact {
    pub url: String,
    pub checksum: Option<String>,
    pub format: ArchiveFormat,
}

#[derive(Debug)]
pub enum ArchiveFormat {
    TarGz,
    TarXz,
    TarZst,
    Zip,
}

impl ArchiveFormat {
    pub fn from_filename(name: &str) -> Option<Self> {
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Some(Self::TarGz)
        } else if name.ends_with(".tar.xz") {
            Some(Self::TarXz)
        } else if name.ends_with(".tar.zst") || name.ends_with(".tar.zstd") {
            Some(Self::TarZst)
        } else if name.ends_with(".zip") {
            Some(Self::Zip)
        } else {
            None
        }
    }

    fn extensions() -> &'static [&'static str] {
        &[".tar.gz", ".tar.xz", ".tar.zst", ".zip", ".tgz"]
    }
}

fn build_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(crate::USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .build()?)
}

fn github_auth_header() -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {token}")) {
            headers.insert(AUTHORIZATION, val);
        }
    }
    headers
}

fn auth_for_url(url: &str) -> HeaderMap {
    if url.contains("github.com") {
        github_auth_header()
    } else {
        HeaderMap::new()
    }
}

pub fn resolve(info: &CrateInfo, bin_name: &str, quiet: bool) -> Result<Artifact> {
    let client = build_client()?;

    for target_triple in target::target_variants() {
        // Try binstall metadata first
        if let Some(ref binstall) = info.binstall {
            if let Some(artifact) =
                try_binstall(&client, info, bin_name, &target_triple, binstall, quiet)?
            {
                return Ok(artifact);
            }
        }

        // Fallback: GitHub releases
        if let Some(ref repo_url) = info.repository {
            if let Some((owner, repo)) = parse_github_url(repo_url) {
                if let Some(artifact) = try_github_release(
                    &client,
                    info,
                    bin_name,
                    &target_triple,
                    &owner,
                    &repo,
                    quiet,
                )? {
                    return Ok(artifact);
                }
            }
        }
    }

    if info.repository.is_none() && info.binstall.is_none() {
        return Err(Error::NoRepositoryUrl);
    }

    Err(Error::NoBinaryForPlatform {
        target: target::current_target().to_string(),
    })
}

fn try_binstall(
    client: &reqwest::blocking::Client,
    info: &CrateInfo,
    bin_name: &str,
    target_triple: &str,
    binstall: &crate::registry::BinstallMeta,
    quiet: bool,
) -> Result<Option<Artifact>> {
    // If pkg_fmt is specified, only try that format
    let extensions: Vec<&str> = if let Some(ref fmt) = binstall.pkg_fmt {
        match fmt.as_str() {
            "tgz" | "tar.gz" => vec![".tar.gz"],
            "txz" | "tar.xz" => vec![".tar.xz"],
            "tar.zst" | "tar.zstd" => vec![".tar.zst"],
            "zip" => vec![".zip"],
            _ => ArchiveFormat::extensions().to_vec(),
        }
    } else {
        ArchiveFormat::extensions().to_vec()
    };

    for ext in &extensions {
        let url = render_binstall_template(&binstall.pkg_url, info, bin_name, target_triple, ext);

        // HEAD check to verify URL exists
        let resp = client.head(&url).headers(auth_for_url(&url)).send();
        match resp {
            Ok(r) if r.status().is_success() => {
                if !quiet {
                    eprintln!("Found binary via binstall: {url}");
                }
                let format = ArchiveFormat::from_filename(&url).unwrap_or(ArchiveFormat::TarGz);

                let checksum = try_fetch_checksum(client, &url)?;

                return Ok(Some(Artifact {
                    url,
                    checksum,
                    format,
                }));
            }
            _ => continue,
        }
    }

    Ok(None)
}

fn render_binstall_template(
    template: &str,
    info: &CrateInfo,
    bin_name: &str,
    target_triple: &str,
    archive_ext: &str,
) -> String {
    let repo_url = info.repository.as_deref().unwrap_or("");
    let binary_ext = target::binary_ext();

    // Strip leading dot from archive_ext for the template variable
    let archive_format = archive_ext.strip_prefix('.').unwrap_or(archive_ext);

    let mut result = template.to_string();

    // Replace both `{name}` and `{ name }` style placeholders
    let replacements = [
        ("repo", repo_url),
        ("version", &info.version),
        ("name", &info.name),
        ("target", target_triple),
        ("bin", bin_name),
        ("archive-format", archive_format),
        ("binary-ext", binary_ext),
    ];

    for (key, value) in &replacements {
        // No spaces
        result = result.replace(&format!("{{{key}}}"), value);
        // With spaces
        result = result.replace(&format!("{{ {key} }}"), value);
    }

    result
}

fn try_github_release(
    client: &reqwest::blocking::Client,
    info: &CrateInfo,
    bin_name: &str,
    target_triple: &str,
    owner: &str,
    repo: &str,
    quiet: bool,
) -> Result<Option<Artifact>> {
    let tag_patterns = [
        format!("v{}", info.version),
        info.version.clone(),
        format!("{}-v{}", info.name, info.version),
        format!("{}-{}", info.name, info.version),
    ];

    let auth_headers = github_auth_header();

    for tag in &tag_patterns {
        let api_url = format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}");

        let resp = client.get(&api_url).headers(auth_headers.clone()).send()?;

        if resp.status() == reqwest::StatusCode::FORBIDDEN
            || resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            return Err(Error::GitHubRateLimit);
        }

        if !resp.status().is_success() {
            continue;
        }

        let release: GitHubRelease = resp.json()?;

        if let Some(asset) =
            find_matching_asset(&release.assets, target_triple, bin_name, &info.name)
        {
            let format = ArchiveFormat::from_filename(&asset.name).unwrap_or(ArchiveFormat::TarGz);

            if !quiet {
                eprintln!(
                    "Found binary via GitHub release: {}",
                    asset.browser_download_url
                );
            }

            let checksum = find_checksum_in_assets(client, &release.assets, &asset.name)?
                .or(try_fetch_checksum(client, &asset.browser_download_url)?);

            return Ok(Some(Artifact {
                url: asset.browser_download_url.clone(),
                checksum,
                format,
            }));
        }
    }

    Ok(None)
}

#[derive(Deserialize)]
struct GitHubRelease {
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

fn find_matching_asset<'a>(
    assets: &'a [GitHubAsset],
    target_triple: &str,
    bin_name: &str,
    crate_name: &str,
) -> Option<&'a GitHubAsset> {
    let mut first = None;
    let mut by_crate = None;

    for asset in assets.iter().filter(|a| {
        a.name.contains(target_triple) && ArchiveFormat::from_filename(&a.name).is_some()
    }) {
        if first.is_none() {
            first = Some(asset);
        }
        if asset.name.contains(bin_name) {
            return Some(asset);
        }
        if asset.name.contains(crate_name) && by_crate.is_none() {
            by_crate = Some(asset);
        }
    }

    by_crate.or(first)
}

fn find_checksum_in_assets(
    client: &reqwest::blocking::Client,
    assets: &[GitHubAsset],
    artifact_name: &str,
) -> Result<Option<String>> {
    let checksum_filenames = [
        "SHA256SUMS",
        "SHA256SUMS.txt",
        "checksums.txt",
        "sha256sums.txt",
        "CHECKSUMS",
        "CHECKSUMS.txt",
    ];

    for asset in assets {
        let name_upper = asset.name.to_uppercase();
        if checksum_filenames
            .iter()
            .any(|c| c.to_uppercase() == name_upper)
        {
            let resp = client
                .get(&asset.browser_download_url)
                .headers(auth_for_url(&asset.browser_download_url))
                .send()?;
            if resp.status().is_success() {
                let body = resp.text()?;
                if let Some(checksum) = parse_checksum_file(&body, artifact_name) {
                    return Ok(Some(checksum));
                }
            }
        }
    }

    Ok(None)
}

fn parse_checksum_file(content: &str, artifact_name: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Format: "hash  filename" or "hash *filename" or "hash filename"
        if line.contains(artifact_name) {
            let hash = line.split_whitespace().next()?;
            // SHA256 hashes are 64 hex characters
            if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(hash.to_string());
            }
        }
    }
    None
}

fn try_fetch_checksum(
    client: &reqwest::blocking::Client,
    artifact_url: &str,
) -> Result<Option<String>> {
    let suffixes = [".sha256", ".sha256sum", ".SHA256SUM"];

    for suffix in &suffixes {
        let checksum_url = format!("{artifact_url}{suffix}");
        let resp = client
            .get(&checksum_url)
            .headers(auth_for_url(&checksum_url))
            .send();
        if let Ok(r) = resp {
            if r.status().is_success() {
                let body = r.text()?;
                let hash = body.split_whitespace().next().unwrap_or("").trim();
                if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Ok(Some(hash.to_string()));
                }
            }
        }
    }

    Ok(None)
}

fn parse_github_url(url: &str) -> Option<(String, String)> {
    // Handle https://github.com/owner/repo[.git][/...]
    if let Some(rest) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
    {
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            let repo = parts[1].strip_suffix(".git").unwrap_or(parts[1]);
            return Some((parts[0].to_string(), repo.to_string()));
        }
    }

    // Handle git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            let repo = parts[1].strip_suffix(".git").unwrap_or(parts[1]);
            return Some((parts[0].to_string(), repo.to_string()));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_url() {
        // HTTPS
        assert_eq!(
            parse_github_url("https://github.com/owner/repo"),
            Some(("owner".to_string(), "repo".to_string()))
        );

        // HTTPS with .git
        assert_eq!(
            parse_github_url("https://github.com/owner/repo.git"),
            Some(("owner".to_string(), "repo".to_string()))
        );

        // git@ format
        assert_eq!(
            parse_github_url("git@github.com:owner/repo.git"),
            Some(("owner".to_string(), "repo".to_string()))
        );

        // Non-GitHub returns None
        assert_eq!(parse_github_url("https://gitlab.com/owner/repo"), None);
    }

    #[test]
    fn test_render_binstall_template() {
        let info = CrateInfo {
            name: "mycrate".to_string(),
            version: "1.2.3".to_string(),
            repository: Some("https://github.com/user/mycrate".to_string()),
            binstall: None,
        };

        let template = "{ repo }/releases/download/v{ version }/{ name }-{ target }-{ bin }{ binary-ext }.{ archive-format }";
        let result = render_binstall_template(
            template,
            &info,
            "mybin",
            "x86_64-unknown-linux-gnu",
            ".tar.gz",
        );
        assert_eq!(
            result,
            "https://github.com/user/mycrate/releases/download/v1.2.3/mycrate-x86_64-unknown-linux-gnu-mybin.tar.gz"
        );

        // Also test without spaces in braces
        let template2 = "{repo}/releases/download/v{version}/{name}-{target}.{archive-format}";
        let result2 = render_binstall_template(
            template2,
            &info,
            "mybin",
            "x86_64-unknown-linux-gnu",
            ".tar.gz",
        );
        assert_eq!(
            result2,
            "https://github.com/user/mycrate/releases/download/v1.2.3/mycrate-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn test_archive_format_from_filename() {
        assert!(matches!(
            ArchiveFormat::from_filename("foo.tar.gz"),
            Some(ArchiveFormat::TarGz)
        ));
        assert!(matches!(
            ArchiveFormat::from_filename("foo.tgz"),
            Some(ArchiveFormat::TarGz)
        ));
        assert!(matches!(
            ArchiveFormat::from_filename("foo.tar.xz"),
            Some(ArchiveFormat::TarXz)
        ));
        assert!(matches!(
            ArchiveFormat::from_filename("foo.tar.zst"),
            Some(ArchiveFormat::TarZst)
        ));
        assert!(matches!(
            ArchiveFormat::from_filename("foo.zip"),
            Some(ArchiveFormat::Zip)
        ));
        assert!(ArchiveFormat::from_filename("foo.txt").is_none());
        assert!(ArchiveFormat::from_filename("foo.rpm").is_none());
    }
}
