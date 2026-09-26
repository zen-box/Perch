//! 判断剪贴板里是什么、该怎么粘贴，以及把剪贴板图片转成可以保存的格式。
//!
//! 只处理数据：界面层用 `cx.read_from_clipboard()` 读到内容后交给这里判断。
//! gpui 在 Windows 上已经把复制的文件读成 `ExternalPaths`、把截图（CF_DIB）读成 BMP，
//! 不需要再直接调用 Win32 剪贴板接口。

use std::io::Cursor;
use std::path::PathBuf;

use gpui_kit::{ClipboardEntry, Image, ImageFormat};

use crate::i18n::{AppLanguage, Key, tr, tr_args};

/// 一次粘贴应该怎么处理
#[derive(Debug)]
pub enum PastePayload {
    /// 从资源管理器等处复制的文件
    Files(Vec<PathBuf>),
    /// 截图、网页上复制的图片
    Image(Image),
    /// 只复制了文件夹
    FoldersOnly,
    /// 普通文字，交给输入框按默认方式插入
    Text(String),
    /// 剪贴板是空的，或者是不支持的格式
    Empty,
}

/// 判断剪贴板内容。
///
/// 复制的文件总是优先：复制一个图片文件时要带上原文件，而不是退化成位图或者路径文字
/// （gpui 的 `ClipboardItem::text()` 在没有文字时会返回文件路径）。
///
/// 文字和图片同时存在时，默认按文字处理：从 Excel、Word 复制内容时，剪贴板里除了文字还会带一张位图。
/// `prefer_image` 为 true（用户明确点了「粘贴图片」）时反过来。
pub fn classify(entries: &[ClipboardEntry], prefer_image: bool) -> PastePayload {
    let copied: Vec<PathBuf> = entries
        .iter()
        .filter_map(|entry| match entry {
            ClipboardEntry::ExternalPaths(paths) => Some(paths.paths().iter().cloned()),
            _ => None,
        })
        .flatten()
        .collect();
    if !copied.is_empty() {
        let files: Vec<PathBuf> = copied.into_iter().filter(|path| path.is_file()).collect();
        return if files.is_empty() {
            PastePayload::FoldersOnly
        } else {
            PastePayload::Files(files)
        };
    }

    let text: String = entries
        .iter()
        .filter_map(|entry| match entry {
            ClipboardEntry::String(text) => Some(text.text().as_str()),
            _ => None,
        })
        .collect();
    let image = entries.iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image.clone()),
        _ => None,
    });
    let has_text = !text.trim().is_empty();
    match image {
        Some(image) if prefer_image || !has_text => PastePayload::Image(image),
        _ if has_text => PastePayload::Text(text),
        _ => PastePayload::Empty,
    }
}

/// 可以直接保存成附件的图片数据
#[derive(Debug)]
pub struct PreparedImage {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    pub mime: &'static str,
}

