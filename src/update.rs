use crate::config::VERSION;
use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct UpdateOptions {
    pub repo: String,
    pub version: Option<String>,
    pub check_only: bool,
    pub force: bool,
}

#[derive(Debug)]
pub enum UpdateOutcome {
    AlreadyCurrent {
        version: String,
    },
    UpdateAvailable {
        current: String,
        latest: String,
        asset_name: String,
        asset_size: u64,
    },
    Updated {
        previous: String,
        current: String,
        path: PathBuf,
    },
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub async fn run(options: UpdateOptions) -> Result<UpdateOutcome> {
    validate_repo(&options.repo)?;

    let client = github_client()?;
    let release = fetch_release(&client, &options.repo, options.version.as_deref()).await?;
    let latest = normalize_tag(&release.tag_name);
    let current = normalize_tag(VERSION);
    let asset_name = current_asset_name()?;
    let asset = release.assets.iter().find(|asset| asset.name == asset_name);
    let asset = match asset {
        Some(asset) => asset,
        None => {
            let available = release
                .assets
                .iter()
                .map(|asset| asset.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "No release asset named {} in {}. Available assets: {}",
                asset_name,
                release.tag_name,
                if available.is_empty() {
                    "none".to_string()
                } else {
                    available
                }
            );
        }
    };

    if !options.force && latest == current {
        return Ok(UpdateOutcome::AlreadyCurrent { version: current });
    }

    if options.check_only {
        return Ok(UpdateOutcome::UpdateAvailable {
            current,
            latest,
            asset_name: asset.name.clone(),
            asset_size: asset.size,
        });
    }

    let bytes = download_asset(&client, &asset.browser_download_url).await?;
    let installed_path = install_binary(&bytes, &asset.name)?;

    Ok(UpdateOutcome::Updated {
        previous: current,
        current: latest,
        path: installed_path,
    })
}

fn github_client() -> Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&format!("anvil/{}", VERSION))
            .context("Failed to build GitHub User-Agent header")?,
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .context("Failed to build HTTP client")
}

async fn fetch_release(
    client: &reqwest::Client,
    repo: &str,
    version: Option<&str>,
) -> Result<GithubRelease> {
    let endpoint = match version {
        Some(version) => {
            let tag = if version.starts_with('v') {
                version.to_string()
            } else {
                format!("v{}", version)
            };
            format!("https://api.github.com/repos/{repo}/releases/tags/{tag}")
        }
        None => format!("https://api.github.com/repos/{repo}/releases/latest"),
    };

    let response = client
        .get(&endpoint)
        .send()
        .await
        .with_context(|| format!("Failed to query GitHub release: {endpoint}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("GitHub release request failed: {status} {body}");
    }

    response
        .json::<GithubRelease>()
        .await
        .context("Failed to parse GitHub release response")
}

async fn download_asset(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to download release asset: {url}"))?;

    let status = response.status();
    if !status.is_success() {
        bail!("Release asset download failed: {status}");
    }

    let bytes = response
        .bytes()
        .await
        .context("Failed to read asset body")?;
    if bytes.is_empty() {
        bail!("Release asset is empty");
    }
    Ok(bytes.to_vec())
}

fn install_binary(bytes: &[u8], asset_name: &str) -> Result<PathBuf> {
    let current_exe = std::env::current_exe().context("Failed to detect current executable")?;
    let install_dir = current_exe
        .parent()
        .context("Current executable has no parent directory")?;
    let temp_path = install_dir.join(format!(".anvil-update-{}-{asset_name}", std::process::id()));

    std::fs::write(&temp_path, bytes)
        .with_context(|| format!("Failed to write {}", temp_path.display()))?;
    set_executable_permissions(&temp_path)?;
    validate_downloaded_binary(&temp_path)?;

    replace_current_binary(&temp_path, &current_exe)?;
    Ok(current_exe)
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)
        .with_context(|| format!("Failed to read permissions for {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)
        .with_context(|| format!("Failed to make {} executable", path.display()))
}

#[cfg(not(unix))]
fn set_executable_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn validate_downloaded_binary(path: &Path) -> Result<()> {
    let output = Command::new(path)
        .arg("version")
        .output()
        .with_context(|| format!("Failed to run downloaded binary {}", path.display()))?;

    if !output.status.success() {
        bail!("Downloaded binary failed its version check");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim_start().starts_with("anvil ") {
        bail!("Downloaded binary did not look like anvil");
    }

    Ok(())
}

#[cfg(unix)]
fn replace_current_binary(temp_path: &Path, current_exe: &Path) -> Result<()> {
    std::fs::rename(temp_path, current_exe).with_context(|| {
        format!(
            "Failed to replace {}. Try running the command with sudo.",
            current_exe.display()
        )
    })
}

#[cfg(not(unix))]
fn replace_current_binary(temp_path: &Path, current_exe: &Path) -> Result<()> {
    let backup_path = current_exe.with_extension("old");
    if backup_path.exists() {
        let _ = std::fs::remove_file(&backup_path);
    }
    std::fs::rename(current_exe, &backup_path)
        .with_context(|| format!("Failed to move {} out of the way", current_exe.display()))?;
    if let Err(err) = std::fs::rename(temp_path, current_exe) {
        let _ = std::fs::rename(&backup_path, current_exe);
        bail!("Failed to install updated binary: {err}");
    }
    let _ = std::fs::remove_file(&backup_path);
    Ok(())
}

fn validate_repo(repo: &str) -> Result<()> {
    let parts = repo.split('/').collect::<Vec<_>>();
    if parts.len() != 2 || parts.iter().any(|part| part.trim().is_empty()) {
        bail!("Update repo must use owner/name format, got {}", repo);
    }
    Ok(())
}

pub fn current_asset_name() -> Result<String> {
    asset_name_for(std::env::consts::OS, std::env::consts::ARCH)
}

pub fn asset_name_for(os: &str, arch: &str) -> Result<String> {
    match (os, arch) {
        ("linux", "x86_64") => Ok("anvil-linux-x86_64".to_string()),
        ("linux", "aarch64") => Ok("anvil-linux-aarch64".to_string()),
        _ => bail!("Self-update is only supported on Linux x86_64 / aarch64 ({os}/{arch})"),
    }
}

pub fn normalize_tag(tag: &str) -> String {
    tag.trim().trim_start_matches('v').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_release_tags() {
        assert_eq!(normalize_tag("v1.2.3"), "1.2.3");
        assert_eq!(normalize_tag("1.2.3"), "1.2.3");
    }

    #[test]
    fn maps_known_platform_assets() {
        assert_eq!(
            asset_name_for("linux", "x86_64").unwrap(),
            "anvil-linux-x86_64"
        );
        assert_eq!(
            asset_name_for("linux", "aarch64").unwrap(),
            "anvil-linux-aarch64"
        );
        assert!(asset_name_for("macos", "aarch64").is_err());
        assert!(asset_name_for("windows", "x86_64").is_err());
    }

    #[test]
    fn rejects_bad_repo_names() {
        assert!(validate_repo("owner/repo").is_ok());
        assert!(validate_repo("owner").is_err());
        assert!(validate_repo("owner/repo/extra").is_err());
    }
}
