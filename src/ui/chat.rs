use gpui_kit::base::text::CodeBlock;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::text::TextView;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::brand_icon::{
    FILE_TYPE_CODE, FILE_TYPE_DOC, FILE_TYPE_PDF, FILE_TYPE_SHEET, FILE_TYPE_SLIDES, model_avatar, model_badges,
    model_id_avatar, provider_avatar,
};
use super::markdown_image::open_local_image_viewer;
use super::{CONTENT_MAX_WIDTH, Palette, SIDEBAR_WIDTH, dialogs, icon_tile, model_picker};
use crate::app::AppState;
use crate::i18n::tr;
use crate::model::{Attachment, ChatMessage};

// ================= 会话侧边栏 =================

pub fn render_sidebar(state: &mut AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let app = cx.entity();
    let lang = state.language();
    let query = state.search_session_input.read(cx).value().to_lowercase();
    let active_id = state
        .storage
        .get_active_session()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    let matching_ids = if query.is_empty() {
        None
    } else {
        Some(state.matching_session_ids(&query))
    };
    let folder_filter = state.folder_filter.clone();
    let favorites_only = state.favorites_only;
    let mut folders: Vec<String> = state
        .storage
        .sessions
        .iter()
        .map(|session| session.folder.clone())
        .filter(|folder| !folder.is_empty() && folder != "默认")
        .collect();
    folders.sort();
    folders.dedup();
    let has_favorite = state.storage.sessions.iter().any(|session| session.favorite);
    let show_filters = !folders.is_empty() || has_favorite || favorites_only || !folder_filter.is_empty();

    let mut pinned = Vec::new();
    let mut rest = Vec::new();
    for session in state.storage.sessions.iter().filter(|session| {
        (query.is_empty() || matching_ids.as_ref().is_some_and(|ids| ids.contains(&session.id)))
            && (folder_filter.is_empty() || session.folder == folder_filter)
            && (!favorites_only || session.favorite)
    }) {
        if session.pinned {
            pinned.push(session);
        } else {
            rest.push(session);
        }
    }

    let mut rows: Vec<AnyElement> = Vec::new();
    let mut row_ix = 0usize;
    if !pinned.is_empty() {
        rows.push(sidebar_group("置顶", true, p));
        for session in pinned {
            rows.push(session_row(&app, row_ix, session, session.id == active_id, &folders, p, cx).into_any_element());
            row_ix += 1;
        }
    }
    let mut current_group = "";
    for session in rest {
        let group = date_group_label(&session.created_at);
        if group != current_group {
            current_group = group;
            rows.push(sidebar_group(group, row_ix == 0, p));
        }
        rows.push(session_row(&app, row_ix, session, session.id == active_id, &folders, p, cx).into_any_element());
        row_ix += 1;
    }

    if rows.is_empty() {
        rows.push(
            div()
                .px_2p5()
                .py_6()
                .text_sm()
                .text_color(p.muted_foreground)
                .text_center()
                .child("没有匹配的对话")
                .into_any_element(),
        );
    }

    v_flex()
        .w(SIDEBAR_WIDTH)
        .h_full()
        .flex_none()
        .bg(p.sidebar)
        .border_r_1()
        .border_color(p.sidebar_border)
        .child(
            v_flex()
                .gap_2()
                .p_3()
                .child(
                    Button::new("new-chat")
                        .outline()
                        .w_full()
                        .icon(IconName::SquarePen)
                        .label(tr(lang, "new_chat"))
                        .tooltip("Ctrl+N")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.create_new_session(window, cx);
                        })),
                )
                .child(
                    Input::new(&state.search_session_input)
                        .cleanable(true)
                        .prefix(Icon::new(IconName::Search).small().text_color(p.muted_foreground)),
                )
                .when(show_filters, |this| {
                    this.child(
                        h_flex()
                            .flex_wrap()
                            .gap_1()
                            .child(filter_chip(
                                "folder-all",
                                "全部",
                                folder_filter.is_empty() && !favorites_only,
                                cx.listener(|this, _, _, cx| {
                                    this.clear_session_filters(cx);
                                }),
                            ))
                            .child(filter_chip(
                                "folder-fav",
                                "收藏",
                                favorites_only,
                                cx.listener(|this, _, _, cx| {
                                    this.toggle_favorites_filter(cx);
                                }),
                            ))
                            .children(folders.iter().enumerate().map(|(ix, folder)| {
                                let selected = folder_filter == *folder;
                                let folder = folder.clone();
                                filter_chip(
                                    SharedString::from(format!("folder-{ix}")),
                                    folder.clone(),
                                    selected,
                                    cx.listener(move |this, _, _, cx| {
                                        this.select_folder_filter(&folder, cx);
                                    }),
                                )
                                .into_any_element()
                            })),
                    )
                }),
        )
        .child(
            v_flex()
                .flex_1()
                .px_2()
                .pb_3()
                .gap_px()
                .children(rows)
                .overflow_y_scrollbar(),
        )
}

fn sidebar_group(label: &str, first: bool, p: &Palette) -> AnyElement {
    div()
        .px_2p5()
        .pt(if first { px(4.) } else { px(14.) })
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.muted_foreground)
        .child(label.to_string())
        .into_any_element()
}

fn filter_chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Button::new(id)
        .xsmall()
        .map(|button| if selected { button.primary() } else { button.ghost() })
        .label(label)
        .on_click(on_click)
}