/// 把剪贴板图片转成常见格式：PNG、JPEG、WebP、GIF 原样保留，
/// 其余（Windows 截图读出来是 BMP）解码后重新编码成 PNG。大图解码较慢，在后台线程调用。
pub fn prepare_image(image: &Image, lang: AppLanguage) -> Result<PreparedImage, String> {
    let keep = |extension, mime| {
        Ok(PreparedImage {
            bytes: image.bytes.clone(),
            extension,
            mime,
        })
    };
    let source_format = match image.format {
        ImageFormat::Png => return keep("png", "image/png"),
        ImageFormat::Jpeg => return keep("jpg", "image/jpeg"),
        ImageFormat::Webp => return keep("webp", "image/webp"),
        ImageFormat::Gif => return keep("gif", "image/gif"),
        ImageFormat::Bmp => image::ImageFormat::Bmp,
        ImageFormat::Tiff => image::ImageFormat::Tiff,
        ImageFormat::Ico => image::ImageFormat::Ico,
        ImageFormat::Pnm => image::ImageFormat::Pnm,
        ImageFormat::Svg => return Err(tr(lang, Key::ClipboardSvgUnsupported).into()),
    };
    let decoded = image::load_from_memory_with_format(&image.bytes, source_format)
        .map_err(|error| tr_args(lang, Key::ClipboardImageUnrecognized, &[&error.to_string()]))?;
    let mut png = Cursor::new(Vec::new());
    decoded
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| tr_args(lang, Key::ClipboardImageConvertFailed, &[&error.to_string()]))?;
    Ok(PreparedImage {
        bytes: png.into_inner(),
        extension: "png",
        mime: "image/png",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::ExternalPaths;

    fn image(format: ImageFormat, bytes: Vec<u8>) -> Image {
        Image { format, bytes, id: 1 }
    }

    fn encode(format: image::ImageFormat) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbImage::from_pixel(4, 3, image::Rgb([40, 160, 90]))
            .write_to(&mut bytes, format)
            .unwrap();
        bytes.into_inner()
    }

    fn paths(paths: Vec<PathBuf>) -> ClipboardEntry {
        ClipboardEntry::ExternalPaths(ExternalPaths(paths.into()))
    }

    #[test]
    fn copied_files_win_over_their_path_text() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("photo.png");
        std::fs::write(&file, b"x").unwrap();
        let entries = vec![paths(vec![file.clone()])];
        assert!(matches!(classify(&entries, false), PastePayload::Files(files) if files == vec![file]));

        let folder_only = vec![paths(vec![dir.path().to_path_buf()])];
        assert!(matches!(classify(&folder_only, false), PastePayload::FoldersOnly));
    }

    #[test]
    fn text_wins_over_the_bitmap_office_apps_add_unless_asked_for_the_image() {
        let entries = vec![
            ClipboardEntry::from("A1\tB1".to_string()),
            ClipboardEntry::from(image(ImageFormat::Bmp, encode(image::ImageFormat::Bmp))),
        ];
        assert!(matches!(classify(&entries, false), PastePayload::Text(text) if text == "A1\tB1"));
        assert!(matches!(classify(&entries, true), PastePayload::Image(_)));

        let screenshot = vec![ClipboardEntry::from(image(
            ImageFormat::Bmp,
            encode(image::ImageFormat::Bmp),
        ))];
        assert!(matches!(classify(&screenshot, false), PastePayload::Image(_)));
        assert!(matches!(classify(&[], false), PastePayload::Empty));
        assert!(matches!(
            classify(&[ClipboardEntry::from("  ".to_string())], false),
            PastePayload::Empty
        ));
    }

    #[test]
    fn path_text_is_just_text() {
        // 复制的是路径文字而不是文件本身时，按文字粘贴，不能自动把那个文件当附件发出去
        let entries = vec![ClipboardEntry::from(r"C:\Users\me\.ssh\id_rsa".to_string())];
        assert!(matches!(classify(&entries, false), PastePayload::Text(_)));
    }

    #[test]
    fn bitmaps_become_png_and_common_formats_are_kept() {
        let png = encode(image::ImageFormat::Png);
        let kept = prepare_image(&image(ImageFormat::Png, png.clone()), AppLanguage::ZhCn).unwrap();
        assert_eq!((kept.bytes, kept.extension, kept.mime), (png, "png", "image/png"));

        let converted = prepare_image(
            &image(ImageFormat::Bmp, encode(image::ImageFormat::Bmp)),
            AppLanguage::ZhCn,
        )
        .unwrap();
        assert_eq!(converted.extension, "png");
        let decoded = image::load_from_memory_with_format(&converted.bytes, image::ImageFormat::Png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (4, 3));

        assert!(prepare_image(&image(ImageFormat::Bmp, b"not an image".to_vec()), AppLanguage::ZhCn).is_err());
        assert!(prepare_image(&image(ImageFormat::Svg, b"<svg/>".to_vec()), AppLanguage::ZhCn).is_err());
    }
}
