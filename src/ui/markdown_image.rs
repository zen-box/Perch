use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::component::{GlobalState, Icon, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use crate::i18n::{AppLanguage, Key, current, tr, tr_args};
use crate::image_http::{image_host, normalize_image_url};

const MAX_WIDTH: f32 = 560.;
const MAX_HEIGHT: f32 = 420.;
const CARD_WIDTH: f32 = 320.;

#[derive(Clone)]
struct ChatImage {
    url: String,
    alt: String,
    width: Option<f32>,
    height: Option<f32>,
}

impl ChatImage {
    /// 图片说明。Markdown 里写了 `alt` 就用它，没写才回落到本地化的「图片」。
    fn title(&self, lang: AppLanguage) -> String {
        if self.alt.trim().is_empty() {
            tr(lang, Key::Image).to_string()
        } else {
            self.alt.trim().to_string()
        }
    }

    /// 解析期能拿到的说明：只有 Markdown 里写的 `alt`，没有本地化兜底。
    ///
    /// 解析回调 `MarkdownPlugin::parse` 只收到 `MarkdownParseContext`（源文本 + 偏移量），
    /// 拿不到 `App`，也就读不了界面语言。而解析结果会进 Markdown 节点缓存，在这里
    /// 固化某种语言，反而会在切换语言后留下陈旧文案。所以解析期只取 `alt` 原文，
    /// 本地化的兜底留给 [`Self::title`] 在渲染时补。
    fn alt_text(&self) -> String {
        self.alt.trim().to_string()
    }
}

/// 对 Markdown 内容做兼容预处理：
/// 大模型在输出 Data URI 时常包含未转义的空格（如 SVG XML 属性间空格）或换行，
/// CommonMark 解析器遇到未转义空格会将图片降级为纯文本。
/// 这里自动将 Data URI 中的空格转义为 `%20` 并剔除内部多余换行，确保能被正确识别为图片。
pub fn normalize_markdown_image_urls(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;

    while let Some(start_ix) = rest.find("![") {
        out.push_str(&rest[..start_ix]);
        let after_bang = &rest[start_ix..];

        if let Some(bracket_end) = after_bang.find("](") {
            let alt_part = &after_bang[..bracket_end + 2];
            let url_part = &after_bang[bracket_end + 2..];

            // 优先在单行内寻找闭合括号
            let line_end = url_part.find('\n').unwrap_or(url_part.len());
            let line_slice = &url_part[..line_end];
            let paren_ix = line_slice.rfind(')').or_else(|| url_part.find(')'));

            if let Some(paren_end) = paren_ix {
                let raw_url = &url_part[..paren_end];
                let trimmed = raw_url.trim();

                if trimmed.to_ascii_lowercase().starts_with("data:") {
                    out.push_str(alt_part);
                    let unbracketed = trimmed.trim_start_matches('<').trim_end_matches('>');
                    let cleaned = unbracketed.replace(['\r', '\n'], "").replace(' ', "%20");
                    out.push_str(&cleaned);
                    out.push(')');
                    rest = &url_part[paren_end + 1..];
                    continue;
                }
            }
        }

        out.push_str(&after_bang[..2]);
        rest = &after_bang[2..];
    }
    out.push_str(rest);
    out
}

/// Markdown 图片渲染插件：支持普通 URL 远程图片与 Base64 内联图片。
///
/// 默认直接显示图片，不再需要手动点击「点击加载」；支持在应用内放大查看与复制。
pub struct ChatImagePlugin;

impl MarkdownPlugin for ChatImagePlugin {
    fn name(&self) -> &str {
        "pc-image"
    }

    fn parse(&self, node: &markdown_ast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let image = match node {
            markdown_ast::Node::Image(image) => ChatImage {
                url: image.url.clone(),
                alt: image.alt.clone(),
                width: None,
                height: None,
            },
            markdown_ast::Node::Html(html) => sole_html_image(&html.value)?,
            _ => return None,
        };
        let label = image.alt_text();
        let source = cx.node_source(node).unwrap_or_default();
        Some(
            MarkdownNode::new("pc-image", image)
                .text(label.clone())
                .markdown(source.to_string())
                .accessibility_label(label),
        )
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(image) = node.data::<ChatImage>().cloned() else {
            return div().child(node.as_text().to_string()).into_any_element();
        };
        render_image(image, window, cx)
    }
}

fn decode_data_url(raw: &str) -> Option<(Arc<gpui_kit::Image>, (f32, f32))> {
    let data_url = data_url::DataUrl::process(raw.trim()).ok()?;
    let mime = data_url.mime_type();
    let format = gpui_kit::ImageFormat::from_mime_type(&format!("{}/{}", mime.type_, mime.subtype))?;
    let (bytes, _) = data_url.decode_to_vec().ok()?;

    let dims = if format == gpui_kit::ImageFormat::Svg {
        None
    } else {
        image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .ok()
            .and_then(|reader| reader.into_dimensions().ok())
            .map(|(w, h)| (w as f32, h as f32))
    };

    let natural = dims.unwrap_or((320., 180.));
    let gpui_image = Arc::new(gpui_kit::Image::from_bytes(format, bytes));
    Some((gpui_image, natural))
}

fn render_image(image: ChatImage, window: &mut Window, cx: &mut App) -> AnyElement {
    let p = Palette::new(cx);
    // 这个函数由 GPUI 的 `MarkdownPlugin::render` 调用，签名定死、拿不到 `AppState`，
    // 所以从语言全局读。
    let lang = current(cx);
    let trimmed = image.url.trim().trim_matches('"').trim_matches('\'');

    // 1. Base64 / Data URI：本地解码并直接渲染
    if trimmed.to_ascii_lowercase().starts_with("data:") {
        return match decode_data_url(trimmed) {
            Some((gpui_image, natural)) => {
                let (width, height) = fit_size(natural.0, natural.1, image.width, image.height);
                render_loaded_base64(&image, gpui_image, (width, height), natural, &p, lang)
            }
            None => image_card(
                &element_id("pc-img-bad", &image.url[..image.url.len().min(64)]),
                IconName::ImageOff,
                image.title(lang),
                tr(lang, Key::ImageBadBase64).into(),
                None,
                &p,
                lang,
            )
            .into_any_element(),
        };
    }

    // 2. 远程网络图片：直接发起加载并显示，不需手动点击
    let Some(url) = normalize_image_url(&image.url) else {
        return image_card(
            &element_id("pc-img-bad", &image.url),
            IconName::ImageOff,
            image.title(lang),
            tr(lang, Key::ImageUnsupportedUrl).into(),
            None,
            &p,
            lang,
        )
        .into_any_element();
    };

    let resource = Resource::Uri(url.clone().into());
    match window.use_asset::<ImgResourceLoader>(&resource, cx) {
        Some(Ok(rendered)) => {
            let size = rendered.size(0);
            let natural = (size.width.0 as f32, size.height.0 as f32);
            let (width, height) = fit_size(natural.0, natural.1, image.width, image.height);
            render_loaded(url, &image, (width, height), natural, &p, lang)
        }
        Some(Err(error)) => render_failed(url, &image, failure_reason(&error, lang), &p, lang),
        None => {
            let hint = image.width.zip(image.height).or_else(|| url_size_hint(&url));
            let (width, height) = match hint {
                Some((width, height)) => fit_size(width, height, image.width, image.height),
                None => (320., 180.),
            };
            div()
                .w(px(width))
                .h(px(height))
                .max_w_full()
                .rounded_md()
                .bg(p.muted)
                .border_1()
                .border_color(p.border)
                .flex()
                .items_center()
                .justify_center()
                .gap_2()
                .text_xs()
                .text_color(p.muted_foreground)
                .child(Spinner::new().small())
                .child(tr(lang, Key::ImageLoading))
                .into_any_element()
        }
    }
}

fn render_loaded_base64(
    image: &ChatImage,
    gpui_image: Arc<gpui_kit::Image>,
    size: (f32, f32),
    natural: (f32, f32),
    p: &Palette,
    lang: AppLanguage,
) -> AnyElement {
    let id = element_id("pc-img-b64", &image.url[..image.url.len().min(64)]);
    let title = image.title(lang);
    let img_for_viewer = gpui_image.clone();
    let raw_for_viewer = image.url.clone();
    div()
        .id(id.clone())
        .w(px(size.0))
        .h(px(size.1))
        .max_w_full()
        .rounded_md()
        .overflow_hidden()
        .bg(p.muted)
        .border_1()
        .border_color(p.border)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| GlobalState::suppress_text_selection(cx))
        .on_click(move |_, window, cx| {
            open_base64_viewer(
                img_for_viewer.clone(),
                raw_for_viewer.clone(),
                title.clone(),
                Some(natural),
                window,
                cx,
            );
        })
        .child(
            img(gpui_image)
                .id(SharedString::from(format!("{id}-img")))
                .size_full()
                .object_fit(ObjectFit::Contain),
        )
        .into_any_element()
}

