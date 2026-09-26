use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use base64::Engine;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::model::AttachmentKind;
use crate::paths;

#[derive(Debug, Clone)]
pub struct SavedFile {
    pub hash: String,
    pub storage_path: String, // 相对路径，例如 "images/a1b2c3d4e5f60718_photo.png"
    pub absolute_path: PathBuf,
    pub size_bytes: u64,
    pub created: bool,
}

/// 计算二进制数据的 SHA-256 哈希值十六进制字符串
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// 清理文件名中的特殊字符与路径遍历符号
pub fn sanitize_file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if clean.is_empty() || clean.starts_with('.') {
        format!("file_{}", clean.trim_start_matches('.'))
    } else {
        clean
    }
}

/// 根据 MIME 类型或文件名确定分桶子目录 ("images" 或 "files")
pub fn file_bucket(mime_type: &str, file_name: &str) -> &'static str {
    if mime_type.starts_with("image/") {
        return "images";
    }
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" => "images",
        _ => "files",
    }
}

/// 根据文件名后缀识别 AttachmentKind 和标准 MIME 类型
pub fn detect_kind_and_mime(path_or_name: &str) -> (AttachmentKind, &'static str) {
    let ext = Path::new(path_or_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        // 图片类
        "png" => (AttachmentKind::Image, "image/png"),
        "jpg" | "jpeg" => (AttachmentKind::Image, "image/jpeg"),
        "webp" => (AttachmentKind::Image, "image/webp"),
        "gif" => (AttachmentKind::Image, "image/gif"),
        "bmp" => (AttachmentKind::Image, "image/bmp"),
        "svg" => (AttachmentKind::Image, "image/svg+xml"),

        // PDF 与办公文档
        "pdf" => (AttachmentKind::Document, "application/pdf"),
        "doc" | "docx" => (AttachmentKind::Document, "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xls" | "xlsx" => (AttachmentKind::Document, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "ppt" | "pptx" => (AttachmentKind::Document, "application/vnd.openxmlformats-officedocument.presentationml.presentation"),

        // 文本与常见代码类
        "txt" => (AttachmentKind::Text, "text/plain"),
        "md" | "markdown" => (AttachmentKind::Text, "text/markdown"),
        "json" => (AttachmentKind::Text, "application/json"),
        "csv" => (AttachmentKind::Text, "text/csv"),
        "xml" => (AttachmentKind::Text, "text/xml"),
        "yaml" | "yml" => (AttachmentKind::Text, "text/yaml"),
        "toml" => (AttachmentKind::Text, "text/toml"),
        "rs" => (AttachmentKind::Text, "text/x-rust"),
        "py" => (AttachmentKind::Text, "text/x-python"),
        "js" | "mjs" | "cjs" => (AttachmentKind::Text, "text/javascript"),
        "ts" | "mts" | "cts" => (AttachmentKind::Text, "text/typescript"),
        "tsx" | "jsx" => (AttachmentKind::Text, "text/typescript"),
        "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" => (AttachmentKind::Text, "text/x-c"),
        "go" => (AttachmentKind::Text, "text/x-go"),
        "java" => (AttachmentKind::Text, "text/x-java"),
        "sh" | "bash" | "zsh" => (AttachmentKind::Text, "text/x-shellscript"),
        "sql" => (AttachmentKind::Text, "text/x-sql"),
        "html" | "htm" => (AttachmentKind::Text, "text/html"),
        "css" => (AttachmentKind::Text, "text/css"),

        _ => (AttachmentKind::Other, "application/octet-stream"),
    }
}

/// 单个附件的大小上限。各家模型接口对单个文件的限制大多在 20–50 MB，
/// 更大的文件发不出去，整个读进内存还可能拖垮程序。
pub const MAX_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

/// 保存二进制文件数据到 attachments 分桶目录（基于 SHA-256 去重与原子写入）
pub fn save_bytes(data: &[u8], original_name: &str, mime_type: &str) -> std::io::Result<SavedFile> {
    save_bytes_in(&paths::attachments_dir(), data, original_name, mime_type)
}

/// `base_dir` 是附件根目录；单独拆出来是为了让测试写到临时目录，不碰用户数据
fn save_bytes_in(base_dir: &Path, data: &[u8], original_name: &str, mime_type: &str) -> std::io::Result<SavedFile> {
    let hash = hash_bytes(data);
    let hash_prefix = &hash[..16];
    let sanitized = sanitize_file_name(original_name);
    let bucket = file_bucket(mime_type, &sanitized);

    let relative_path = format!("{}/{}_{}", bucket, hash_prefix, sanitized);
    let abs_path = base_dir.join(&relative_path);

    if let Some(parent) = abs_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let created = if abs_path.exists() {
        // 文件已存在且内容一致，直接复用，完全去重
        false
    } else {
        // 原子写入：先写入临时文件，再原子 rename
        let staging_name = format!(".staging_{}_{}", Uuid::new_v4(), sanitized);
        let staging_path = abs_path.with_file_name(staging_name);

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&staging_path)?;

        file.write_all(data)?;
        file.sync_all()?;
        drop(file);

        if let Err(e) = fs::rename(&staging_path, &abs_path) {
            let _ = fs::remove_file(&staging_path);
            return Err(e);
        }
        true
    };

    Ok(SavedFile {
        hash,
        storage_path: relative_path,
        absolute_path: abs_path,
        size_bytes: data.len() as u64,
        created,
    })
}

/// 从外部物理文件路径读取并导入到 attachments 存储库（去重）
pub fn save_file_from_path(src: &Path) -> std::io::Result<SavedFile> {
    check_attachment_size(fs::metadata(src)?.len())?;
    let data = fs::read(src)?;
    let name = src
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment");
    let (_, mime) = detect_kind_and_mime(name);
    save_bytes(&data, name, mime)
}

fn check_attachment_size(size: u64) -> std::io::Result<()> {
    if size > MAX_ATTACHMENT_BYTES {
        return Err(std::io::Error::other(format!(
            "文件有 {:.1} MB，超过了 {} MB 的上限",
            size as f64 / 1024.0 / 1024.0,
            MAX_ATTACHMENT_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}

/// 根据 relative storage_path（如 "images/xxx.png"）或兼容旧绝对路径解析出真实完整物理路径
pub fn resolve_path(storage_path: &str) -> PathBuf {
    let p = Path::new(storage_path);
    if p.is_absolute() && p.exists() {
        return p.to_path_buf();
    }
    let base = paths::attachments_dir();
    let resolved = base.join(storage_path);
    if resolved.exists() {
        return resolved;
    }
    // 兼容之前直接存放在 attachments 根目录下的历史文件
    if let Some(file_name) = p.file_name() {
        let legacy = base.join(file_name);
        if legacy.exists() {
            return legacy;
        }
    }
    resolved
}

/// 读取附件的 base64 字符串（用于 API 多模态请求）
pub fn read_base64(storage_path: &str) -> Option<String> {
    let abs = resolve_path(storage_path);
    let bytes = fs::read(abs).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

/// 读取附件的原始二进制字节
pub fn read_bytes(storage_path: &str) -> Option<Vec<u8>> {
    let abs = resolve_path(storage_path);
    fs::read(abs).ok()
}

/// 读取文本类附件的 UTF-8 文本内容
pub fn read_text(storage_path: &str) -> Option<String> {
    let abs = resolve_path(storage_path);
    let bytes = fs::read(abs).ok()?;
    String::from_utf8(bytes).ok()
}

/// 安全删除物理文件
pub fn delete_file(storage_path: &str) -> std::io::Result<()> {
    let abs = resolve_path(storage_path);
    if abs.exists() {
        fs::remove_file(abs)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_store_save_and_deduplicate() {
        let dir = tempfile::tempdir().unwrap();
        let data = b"hello personal control attachment";
        let saved1 = save_bytes_in(dir.path(), data, "test.txt", "text/plain").expect("save1");
        assert!(saved1.storage_path.starts_with("files/"));
        assert!(saved1.absolute_path.exists());

        // 第二次保存相同数据应返回相同哈希和路径，且 created 为 false
        let saved2 = save_bytes_in(dir.path(), data, "test.txt", "text/plain").expect("save2");
        assert_eq!(saved1.hash, saved2.hash);
        assert_eq!(saved1.storage_path, saved2.storage_path);
        assert!(!saved2.created);

        let read_back = fs::read_to_string(&saved1.absolute_path).expect("read");
        assert_eq!(read_back, "hello personal control attachment");
    }

    #[test]
    fn oversized_files_are_rejected_before_reading() {
        assert!(check_attachment_size(MAX_ATTACHMENT_BYTES).is_ok());
        let error = check_attachment_size(MAX_ATTACHMENT_BYTES + 1).unwrap_err();
        assert!(error.to_string().contains("50 MB"));
    }

    #[test]
    fn test_detect_kind_and_mime() {
        let (k_png, m_png) = detect_kind_and_mime("photo.png");
        assert_eq!(k_png, AttachmentKind::Image);
        assert_eq!(m_png, "image/png");

        let (k_pdf, m_pdf) = detect_kind_and_mime("report.pdf");
        assert_eq!(k_pdf, AttachmentKind::Document);
        assert_eq!(m_pdf, "application/pdf");

        let (k_docx, _) = detect_kind_and_mime("doc.docx");
        assert_eq!(k_docx, AttachmentKind::Document);

        let (k_xlsx, _) = detect_kind_and_mime("sheet.xlsx");
        assert_eq!(k_xlsx, AttachmentKind::Document);

        let (k_rs, m_rs) = detect_kind_and_mime("main.rs");
        assert_eq!(k_rs, AttachmentKind::Text);
        assert_eq!(m_rs, "text/x-rust");

        let (k_py, _) = detect_kind_and_mime("script.py");
        assert_eq!(k_py, AttachmentKind::Text);
    }
}