fn session_row(
    app: &Entity<AppState>,
    ix: usize,
    session: &crate::model::ChatSession,
    is_active: bool,
    folders: &[String],
    p: &Palette,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let session_id = session.id.clone();
    let title = session.title.clone();
    let pinned = session.pinned;
    let favorite = session.favorite;
    let folder = session.folder.clone();
    let row_id = SharedString::from(format!("session-{session_id}"));
    let switch_id = session_id.clone();
    let menu_app = app.clone();
    let menu_id = session_id.clone();
    let menu_title = title.clone();
    let menu_folder = folder.clone();
    let menu_folders = folders.to_vec();

    // 标题区域和右侧按钮是兄弟节点：按钮的点击不会冒泡到切换对话，
    // 不需要 stop_propagation（它会让窗口级的文字选择收不到鼠标抬起）。
    h_flex()
        .id(row_id)
        .group("session-row")
        .w_full()
        .h(px(34.))
        .pr_1()
        .gap_1()
        .rounded_md()
        .text_sm()
        .map(|this| {
            if is_active {
                this.bg(p.sidebar_accent)
                    .text_color(p.sidebar_accent_foreground)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                this.text_color(p.sidebar_foreground)
                    .hover(|style| style.bg(p.sidebar_accent.opacity(0.6)))
            }
        })
        .child(
            h_flex()
                .id(SharedString::from(format!("session-open-{session_id}")))
                .flex_1()
                .min_w_0()
                .h_full()
                .pl_2p5()
                .gap_1()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.switch_session(switch_id.clone(), window, cx);
                }))
                .when(pinned, |this| {
                    this.child(Icon::new(IconName::Pin).size(px(12.)).text_color(p.muted_foreground))
                })
                .when(favorite, |this| {
                    this.child(Icon::new(IconName::Star).size(px(12.)).text_color(p.warning))
                })
                .child(div().flex_1().min_w_0().truncate().child(title.clone())),
        )
        .child(
            h_flex()
                .flex_none()
                .gap_px()
                .invisible()
                .group_hover("session-row", |style| style.visible())
                .child(session_icon_button(
                    ("pin-session", ix),
                    if pinned { IconName::PinOff } else { IconName::Pin },
                    if pinned { "取消置顶" } else { "置顶" },
                    {
                        let id = session_id.clone();
                        cx.listener(move |this, _, _, cx| this.toggle_session_pin(&id, cx))
                    },
                ))
                .child(session_icon_button(
                    ("fav-session", ix),
                    if favorite { IconName::StarOff } else { IconName::Star },
                    if favorite { "取消收藏" } else { "收藏" },
                    {
                        let id = session_id.clone();
                        cx.listener(move |this, _, _, cx| this.toggle_session_favorite(&id, cx))
                    },
                ))
                .child(session_icon_button(
                    ("rename-session", ix),
                    IconName::Pencil,
                    "重命名",
                    {
                        let rename_id = session_id.clone();
                        let rename_title = title.clone();
                        cx.listener(move |this, _, window, cx| {
                            dialogs::open_rename_dialog(cx.entity(), this.rename_input.clone(), window, cx);
                            this.start_rename_session(rename_id.clone(), &rename_title, window, cx);
                        })
                    },
                ))
                .child(session_icon_button(("delete-session", ix), IconName::Trash, "删除", {
                    let delete_id = session_id.clone();
                    let delete_title = title.clone();
                    cx.listener(move |_, _, window, cx| {
                        dialogs::confirm_delete_session(cx.entity(), delete_id.clone(), &delete_title, window, cx);
                    })
                })),
        )
        .context_menu(move |menu, _, _| {
            let pin_app = menu_app.clone();
            let pin_id = menu_id.clone();
            let fav_app = menu_app.clone();
            let fav_id = menu_id.clone();
            let rename_app = menu_app.clone();
            let rename_id = menu_id.clone();
            let rename_title = menu_title.clone();
            let delete_app = menu_app.clone();
            let delete_id = menu_id.clone();
            let delete_title = menu_title.clone();
            let folder_app = menu_app.clone();
            let folder_id = menu_id.clone();
            let current_folder = menu_folder.clone();
            let mut menu = menu
                .item(
                    PopupMenuItem::new(if pinned { "取消置顶" } else { "置顶" })
                        .icon(IconName::Pin)
                        .on_click(move |_, _, cx| {
                            pin_app.update(cx, |this, cx| this.toggle_session_pin(&pin_id, cx));
                        }),
                )
                .item(
                    PopupMenuItem::new(if favorite { "取消收藏" } else { "收藏" })
                        .icon(IconName::Star)
                        .on_click(move |_, _, cx| {
                            fav_app.update(cx, |this, cx| this.toggle_session_favorite(&fav_id, cx));
                        }),
                )
                .separator();
            if current_folder != "默认" {
                let app = menu_app.clone();
                let id = menu_id.clone();
                menu = menu.item(
                    PopupMenuItem::new("移出文件夹")
                        .icon(IconName::Folder)
                        .on_click(move |_, _, cx| {
                            app.update(cx, |this, cx| this.set_session_folder(&id, "默认", cx));
                        }),
                );
            }
            for folder_name in &menu_folders {
                if folder_name == &current_folder {
                    continue;
                }
                let app = menu_app.clone();
                let id = menu_id.clone();
                let target = folder_name.clone();
                menu = menu.item(
                    PopupMenuItem::new(format!("移到「{folder_name}」"))
                        .icon(IconName::Folder)
                        .on_click(move |_, _, cx| {
                            app.update(cx, |this, cx| this.set_session_folder(&id, &target, cx));
                        }),
                );
            }
            menu.item(
                PopupMenuItem::new("新建文件夹…")
                    .icon(IconName::FolderPlus)
                    .on_click(move |_, window, cx| {
                        dialogs::open_folder_dialog(folder_app.clone(), window, cx);
                        folder_app.update(cx, |this, cx| {
                            this.begin_move_folder(&folder_id, &current_folder, window, cx)
                        });
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new("重命名")
                    .icon(IconName::Pencil)
                    .on_click(move |_, window, cx| {
                        let input = rename_app.read(cx).rename_input.clone();
                        dialogs::open_rename_dialog(rename_app.clone(), input, window, cx);
                        rename_app.update(cx, |this, cx| {
                            this.start_rename_session(rename_id.clone(), &rename_title, window, cx)
                        });
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new("删除")
                    .icon(IconName::Trash)
                    .on_click(move |_, window, cx| {
                        dialogs::confirm_delete_session(
                            delete_app.clone(),
                            delete_id.clone(),
                            &delete_title,
                            window,
                            cx,
                        );
                    }),
            )
        })
}

fn session_icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(icon)
        .tooltip(tooltip)
        .on_click(on_click)
}

fn date_group_label(created_at: &str) -> &'static str {
    let today = chrono::Local::now().date_naive();
    match chrono::NaiveDate::parse_from_str(created_at.get(..10).unwrap_or(""), "%Y-%m-%d") {
        Ok(date) => match (today - date).num_days() {
            ..=0 => "今天",
            1 => "昨天",
            2..=6 => "近 7 天",
            7..=29 => "近 30 天",
            _ => "更早",
        },
        Err(_) => "更早",
    }
}

fn format_msg_time(created_at: &str) -> &str {
    if let Some(pos) = created_at.rfind(' ') {
        &created_at[pos + 1..]
    } else {
        created_at
    }
}

// ================= 对话主区域 =================

pub fn render_chat_panel(state: &mut AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    state.sync_message_list(cx);

    let app = cx.entity();
    let has_messages = state
        .storage
        .get_active_session()
        .is_some_and(|s| !s.messages.is_empty());

    v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .child(render_chat_header(state, p, cx))
        .child(div().flex_1().min_h_0().w_full().map(|this| {
            if has_messages {
                this.child(
                    MessageScroller::new("messages", state.message_list.clone(), {
                        let app = app.clone();
                        move |ix, window, cx| render_message_row(&app, ix, window, cx)
                    })
                    .with_list_style(StyleRefinement::default().pt_6().pb_4())
                    .with_jump_button_label("回到最新")
                    .with_bottom_fade(p.background),
                )
            } else {
                this.child(render_empty_state(state, p, cx))
            }
        }))
        .child(
            v_flex()
                .w_full()
                .items_center()
                .gap_3()
                .px_6()
                .pb_4()
                .pt_1()
                .when(state.pending_import.is_some(), |this| {
                    this.child(render_import_banner(p, cx))
                })
                .when(
                    state
                        .storage
                        .get_active_session()
                        .is_some_and(|session| session.has_unresolved_compare()),
                    |this| this.child(render_compare_notice(p)),
                )
                .when(state.pending_tool_name.is_some(), |this| {
                    this.child(render_tool_permission(state, p, cx))
                })
                .child(render_composer(state, p, cx)),
        )
}

fn render_chat_header(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let (title, message_count, session_id, pinned, favorite) = state
        .storage
        .get_active_session()
        .map(|session| {
            (
                session.title.clone(),
                session.messages.len(),
                session.id.clone(),
                session.pinned,
                session.favorite,
            )
        })
        .unwrap_or_else(|| ("新对话".into(), 0, String::new(), false, false));

    h_flex()
        .h(px(48.))
        .flex_none()
        .px_4()
        .gap_3()
        .justify_between()
        .border_b_1()
        .border_color(p.border)
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .when(message_count > 0, |this| {
                    this.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child(format!("{} 条消息", message_count)),
                    )
                }),
        )
        .child(
            h_flex()
                .flex_none()
                .gap_1()
                .when(state.sidebar_collapsed, |this| {
                    this.child(
                        Button::new("header-new-chat")
                            .ghost()
                            .small()
                            .icon(IconName::SquarePen)
                            .tooltip("新建对话 (Ctrl+N)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_new_session(window, cx);
                            })),
                    )
                })
                .when(!session_id.is_empty(), |this| {
                    let pin_id = session_id.clone();
                    let fav_id = session_id.clone();
                    this.child(
                        Button::new("header-pin")
                            .ghost()
                            .small()
                            .icon(if pinned { IconName::PinOff } else { IconName::Pin })
                            .selected(pinned)
                            .tooltip(if pinned { "取消置顶" } else { "置顶" })
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_session_pin(&pin_id, cx))),
                    )
                    .child(
                        Button::new("header-favorite")
                            .ghost()
                            .small()
                            .icon(if favorite { IconName::StarOff } else { IconName::Star })
                            .selected(favorite)
                            .tooltip(if favorite { "取消收藏" } else { "收藏" })
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_session_favorite(&fav_id, cx))),
                    )
                })
                .child(
                    Button::new("export-markdown")
                        .ghost()
                        .small()
                        .icon(IconName::Download)
                        .tooltip("导出为 Markdown")
                        .disabled(message_count == 0)
                        .on_click(cx.listener(|this, _, _, cx| this.export_current_session(cx))),
                )
                .child(
                    Button::new("export-json")
                        .ghost()
                        .small()
                        .icon(IconName::FileDown)
                        .tooltip("导出 JSON 备份")
                        .on_click(cx.listener(|this, _, _, cx| this.export_json_backup(cx))),
                )
                .child(
                    Button::new("import-json")
                        .ghost()
                        .small()
                        .icon(IconName::FileUp)
                        .tooltip("从 JSON 备份恢复")
                        .on_click(cx.listener(|this, _, _, cx| this.pick_import_backup(cx))),
                )
                .child(
                    Button::new("clear-session")
                        .ghost()
                        .small()
                        .icon(IconName::Eraser)
                        .tooltip("清空当前对话")
                        .disabled(message_count == 0)
                        .on_click(cx.listener(|_, _, window, cx| {
                            dialogs::confirm_clear_session(cx.entity(), window, cx);
                        })),
                ),
        )
}