fn render_loaded(
    url: String,
    image: &ChatImage,
    size: (f32, f32),
    natural: (f32, f32),
    p: &Palette,
    lang: AppLanguage,
) -> AnyElement {
    let id = element_id("pc-img", &url);
    let title = image.title(lang);
    let view_url = url.clone();
    div()
        .id(id.clone())
        .w(px(size.0))
        .h(px(size.1))
        .max_w_full()
        .rounded_md()
        .overflow_hidden()
        .bg(p.muted)
        .border_1()
        .border_color(p.border)
        .cursor_pointer()
        // 按下时不开始文字选择；点击事件不拦截，文字选择才能正常结束
        .on_mouse_down(MouseButton::Left, |_, _, cx| GlobalState::suppress_text_selection(cx))
        .on_click(move |_, window, cx| {
            open_image_viewer(view_url.clone(), title.clone(), Some(natural), window, cx);
        })
        .child(
            img(url)
                .id(SharedString::from(format!("{id}-img")))
                .size_full()
                .object_fit(ObjectFit::Contain),
        )
        .into_any_element()
}

fn render_failed(url: String, image: &ChatImage, error: String, p: &Palette, lang: AppLanguage) -> AnyElement {
    let id = element_id("pc-img-err", &url);
    let reason = error.lines().next().unwrap_or("").chars().take(160).collect::<String>();
    let retry_url = url.clone();
    let open_url = url;
    image_card(
        &id,
        IconName::ImageOff,
        tr_args(lang, Key::ImageLoadFailed, &[&image.title(lang)]),
        reason,
        None,
        p,
        lang,
    )
    .child(
        h_flex()
            .flex_none()
            .gap_1()
            .child(
                Button::new(SharedString::from(format!("{id}-retry")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::RotateCw)
                    .tooltip(tr(lang, Key::Retry))
                    .on_click(move |_, window, cx| {
                        cx.remove_asset::<ImgResourceLoader>(&Resource::Uri(retry_url.clone().into()));
                        window.refresh();
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("{id}-open")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::ExternalLink)
                    .tooltip(tr(lang, Key::OpenInBrowser))
                    .on_click(move |_, _, cx| cx.open_url(&open_url)),
            ),
    )
    .into_any_element()
}

/// GPUI 会在错误外面包一层「loading image asset from ...」，这里取出真正的原因
fn failure_reason(error: &ImageCacheError, lang: AppLanguage) -> String {
    match error {
        ImageCacheError::Other(error) => error.root_cause().to_string(),
        ImageCacheError::BadStatus { status, .. } => tr_args(lang, Key::ImageHttpStatus, &[&status.to_string()]),
        ImageCacheError::Image(_) | ImageCacheError::Usvg(_) => tr(lang, Key::ImageBadFormat).into(),
        other => other.to_string(),
    }
}

/// 占位与失败状态共用的小卡片：图标 + 标题 + 说明
fn image_card(
    id: &SharedString,
    icon: IconName,
    title: String,
    detail: String,
    action: Option<&'static str>,
    p: &Palette,
    lang: AppLanguage,
) -> Stateful<Div> {
    h_flex()
        .id(id.clone())
        .w(px(CARD_WIDTH))
        .max_w_full()
        .gap_2p5()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(p.border)
        .bg(p.muted.opacity(0.4))
        .child(
            div()
                .flex_none()
                .size(px(32.))
                .rounded_md()
                .bg(p.muted)
                .flex()
                .items_center()
                .justify_center()
                .text_color(p.muted_foreground)
                .child(Icon::new(icon).size(px(16.))),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().truncate().text_sm().text_color(p.foreground).child(title))
                .child(
                    div()
                        .truncate()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child(if detail.is_empty() {
                            tr(lang, Key::ImageRemote).to_string()
                        } else {
                            detail
                        }),
                ),
        )
        .children(action.map(|label| {
            div()
                .flex_none()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.primary)
                .child(label)
        }))
}

