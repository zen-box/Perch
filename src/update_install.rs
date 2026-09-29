//! Hand off a verified package to the native installer after explicit confirmation.

use std::path::Path;
use std::process::Command;

use crate::update::{Release, UpdateError, current_platform, verify_download};

pub enum InstallResult {
    QuitApp,
    Manual,
}

pub fn start_install(release: &Release, downloaded: &Path) -> Result<InstallResult, UpdateError> {
    if current_platform() != Some(release.platform.as_str())
        || downloaded.file_name().and_then(|name| name.to_str()) != Some(release.asset.filename.as_str())
    {
        return Err(UpdateError::Platform);
    }
    if !verify_download(downloaded, &release.asset)? {
        return Err(UpdateError::Checksum);
    }
    #[cfg(target_os = "windows")]
    {
        if Command::new(downloaded)
            .args(["/SILENT", "/NORESTART", "/CLOSEAPPLICATIONS"])
            .spawn()
            .is_ok()
        {
            return Ok(InstallResult::QuitApp);
        }
        Command::new("explorer")
            .arg("/select,")
            .arg(downloaded)
            .spawn()
            .map_err(|_| UpdateError::Install)?;
        Ok(InstallResult::Manual)
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let Some(target) = std::env::var_os("APPIMAGE")
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
        else {
            Command::new("xdg-open")
                .arg(downloaded.parent().ok_or(UpdateError::Install)?)
                .spawn()
                .map_err(|_| UpdateError::Install)?;
            return Ok(InstallResult::Manual);
        };
        let target = std::fs::canonicalize(target).map_err(|_| UpdateError::Install)?;
        std::fs::set_permissions(downloaded, std::fs::Permissions::from_mode(0o755)).map_err(|_| UpdateError::Disk)?;
        Command::new(downloaded)
            .arg("--perch-update-helper")
            .arg(std::process::id().to_string())
            .arg(target)
            .arg(release.asset.size.to_string())
            .arg(&release.asset.sha256)
            .spawn()
            .map_err(|_| UpdateError::Install)?;
        return Ok(InstallResult::QuitApp);
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("-R")
            .arg(downloaded)
            .spawn()
            .map_err(|_| UpdateError::Install)?;
        return Ok(InstallResult::Manual);
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    Err(UpdateError::Platform)
}

#[cfg(target_os = "linux")]
pub fn maybe_run_linux_helper() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).and_then(|value| value.to_str()) != Some("--perch-update-helper") {
        return None;
    }
    let result = apply_linux_update(&args);
    if result.is_err() {
        let marker = crate::paths::data_dir().join("updates").join("install-failed.txt");
        let _ = crate::paths::write_atomic(&marker, "AppImage update did not complete");
        // 旧文件能启动时让用户回到原版本；失败原因由下一次启动的界面提示。
        if let Some(target) = args.get(3) {
            let _ = Command::new(target).spawn();
        }
    }
    Some(if result.is_ok() { 0 } else { 1 })
}

#[cfg(target_os = "linux")]
fn apply_linux_update(args: &[std::ffi::OsString]) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{Duration, Instant};

    if args.len() != 6 {
        return Err(UpdateError::Install);
    }
    let pid: u32 = args[2].to_string_lossy().parse().map_err(|_| UpdateError::Install)?;
    let target = std::path::PathBuf::from(&args[3]);
    let target = std::fs::canonicalize(target).map_err(|_| UpdateError::Install)?;
    let size: u64 = args[4].to_string_lossy().parse().map_err(|_| UpdateError::Install)?;
    let asset = crate::update::Asset {
        filename: String::new(),
        size,
        sha256: args[5].to_string_lossy().to_string(),
    };
    let staged = std::env::var_os("APPIMAGE")
        .map(std::path::PathBuf::from)
        .ok_or(UpdateError::Install)?;
    let staged = std::fs::canonicalize(staged).map_err(|_| UpdateError::Install)?;
    let updates = std::fs::canonicalize(crate::paths::data_dir().join("updates")).map_err(|_| UpdateError::Install)?;
    if !staged.starts_with(updates) || !verify_download(&staged, &asset)? || staged == target {
        return Err(UpdateError::Install);
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while std::path::Path::new("/proc").join(pid.to_string()).exists() {
        if Instant::now() >= deadline {
            return Err(UpdateError::Install);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let parent = target.parent().ok_or(UpdateError::Install)?;
    let id = uuid::Uuid::new_v4();
    let incoming = parent.join(format!(".perch-update-{id}.tmp"));
    let previous = parent.join(format!(".perch-previous-{id}.AppImage"));
    if std::fs::copy(&staged, &incoming).is_err() {
        let _ = std::fs::remove_file(&incoming);
        return Err(UpdateError::Install);
    }
    if let Err(error) = std::fs::set_permissions(&incoming, std::fs::Permissions::from_mode(0o755)) {
        let _ = std::fs::remove_file(&incoming);
        return Err(if error.kind() == std::io::ErrorKind::PermissionDenied {
            UpdateError::Install
        } else {
            UpdateError::Disk
        });
    }
    if std::fs::rename(&target, &previous).is_err() {
        let _ = std::fs::remove_file(&incoming);
        return Err(UpdateError::Install);
    }
    if std::fs::rename(&incoming, &target).is_err() {
        let _ = std::fs::rename(&previous, &target);
        let _ = std::fs::remove_file(&incoming);
        return Err(UpdateError::Install);
    }
    if Command::new(&target).spawn().is_err() {
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::rename(&previous, &target);
        return Err(UpdateError::Install);
    }
    let _ = std::fs::remove_file(previous);
    Ok(())
}

pub fn take_helper_failure() -> bool {
    let marker = crate::paths::data_dir().join("updates").join("install-failed.txt");
    if !marker.exists() {
        return false;
    }
    std::fs::remove_file(marker).is_ok()
}