// ================= 消息 =================

fn render_message_row(app: &Entity<AppState>, ix: usize, _: &mut Window, cx: &mut App) -> AnyElement {
    let (msg, owner, expanded, later, streaming) = {
        let state = app.read(cx);
        let Some(session) = state.storage.get_active_session() else {
            return div().into_any_element();
        };
        let Some(msg) = session.messages.get(ix).cloned() else {
            return div().into_any_element();
        };
        let owner = state
            .config
            .providers
            .iter()
            .find_map(|provider| provider.models.iter().find(|model| model.id == msg.model).cloned());
        let expanded = state.expanded_reasoning.contains(&msg.id);
        // 这条消息之后还有几条，重新生成时会被删掉
        let later = session.messages.len().saturating_sub(ix + 1);
        (msg, owner, expanded, later, state.is_streaming)
    };

    let p = Palette::new(cx);
    let content = if msg.role == "user" {
        render_user_message(app, ix, msg, streaming, &p).into_any_element()
    } else {
        let (avatar, model_label) = match owner {
            Some(model) => (model_avatar(&model, px(28.), &p), model.name.clone()),
            None if msg.model.is_empty() => (
                icon_tile(IconName::Sparkles, px(28.), p.primary.opacity(0.12), p.primary).into_any_element(),
                "Assistant".to_string(),
            ),
            None => (model_id_avatar(&msg.model, px(28.), &p), msg.model.clone()),
        };
        render_assistant_message(app, ix, msg, avatar, model_label, expanded, later, &p, cx).into_any_element()
    };

    div()
        .w_full()
        .flex()
        .justify_center()
        .px_3()
        .child(div().w_full().max_w(CONTENT_MAX_WIDTH).child(content))
        .into_any_element()
}

fn attachment_badge(att: &Attachment, p: &Palette) -> (IconName, Hsla, &'static str) {
    if att.is_image() {
        (IconName::Image, p.primary, "图片")
    } else if att.is_pdf() {
        (IconName::FileText, FILE_TYPE_PDF, "PDF")
    } else if att.is_text() {
        (IconName::FileCode, FILE_TYPE_CODE, "文本/代码")
    } else {
        let ext = std::path::Path::new(&att.name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "xlsx" | "xls" | "csv" => (IconName::FileSpreadsheet, FILE_TYPE_SHEET, "表格"),
            "docx" | "doc" => (IconName::FileText, FILE_TYPE_DOC, "Word"),
            "pptx" | "ppt" => (IconName::FileText, FILE_TYPE_SLIDES, "PPT"),
            _ => (IconName::File, p.muted_foreground, "文件"),
        }
    }
}

fn render_message_attachments(attachments: &[Attachment], p: &Palette) -> impl IntoElement {
    h_flex()
        .gap_2()
        .flex_wrap()
        .justify_end()
        .max_w(relative(0.85))
        .children(attachments.iter().map(|att| {
            let abs_path = att.absolute_path();
            let name = att.name.clone();
            let view_path = abs_path.clone();
            let view_title = name.clone();
            let (icon, badge_color, type_label) = attachment_badge(att, p);

            let size_kb = (att.size as f32 / 1024.0).max(0.1);
            let size_label = if size_kb > 1024.0 {
                format!("{:.1} MB", size_kb / 1024.0)
            } else {
                format!("{:.0} KB", size_kb)
            };

            let elem_id = SharedString::from(format!("msg-att-{}", att.id));
            if att.is_image() {
                div()
                    .id(elem_id)
                    .cursor_pointer()
                    .rounded_lg()
                    .overflow_hidden()
                    .border_1()
                    .border_color(p.border)
                    .bg(p.muted)
                    .shadow_xs()
                    .w(px(140.))
                    .h(px(100.))
                    .hover(|style| style.border_color(p.primary.opacity(0.8)))
                    .on_click(move |_, window, cx| {
                        open_local_image_viewer(view_path.clone(), view_title.clone(), window, cx);
                    })
                    .child(img(abs_path).size_full().object_fit(ObjectFit::Cover))
                    .into_any_element()
            } else {
                h_flex()
                    .id(elem_id)
                    .cursor_pointer()
                    .rounded_lg()
                    .border_1()
                    .border_color(p.border)
                    .bg(p.muted)
                    .shadow_xs()
                    .p_2()
                    .gap_2p5()
                    .items_center()
                    .min_w(px(160.))
                    .max_w(px(240.))
                    .hover(|style| style.border_color(p.primary.opacity(0.8)))
                    .on_click(move |_, _, _| {
                        #[cfg(target_os = "windows")]
                        {
                            let _ = std::process::Command::new("explorer").arg(&view_path).spawn();
                        }
                    })
                    .child(
                        div()
                            .size(px(34.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(badge_color.opacity(0.15))
                            .child(Icon::new(icon).size(px(18.)).text_color(badge_color)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.foreground)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted_foreground)
                                    .child(format!("{size_label} · {type_label}")),
                            ),
                    )
                    .into_any_element()
            }
        }))
}

