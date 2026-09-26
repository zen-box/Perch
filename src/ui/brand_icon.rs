use gpui_kit::component::{Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use crate::brand::{self, Brand, BrandGlyph};
use crate::config::{ModelConfig, ProviderConfig};
use crate::model_info::{Capability, format_tokens};

/// 没有匹配到品牌时，按名字挑一个固定的颜色显示首字母
const FALLBACK_COLORS: [u32; 10] = [
    0x3B82F6, 0x8B5CF6, 0xEC4899, 0xF97316, 0x10B981, 0x14B8A6, 0x6366F1, 0xEF4444, 0x0EA5E9, 0x84CC16,
];

/// 品牌头像：品牌色底 + 图标。没有图标文件时显示品牌首字母，没有品牌时显示名字首字母。
pub fn brand_avatar(brand: Option<&'static Brand>, label: &str, size: Pixels, p: &Palette) -> AnyElement {
    let Some(brand) = brand else {
        return letter_avatar(label, size);
    };
    let tile = div()
        .flex_none()
        .size(size)
        .rounded(size * 0.28)
        .overflow_hidden()
        .bg(rgb(brand.background))
        .flex()
        .items_center()
        .justify_center()
        // 和界面背景颜色接近的头像加一圈描边，否则看不出边界
        .when(if p.is_dark { brand.is_dark() } else { brand.is_light() }, |this| {
            this.border_1().border_color(p.border)
        });
    match brand.glyph() {
        BrandGlyph::Mono(path) => tile
            .child(svg().path(path).size(size * brand.scale).text_color(rgb(brand.foreground)))
            .into_any_element(),
        BrandGlyph::Color(path) => tile.child(img(path).size(size * brand.scale)).into_any_element(),
        BrandGlyph::Letter(letter) => tile
            .text_color(rgb(brand.letter_color()))
            .text_size(size * 0.5)
            .font_weight(FontWeight::SEMIBOLD)
            .child(letter.to_string())
            .into_any_element(),
    }
}

fn letter_avatar(label: &str, size: Pixels) -> AnyElement {
    let letter = label
        .trim()
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .map(|ch| ch.to_uppercase().next().unwrap_or(ch))
        .unwrap_or('?');
    let hash = label.bytes().fold(0u32, |hash, byte| hash.wrapping_mul(31).wrapping_add(byte as u32));
    div()
        .flex_none()
        .size(size)
        .rounded(size * 0.28)
        .bg(rgb(FALLBACK_COLORS[hash as usize % FALLBACK_COLORS.len()]))
        .text_color(rgb(0xFFFFFF))
        .text_size(size * 0.5)
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(letter.to_string())
        .into_any_element()
}

pub fn model_avatar(model: &ModelConfig, size: Pixels, p: &Palette) -> AnyElement {
    brand_avatar(brand::brand_for_model(model), &model.name, size, p)
}

/// 只知道模型 ID 的场景（例如模型已被删除的历史消息）
pub fn model_id_avatar(model_id: &str, size: Pixels, p: &Palette) -> AnyElement {
    brand_avatar(brand::brand_for_model_id(model_id), model_id, size, p)
}

pub fn provider_avatar(provider: &ProviderConfig, size: Pixels, p: &Palette) -> AnyElement {
    brand_avatar(brand::brand_for_provider(provider), &provider.name, size, p)
}

pub fn capability_icon(capability: Capability) -> IconName {
    match capability {
        Capability::Vision => IconName::Eye,
        Capability::Files => IconName::FileText,
        Capability::Tools => IconName::Wrench,
        Capability::WebSearch => IconName::Globe,
        Capability::ImageOutput => IconName::Palette,
    }
}

pub fn capability_color(capability: Capability) -> Hsla {
    rgb(match capability {
        Capability::Vision => 0x10B981,
        Capability::Files => 0x3B82F6,
        Capability::Tools => 0xF59E0B,
        Capability::WebSearch => 0x06B6D4,
        Capability::ImageOutput => 0xEC4899,
    })
    .into()
}

pub fn thinking_color() -> Hsla {
    rgb(0x8B5CF6).into()
}

/// 模型列表里的小标签：上下文长度、思考、能力图标
pub fn model_badges(model: &ModelConfig, p: &Palette) -> impl IntoElement {
    let context = model.effective_context_window();
    h_flex()
        .flex_none()
        .gap_1()
        .items_center()
        .when_some(context, |this, tokens| {
            this.child(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(p.muted)
                    .text_xs()
                    .text_color(p.muted_foreground)
                    .child(format_tokens(tokens)),
            )
        })
        .when(model.thinks(), |this| {
            this.child(Icon::new(IconName::Brain).size(px(13.)).text_color(thinking_color()))
        })
        .children(
            model
                .effective_capabilities()
                .into_iter()
                .map(|capability| Icon::new(capability_icon(capability)).size(px(13.)).text_color(capability_color(capability))),
        )
}