/// 在应用内放大查看图片，不跳转浏览器
pub fn open_image_viewer(url: String, title: String, natural: Option<(f32, f32)>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let p = Palette::new(cx);
        // 对话框内容闭包的签名由 GPUI 定死（只有 `&mut App`），拿不到 `AppState`，从语言全局读。
        let lang = current(cx);
        let viewport = window.viewport_size();
        let dialog_width = f32::from(viewport.width - px(80.)).clamp(360., 1200.);
        // 弹窗左右各 16px 内边距；上下留给标题栏、底部按钮和外边距
        let available_width = dialog_width - 32.;
        let available_height = (f32::from(viewport.height) - 230.).max(160.);
        let (width, height) = match natural {
            Some((w, h)) if w > 1. && h > 1. => {
                // 小图最多放大两倍，大图缩到能完整显示
                let scale = (available_width / w).min(available_height / h).min(2.);
                (w * scale, h * scale)
            }
            _ => (available_width, available_height),
        };
        let host = image_host(&url);
        let copy_url = url.clone();
        let open_url = url.clone();

        dialog
            .title(title.clone())
            .w(px(dialog_width))
            .margin_top(px(40.))
            .child(
                div().w_full().flex().justify_center().child(
                    div()
                        .w(px(width))
                        .h(px(height))
                        .child(img(url.clone()).size_full().object_fit(ObjectFit::Contain)),
                ),
            )
            .footer(
                h_flex()
                    .w_full()
                    .gap_3()
                    .justify_between()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child(host),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_2()
                            .child(
                                Button::new("image-viewer-copy")
                                    .outline()
                                    .small()
                                    .icon(IconName::Link)
                                    .label(tr(lang, Key::CopyLink))
                                    .on_click(move |_, window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                                        window.push_notification(
                                            Notification::success(tr(lang, Key::ImageLinkCopied)),
                                            cx,
                                        );
                                    }),
                            )
                            .child(
                                Button::new("image-viewer-browser")
                                    .outline()
                                    .small()
                                    .icon(IconName::ExternalLink)
                                    .label(tr(lang, Key::OpenInBrowser))
                                    .on_click(move |_, _, cx| cx.open_url(&open_url)),
                            )
                            .child(
                                Button::new("image-viewer-close")
                                    .primary()
                                    .small()
                                    .label(tr(lang, Key::Close))
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            ),
                    ),
            )
    });
}