fn render_user_message(
    app: &Entity<AppState>,
    ix: usize,
    msg: ChatMessage,
    streaming: bool,
    p: &Palette,
) -> impl IntoElement {
    let copy_app = app.clone();
    let copy_text = msg.content.clone();
    let edit_app = app.clone();
    let quote_app = app.clone();
    let delete_app = app.clone();
    let message_id = msg.id.clone();
    let edit_id = message_id.clone();
    let quote_id = message_id.clone();
    let delete_id = message_id;
    let quote = msg.quote.clone();
    let created_at = msg.created_at.clone();
    let attachments = msg.attachments;
    let content = msg.content;
    let has_content = !content.trim().is_empty();
    let has_attachments = !attachments.is_empty();

    v_flex()
        .w_full()
        .items_end()
        .gap_1()
        .group("user-message")
        .when_some(quote, |this, quote| {
            this.child(
                div()
                    .max_w(relative(0.82))
                    .px_3()
                    .py_1()
                    .border_l_2()
                    .border_color(p.border)
                    .text_xs()
                    .text_color(p.muted_foreground)
                    .child(preview(&quote, 160)),
            )
        })
        .when(has_attachments, |this| {
            this.child(render_message_attachments(&attachments, p))
        })
        .when(has_content, |this| {
            this.child(
                div()
                    .max_w(relative(0.82))
                    .px_4()
                    .py_2p5()
                    .rounded(px(18.))
                    .bg(p.muted)
                    .text_sm()
                    .line_height(relative(1.6))
                    .child(content),
            )
        })
        .child(
            h_flex()
                .gap_1()
                .text_xs()
                .text_color(p.muted_foreground)
                .invisible()
                .group_hover("user-message", |style| style.visible())
                .child(format_msg_time(&created_at).to_string())
                .child(
                    Button::new(("copy-user", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Copy)
                        .tooltip("复制")
                        .on_click(move |_, _, cx| {
                            copy_app.update(cx, |this, cx| this.copy_to_clipboard(&copy_text, cx));
                        }),
                )
                .child(
                    Button::new(("edit-user", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Pencil)
                        .tooltip("编辑并重发")
                        .disabled(streaming)
                        .on_click(move |_, window, cx| {
                            dialogs::open_edit_message_dialog(edit_app.clone(), window, cx);
                            edit_app.update(cx, |this, cx| this.begin_edit_message(&edit_id, window, cx));
                        }),
                )
                .child(
                    Button::new(("quote-user", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Quote)
                        .tooltip("引用")
                        .on_click(move |_, _, cx| quote_app.update(cx, |this, cx| this.quote_message(&quote_id, cx))),
                )
                .child(
                    Button::new(("delete-user", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Trash)
                        .tooltip("删除")
                        .disabled(streaming)
                        .on_click(move |_, _, cx| {
                            delete_app.update(cx, |this, cx| this.delete_message(&delete_id, cx))
                        }),
                ),
        )
}

#[allow(clippy::too_many_arguments)]
fn render_assistant_message(
    app: &Entity<AppState>,
    ix: usize,
    msg: ChatMessage,
    avatar: AnyElement,
    model_label: String,
    expanded: bool,
    later: usize,
    p: &Palette,
    cx: &mut App,
) -> impl IntoElement {
    let is_last = later == 0;
    let mono_font = cx.theme().mono_font_family.clone();
    let has_content = !msg.content.trim().is_empty();
    let has_unresolved_variants = !msg.variants.is_empty() && !has_content;
    let total_variants = msg.variants.len();
    let current_var_ix = msg
        .variants
        .iter()
        .position(|v| v.model == msg.model && v.content == msg.content)
        .unwrap_or(0);
    let reasoning = msg.reasoning_content.clone().filter(|text| !text.trim().is_empty());
    let thinking = msg.is_streaming && !has_content;
    let waiting = thinking && reasoning.is_none() && !has_unresolved_variants;
    let has_metrics = msg.completion_tokens > 0 || msg.speed_tps > 0.0;
    let show_actions = !msg.is_streaming && !has_unresolved_variants && (has_content || msg.error.is_some());
    let copy_app = app.clone();
    let copy_text = msg.content.clone();
    let action_id = msg.id.clone();
    let toggle_app = app.clone();
    let toggle_id = msg.id.clone();

    h_flex().w_full().items_start().gap_3().child(avatar).child(
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .child(
                h_flex()
                    .h(px(28.))
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(model_label.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child(format_msg_time(&msg.created_at).to_string()),
                    ),
            )
            .when(has_unresolved_variants, |this| {
                this.child(render_variants(app, ix, &msg, p, cx))
            })
            // 思考过程（可折叠）
            .when_some(reasoning, |this, reasoning| {
                let open = expanded || thinking;
                this.child(
                    v_flex()
                        .w_full()
                        .rounded_lg()
                        .border_1()
                        .border_color(p.border)
                        .overflow_hidden()
                        .child(
                            h_flex()
                                .id(("reasoning-toggle", ix))
                                .gap_2()
                                .px_3()
                                .py_2()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .hover(|s| s.bg(p.muted))
                                .on_click(move |_, _, cx| {
                                    toggle_app.update(cx, |this, cx| this.toggle_reasoning(&toggle_id, cx));
                                })
                                .child(Icon::new(IconName::Brain).size(px(14.)))
                                .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(if thinking {
                                    "正在思考…"
                                } else {
                                    "思考过程"
                                }))
                                .child(
                                    Icon::new(if open {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .size(px(14.)),
                                ),
                        )
                        .when(open, |this| {
                            this.child(
                                div()
                                    .px_3()
                                    .pb_3()
                                    .pt_1()
                                    .text_xs()
                                    .line_height(relative(1.7))
                                    .text_color(p.muted_foreground)
                                    .child(reasoning),
                            )
                        }),
                )
            })
            // 本地工具调用记录
            .when(!msg.tool_calls.is_empty(), |this| {
                this.child(h_flex().flex_wrap().gap_2().children(msg.tool_calls.iter().map(|tc| {
                    h_flex()
                        .gap_1p5()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(p.border)
                        .bg(p.muted)
                        .text_xs()
                        .font_family(mono_font.clone())
                        .child(Icon::new(IconName::Wrench).size(px(12.)).text_color(p.muted_foreground))
                        .child(tc.clone())
                })))
            })
            .when(has_content, |this| {
                this.child(
                    div().w_full().text_sm().child(
                        markdown_view(format!("md-{}", msg.id), msg.content.clone())
                            .stream_fade(msg.is_streaming)
                            .code_block_actions(|block, _, cx| render_code_block_actions(block, cx)),
                    ),
                )
            })
            .when(waiting, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .text_color(p.muted_foreground)
                        .child(Spinner::new().small())
                        .child("正在生成…"),
                )
            })
            .when_some(msg.error.clone(), |this, err| {
                this.child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .px_3()
                        .py_2p5()
                        .rounded_lg()
                        .border_1()
                        .border_color(p.danger.opacity(0.35))
                        .bg(p.danger.opacity(if p.is_dark { 0.12 } else { 0.06 }))
                        .text_sm()
                        .text_color(p.danger)
                        .child(Icon::new(IconName::CircleAlert).size(px(16.)).mt_0p5())
                        .child(div().flex_1().min_w_0().child(err)),
                )
            })
            .when(show_actions, |this| {
                let regen_app = app.clone();
                let other_app = app.clone();
                let quote_app = app.clone();
                let delete_app = app.clone();
                let continue_app = app.clone();
                let regen_id = action_id.clone();
                let other_id = action_id.clone();
                let quote_id = action_id.clone();
                let delete_id = action_id.clone();
                this.child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .when(total_variants > 1 && has_content, |this| {
                            let switch_app = app.clone();
                            let message_id = action_id.clone();
                            this.child(
                                h_flex()
                                    .items_center()
                                    .gap_0p5()
                                    .mr_1p5()
                                    .child(
                                        Button::new(SharedString::from(format!("prev-var-{}", action_id)))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::ChevronLeft)
                                            .disabled(current_var_ix == 0)
                                            .tooltip("上一个模型回答")
                                            .on_click({
                                                let switch_app = switch_app.clone();
                                                let message_id = message_id.clone();
                                                move |_, _, cx| {
                                                    if current_var_ix > 0 {
                                                        switch_app.update(cx, |this, cx| {
                                                            this.switch_variant(&message_id, current_var_ix - 1, cx);
                                                        });
                                                    }
                                                }
                                            }),
                                    )
                                    .child(
                                        div()
                                            .px_1()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.muted_foreground)
                                            .child(format!("{}/{}", current_var_ix + 1, total_variants)),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("next-var-{}", action_id)))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::ChevronRight)
                                            .disabled(current_var_ix + 1 >= total_variants)
                                            .tooltip("下一个模型回答")
                                            .on_click({
                                                let switch_app = switch_app.clone();
                                                let message_id = message_id.clone();
                                                move |_, _, cx| {
                                                    if current_var_ix + 1 < total_variants {
                                                        switch_app.update(cx, |this, cx| {
                                                            this.switch_variant(&message_id, current_var_ix + 1, cx);
                                                        });
                                                    }
                                                }
                                            }),
                                    ),
                            )
                        })
                        .when(has_content, |this| {
                            this.child(
                                Button::new(("copy-assistant", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Copy)
                                    .tooltip("复制回答")
                                    .on_click(move |_, _, cx| {
                                        copy_app.update(cx, |this, cx| this.copy_to_clipboard(&copy_text, cx));
                                    }),
                            )
                        })
                        .child(
                            Button::new(("regen", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::RefreshCw)
                                .tooltip("重新生成")
                                .on_click(move |_, window, cx| {
                                    // 较早的回答重新生成会删掉后面的对话，先确认
                                    if later > 0 {
                                        dialogs::confirm_regenerate(
                                            regen_app.clone(),
                                            regen_id.clone(),
                                            None,
                                            later,
                                            window,
                                            cx,
                                        );
                                    } else {
                                        regen_app
                                            .update(cx, |this, cx| this.regenerate_message(&regen_id, None, None, cx));
                                    }
                                }),
                        )
                        .child(
                            Popover::new(SharedString::from(format!("regen-popover-{}", other_id)))
                                .anchor(Anchor::TopLeft)
                                .w(px(320.))
                                .trigger(
                                    Button::new(("regen-other", ix))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Repeat)
                                        .tooltip("换模型重答"),
                                )
                                .content(move |popover, window, cx| {
                                    render_regen_popover(&other_app, &other_id, later, popover, window, cx)
                                }),
                        )
                        .when(is_last && has_content, |this| {
                            this.child(
                                Button::new(("continue", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Forward)
                                    .tooltip("继续生成")
                                    .on_click(move |_, _, cx| {
                                        continue_app.update(cx, |this, cx| this.continue_message(cx))
                                    }),
                            )
                        })
                        .child(
                            Button::new(("quote-assistant", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Quote)
                                .tooltip("引用")
                                .on_click(move |_, _, cx| {
                                    quote_app.update(cx, |this, cx| this.quote_message(&quote_id, cx))
                                }),
                        )
                        .child(
                            Button::new(("delete-assistant", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Trash)
                                .tooltip("删除")
                                .on_click(move |_, _, cx| {
                                    delete_app.update(cx, |this, cx| this.delete_message(&delete_id, cx))
                                }),
                        )
                        .when(has_metrics, |this| {
                            this.child(render_message_metrics(&msg, &model_label, p))
                        }),
                )
            }),
    )
}

