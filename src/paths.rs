use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::i18n::{AppLanguage, Key, tr_args};

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

/// 一次迁移失败。
///
/// **只带结构化数据，不带现成文案**：迁移发生在 `data_dir()` 里，那会儿 `AppState`
/// 还没建、界面语言也还没读出来，在这里拼字符串就没法跟着语言走了。文案留到
/// `app.rs` 弹提示时按当时的语言渲染（见 [`MigrationFailure::message`]）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrationFailure {
    /// 整个数据目录没复制过去，只能继续用旧目录
    CopyDir { from: String, to: String },
    /// 单个旧文件没复制成新名字
    CopyFile { old: String, new: String, error: String },
}

impl MigrationFailure {
    /// 渲染成给用户看的一句话。
    pub fn message(&self, lang: AppLanguage) -> String {
        match self {
            Self::CopyDir { from, to } => tr_args(lang, Key::MigrationCopyDir, &[from, to]),
            Self::CopyFile { old, new, error } => tr_args(lang, Key::MigrationCopyFile, &[old, new, error]),
        }
    }
}

/// 迁移旧数据时失败的记录。
///
/// 迁移发生在 `data_dir()` 里，那里拿不到 `Context`，弹不了提示，只能先攒着；
/// 启动后由 `app.rs` 取走并提示用户。这个必须让用户看到：新文件没建出来的话，
/// 程序会当成全新安装，用户的渠道和历史会话看起来就"没了"。
static MIGRATION_FAILURES: Mutex<Vec<MigrationFailure>> = Mutex::new(Vec::new());

fn record_migration_failure(failure: MigrationFailure) {
    MIGRATION_FAILURES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .push(failure);
}

/// 取走并清空迁移失败记录。启动时调用一次。
pub fn take_migration_failures() -> Vec<MigrationFailure> {
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
                        record_migration_failure(MigrationFailure::CopyDir {
                            from: legacy.display().to_string(),
                            to: dir.display().to_string(),
                        });
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
            record_migration_failure(MigrationFailure::CopyFile {
                old: old.to_string(),
                new: new.to_string(),
                error: error.to_string(),
            });
        }
    }
}

/// 递归复制目录。标准库没有现成的，这里手写一份。
///
/// `pub(crate)` 是因为导入 Skill 也要用它（见 `skill_ops.rs`）——再写一份递归复制，
/// 两份迟早会在「软链接怎么算」「权限位要不要带」这类细节上走偏。
pub(crate) fn copy_dir_all(from: &Path, to: &Path) -> io::Result<()> {
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
            // 复制不过去就等于读不到旧数据，必须让用户知道。
            //
            // 这里**不能 panic**：`data_file` 在启动早期就会被调用（读配置、开会话库），
            // 崩在这里的话用户连错误页都看不到，只会看到程序一闪而过。走已有的
            // 迁移失败通道——启动后由 `app.rs` 弹提示，或者由错误页一并显示。
            if let Err(failure) = copy_legacy_file(&legacy, &path, name) {
                record_migration_failure(failure);
            }
            break;
        }
    }
    path
}

/// 把旧位置的文件复制到数据目录。失败时**返回原因**而不是自己记进全局，
/// 这样逻辑本身可以直接测（`data_dir()` 是 `OnceLock`，测试里改不动）。
fn copy_legacy_file(legacy: &Path, target: &Path, name: &str) -> Result<(), MigrationFailure> {
    fs::copy(legacy, target)
        .map(|_| ())
        .map_err(|error| MigrationFailure::CopyFile {
            old: legacy.display().to_string(),
            new: name.to_string(),
            error: error.to_string(),
        })
}

/// 在系统文件管理器里打开一个路径。
///
/// 只有 Windows 有实现；别的平台返回 `Unsupported`，调用方按"打不开"处理即可。
pub fn reveal(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(path).spawn().map(|_| ())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "reveal is not implemented on this platform",
        ))
    }
}

/// 附件存储目录（如多模态图片、文件等）
pub fn attachments_dir() -> PathBuf {
    let dir = data_dir().join("attachments");
    // 建不出来也无所谓：真往里写文件时 file_store 还会再建一次并报错，不用在这儿打断启动
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Skills 目录：一个子目录一个 skill，入口是里面的 `SKILL.md`。
///
/// 和附件一样，建不出来也不打断启动——真读的时候 `skills::reload` 会当成"一个都没装"。
pub fn skills_dir() -> PathBuf {
    let dir = data_dir().join("skills");
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
    fn a_failed_legacy_copy_is_reported_instead_of_panicking() {
        let dir = tempdir().unwrap();
        let legacy = dir.path().join("perch-config.json");
        fs::write(&legacy, "{}").unwrap();
        // 目标目录不存在，复制必然失败
        let target = dir.path().join("missing/perch-config.json");

        let failure = copy_legacy_file(&legacy, &target, "perch-config.json").unwrap_err();

        match failure {
            MigrationFailure::CopyFile { old, new, error } => {
                assert!(old.ends_with("perch-config.json"), "要指出是哪个文件：{old}");
                assert_eq!(new, "perch-config.json");
                assert!(!error.is_empty(), "要带上系统给的原因");
            }
            other => panic!("应当是复制失败，实际是 {other:?}"),
        }
    }

    #[test]
    fn migration_failures_are_collected_and_taken_once() {
        record_migration_failure(MigrationFailure::CopyFile {
            old: "old.json".into(),
            new: "new.json".into(),
            error: "boom".into(),
        });

        let taken = take_migration_failures();
        assert_eq!(taken.len(), 1);
        // 记录的是结构化原因，文案按渲染时的语言生成
        assert_eq!(
            taken[0].message(AppLanguage::ZhCn),
            "old.json 复制为 new.json 失败: boom"
        );
        assert!(taken[0].message(AppLanguage::EnUs).starts_with("Failed to copy"));
        assert!(take_migration_failures().is_empty(), "取过一次就该清空");
    }
}
