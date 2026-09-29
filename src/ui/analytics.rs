use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Palette;
use crate::analytics::{AnalyticsTab, TimeRange, collect_stats};
use crate::app::AppState;
use crate::i18n::{Key, tr, tr_args};
use crate::model_info::format_tokens;
use crate::models_dev::USD_TO_CNY_RATE;

const PALETTE_COLORS: [u32; 6] = [
    0x3B82F6, // 蓝色
    0x8B5CF6, // 紫色
    0x06B6D4, // 青色
    0x10B981, // 绿色
    0xF59E0B, // 琥珀/橙
    0xEC4899, // 粉红
];

pub fn open_analytics_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let lang = app.read(cx).language();
        let (current_range, current_tab) = {
            let state = app.read(cx);
            (state.analytics_range, state.analytics_tab)
        };
        let summary = {
            let state = app.read(cx);
            // 单价查询来自服务层的 models.dev，在这里注入，analytics.rs 才能保持纯计算
            collect_stats(
                &state.storage.sessions,
                &state.config.default_model_selection().1,
                &|model, input, output| crate::models_dev::calculate_cost(model, input, output, 0).map(|(usd, _)| usd),
                current_range,
            )
        };

        let max_daily_tokens = summary.daily.iter().map(|d| d.total_tokens).max().unwrap_or(1).max(1);
        let estimated_cost = if summary.unknown_costs > 0 {
            tr(lang, Key::Unknown).to_string()
        } else {
            format!("${:.2}", summary.total_cost_usd)
        };
        let estimated_cost_sub = if summary.unknown_costs > 0 {
            tr(lang, Key::Unknown).to_string()
        } else {
            tr_args(
                lang,
                Key::CostCny,
                &[&format!("{:.2}", summary.total_cost_usd * USD_TO_CNY_RATE)],
            )
        };

        let tab_overview_app = app.clone();
        let tab_models_app = app.clone();
        let range_7d_app = app.clone();
        let range_30d_app = app.clone();
        let range_all_app = app.clone();

        let first_date = summary.daily.first().map(|d| d.date_str.clone()).unwrap_or_default();
        let mid_date = summary
            .daily
            .get(summary.daily.len() / 2)
            .map(|d| d.date_str.clone())
            .unwrap_or_default();
        let last_date = summary.daily.last().map(|d| d.date_str.clone()).unwrap_or_default();

        dialog
            .title(tr(lang, Key::UsageDashboard))
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
                                            .map(|b| {
                                                if current_tab == AnalyticsTab::Overview {
                                                    b.primary()
                                                } else {
                                                    b.ghost()
                                                }
                                            })
                                            .label(tr(lang, Key::OverviewTab))
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
                                            .map(|b| {
                                                if current_tab == AnalyticsTab::Models {
                                                    b.primary()
                                                } else {
                                                    b.ghost()
                                                }
                                            })
                                            .label(tr(lang, Key::ModelsTab))
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
                                            .map(|b| {
                                                if current_range == TimeRange::Days7 {
                                                    b.primary()
                                                } else {
                                                    b.ghost()
                                                }
                                            })
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
                                            .map(|b| {
                                                if current_range == TimeRange::Days30 {
                                                    b.primary()
                                                } else {
                                                    b.ghost()
                                                }
                                            })
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
                                            .map(|b| {
                                                if current_range == TimeRange::All {
                                                    b.primary()
                                                } else {
                                                    b.ghost()
                                                }
                                            })
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
                            .child(metric_card(
                                tr(lang, Key::TotalTokens),
                                &format_tokens(summary.total_tokens as u32),
                                tr_args(
                                    lang,
                                    Key::TokenInOut,
                                    &[
                                        &format_tokens(summary.total_input as u32),
                                        &format_tokens(summary.total_output as u32),
                                    ],
                                ),
                                &p,
                            ))
                            .child(metric_card(
                                tr(lang, Key::EstimatedCost),
                                &estimated_cost,
                                estimated_cost_sub,
                                &p,
                            ))
                            .child(metric_card(
                                tr(lang, Key::ReplyCount),
                                &summary.total_messages.to_string(),
                                tr(lang, Key::AssistantReplies).to_string(),
                                &p,
                            )),
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
                                        .child(tr(lang, Key::DailyTokenTrend))
                                        .child(tr_args(
                                            lang,
                                            Key::PeakPerDay,
                                            &[&format_tokens(max_daily_tokens as u32)],
                                        )),
                                )
                                .child(h_flex().h(px(120.)).items_end().gap_1().pt_2().children(
                                    summary.daily.iter().map(|day| {
                                        let ratio =
                                            (day.total_tokens as f32 / max_daily_tokens as f32).clamp(0.04, 1.0);
                                        let bar_h = px(100.0 * ratio);
                                        let day_text = day.full_date.clone();
                                        let total_tok = day.total_tokens;
                                        let day_in = day.input_tokens;
                                        let day_out = day.output_tokens;
                                        let cost_text = if day.cost_known {
                                            format!("{:.4}", day.cost_usd)
                                        } else {
                                            tr(lang, Key::Unknown).to_string()
                                        };
                                        let cny_text = if day.cost_known {
                                            format!("{:.3}", day.cost_usd * USD_TO_CNY_RATE)
                                        } else {
                                            tr(lang, Key::Unknown).to_string()
                                        };

                                        v_flex().flex_1().h_full().justify_end().items_center().gap_1().child(
                                            div()
                                                .id(SharedString::from(format!("bar-{}", day.full_date)))
                                                .w_full()
                                                .max_w(px(24.))
                                                .h(bar_h)
                                                .rounded_t_sm()
                                                .bg(if day.total_tokens > 0 { p.primary } else { p.muted })
                                                .hover(|s| s.bg(p.primary.opacity(0.8)))
                                                .tooltip(move |window, cx| {
                                                    Tooltip::new(tr_args(
                                                        lang,
                                                        Key::DayTooltip,
                                                        &[
                                                            &day_text,
                                                            &format_tokens(total_tok as u32),
                                                            &format_tokens(day_in as u32),
                                                            &format_tokens(day_out as u32),
                                                            &cost_text,
                                                            &cny_text,
                                                        ],
                                                    ))
                                                    .build(window, cx)
                                                }),
                                        )
                                    }),
                                ))
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
                                    .child(if current_tab == AnalyticsTab::Overview {
                                        tr(lang, Key::TopModelsTitle)
                                    } else {
                                        tr(lang, Key::AllModelsTitle)
                                    })
                                    .child(tr(lang, Key::Share)),
                            )
                            .child(
                                div()
                                    .map(|this| {
                                        if current_tab == AnalyticsTab::Overview {
                                            this.max_h(px(160.))
                                        } else {
                                            this.max_h(px(280.))
                                        }
                                    })
                                    .child(if summary.models.is_empty() {
                                        v_flex()
                                            .py_6()
                                            .items_center()
                                            .justify_center()
                                            .text_xs()
                                            .text_color(p.muted_foreground)
                                            .child(tr(lang, Key::NoUsageInRange))
                                            .into_any_element()
                                    } else {
                                        v_flex()
                                            .gap_2p5()
                                            .children(summary.models.iter().enumerate().map(|(ix, stat)| {
                                                let color = rgb(PALETTE_COLORS[ix % PALETTE_COLORS.len()]);
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
                                                                div().text_xs().text_color(p.muted_foreground).child(
                                                                    tr_args(
                                                                        lang,
                                                                        Key::ModelUsageDetail,
                                                                        &[
                                                                            &format_tokens(stat.total_tokens as u32),
                                                                            &format_tokens(stat.input_tokens as u32),
                                                                            &format_tokens(stat.output_tokens as u32),
                                                                        ],
                                                                    ),
                                                                ),
                                                            )
                                                            .child(
                                                                div().text_xs().font_weight(FontWeight::MEDIUM).child(
                                                                    if stat.cost_known {
                                                                        format!("${:.2}", stat.cost_usd)
                                                                    } else {
                                                                        tr(lang, Key::Unknown).to_string()
                                                                    },
                                                                ),
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
                            .child(tr(lang, Key::PricingNote)),
                    )
                    .child(
                        Button::new("analytics-close")
                            .primary()
                            .small()
                            .label(tr(lang, Key::Done))
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