fn render_regen_popover(
    app: &Entity<AppState>,
    message_id: &str,
    later: usize,
    _: &mut PopoverState,
    _: &mut Window,
    cx: &mut Context<PopoverState>,
) -> impl IntoElement + use<> {
    let p = Palette::new(cx);
    let (providers, current_model_id) = {
        let state = app.read(cx);
        let curr = state
            .storage
            .get_active_session()
            .and_then(|s| s.messages.iter().find(|m| m.id == message_id))
            .map(|m| m.model.clone())
            .unwrap_or_default();
        (state.config.providers.clone(), curr)
    };

    let mut rows = Vec::new();
    for provider in providers.iter().filter(|p| p.enabled) {
        let models: Vec<_> = provider.models.iter().filter(|m| m.enabled).collect();
        if models.is_empty() {
            continue;
        }

        rows.push(
            h_flex()
                .gap_1p5()
                .px_2()
                .pt_2()
                .pb_1()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted_foreground)
                .child(provider_avatar(provider, px(14.), &p))
                .child(provider.name.clone())
                .into_any_element(),
        );

        for model in models {
            let is_current = !current_model_id.is_empty() && model.id == current_model_id;
            let select_app = app.clone();
            let msg_id = message_id.to_string();
            let pid = provider.id.clone();
            let mid = model.id.clone();

            rows.push(
                h_flex()
                    .id(SharedString::from(format!(
                        "regen-pick-{}-{}-{}",
                        msg_id, provider.id, model.id
                    )))
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .when(is_current, |this| this.bg(p.accent.opacity(0.5)))
                    .hover(|s| s.bg(p.accent))
                    .on_click(cx.listener(move |popover, _, window, cx| {
                        if later > 0 {
                            let target = Some((pid.clone(), mid.clone()));
                            dialogs::confirm_regenerate(select_app.clone(), msg_id.clone(), target, later, window, cx);
                        } else {
                            select_app.update(cx, |this, cx| {
                                this.regenerate_message(&msg_id, Some(pid.clone()), Some(mid.clone()), cx);
                            });
                        }
                        popover.dismiss(window, cx);
                    }))
                    .child(model_avatar(model, px(18.), &p))
                    .child(div().flex_1().min_w_0().truncate().child(model.name.clone()))
                    .child(model_badges(model, &p))
                    .when(is_current, |this| {
                        this.child(div().text_xs().text_color(p.muted_foreground).child("当前"))
                    })
                    .into_any_element(),
            );
        }
    }

    let has_models = !rows.is_empty();

    v_flex()
        .gap_1()
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .px_2()
                .pt_1()
                .pb_1p5()
                .border_b_1()
                .border_color(p.border)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.foreground)
                        .child("换模型重答"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child("选择重新生成的模型"),
                ),
        )
        .map(|this| {
            if has_models {
                this.child(
                    div()
                        .max_h(px(300.))
                        .child(v_flex().gap_px().children(rows).overflow_y_scrollbar()),
                )
            } else {
                this.child(
                    v_flex()
                        .items_center()
                        .justify_center()
                        .py_6()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child("没有可用的模型渠道"),
                )
            }
        })
}