/// 在应用内放大查看 Base64 内联图片
pub fn open_base64_viewer(
    image: Arc<gpui_kit::Image>,
    raw_url: String,
    title: String,
    natural: Option<(f32, f32)>,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let p = Palette::new(cx);
        // 对话框内容闭包的签名由 GPUI 定死（只有 `&mut App`），拿不到 `AppState`，从语言全局读。
        let lang = current(cx);
        let viewport = window.viewport_size();
        let dialog_width = f32::from(viewport.width - px(80.)).clamp(360., 1200.);
        let available_width = dialog_width - 32.;
        let available_height = (f32::from(viewport.height) - 230.).max(160.);
        let (width, height) = match natural {
            Some((w, h)) if w > 1. && h > 1. => {
                let scale = (available_width / w).min(available_height / h).min(2.);
                (w * scale, h * scale)
            }
            _ => (available_width, available_height),
        };
        let copy_data = raw_url.clone();

        dialog
            .title(title.clone())
            .w(px(dialog_width))
            .margin_top(px(40.))
            .child(
                div().w_full().flex().justify_center().child(
                    div()
                        .w(px(width))
                        .h(px(height))
                        .child(img(image.clone()).size_full().object_fit(ObjectFit::Contain)),
                ),
            )
            .footer(
                h_flex()
                    .w_full()
                    .gap_3()
                    .justify_between()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child(tr(lang, Key::ImageInlineBase64)),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_2()
                            .child(
                                Button::new("base64-viewer-copy")
                                    .outline()
                                    .small()
                                    .icon(IconName::Copy)
                                    .label(tr(lang, Key::CopyBase64))
                                    .on_click(move |_, window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(copy_data.clone()));
                                        window
                                            .push_notification(Notification::success(tr(lang, Key::Base64Copied)), cx);
                                    }),
                            )
                            .child(
                                Button::new("base64-viewer-close")
                                    .primary()
                                    .small()
                                    .label(tr(lang, Key::Close))
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            ),
                    ),
            )
    });
}

