use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 当前应用名。数据目录名、凭据管理器里的服务名都用它。
pub const APP_NAME: &str = "Perch";

/// 改名前的应用名。首次启动时据此把旧数据目录和旧凭据搬过来。
pub const LEGACY_APP_NAME: &str = "PersonalControl";

/// 改名前的数据文件名 → 现在用的名字。
///
/// 迁移一律用**复制**而不是改名：旧文件原样留着，万一新版有问题还能退回去。
pub const LEGACY_FILES: &[(&str, &str)] = &[
    ("personal-control-config.json", "perch-config.json"),
    ("personal-control-sessions.json", "perch-sessions.json"),
    ("personal-control.db", "perch.db"),
];

/// 应用数据目录：
/// - Windows: `%APPDATA%\Perch`
/// - macOS: `~/Library/Application Support/Perch`
/// - Linux: `$XDG_DATA_HOME/Perch` 或 `~/.local/share/Perch`
///
/// 首次启动时，如果旧目录 `PersonalControl` 还在、新目录还没有，就整体复制一份过去，
/// 旧目录保留不动，作为回退用的备份。
///
/// 找不到时退回当前工作目录。
pub fn data_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = if cfg!(target_os = "windows") {
            env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
        } else {
            env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        };

        match base {
            Some(base) => {
                let dir = base.join(APP_NAME);
                if !dir.exists() {
                    let legacy = base.join(LEGACY_APP_NAME);
                    if legacy.exists() {
                        let _ = copy_dir_all(&legacy, &dir);
                    }
                }
                match fs::create_dir_all(&dir) {
                    Ok(()) => {
                        migrate_legacy_files(&dir);
                        dir
                    }
                    Err(_) => PathBuf::from("."),
                }
            }
            None => PathBuf::from("."),
        }
    })
}

/// 把数据目录里的旧文件名复制成新文件名。旧文件已不在或新文件已存在时跳过。
fn migrate_legacy_files(dir: &Path) {
    for (old, new) in LEGACY_FILES {
        let old_path = dir.join(old);
        let new_path = dir.join(new);
        if old_path.exists() && !new_path.exists() {
            let _ = fs::copy(&old_path, &new_path);
        }
    }
}

/// 递归复制目录。标准库没有现成的，这里手写一份。
fn copy_dir_all(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// 数据文件的完整路径。
///
/// 旧版本把数据写在当前工作目录下，文件名还是 `personal-control-*`。
/// 新位置没有该文件时，依次去找当前工作目录下的新名和旧名，找到就复制过来，
/// 原文件保留不动，作为备份。（数据目录里的旧名由 [`migrate_legacy_files`] 处理。）
pub fn data_file(name: &str) -> PathBuf {
    let path = data_dir().join(name);
    if path.exists() {
        return path;
    }
    let legacy_name = LEGACY_FILES.iter().find(|(_, new)| *new == name).map(|(old, _)| *old);
    let mut candidates = vec![Path::new(name).to_path_buf()];
    if let Some(old) = legacy_name {
        candidates.push(Path::new(old).to_path_buf());
    }
    for legacy in candidates {
        if legacy.exists() {
            fs::copy(&legacy, &path).unwrap_or_else(|error| panic!("Unable to migrate {}: {error}", legacy.display()));
            break;
        }
    }
    path
}

/// 附件存储目录（如多模态图片、文件等）
pub fn attachments_dir() -> PathBuf {
    let dir = data_dir().join("attachments");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// 先写临时文件再替换，避免写到一半时崩溃把数据弄坏
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        write_atomic(&path, "first").unwrap();
        write_atomic(&path, "second").unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "second");
    }

    #[test]
    fn copy_dir_all_copies_nested_entries() {
        let from = tempdir().unwrap();
        let to = tempdir().unwrap();
        fs::create_dir_all(from.path().join("attachments")).unwrap();
        fs::write(from.path().join("config.json"), "{}").unwrap();
        fs::write(from.path().join("attachments/a.png"), "png").unwrap();

        copy_dir_all(from.path(), &to.path().join("Perch")).unwrap();

        let copied = to.path().join("Perch");
        assert_eq!(fs::read_to_string(copied.join("config.json")).unwrap(), "{}");
        assert_eq!(fs::read_to_string(copied.join("attachments/a.png")).unwrap(), "png");
    }

    #[test]
    fn migrate_legacy_files_keeps_original_and_copies_new_name() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("personal-control.db"), "db").unwrap();

        migrate_legacy_files(dir.path());

        assert_eq!(fs::read_to_string(dir.path().join("perch.db")).unwrap(), "db");
        assert!(dir.path().join("personal-control.db").exists(), "旧文件要留着");
    }

    #[test]
    fn migrate_legacy_files_does_not_overwrite_existing_new_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("personal-control.db"), "old").unwrap();
        fs::write(dir.path().join("perch.db"), "new").unwrap();

        migrate_legacy_files(dir.path());

        assert_eq!(fs::read_to_string(dir.path().join("perch.db")).unwrap(), "new");
    }
}
