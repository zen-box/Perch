//! 设置页的「MCP 服务器」与「关于」两个页面。

use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::settings::{page, section};
use super::{Palette, icon_tile};

// ================= MCP / 关于 =================

pub(super) fn render_mcp(p: &Palette) -> impl IntoElement {
    page(
        "settings-mcp",
        "MCP 服务器",
        "通过 Model Context Protocol 为 Agent 接入外部工具",
        p,
        v_flex()
            .items_center()
            .gap_3()
            .py_12()
            .rounded_lg()
            .border_1()
            .border_dashed()
            .border_color(p.border)
            .child(icon_tile(IconName::Plug, px(44.), p.muted, p.muted_foreground))
            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child("即将推出"))
            .child(
                div()
                    .max_w(px(420.))
                    .text_center()
                    .text_sm()
                    .text_color(p.muted_foreground)
                    .child("后续将支持接入本地 Stdio 与远程 SSE 类型的 MCP 服务器，让 Agent 可以使用文件系统、GitHub、数据库等工具。"),
            ),
    )
}

pub(super) fn render_about(p: &Palette) -> impl IntoElement {
    let features = [
        (IconName::HardDrive, "数据只保存在本地，没有任何云端遥测"),
        (
            IconName::Layers,
            "支持 OpenAI Chat、OpenAI Responses、Gemini、Claude 四种接口规范",
        ),
        (IconName::Zap, "原生 SSE 流式解析，Markdown 实时渲染"),
        (
            IconName::SquareTerminal,
            "内置本地工具：/ls、/read、/git、/bash（执行前需授权）",
        ),
        (IconName::Languages, "界面支持简体中文、繁體中文、English、日本語"),
    ];

    page(
        "settings-about",
        "关于",
        "纯 Rust + GPUI 构建的桌面 AI 工作台",
        p,
        v_flex()
            .gap_6()
            .child(
                h_flex()
                    .gap_4()
                    .child(icon_tile(IconName::Sparkles, px(56.), p.primary, p.primary_foreground))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Perch"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(p.muted_foreground)
                                    .child(format!("版本 {}", env!("CARGO_PKG_VERSION"))),
                            ),
                    ),
            )
            .child(section(
                "特性",
                p,
                features
                    .into_iter()
                    .map(|(icon, text)| {
                        h_flex()
                            .gap_3()
                            .px_4()
                            .py_3()
                            .text_sm()
                            .child(Icon::new(icon).size(px(16.)).text_color(p.muted_foreground))
                            .child(text)
                            .into_any_element()
                    })
                    .collect(),
            )),
    )
}