/// 在应用内放大查看本地图片附件
pub fn open_local_image_viewer(path: std::path::PathBuf, title: String, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let p = Palette::new(cx);
        // 对话框内容闭包的签名由 GPUI 定死（只有 `&mut App`），拿不到 `AppState`，从语言全局读。
        let lang = current(cx);
        let viewport = window.viewport_size();
        let dialog_width = f32::from(viewport.width - px(80.)).clamp(360., 1200.);
        let available_height = (f32::from(viewport.height) - 200.).max(160.);
        let file_path_str = path.to_string_lossy().to_string();

        dialog
            .title(title.clone())
            .w(px(dialog_width))
            .margin_top(px(40.))
            .child(
                div().w_full().flex().justify_center().child(
                    div()
                        .max_w_full()
                        .max_h(px(available_height))
                        .child(img(path.clone()).size_full().object_fit(ObjectFit::Contain)),
                ),
            )
            .footer(
                h_flex()
                    .w_full()
                    .gap_3()
                    .justify_between()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child(file_path_str),
                    )
                    .child(
                        Button::new("local-image-close")
                            .primary()
                            .small()
                            .label(tr(lang, Key::Close))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}

fn element_id(prefix: &str, url: &str) -> SharedString {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in url.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{prefix}-{hash:x}").into()
}

fn fit_size(width: f32, height: f32, requested_width: Option<f32>, requested_height: Option<f32>) -> (f32, f32) {
    let mut width = if width > 1. {
        width
    } else {
        requested_width.unwrap_or(320.)
    };
    let mut height = if height > 1. {
        height
    } else {
        requested_height.unwrap_or(180.)
    };
    if let Some(requested) = requested_width.filter(|value| *value > 1.) {
        let scale = requested / width;
        width = requested;
        height *= scale;
    } else if let Some(requested) = requested_height.filter(|value| *value > 1.) {
        let scale = requested / height;
        height = requested;
        width *= scale;
    }
    let scale = (MAX_WIDTH / width).min(MAX_HEIGHT / height).min(1.);
    ((width * scale).max(1.), (height * scale).max(1.))
}

fn url_size_hint(url: &str) -> Option<(f32, f32)> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let mut parts = path.trim_end_matches('/').rsplit('/');
    let height: f32 = parts.next()?.parse().ok()?;
    let width: f32 = parts.next()?.parse().ok()?;
    (width >= 16. && height >= 16. && width <= 8000. && height <= 8000.).then_some((width, height))
}

