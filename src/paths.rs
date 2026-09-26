use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 当前应用名。数据目录名、凭据管理器里的服务名都用它。
pub const APP_NAME: &str = "Perch";

/// 改名前的应用名。首次启动时据此把旧数据目录和旧凭据搬过来。
pub const LEGACY_APP_NAME: &str = "PersonalControl";

// 数据目录里的文件名。集中放在这里，改名时只要动这一处。

pub const CONFIG_FILE: &str = "perch-config.json";
pub const SESSIONS_FILE: &str = "perch-sessions.json";
pub const DATABASE_FILE: &str = "perch.db";
pub const PROMPTS_FILE: &str = "prompts.json";
pub const MODELS_DEV_CACHE_FILE: &str = "models-dev-cache.json";

/// 改名前的数据文件名 → 现在用的名字。
///
/// 迁移一律用**复制**而不是改名：旧文件原样留着，万一新版有问题还能退回去。
pub const LEGACY_FILES: &[(&str, &str)] = &[
    ("personal-control-config.json", CONFIG_FILE),
    ("personal-control-sessions.json", SESSIONS_FILE),
    ("personal-control.db", DATABASE_FILE),
];

/// 迁移旧数据时失败的记录。
///
/// 迁移发生在 `data_dir()` 里，那里拿不到 `Context`，弹不了提示，只能先攒着；
/// 启动后由 `app.rs` 取走并提示用户。这个必须让用户看到：新文件没建出来的话，
/// 程序会当成全新安装，用户的渠道和历史会话看起来就"没了"。
static MIGRATION_FAILURES: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record_migration_failure(message: String) {
    MIGRATION_FAILURES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .push(message);
}

/// 取走并清空迁移失败记录。启动时调用一次。
pub fn take_migration_failures() -> Vec<String> {
    std::mem::take(&mut *MIGRATION_FAILURES.lock().unwrap_or_else(|poison| poison.into_inner()))
}

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
                    if legacy.exists() && !migrate_dir(&legacy, &dir) {
                        // 复制失败就继续用旧目录：宁可留在旧路径，也不能让用户看到空数据
                        record_migration_failure(format!(
                            "{} 复制到 {} 失败，继续使用旧目录",
                            legacy.display(),
                            dir.display()
                        ));
                        return legacy;
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
        if old_path.exists()
            && !new_path.exists()
            && let Err(error) = fs::copy(&old_path, &new_path)
        {
            // 复制不过去就等于读不到旧数据，必须让用户知道
            record_migration_failure(format!("{old} 复制为 {new} 失败: {error}"));
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

/// 把整个旧数据目录搬到新位置，成功返回 `true`。
///
/// 先复制到同级的临时目录、成功后再改名过去。这样正式路径上永远不会出现复制了一半的目录——
/// 启动时只看正式路径在不在，半成品会被当成"迁移已完成"，用户数据就再也读不到了。
fn migrate_dir(from: &Path, to: &Path) -> bool {
    let staging = to.with_file_name(format!("{APP_NAME}.migrating"));
    // 清不掉上次的残骸就放弃这次迁移，退回旧目录，不冒险往脏目录里复制
    if staging.exists() && fs::remove_dir_all(&staging).is_err() {
        return false;
    }
    // 后面这几处清理失败都无所谓：正式路径上没有半成品，最多留个 `.migrating` 目录，
    // 下次启动会再清一次；清不掉也只是继续用旧目录，数据不受影响。
    if copy_dir_all(from, &staging).is_err() {
        let _ = fs::remove_dir_all(&staging);
        return false;
    }
    if fs::rename(&staging, to).is_err() {
        let _ = fs::remove_dir_all(&staging);
        return false;
    }
    true
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
    // 建不出来也无所谓：真往里写文件时 file_store 还会再建一次并报错，不用在这儿打断启动
    let _ = fs::create_dir_all(&dir);
    dir
}

/// 先写临时文件再替换，避免写到一半时崩溃把数据弄坏
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    write_atomic_bytes(path, contents.as_bytes())
}

/// `write_atomic` 的字节版本，给图片缓存这类二进制数据用
pub fn write_atomic_bytes(path: &Path, contents: &[u8]) -> io::Result<()> {
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

    #[test]
    fn migrate_dir_moves_whole_tree_and_leaves_no_staging() {
        let base = tempdir().unwrap();
        let from = base.path().join("PersonalControl");
        let to = base.path().join("Perch");
        fs::create_dir_all(from.join("attachments")).unwrap();
        fs::write(from.join("perch-config.json"), "{}").unwrap();
        fs::write(from.join("attachments/a.png"), "png").unwrap();

        assert!(migrate_dir(&from, &to));

        assert_eq!(fs::read_to_string(to.join("perch-config.json")).unwrap(), "{}");
        assert_eq!(fs::read_to_string(to.join("attachments/a.png")).unwrap(), "png");
        assert!(from.exists(), "旧目录要留着当回退备份");
        assert!(!base.path().join("Perch.migrating").exists(), "staging 要清干净");
    }

    #[test]
    fn migrate_dir_leaves_no_half_copy_at_target_when_it_fails() {
        let base = tempdir().unwrap();
        let to = base.path().join("Perch");

        // 源目录不存在，复制必然失败
        assert!(!migrate_dir(&base.path().join("missing"), &to));

        assert!(
            !to.exists(),
            "失败时正式路径上不能留下半成品，否则下次启动会当成迁移已完成"
        );
        assert!(!base.path().join("Perch.migrating").exists(), "staging 也要清掉");
    }

    #[test]
    fn migration_failures_are_collected_and_taken_once() {
        record_migration_failure("测试用的失败信息".to_string());

        let taken = take_migration_failures();
        assert!(taken.iter().any(|message| message == "测试用的失败信息"));
        assert!(take_migration_failures().is_empty(), "取过一次就该清空");
    }
}
