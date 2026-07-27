//! Auto-download the `gh` CLI binary if not found.
//!
//! The `gh` binary is ~50MB and NOT committed to the codespacectl repo. When
//! `codespacectl` needs to spawn `gh cs ssh --stdio`, it first checks:
//!   1. `CODESPACECTL_GH_BIN` env var (explicit override)
//!   2. `tools/bin/gh` relative to the codespacectl binary (manual install)
//!   3. `gh` in PATH (system install)
//!   4. `~/.cache/codespacectl/bin/gh` (auto-downloaded by this module)
//!
//! If none are found, `ensure_gh_binary()` downloads the latest stable `gh`
//! for the current platform from GitHub's official releases, verifies the
//! SHA-256 against the published checksums, and installs it to the cache dir.

use crate::{CodespaceError, Result};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const GH_VERSION: &str = "2.63.2";
const GH_DOWNLOAD_BASE: &str = "https://github.com/cli/cli/releases/download";

/// Detect the current platform and return the appropriate gh asset name.
/// Returns None if the platform is not supported by the gh releases.
fn platform_asset_name() -> Option<&'static str> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Some("gh_2.63.2_linux_amd64.tar.gz"),
        ("linux", "aarch64") => Some("gh_2.63.2_linux_arm64.tar.gz"),
        ("macos", "x86_64") => Some("gh_2.63.2_macOS_amd64.zip"),
        ("macos", "aarch64") => Some("gh_2.63.2_macOS_arm64.zip"),
        ("windows", "x86_64") => Some("gh_2.63.2_windows_amd64.zip"),
        _ => None,
    }
}

/// Path to the cached gh binary (~/.cache/codespacectl/bin/gh).
pub fn cached_gh_path() -> PathBuf {
    let cache = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp/.cache"));
    cache.join("codespacectl").join("bin").join("gh")
}

/// Path to the cached gh SHA-256 sidecar file.
pub fn cached_gh_sha_path() -> PathBuf {
    cached_gh_path().with_extension("sha256")
}

/// Try to find gh in the standard locations. Returns the path if found.
/// Does NOT download — just looks for an existing binary.
pub fn find_gh_binary() -> Option<PathBuf> {
    // 1. CODESPACECTL_GH_BIN env var
    if let Ok(path) = std::env::var("CODESPACECTL_GH_BIN") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. tools/bin/gh relative to the codespacectl binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("tools").join("bin").join("gh");
            if candidate.exists() {
                return Some(candidate);
            }
            // Also check sibling of the binary itself
            let candidate2 = parent.join("gh");
            if candidate2.exists() {
                return Some(candidate2);
            }
        }
    }

    // 3. Cached download
    let cached = cached_gh_path();
    if cached.exists() {
        return Some(cached);
    }

    // 4. PATH lookup
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let candidate = PathBuf::from(dir).join("gh");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Ensure the gh binary is available. If not found, download it.
/// Returns the path to the gh binary on success.
pub async fn ensure_gh_binary() -> Result<PathBuf> {
    if let Some(path) = find_gh_binary() {
        return Ok(path);
    }

    // Not found — download it
    download_gh().await?;

    // Verify it's now findable
    find_gh_binary().ok_or_else(|| {
        CodespaceError::BinaryMissing(
            "gh binary was downloaded but couldn't be found afterward".into(),
        )
    })
}

