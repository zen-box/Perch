//! GitHub Release update metadata and verified package download.
//! No network call happens until the user explicitly checks for updates.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt as _;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::paths;

const REPOSITORY: &str = "zen-box/Perch";
const MANIFEST_URL: &str = "https://github.com/zen-box/Perch/releases/latest/download/update.json";
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_PACKAGE_BYTES: u64 = 800 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateError {
    Network,
    NoRelease,
    DownloadNetwork,
    Manifest,
    Platform,
    Disk,
    Size,
    Checksum,
    Install,
}

#[derive(Clone, Debug, Deserialize)]
struct Manifest {
    version: String,
    assets: HashMap<String, Asset>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub filename: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub platform: String,
    pub asset: Asset,
}

impl Release {
    pub fn download_url(&self) -> String {
        format!(
            "https://github.com/{REPOSITORY}/releases/download/v{}/{}",
            self.version, self.asset.filename
        )
    }
}

pub fn platform_key(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("windows-x86_64"),
        ("windows", "aarch64") => Some("windows-aarch64"),
        ("linux", "x86_64") => Some("linux-x86_64"),
        ("linux", "aarch64") => Some("linux-aarch64"),
        ("macos", "x86_64") => Some("macos-x86_64"),
        ("macos", "aarch64") => Some("macos-aarch64"),
        _ => None,
    }
}

pub fn current_platform() -> Option<&'static str> {
    platform_key(std::env::consts::OS, std::env::consts::ARCH)
}

fn version(value: &str) -> Option<[u64; 3]> {
    let components: Vec<&str> = value.split('.').collect();
    if components.len() != 3
        || components
            .iter()
            .any(|item| item.is_empty() || !item.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some([
        components[0].parse().ok()?,
        components[1].parse().ok()?,
        components[2].parse().ok()?,
    ])
}

fn expected_filename(version: &str, platform: &str) -> Option<String> {
    let suffix = if platform.starts_with("windows-") {
        "-setup.exe"
    } else if platform.starts_with("linux-") {
        ".AppImage"
    } else if platform.starts_with("macos-") {
        ".zip"
    } else {
        return None;
    };
    Some(format!("Perch-{version}-{platform}{suffix}"))
}

pub fn parse_manifest(data: &[u8], current_version: &str, platform: &str) -> Result<Option<Release>, UpdateError> {
    if data.len() > MAX_MANIFEST_BYTES || version(current_version).is_none() {
        return Err(UpdateError::Manifest);
    }
    let manifest: Manifest = serde_json::from_slice(data).map_err(|_| UpdateError::Manifest)?;
    let latest = version(&manifest.version).ok_or(UpdateError::Manifest)?;
    if latest <= version(current_version).ok_or(UpdateError::Manifest)? {
        return Ok(None);
    }
    let asset = manifest.assets.get(platform).ok_or(UpdateError::Platform)?;
    if Some(asset.filename.clone()) != expected_filename(&manifest.version, platform)
        || asset.size == 0
        || asset.size > MAX_PACKAGE_BYTES
        || asset.sha256.len() != 64
        || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(UpdateError::Manifest);
    }
    Ok(Some(Release {
        version: manifest.version,
        platform: platform.to_string(),
        asset: asset.clone(),
    }))
}

fn client() -> Result<Client, UpdateError> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|_| UpdateError::Network)
}

pub async fn check() -> Result<Option<Release>, UpdateError> {
    let platform = current_platform().ok_or(UpdateError::Platform)?;
    check_url(MANIFEST_URL, env!("CARGO_PKG_VERSION"), platform).await
}

async fn check_url(url: &str, current_version: &str, platform: &str) -> Result<Option<Release>, UpdateError> {
    let response = client()?
        .get(url)
        .header("User-Agent", concat!("Perch/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|_| UpdateError::Network)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(UpdateError::NoRelease);
    }
    let response = response.error_for_status().map_err(|_| UpdateError::Network)?;
    if response
        .content_length()
        .is_some_and(|len| len > MAX_MANIFEST_BYTES as u64)
    {
        return Err(UpdateError::Manifest);
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| UpdateError::Network)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_MANIFEST_BYTES {
            return Err(UpdateError::Manifest);
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_manifest(&bytes, current_version, platform)
}

pub(crate) fn verify_download(path: &Path, asset: &Asset) -> Result<bool, UpdateError> {
    let mut file = File::open(path).map_err(|_| UpdateError::Disk)?;
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|_| UpdateError::Disk)?;
        if count == 0 {
            break;
        }
        bytes = bytes.checked_add(count as u64).ok_or(UpdateError::Size)?;
        if bytes > asset.size {
            return Ok(false);
        }
        digest.update(&buffer[..count]);
    }
    Ok(bytes == asset.size && format!("{:x}", digest.finalize()).eq_ignore_ascii_case(&asset.sha256))
}