fn render_message_metrics(msg: &ChatMessage, model_label: &str, p: &Palette) -> impl IntoElement {
    let (cost_usd, cost_cny) = crate::models_dev::calculate_cost(
        if !msg.model.is_empty() { &msg.model } else { model_label },
        msg.prompt_tokens,
        msg.completion_tokens,
        0,
    );

    let total_tokens = msg.prompt_tokens + msg.completion_tokens;
    let label = if cost_usd > 0.00001 {
        format!(
            "{} tokens · ≈${:.4} · {:.1} tok/s · {:.1}s",
            msg.completion_tokens,
            cost_usd,
            msg.speed_tps,
            msg.latency_ms as f64 / 1000.0
        )
    } else {
        format!(
            "{} tokens · {:.1} tok/s · {:.1}s",
            msg.completion_tokens,
            msg.speed_tps,
            msg.latency_ms as f64 / 1000.0
        )
    };

    let input_t = msg.prompt_tokens;
    let output_t = msg.completion_tokens;
    let speed = msg.speed_tps;
    let sec = msg.latency_ms as f64 / 1000.0;
    let reasoning_len = msg.reasoning_content.as_ref().map(|r| r.chars().count()).unwrap_or(0);
    let reasoning_t_est = if reasoning_len > 0 { reasoning_len / 2 } else { 0 };

    h_flex()
        .id(SharedString::from(format!("metrics-{}", msg.id)))
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .cursor_pointer()
        .hover(|s| s.bg(p.muted))
        .tooltip(move |window, cx| {
            let mut text = String::from("Token 与费用明细\n");
            text.push_str(&format!("• 输入 (Input):    {} tokens\n", input_t));
            text.push_str(&format!("• 输出 (Output):   {} tokens\n", output_t));
            if reasoning_t_est > 0 {
                text.push_str(&format!("  └ 思考生成:      ≈{} tokens\n", reasoning_t_est));
            }
            let tot = if total_tokens > 0 { total_tokens } else { output_t };
            text.push_str(&format!("• 总计 (Total):    {} tokens\n", tot));
            if cost_usd > 0.00001 {
                text.push_str(&format!("• 预估费用:        ${:.4} (≈ ¥{:.3})\n", cost_usd, cost_cny));
            }
            text.push_str(&format!("• 速率与耗时:       {:.1} tok/s · {:.1}s", speed, sec));
            Tooltip::new(text).build(window, cx)
        })
        .child(label)
}

fn render_code_block_actions(block: &CodeBlock, cx: &mut App) -> impl IntoElement + use<> {
    let code = block.code();
    let muted = cx.theme().muted_foreground;

    h_flex()
        .gap_1()
        .pl_2()
        .when_some(block.lang().filter(|l| !l.is_empty()), |this, lang| {
            this.child(div().text_xs().text_color(muted).child(lang))
        })
        .child(
            Button::new("copy")
                .ghost()
                .xsmall()
                .icon(IconName::Copy)
                .tooltip("复制代码")
                .on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                    window.push_notification(Notification::success("代码已复制"), cx);
                }),
        )
}

// ================= 空状态 =================

fn render_empty_state(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let has_providers = !state.config.providers.is_empty();
    let subtitle = if has_providers {
        format!(
            "{} · {}",
            state.config.get_active_provider_name(),
            model_picker::current_model_label(state)
        )
    } else {
        "还没有配置模型渠道，先添加一个吧".to_string()
    };

    v_flex()
        .id("empty-state")
        .size_full()
        .items_center()
        .justify_center()
        .gap_8()
        .px_6()
        .overflow_y_scroll()
        .child(
            v_flex()
                .items_center()
                .gap_3()
                .child(icon_tile(IconName::Sparkles, px(48.), p.primary, p.primary_foreground))
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("今天想聊点什么？"),
                )
                .child(div().text_sm().text_color(p.muted_foreground).child(subtitle)),
        )
        .when(has_providers, |this| {
            let presets: Vec<_> = state
                .prompts
                .presets
                .iter()
                .filter(|preset| !preset.system_prompt.trim().is_empty())
                .cloned()
                .collect();
            if presets.is_empty() {
                this
            } else {
                this.child(
                    h_flex()
                        .w_full()
                        .max_w(px(600.))
                        .flex_wrap()
                        .gap_2()
                        .justify_center()
                        .children(presets.into_iter().enumerate().map(|(ix, preset)| {
                            let id = preset.id;
                            Button::new(("empty-preset", ix))
                                .outline()
                                .small()
                                .label(format!("{} {}", preset.icon, preset.name))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.create_session_from_preset(&id, window, cx)
                                }))
                        })),
                )
            }
        })
        .child(if has_providers {
            div()
                .w_full()
                .max_w(px(600.))
                .grid()
                .grid_cols(2)
                .gap_3()
                .child(render_suggestion(
                    "suggest-translate",
                    IconName::Languages,
                    "翻译",
                    "中英互译，保留原文格式",
                    p,
                    cx.listener(|this, _, window, cx| {
                        this.fill_chat_input(
                            "请把下面的内容翻译成英文（如果原文是英文则翻译成中文），保留原有格式：\n",
                            window,
                            cx,
                        )
                    }),
                ))
                .child(render_suggestion(
                    "suggest-polish",
                    IconName::PencilLine,
                    "润色文字",
                    "让表达更通顺、更专业",
                    p,
                    cx.listener(|this, _, window, cx| {
                        this.fill_chat_input(
                            "请帮我润色下面这段文字，使表达更通顺专业，并说明主要改动：\n",
                            window,
                            cx,
                        )
                    }),
                ))
                .child(render_suggestion(
                    "suggest-summary",
                    IconName::FileText,
                    "总结要点",
                    "提炼长文的核心内容",
                    p,
                    cx.listener(|this, _, window, cx| {
                        this.fill_chat_input("请用要点的形式总结下面的内容：\n", window, cx)
                    }),
                ))
                .child(render_suggestion(
                    "suggest-explain",
                    IconName::Code,
                    "解释一段代码",
                    "粘贴代码，让 AI 逐段讲解",
                    p,
                    cx.listener(|this, _, window, cx| this.fill_chat_input("请逐段解释下面这段代码：\n", window, cx)),
                ))
                .into_any_element()
        } else {
            Button::new("empty-add-provider")
                .primary()
                .icon(IconName::Plus)
                .label("添加模型渠道")
                .on_click(cx.listener(|this, _, window, cx| this.open_providers_settings(window, cx)))
                .into_any_element()
        })
}

fn render_suggestion(
    id: &'static str,
    icon: IconName,
    title: &'static str,
    description: &'static str,
    p: &Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    h_flex()
        .id(id)
        .items_start()
        .gap_3()
        .p_3()
        .rounded_xl()
        .border_1()
        .border_color(p.border)
        .bg(p.background)
        .cursor_pointer()
        .hover(|s| s.bg(p.muted))
        .on_click(on_click)
        .child(icon_tile(icon, px(32.), p.muted, p.foreground))
        .child(
            v_flex()
                .min_w_0()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_xs().text_color(p.muted_foreground).child(description)),
        )
}

// ================= 工具授权卡片 =================

fn render_tool_permission(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let tool_name = state.pending_tool_name.clone().unwrap_or_default();
    let tool_cmd = state.pending_tool_cmd.clone().unwrap_or_default();
    let exec_tool = tool_name.clone();
    let exec_arg = tool_cmd.clone();
    let mono_font = cx.theme().mono_font_family.clone();

    v_flex()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .gap_3()
        .p_4()
        .rounded_xl()
        .border_1()
        .border_color(p.warning.opacity(0.45))
        .bg(p.warning.opacity(if p.is_dark { 0.12 } else { 0.07 }))
        .child(
            h_flex()
                .gap_2()
                .child(Icon::new(IconName::ShieldAlert).size(px(16.)).text_color(p.warning))
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("需要你的授权"))
                .child(Tag::warning().small().child(tool_name)),
        )
        .child(
            div()
                .text_xs()
                .text_color(p.muted_foreground)
                .child("即将在本机执行下面的命令，请确认内容安全："),
        )
        .child(
            div()
                .w_full()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(p.background)
                .border_1()
                .border_color(p.border)
                .font_family(mono_font)
                .text_xs()
                .child(tool_cmd),
        )
        .child(
            h_flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("deny-tool")
                        .ghost()
                        .small()
                        .label("拒绝")
                        .on_click(cx.listener(|this, _, _, cx| this.deny_pending_tool(cx))),
                )
                .child(
                    Button::new("allow-tool")
                        .primary()
                        .small()
                        .icon(IconName::Check)
                        .label("允许执行一次")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.execute_agent_tool(&exec_tool, &exec_arg, cx);
                        })),
                ),
        )
}

