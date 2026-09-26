//! 从接口拉回模型列表后的"勾选要添加哪些"弹窗。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use super::Palette;
use super::brand_icon::model_badges;
use crate::app::AppState;
use crate::config::ModelConfig;

/// 打开「选择要添加的模型」弹窗。打开前先由调用方拉好列表，填进 `pending_models`。
pub fn open_fetch_models_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let (models, selected, existing, query, search) = {
            let state = app.read(cx);
            let existing = state
                .config
                .providers
                .iter()
                .find(|provider| provider.id == state.selected_settings_provider_id)
                .map(|provider| provider.models.iter().map(|model| model.id.clone()).collect::<Vec<_>>())
                .unwrap_or_default();
            (
                state.pending_models.clone(),
                state.pending_model_selection.clone(),
                existing,
                state.model_fetch_query.to_lowercase(),
                state.model_fetch_search.clone(),
            )
        };
        let available: Vec<_> = models
            .iter()
            .filter(|(id, _)| !existing.iter().any(|old| old == id))
            .filter(|(id, name)| {
                query.is_empty() || id.to_lowercase().contains(&query) || name.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        let visible_ids: Vec<String> = available.iter().map(|(id, _)| id.clone()).collect();
        let selected_count = selected.len();
        let already = models
            .iter()
            .filter(|(id, _)| existing.iter().any(|old| old == id))
            .count();
        let rows = available.iter().enumerate().map(|(ix, (id, name))| {
            let checked = selected.contains(id);
            let toggle_id = id.clone();
            let box_app = app.clone();
            h_flex()
                .id(SharedString::from(format!("fetch-row-{ix}")))
                .gap_2()
                .px_2()
                .py_1p5()
                .rounded_md()
                .items_center()
                .child(
                    Checkbox::new(SharedString::from(format!("fetch-model-{ix}")))
                        .checked(checked)
                        .on_click({
                            let id = toggle_id;
                            move |checked, _, cx| {
                                let checked = *checked;
                                box_app.update(cx, |this, cx| this.set_pending_model(&id, checked, cx));
                            }
                        }),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().text_sm().child(name.clone()))
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child(id.clone()),
                        ),
                )
                .child(model_badges(&ModelConfig::new(id.clone(), name.clone()), &p))
                .into_any_element()
        });
        let select_app = app.clone();
        let select_ids = visible_ids.clone();
        let clear_app = app.clone();
        let clear_ids = visible_ids;
        let ok_app = app.clone();
        let cancel_app = app.clone();
        let dismiss_app = app.clone();
        let enter_app = app.clone();
        dialog
            .title("选择要添加的模型")
            .w(px(560.))
            .child(
                v_flex()
                    .gap_3()
                    .child(div().text_xs().text_color(p.muted_foreground).child(format!(
                        "可添加 {} 个，已存在 {already} 个。已选 {selected_count} 个",
                        available.len()
                    )))
                    .child(Input::new(&search))
                    .child(if available.is_empty() {
                        div()
                            .py_8()
                            .text_sm()
                            .text_center()
                            .text_color(p.muted_foreground)
                            .child("没有匹配的新模型")
                            .into_any_element()
                    } else {
                        div()
                            .max_h(px(360.))
                            .border_1()
                            .border_color(p.border)
                            .rounded_lg()
                            .child(v_flex().p_1().children(rows).overflow_y_scrollbar())
                            .into_any_element()
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("select-visible-models")
                                    .outline()
                                    .xsmall()
                                    .label("全选当前")
                                    .on_click(move |_, _, cx| {
                                        select_app
                                            .update(cx, |this, cx| this.select_pending_models(&select_ids, true, cx));
                                    }),
                            )
                            .child(
                                Button::new("clear-visible-models")
                                    .ghost()
                                    .xsmall()
                                    .label("清空当前")
                                    .on_click(move |_, _, cx| {
                                        clear_app
                                            .update(cx, |this, cx| this.select_pending_models(&clear_ids, false, cx));
                                    }),
                            ),
                    ),
            )
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("fetch-cancel")
                            .outline()
                            .label("取消")
                            .on_click(move |_, window, cx| {
                                cancel_app.update(cx, |this, cx| this.cancel_pending_models(cx));
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("fetch-ok")
                            .primary()
                            .label("添加所选")
                            .on_click(move |_, window, cx| {
                                if ok_app.update(cx, |this, cx| this.confirm_pending_models(cx)) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
            .on_ok(move |_, _, cx| enter_app.update(cx, |this, cx| this.confirm_pending_models(cx)))
            .on_cancel(move |_, _, cx| {
                dismiss_app.update(cx, |this, cx| this.cancel_pending_models(cx));
                true
            })
    });
}