fn sole_html_image(raw: &str) -> Option<ChatImage> {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("<img") || lower.matches("<img").count() != 1 {
        return None;
    }
    let tag_end = trimmed.find('>')?;
    let after = trimmed[tag_end + 1..].trim();
    if !after.is_empty() {
        return None;
    }
    let tag = &trimmed[..=tag_end];
    let url = html_attr(tag, "src")?;
    if url.is_empty() {
        return None;
    }
    Some(ChatImage {
        url,
        alt: html_attr(tag, "alt").unwrap_or_default(),
        width: html_attr(tag, "width").as_deref().and_then(parse_px),
        height: html_attr(tag, "height").as_deref().and_then(parse_px),
    })
}

fn html_attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let key = format!("{name}=");
    let index = lower.find(&key)?;
    let rest = tag[index + key.len()..].trim_start();
    let mut chars = rest.chars();
    let first = chars.next()?;
    if first == '"' || first == '\'' {
        let end = rest[1..].find(first)?;
        return Some(rest[1..1 + end].to_string());
    }
    let end = rest
        .find(|ch: char| ch.is_whitespace() || ch == '>')
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

fn parse_px(value: &str) -> Option<f32> {
    let value = value.trim().trim_end_matches("px").trim_end_matches('%');
    let parsed = value.parse::<f32>().ok()?;
    (parsed > 0.).then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[std::prelude::v1::test]
    fn large_images_shrink_and_small_images_stay() {
        assert_eq!(fit_size(200., 100., None, None), (200., 100.));
        let (width, height) = fit_size(2000., 1000., None, None);
        assert!((width - 560.).abs() < 0.1);
        assert!((height - 280.).abs() < 0.1);
    }

    #[std::prelude::v1::test]
    fn picsum_style_url_hints_the_placeholder_size() {
        assert_eq!(url_size_hint("https://picsum.photos/200/100"), Some((200., 100.)));
        assert_eq!(url_size_hint("https://example.com/a.png"), None);
    }

    #[std::prelude::v1::test]
    fn html_img_is_only_taken_when_it_is_the_whole_node() {
        let image = sole_html_image(r#"<img src="https://example.com/a.png" alt="示意图" width="320">"#).unwrap();
        assert_eq!(image.url, "https://example.com/a.png");
        assert_eq!(image.alt, "示意图");
        assert_eq!(image.width, Some(320.));
        assert!(sole_html_image("<img src=\"https://example.com/a.png\"> and text").is_none());
    }

    #[std::prelude::v1::test]
    fn decodes_valid_base64_data_url() {
        let b64 = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let decoded = decode_data_url(b64);
        assert!(decoded.is_some());
        let (img, natural) = decoded.unwrap();
        assert_eq!(img.format(), gpui_kit::ImageFormat::Png);
        assert_eq!(natural, (1.0, 1.0));
    }

    #[std::prelude::v1::test]
    fn decodes_svg_data_url_with_spaces() {
        let raw = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='200' height='100'%3E%3Crect width='200' height='100' fill='%234a90d9'/%3E%3Ctext x='50' y='55' fill='white' font-family='Arial' font-size='24' text-anchor='middle'%3EHello!%3C/text%3E%3C/svg%3E";
        let decoded = decode_data_url(raw);
        assert!(decoded.is_some(), "Failed to decode svg data url: {:?}", decoded);
    }

    #[std::prelude::v1::test]
    fn normalizes_svg_data_url_markdown() {
        let input = "![示例图片: 蓝色背景上的 Hello 文字](data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='200' height='100'%3E%3Crect width='200' height='100' fill='%234a90d9'/%3E%3Ctext x='50' y='55' fill='white' font-family='Arial' font-size='24' text-anchor='middle'%3EHello!%3C/text%3E%3C/svg%3E)";
        let normalized = normalize_markdown_image_urls(input);
        assert!(!normalized.contains(" xmlns='http"));
        assert!(normalized.contains("%20xmlns='http"));
    }
}