// ================= 输入框 =================

fn render_pending_attachments(attachments: &[Attachment], p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    h_flex()
        .gap_2()
        .px_3()
        .pt_2p5()
        .pb_1()
        .flex_wrap()
        .children(attachments.iter().map(|att| {
            let id = att.id.clone();
            let name = att.name.clone();
            let abs_path = att.absolute_path();
            let size_kb = (att.size as f32 / 1024.0).max(0.1);
            let size_label = if size_kb > 1024.0 {
                format!("{:.1} MB", size_kb / 1024.0)
            } else {
                format!("{:.0} KB", size_kb)
            };
            let (icon, badge_color, type_label) = attachment_badge(att, p);

            h_flex()
                .gap_2()
                .items_center()
                .p_1p5()
                .rounded_md()
                .bg(p.muted.opacity(0.7))
                .border_1()
                .border_color(p.border)
                .shadow_xs()
                .child(
                    div()
                        .size(px(38.))
                        .rounded_sm()
                        .overflow_hidden()
                        .bg(p.background)
                        .map(|this| {
                            if att.is_image() {
                                this.child(img(abs_path).size_full().object_fit(ObjectFit::Cover))
                            } else {
                                this.flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(badge_color.opacity(0.12))
                                    .child(Icon::new(icon).size(px(20.)).text_color(badge_color))
                            }
                        }),
                )
                .child(
                    v_flex()
                        .min_w(px(70.))
                        .max_w(px(160.))
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.foreground)
                                .child(name),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child(format!("{size_label} · {type_label}")),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("del-pending-{}", id)))
                        .ghost()
                        .xsmall()
                        .icon(IconName::X)
                        .tooltip("移除")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_pending_attachment(&id, cx);
                        })),
                )
        }))
}

fn is_current_model_vision_capable(state: &AppState) -> bool {
    let session = state.storage.get_active_session();
    let provider_id = session
        .map(|s| s.provider_id.as_str())
        .filter(|id| !id.is_empty())
        .unwrap_or(&state.config.active_provider_id);
    let default_model = state.config.default_model_selection().1;
    let model_id = session
        .map(|s| s.model.as_str())
        .filter(|id| !id.is_empty() && *id != "default")
        .unwrap_or(&default_model);
    let model_config = state
        .config
        .providers
        .iter()
        .find(|p| p.id == provider_id)
        .or_else(|| state.config.get_active_provider())
        .and_then(|p| p.models.iter().find(|m| m.id == model_id));

    if let Some(model) = model_config {
        model
            .effective_capabilities()
            .contains(&crate::model_info::Capability::Vision)
    } else {
        crate::model_info::detect(model_id, "")
            .capabilities
            .contains(&crate::model_info::Capability::Vision)
    }
}

fn render_composer(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let is_streaming = state.is_streaming;
    let draft = state.chat_input.read(cx).value().to_string();
    let has_attachments = !state.pending_attachments.is_empty();
    let has_images = state.pending_attachments.iter().any(|a| a.is_image());
    let is_vision = is_current_model_vision_capable(state);
    let input_empty = draft.trim().is_empty() && !has_attachments;
    let slash = slash_matches(state, &draft);
    let quote = state.pending_quote.clone();

    v_flex()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .gap_2()
        .when(!slash.is_empty(), |this| this.child(render_slash_menu(slash, p, cx)))
        .when_some(quote, |this, quote| this.child(render_quote_chip(&quote, p, cx)))
        .child(
            v_flex()
                .w_full()
                .rounded(px(16.))
                .border_1()
                .border_color(p.border)
                .bg(p.background)
                .shadow_sm()
                .when(has_attachments, |this| {
                    this.child(render_pending_attachments(&state.pending_attachments, p, cx))
                })
                .when(has_images && !is_vision, |this| {
                    this.child(
                        h_flex()
                            .gap_1p5()
                            .px_3()
                            .py_1p5()
                            .mx_2()
                            .mb_1()
                            .rounded_md()
                            .bg(p.warning.opacity(0.12))
                            .items_center()
                            .child(Icon::new(IconName::Info).size(px(13.)).text_color(p.warning))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.warning)
                                    .child("提示：当前选中的模型未标注视觉能力，建议切换为支持视觉的多模态模型"),
                            ),
                    )
                })
                .child({
                    let app = cx.entity();
                    div().px_2().pt_2().child(
                        Textarea::new(&state.chat_input)
                            .appearance(false)
                            .bordered(false)
                            // Ctrl+V 和右键「粘贴」都走输入框的 Paste 动作，在这里统一接住：
                            // 复制的文件和截图变成附件，文字照常插入。
                            // 不能用 cx.listener：它要求闭包返回 ()，而 on_paste 要返回 bool。
                            .on_paste(move |item: &ClipboardItem, _window: &mut Window, cx: &mut App| {
                                app.update(cx, |this, cx| this.handle_clipboard_paste(item, cx))
                            }),
                    )
                })
                .child(
                    h_flex()
                        .justify_between()
                        .gap_2()
                        .px_2()
                        .pb_2()
                        .child(
                            h_flex()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    Button::new("composer-pick-attachment")
                                        .ghost()
                                        .small()
                                        .icon(IconName::Paperclip)
                                        .tooltip("添加附件 (图片/文档/表格/代码)")
                                        .on_click(cx.listener(|this, _, _, cx| this.pick_attachments(cx))),
                                )
                                .child(model_picker::render_model_picker(state, p, cx))
                                .child(super::params::render_params_button(cx))
                                .child(super::params::render_compare_button(state, cx)),
                        )
                        .child(if is_streaming {
                            Button::new("stop")
                                .primary()
                                .small()
                                .rounded(px(999.))
                                .icon(IconName::Square)
                                .tooltip("停止生成")
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_streaming(cx)))
                        } else {
                            Button::new("send")
                                .primary()
                                .small()
                                .rounded(px(999.))
                                .icon(IconName::ArrowUp)
                                .tooltip(if state.compare_selection.is_empty() {
                                    "发送 (Enter)"
                                } else {
                                    "对比发送 (Enter)"
                                })
                                .disabled(input_empty)
                                .on_click(cx.listener(|this, _, window, cx| this.send_message(window, cx)))
                        }),
                ),
        )
}

fn render_import_banner(p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    h_flex()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(p.warning.opacity(0.45))
        .bg(p.warning.opacity(if p.is_dark { 0.12 } else { 0.07 }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .child("已读取 JSON 备份。恢复会覆盖本机会话、提示词和渠道配置，不会写入 API Key。"),
        )
        .child(
            Button::new("cancel-import")
                .ghost()
                .small()
                .label("取消")
                .on_click(cx.listener(|this, _, _, cx| this.cancel_import(cx))),
        )
        .child(
            Button::new("confirm-import")
                .primary()
                .small()
                .label("恢复")
                .on_click(cx.listener(|this, _, _, cx| this.confirm_import(cx))),
        )
}

fn render_compare_notice(p: &Palette) -> impl IntoElement {
    div()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(p.muted)
        .text_sm()
        .text_color(p.muted_foreground)
        .child("请先采用一条对比回答，再继续对话")
}

fn render_quote_chip(quote: &str, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    h_flex()
        .gap_2()
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(p.muted)
        .child(Icon::new(IconName::Quote).size(px(14.)).text_color(p.muted_foreground))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(p.muted_foreground)
                .child(preview(quote, 120)),
        )
        .child(
            Button::new("clear-quote")
                .ghost()
                .xsmall()
                .icon(IconName::X)
                .tooltip("取消引用")
                .on_click(cx.listener(|this, _, _, cx| this.clear_quote(cx))),
        )
}