pub async fn download(release: &Release) -> Result<PathBuf, UpdateError> {
    let directory = paths::data_dir().join("updates");
    download_to(release, &release.download_url(), &directory).await
}

async fn download_to(release: &Release, url: &str, directory: &Path) -> Result<PathBuf, UpdateError> {
    fs::create_dir_all(directory).map_err(|_| UpdateError::Disk)?;
    let final_path = directory.join(&release.asset.filename);
    if final_path.exists() {
        if verify_download(&final_path, &release.asset)? {
            return Ok(final_path);
        }
        fs::remove_file(&final_path).map_err(|_| UpdateError::Disk)?;
    }
    let response = client()
        .map_err(|_| UpdateError::DownloadNetwork)?
        .get(url)
        .header("User-Agent", concat!("Perch/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|_| UpdateError::DownloadNetwork)?
        .error_for_status()
        .map_err(|_| UpdateError::DownloadNetwork)?;
    if response.content_length().is_some_and(|len| len != release.asset.size) {
        return Err(UpdateError::Size);
    }
    let staging = directory.join(format!(".{}.{}.part", release.asset.filename, uuid::Uuid::new_v4()));
    let result = async {
        let mut file = File::create(&staging).map_err(|_| UpdateError::Disk)?;
        let mut digest = Sha256::new();
        let mut total = 0u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| UpdateError::DownloadNetwork)?;
            total = total.checked_add(chunk.len() as u64).ok_or(UpdateError::Size)?;
            if total > release.asset.size || total > MAX_PACKAGE_BYTES {
                return Err(UpdateError::Size);
            }
            file.write_all(&chunk).map_err(|_| UpdateError::Disk)?;
            digest.update(&chunk);
        }
        file.sync_all().map_err(|_| UpdateError::Disk)?;
        if total != release.asset.size {
            return Err(UpdateError::Size);
        }
        if !format!("{:x}", digest.finalize()).eq_ignore_ascii_case(&release.asset.sha256) {
            return Err(UpdateError::Checksum);
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    if fs::rename(&staging, &final_path).is_err() {
        let _ = fs::remove_file(&staging);
        return Err(UpdateError::Disk);
    }
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;

    async fn local_response(body: Vec<u8>, declared_size: usize) -> String {
        local_response_status(body, declared_size, "200 OK").await
    }

    async fn local_response_status(body: Vec<u8>, declared_size: usize, status: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            let header = format!("HTTP/1.1 {status}\r\nContent-Length: {declared_size}\r\nConnection: close\r\n\r\n");
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
        });
        format!("http://{address}/asset")
    }

    fn manifest(version: &str, filename: &str, size: u64, sha256: &str) -> Vec<u8> {
        json!({"version": version, "assets": {"windows-x86_64": {
            "filename": filename, "size": size, "sha256": sha256,
        }}})
        .to_string()
        .into_bytes()
    }

    #[test]
    fn only_supported_desktop_targets_have_packages() {
        for platform in ["windows", "linux", "macos"] {
            for arch in ["x86_64", "aarch64"] {
                assert!(platform_key(platform, arch).is_some());
            }
        }
        assert_eq!(platform_key("android", "aarch64"), None);
        assert_eq!(platform_key("windows", "x86"), None);
    }

    #[test]
    fn stable_versions_compare_by_number_not_text() {
        let filename = "Perch-0.0.10-windows-x86_64-setup.exe";
        let body = manifest("0.0.10", filename, 7, &"a".repeat(64));
        assert!(parse_manifest(&body, "0.0.9", "windows-x86_64").unwrap().is_some());
        assert!(parse_manifest(&body, "0.0.10", "windows-x86_64").unwrap().is_none());
        assert!(parse_manifest(&body, "0.1.0", "windows-x86_64").unwrap().is_none());
        assert!(version("v0.0.1").is_none());
        assert!(version("0.0.1-beta").is_none());
    }

    #[test]
    fn rejects_foreign_filename_and_invalid_digest() {
        let body = manifest("0.0.2", "../setup.exe", 10, &"a".repeat(64));
        assert_eq!(
            parse_manifest(&body, "0.0.1", "windows-x86_64").unwrap_err(),
            UpdateError::Manifest
        );
        let body = manifest("0.0.2", "Perch-0.0.2-windows-x86_64-setup.exe", 10, "bad");
        assert_eq!(
            parse_manifest(&body, "0.0.1", "windows-x86_64").unwrap_err(),
            UpdateError::Manifest
        );
    }

    #[test]
    fn rejects_oversize_and_missing_target() {
        let body = manifest(
            "0.0.2",
            "Perch-0.0.2-windows-x86_64-setup.exe",
            MAX_PACKAGE_BYTES + 1,
            &"a".repeat(64),
        );
        assert_eq!(
            parse_manifest(&body, "0.0.1", "windows-x86_64").unwrap_err(),
            UpdateError::Manifest
        );
        assert_eq!(
            parse_manifest(&body, "0.0.1", "linux-aarch64").unwrap_err(),
            UpdateError::Platform
        );
    }

    #[test]
    fn existing_package_must_match_declared_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("package");
        fs::write(&path, b"fixture").unwrap();
        let asset = Asset {
            filename: "package".into(),
            size: 7,
            sha256: format!("{:x}", Sha256::digest(b"fixture")),
        };
        assert!(verify_download(&path, &asset).unwrap());
        fs::write(&path, b"tampered").unwrap();
        assert!(!verify_download(&path, &asset).unwrap());
    }
    #[tokio::test]
    async fn checks_release_manifest_from_local_server() {
        let filename = "Perch-0.0.2-windows-x86_64-setup.exe";
        let body = manifest("0.0.2", filename, 7, &"b".repeat(64));
        let url = local_response(body.clone(), body.len()).await;
        let release = check_url(&url, "0.0.1", "windows-x86_64").await.unwrap().unwrap();
        assert_eq!(release.version, "0.0.2");
        assert_eq!(
            release.download_url(),
            format!("https://github.com/zen-box/Perch/releases/download/v0.0.2/{filename}")
        );
    }

    #[tokio::test]
    async fn only_verified_download_is_kept() {
        let data = b"fixture".to_vec();
        let filename = "Perch-0.0.2-windows-x86_64-setup.exe";
        let body = manifest(
            "0.0.2",
            filename,
            data.len() as u64,
            &format!("{:x}", Sha256::digest(&data)),
        );
        let release = parse_manifest(&body, "0.0.1", "windows-x86_64").unwrap().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let url = local_response(data.clone(), data.len()).await;
        let saved = download_to(&release, &url, dir.path()).await.unwrap();
        assert_eq!(fs::read(&saved).unwrap(), data);

        fs::remove_file(saved).unwrap();
        let url = local_response(b"altered".to_vec(), data.len()).await;
        assert_eq!(
            download_to(&release, &url, dir.path()).await.unwrap_err(),
            UpdateError::Checksum
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);

        let url = local_response(data.clone(), data.len() + 1).await;
        assert_eq!(
            download_to(&release, &url, dir.path()).await.unwrap_err(),
            UpdateError::Size
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[tokio::test]
    async fn missing_release_and_failed_download_are_distinct() {
        let url = local_response_status(Vec::new(), 0, "404 Not Found").await;
        assert_eq!(
            check_url(&url, "0.0.1", "windows-x86_64").await.unwrap_err(),
            UpdateError::NoRelease
        );
        let body = manifest("0.0.2", "Perch-0.0.2-windows-x86_64-setup.exe", 7, &"a".repeat(64));
        let release = parse_manifest(&body, "0.0.1", "windows-x86_64").unwrap().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let url = local_response_status(Vec::new(), 0, "404 Not Found").await;
        assert_eq!(
            download_to(&release, &url, dir.path()).await.unwrap_err(),
            UpdateError::DownloadNetwork
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