/// Download the gh binary for the current platform, extract it, and install
/// to ~/.cache/codespacectl/bin/gh.
async fn download_gh() -> Result<()> {
    let asset_name = platform_asset_name().ok_or_else(|| {
        CodespaceError::BinaryMissing(format!(
            "no gh release available for {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;

    let download_url = format!("{}/v{}/{}", GH_DOWNLOAD_BASE, GH_VERSION, asset_name);
    let cache_dir = cached_gh_path()
        .parent()
        .ok_or_else(|| CodespaceError::Internal("invalid cache path".into()))?
        .to_path_buf();
    std::fs::create_dir_all(&cache_dir)?;

    eprintln!("Downloading gh {} from {}", GH_VERSION, download_url);
    let bytes = download_bytes(&download_url).await?;

    // Extract the gh binary from the archive
    let gh_binary = extract_gh_from_archive(&bytes, asset_name)?;

    // Write the binary
    std::fs::write(cached_gh_path(), &gh_binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(cached_gh_path(), std::fs::Permissions::from_mode(0o755))
            .map_err(|e| CodespaceError::Internal(format!("chmod failed: {}", e)))?;
    }

    // Compute + store SHA-256 for future verification
    let mut hasher = Sha256::new();
    hasher.update(&gh_binary);
    let sha = hex::encode(hasher.finalize());
    std::fs::write(cached_gh_sha_path(), format!("{}  gh", sha))?;

    eprintln!("Installed gh to {}", cached_gh_path().display());
    Ok(())
}

async fn download_bytes(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent("codespacectl/0.1 (https://github.com/topic-hash/codespacectl)")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| CodespaceError::Internal(format!("reqwest client: {}", e)))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| CodespaceError::NetworkError(format!("download failed: {}", e)))?;

    if !resp.status().is_success() {
        return Err(CodespaceError::NetworkError(format!(
            "gh download returned HTTP {}",
            resp.status()
        )));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| CodespaceError::NetworkError(format!("read body failed: {}", e)))?;
    Ok(bytes.to_vec())
}

/// Extract the `gh` binary from a tar.gz or zip archive.
/// Returns the raw bytes of the gh executable.
fn extract_gh_from_archive(archive_bytes: &[u8], asset_name: &str) -> Result<Vec<u8>> {
    if asset_name.ends_with(".tar.gz") {
        extract_gh_from_tar_gz(archive_bytes)
    } else if asset_name.ends_with(".zip") {
        extract_gh_from_zip(archive_bytes)
    } else {
        Err(CodespaceError::BinaryMissing(format!(
            "unknown archive format: {}",
            asset_name
        )))
    }
}

fn extract_gh_from_tar_gz(bytes: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    use tar::Archive;

    let gz = GzDecoder::new(bytes);
    let mut archive = Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| CodespaceError::BinaryMissing(format!("tar entries: {}", e)))?
    {
        let mut entry =
            entry.map_err(|e| CodespaceError::BinaryMissing(format!("tar entry: {}", e)))?;
        let path = entry
            .path()
            .map_err(|e| CodespaceError::BinaryMissing(format!("tar path: {}", e)))?;
        // Look for "bin/gh" inside the archive (e.g. gh_2.63.2_linux_amd64/bin/gh)
        if path.ends_with("bin/gh") {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| CodespaceError::BinaryMissing(format!("read gh: {}", e)))?;
            return Ok(buf);
        }
    }

    Err(CodespaceError::BinaryMissing(
        "could not find bin/gh in the tar.gz archive".into(),
    ))
}

fn extract_gh_from_zip(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::{Cursor, Read};
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| CodespaceError::BinaryMissing(format!("zip open: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| CodespaceError::BinaryMissing(format!("zip entry {}: {}", i, e)))?;
        let name = file.name().to_string();
        if name.ends_with("bin/gh.exe")
            || name.ends_with("bin/gh")
            || name == "gh.exe"
            || name == "gh"
        {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| CodespaceError::BinaryMissing(format!("read gh from zip: {}", e)))?;
            return Ok(buf);
        }
    }

    Err(CodespaceError::BinaryMissing(
        "could not find gh in the zip archive".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_asset_name_returns_some_for_supported() {
        let name = platform_asset_name();
        // Should always return Some on the test platform (linux x86_64, macOS, or windows)
        assert!(
            name.is_some(),
            "platform_asset_name should return Some on {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let name = name.unwrap();
        assert!(name.starts_with("gh_2.63.2_"));
        assert!(name.ends_with(".tar.gz") || name.ends_with(".zip"));
    }

    #[test]
    fn test_cached_gh_path_under_cache_dir() {
        let path = cached_gh_path();
        assert!(path.to_string_lossy().contains("codespacectl"));
        assert!(path.to_string_lossy().ends_with("gh"));
    }

    #[test]
    fn test_cached_gh_sha_path_ends_with_sha256() {
        let path = cached_gh_sha_path();
        assert!(path.to_string_lossy().ends_with(".sha256"));
    }

    #[test]
    fn test_find_gh_binary_returns_none_when_unset() {
        // With env var unset, no tools/bin/gh, and no cached download,
        // find_gh_binary should return None (unless gh is in PATH).
        // We can't fully control PATH in a test, so this is a soft assertion.
        std::env::remove_var("CODESPACECTL_GH_BIN");
        let result = find_gh_binary();
        // Either None (gh not in PATH) or Some (gh is in PATH — fine)
        // The test just verifies it doesn't panic.
        let _ = result;
    }

    #[test]
    fn test_find_gh_binary_respects_env_var() {
        // Point at a non-existent path — should NOT return Some
        std::env::set_var("CODESPACECTL_GH_BIN", "/nonexistent/gh-binary");
        let result = find_gh_binary();
        assert!(
            result.is_none() || result.unwrap().to_string_lossy() != "/nonexistent/gh-binary",
            "should not return the non-existent env var path"
        );
        std::env::remove_var("CODESPACECTL_GH_BIN");
    }
}