fn slash_matches(state: &AppState, draft: &str) -> Vec<(String, String, String)> {
    let token = draft.trim();
    if !token.starts_with('/') || token.contains(char::is_whitespace) {
        return Vec::new();
    }
    let query = token.trim_start_matches('/').to_lowercase();
    state
        .prompts
        .templates
        .iter()
        .filter(|template| query.is_empty() || template.name.to_lowercase().contains(&query))
        .take(6)
        .map(|template| (template.id.clone(), template.name.clone(), preview(&template.body, 48)))
        .collect()
}

fn render_slash_menu(
    items: Vec<(String, String, String)>,
    p: &Palette,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(p.border)
        .bg(p.background)
        .shadow_sm()
        .overflow_hidden()
        .child(
            div()
                .px_3()
                .py_1p5()
                .text_xs()
                .text_color(p.muted_foreground)
                .child("提示词模板 · 回车或点击插入"),
        )
        .children(items.into_iter().enumerate().map(|(ix, (id, name, body))| {
            h_flex()
                .id(SharedString::from(format!("slash-{ix}")))
                .gap_2()
                .px_3()
                .py_2()
                .cursor_pointer()
                .hover(|style| style.bg(p.muted))
                .on_click(cx.listener(move |this, _, window, cx| this.insert_template(&id, window, cx)))
                .child(
                    Icon::new(IconName::BookOpen)
                        .size(px(14.))
                        .text_color(p.muted_foreground),
                )
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(name))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child(body),
                )
        }))
}

fn render_variants(
    app: &Entity<AppState>,
    ix: usize,
    msg: &ChatMessage,
    p: &Palette,
    cx: &mut App,
) -> impl IntoElement {
    let (owners, expanded_ids) = {
        let state = app.read(cx);
        let owners: Vec<Option<crate::config::ModelConfig>> = msg
            .variants
            .iter()
            .map(|variant| {
                state
                    .config
                    .providers
                    .iter()
                    .find(|provider| provider.id == variant.provider_id)
                    .and_then(|provider| provider.models.iter().find(|model| model.id == variant.model).cloned())
            })
            .collect();
        (owners, state.expanded_reasoning.clone())
    };

    let columns: Vec<_> =
        msg.variants
            .iter()
            .enumerate()
            .map(|(variant_ix, variant)| {
                let owner = owners.get(variant_ix).cloned().flatten();
                let label = owner
                    .as_ref()
                    .map(|model| model.name.clone())
                    .unwrap_or_else(|| variant.model.clone());
                let avatar = match &owner {
                    Some(model) => model_avatar(model, px(20.), p),
                    None => model_id_avatar(&variant.model, px(20.), p),
                };
                let adopt_app = app.clone();
                let toggle_app = app.clone();
                let message_id = msg.id.clone();
                let variant_id = variant.id.clone();

                let variant_has_content = !variant.content.trim().is_empty();
                let variant_reasoning = variant.reasoning_content.clone().filter(|text| !text.trim().is_empty());
                let variant_thinking = variant.is_streaming && !variant_has_content;
                let variant_open = expanded_ids.contains(&variant.id) || variant_thinking;
                let show_thinking = variant_reasoning.is_some() || variant_thinking;

                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(p.border)
                    .bg(p.muted.opacity(if p.is_dark { 0.25 } else { 0.12 }))
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_2()
                            .child(
                                h_flex().items_center().gap_2().min_w_0().child(avatar).child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .truncate()
                                        .child(label),
                                ),
                            )
                            .child({
                                let adopt_var_id = variant_id.clone();
                                Button::new(SharedString::from(format!("adopt-{ix}-{variant_ix}")))
                                    .xsmall()
                                    .primary()
                                    .label("采用")
                                    .disabled(variant.is_streaming)
                                    .on_click(move |_, _, cx| {
                                        adopt_app
                                            .update(cx, |this, cx| this.adopt_variant(&message_id, &adopt_var_id, cx));
                                    })
                            }),
                    )
                    .when(show_thinking, |this| {
                        let var_id = variant_id.clone();
                        this.child(
                            v_flex()
                                .w_full()
                                .rounded_md()
                                .border_1()
                                .border_color(p.border)
                                .overflow_hidden()
                                .bg(p.background)
                                .child(
                                    h_flex()
                                        .id(SharedString::from(format!("var-think-toggle-{}", variant.id)))
                                        .gap_2()
                                        .px_2p5()
                                        .py_1p5()
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(p.muted_foreground)
                                        .hover(|s| s.bg(p.muted))
                                        .on_click(move |_, _, cx| {
                                            toggle_app.update(cx, |this, cx| this.toggle_reasoning(&var_id, cx));
                                        })
                                        .child(Icon::new(IconName::Brain).size(px(14.)))
                                        .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(
                                            if variant_thinking {
                                                "正在思考…"
                                            } else {
                                                "思考过程"
                                            },
                                        ))
                                        .child(
                                            Icon::new(if variant_open {
                                                IconName::ChevronDown
                                            } else {
                                                IconName::ChevronRight
                                            })
                                            .size(px(14.)),
                                        ),
                                )
                                .when(variant_open, |this| {
                                    this.when_some(variant_reasoning, |this, reasoning| {
                                        this.child(
                                            div()
                                                .px_2p5()
                                                .pb_2p5()
                                                .pt_1()
                                                .text_xs()
                                                .line_height(relative(1.7))
                                                .text_color(p.muted_foreground)
                                                .child(reasoning),
                                        )
                                    })
                                }),
                        )
                    })
                    .when(variant.is_streaming && !variant_has_content && !show_thinking, |this| {
                        this.child(
                            h_flex()
                                .gap_2()
                                .text_sm()
                                .text_color(p.muted_foreground)
                                .child(Spinner::new().small())
                                .child("正在生成…"),
                        )
                    })
                    .when(variant_has_content, |this| {
                        this.child(
                            div().text_sm().child(
                                markdown_view(format!("var-{}", variant.id), variant.content.clone())
                                    .stream_fade(variant.is_streaming),
                            ),
                        )
                    })
                    .when_some(variant.error.clone(), |this, err| {
                        this.child(div().text_sm().text_color(p.danger).child(err))
                    })
                    .when(variant.speed_tps > 0.0 || variant.completion_tokens > 0, |this| {
                        this.child(div().pt_1().text_xs().text_color(p.muted_foreground).child(format!(
                            "{} tokens · {:.1} tok/s · {:.1}s",
                            variant.completion_tokens,
                            variant.speed_tps,
                            variant.latency_ms as f32 / 1000.0
                        )))
                    })
            })
            .collect();

    v_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(p.muted_foreground)
                .child("模型对比输出中，采用一条后继续对话："),
        )
        .child(h_flex().w_full().items_start().gap_3().children(columns))
}

fn markdown_view(id: impl Into<SharedString>, source: impl Into<SharedString>) -> TextView {
    let source_str: SharedString = source.into();
    let cleaned = super::markdown_image::normalize_markdown_image_urls(&source_str);
    TextView::markdown(id.into(), cleaned)
        .selectable(true)
        .plugin(super::markdown_image::ChatImagePlugin)
}

fn preview(text: &str, limit: usize) -> String {
    let flat = text.trim().replace('\n', " ");
    let mut out: String = flat.chars().take(limit).collect();
    if flat.chars().count() > limit {
        out.push('…');
    }
    out
}
