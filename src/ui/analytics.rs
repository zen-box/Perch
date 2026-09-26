use std::collections::HashMap;

use chrono::{Duration, Local, NaiveDate};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Palette;
use crate::app::AppState;
use crate::model_info::format_tokens;
use crate::models_dev::{USD_TO_CNY_RATE, calculate_cost};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeRange {
    Days7,
    Days30,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalyticsTab {
    Overview,
    Models,
}

#[derive(Clone, Debug)]
pub struct DayStat {
    pub date_str: String, // "09-26"
    pub full_date: String, // "2026-09-26"
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub total_tokens: usize,
    pub cost_usd: f64,
}

#[derive(Clone, Debug)]
pub struct ModelStat {
    pub model_id: String,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub total_tokens: usize,
    pub cost_usd: f64,
    pub percentage: f32,
    pub color_ix: usize,
}

pub struct AnalyticsSummary {
    pub total_input: usize,
    pub total_output: usize,
    pub total_tokens: usize,
    pub total_cost_usd: f64,
    pub total_messages: usize,
    pub daily: Vec<DayStat>,
    pub models: Vec<ModelStat>,
}

const PALETTE_COLORS: [u32; 6] = [
    0x3B82F6, // 蓝色
    0x8B5CF6, // 紫色
    0x06B6D4, // 青色
    0x10B981, // 绿色
    0xF59E0B, // 琥珀/橙
    0xEC4899, // 粉红
];

pub fn collect_stats(state: &AppState, range: TimeRange) -> AnalyticsSummary {
    let now = Local::now().date_naive();
    let min_date = match range {
        TimeRange::Days7 => Some(now - Duration::days(6)),
        TimeRange::Days30 => Some(now - Duration::days(29)),
        TimeRange::All => None,
    };

    let mut total_input = 0usize;
    let mut total_output = 0usize;
    let mut total_messages = 0usize;
    let mut total_cost_usd = 0.0f64;

    let mut daily_map: HashMap<String, (usize, usize, f64)> = HashMap::new();
    let mut model_map: HashMap<String, (usize, usize, f64)> = HashMap::new();

    for session in &state.storage.sessions {
        for msg in &session.messages {
            if msg.role != "assistant" || (msg.prompt_tokens == 0 && msg.completion_tokens == 0) {
                continue;
            }

            let date = if msg.created_at.len() >= 10 && msg.created_at.contains('-') {
                NaiveDate::parse_from_str(&msg.created_at[..10], "%Y-%m-%d").ok()
            } else if session.updated_at.len() >= 10 && session.updated_at.contains('-') {
                NaiveDate::parse_from_str(&session.updated_at[..10], "%Y-%m-%d").ok()
            } else if session.created_at.len() >= 10 && session.created_at.contains('-') {
                NaiveDate::parse_from_str(&session.created_at[..10], "%Y-%m-%d").ok()
            } else {
                None
            }
            .unwrap_or(now);

            if let Some(min) = min_date {
                if date < min {
                    continue;
                }
            }

            let date_prefix = date.format("%Y-%m-%d").to_string();

            let in_tok = msg.prompt_tokens;
            let out_tok = msg.completion_tokens;
            let model = if !msg.model.is_empty() {
                msg.model.clone()
            } else if !session.model.is_empty() && session.model != "default" {
                session.model.clone()
            } else {
                state.config.default_model_selection().1
            };

            let (cost_usd, _) = calculate_cost(&model, in_tok, out_tok, 0);

            total_input += in_tok;
            total_output += out_tok;
            total_messages += 1;
            total_cost_usd += cost_usd;

            let day_entry = daily_map.entry(date_prefix).or_insert((0, 0, 0.0));
            day_entry.0 += in_tok;
            day_entry.1 += out_tok;
            day_entry.2 += cost_usd;

            let model_entry = model_map.entry(model).or_insert((0, 0, 0.0));
            model_entry.0 += in_tok;
            model_entry.1 += out_tok;
            model_entry.2 += cost_usd;
        }
    }

    let total_tokens = total_input + total_output;

    let days_count = match range {
        TimeRange::Days7 => 7,
        TimeRange::Days30 => 30,
        TimeRange::All => {
            if let Some(min) = daily_map.keys().min() {
                if let Ok(earliest) = NaiveDate::parse_from_str(min, "%Y-%m-%d") {
                    ((now - earliest).num_days() + 1).clamp(7, 60) as i64
                } else {
                    14
                }
            } else {
                14
            }
        }
    };

    let mut daily = Vec::new();
    for i in (0..days_count).rev() {
        let d = now - Duration::days(i);
        let full_date = d.format("%Y-%m-%d").to_string();
        let date_str = d.format("%m/%d").to_string();
        let (in_t, out_t, c_usd) = daily_map.get(&full_date).copied().unwrap_or((0, 0, 0.0));
        daily.push(DayStat {
            date_str,
            full_date,
            input_tokens: in_t,
            output_tokens: out_t,
            total_tokens: in_t + out_t,
            cost_usd: c_usd,
        });
    }

    let mut model_list: Vec<(String, usize, usize, f64)> = model_map
        .into_iter()
        .map(|(m, (i, o, c))| (m, i, o, c))
        .collect();
    model_list.sort_by(|a, b| (b.1 + b.2).cmp(&(a.1 + a.2)));

    let mut models = Vec::new();
    for (ix, (model_id, in_t, out_t, c_usd)) in model_list.into_iter().enumerate() {
        let m_tot = in_t + out_t;
        let percentage = if total_tokens > 0 {
            (m_tot as f32 / total_tokens as f32) * 100.0
        } else {
            0.0
        };
        models.push(ModelStat {
            model_id,
            input_tokens: in_t,
            output_tokens: out_t,
            total_tokens: m_tot,
            cost_usd: c_usd,
            percentage,
            color_ix: ix % PALETTE_COLORS.len(),
        });
    }

    AnalyticsSummary {
        total_input,
        total_output,
        total_tokens,
        total_cost_usd,
        total_messages,
        daily,
        models,
    }
}

pub fn open_analytics_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let (current_range, current_tab) = {
            let state = app.read(cx);
            (state.analytics_range, state.analytics_tab)
        };
        let summary = {
            let state = app.read(cx);
            collect_stats(state, current_range)
        };

        let max_daily_tokens = summary.daily.iter().map(|d| d.total_tokens).max().unwrap_or(1).max(1);

        let tab_overview_app = app.clone();
        let tab_models_app = app.clone();
        let range_7d_app = app.clone();
        let range_30d_app = app.clone();
        let range_all_app = app.clone();

        let first_date = summary.daily.first().map(|d| d.date_str.clone()).unwrap_or_default();
        let mid_date = summary.daily.get(summary.daily.len() / 2).map(|d| d.date_str.clone()).unwrap_or_default();
        let last_date = summary.daily.last().map(|d| d.date_str.clone()).unwrap_or_default();

        dialog
            .title("用量与费用统计看板")
            .w(px(680.))
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .p_0p5()
                                    .rounded_lg()
                                    .bg(p.muted)
                                    .child(
                                        Button::new("tab-overview")
                                            .xsmall()
                                            .map(|b| if current_tab == AnalyticsTab::Overview { b.primary() } else { b.ghost() })
                                            .label("Overview 概览")
                                            .on_click(move |_, window, cx| {
                                                tab_overview_app.update(cx, |this, _| {
                                                    this.analytics_tab = AnalyticsTab::Overview;
                                                });
                                                window.refresh();
                                            }),
                                    )
                                    .child(
                                        Button::new("tab-models")
                                            .xsmall()
                                            .map(|b| if current_tab == AnalyticsTab::Models { b.primary() } else { b.ghost() })
                                            .label("Models 模型排行")
                                            .on_click(move |_, window, cx| {
                                                tab_models_app.update(cx, |this, _| {
                                                    this.analytics_tab = AnalyticsTab::Models;
                                                });
                                                window.refresh();
                                            }),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .p_0p5()
                                    .rounded_lg()
                                    .bg(p.muted)
                                    .child(
                                        Button::new("range-7d")
                                            .xsmall()
                                            .map(|b| if current_range == TimeRange::Days7 { b.primary() } else { b.ghost() })
                                            .label("7d")
                                            .on_click(move |_, window, cx| {
                                                range_7d_app.update(cx, |this, _| {
                                                    this.analytics_range = TimeRange::Days7;
                                                });
                                                window.refresh();
                                            }),
                                    )
                                    .child(
                                        Button::new("range-30d")
                                            .xsmall()
                                            .map(|b| if current_range == TimeRange::Days30 { b.primary() } else { b.ghost() })
                                            .label("30d")
                                            .on_click(move |_, window, cx| {
                                                range_30d_app.update(cx, |this, _| {
                                                    this.analytics_range = TimeRange::Days30;
                                                });
                                                window.refresh();
                                            }),
                                    )
                                    .child(
                                        Button::new("range-all")
                                            .xsmall()
                                            .map(|b| if current_range == TimeRange::All { b.primary() } else { b.ghost() })
                                            .label("All")
                                            .on_click(move |_, window, cx| {
                                                range_all_app.update(cx, |this, _| {
                                                    this.analytics_range = TimeRange::All;
                                                });
                                                window.refresh();
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(metric_card("总 Token 消耗", &format_tokens(summary.total_tokens as u32), format!("入 {} · 出 {}", format_tokens(summary.total_input as u32), format_tokens(summary.total_output as u32)), &p))
                            .child(metric_card("预估费用", &format!("${:.2}", summary.total_cost_usd), format!("约合 ¥{:.2}", summary.total_cost_usd * USD_TO_CNY_RATE), &p))
                            .child(metric_card("回答条数", &summary.total_messages.to_string(), "条助手回复".to_string(), &p)),
                    )
                    .when(current_tab == AnalyticsTab::Overview, |this| {
                        this.child(
                            v_flex()
                                .gap_2()
                                .p_3()
                                .rounded_xl()
                                .border_1()
                                .border_color(p.border)
                                .bg(p.background)
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.muted_foreground)
                                        .child("每日 Token 消耗走势")
                                        .child(format!("峰值: {} / 天", format_tokens(max_daily_tokens as u32))),
                                )
                                .child(
                                    h_flex()
                                        .h(px(120.))
                                        .items_end()
                                        .gap_1()
                                        .pt_2()
                                        .children(summary.daily.iter().map(|day| {
                                            let ratio = (day.total_tokens as f32 / max_daily_tokens as f32).clamp(0.04, 1.0);
                                            let bar_h = px(100.0 * ratio);
                                            let day_text = day.full_date.clone();
                                            let total_tok = day.total_tokens;
                                            let day_in = day.input_tokens;
                                            let day_out = day.output_tokens;
                                            let cost_val = day.cost_usd;

                                            v_flex()
                                                .flex_1()
                                                .h_full()
                                                .justify_end()
                                                .items_center()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .id(SharedString::from(format!("bar-{}", day.full_date)))
                                                        .w_full()
                                                        .max_w(px(24.))
                                                        .h(bar_h)
                                                        .rounded_t_sm()
                                                        .bg(if day.total_tokens > 0 { p.primary } else { p.muted })
                                                        .hover(|s| s.bg(p.primary.opacity(0.8)))
                                                        .tooltip(move |window, cx| {
                                                            Tooltip::new(format!(
                                                                "{}\n• 消耗: {} tokens (入 {} · 出 {})\n• 预估: ${:.4} (¥{:.3})",
                                                                day_text,
                                                                format_tokens(total_tok as u32),
                                                                format_tokens(day_in as u32),
                                                                format_tokens(day_out as u32),
                                                                cost_val,
                                                                cost_val * USD_TO_CNY_RATE,
                                                            ))
                                                            .build(window, cx)
                                                        }),
                                                )
                                        })),
                                )
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .text_xs()
                                        .text_color(p.muted_foreground)
                                        .child(first_date)
                                        .child(mid_date)
                                        .child(last_date),
                                ),
                        )
                    })
                    .child(
                        v_flex()
                            .gap_2()
                            .p_3()
                            .rounded_xl()
                            .border_1()
                            .border_color(p.border)
                            .bg(p.background)
                            .child(
                                h_flex()
                                    .justify_between()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.muted_foreground)
                                    .child(if current_tab == AnalyticsTab::Overview { "模型用量排行 (Top Models)" } else { "全部模型用量与明细 (All Models)" })
                                    .child("占比"),
                            )
                            .child(
                                div()
                                    .map(|this| if current_tab == AnalyticsTab::Overview { this.max_h(px(160.)) } else { this.max_h(px(280.)) })
                                    .child(if summary.models.is_empty() {
                                        v_flex()
                                            .py_6()
                                            .items_center()
                                            .justify_center()
                                            .text_xs()
                                            .text_color(p.muted_foreground)
                                            .child("暂无该时间范围内的模型用量记录")
                                            .into_any_element()
                                    } else {
                                        v_flex()
                                            .gap_2p5()
                                            .children(summary.models.iter().map(|stat| {
                                                let color = rgb(PALETTE_COLORS[stat.color_ix]);
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap_3()
                                                    .child(
                                                        h_flex()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .gap_2()
                                                            .items_center()
                                                            .child(div().size(px(8.)).rounded_full().bg(color))
                                                            .child(
                                                                div()
                                                                    .truncate()
                                                                    .text_sm()
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .child(stat.model_id.clone()),
                                                            ),
                                                    )
                                                    .child(
                                                        h_flex()
                                                            .flex_none()
                                                            .gap_3()
                                                            .items_center()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(p.muted_foreground)
                                                                    .child(format!("{} (入 {} · 出 {})", format_tokens(stat.total_tokens as u32), format_tokens(stat.input_tokens as u32), format_tokens(stat.output_tokens as u32))),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .child(format!("${:.2}", stat.cost_usd)),
                                                            )
                                                            .child(
                                                                div()
                                                                    .w(px(50.))
                                                                    .text_right()
                                                                    .text_xs()
                                                                    .font_weight(FontWeight::SEMIBOLD)
                                                                    .child(format!("{:.1}%", stat.percentage)),
                                                            ),
                                                    )
                                            }))
                                            .overflow_y_scrollbar()
                                            .into_any_element()
                                    }),
                            ),
                    ),
            )
            .footer(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child("计费单价同步自 models.dev，汇率按 1 USD = 7.2 CNY 换算"),
                    )
                    .child(
                        Button::new("analytics-close")
                            .primary()
                            .small()
                            .label("完成")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}

fn metric_card(title: &'static str, value: &str, sub: String, p: &Palette) -> impl IntoElement {
    v_flex()
        .flex_1()
        .p_3()
        .rounded_xl()
        .border_1()
        .border_color(p.border)
        .bg(p.background)
        .gap_1()
        .child(div().text_xs().text_color(p.muted_foreground).child(title))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(value.to_string()))
        .child(div().text_xs().text_color(p.muted_foreground).child(sub))
}
