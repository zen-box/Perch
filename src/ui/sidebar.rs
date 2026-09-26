//! 对话侧边栏：会话列表、搜索与筛选、按日期分组、右键菜单。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::{Palette, SIDEBAR_WIDTH, dialogs};
use crate::app::AppState;
use crate::i18n::{AppLanguage, Key, tr, tr_args};
use crate::model::{DEFAULT_SESSION_FOLDER, DEFAULT_SESSION_TITLE};

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
        .filter(|folder| !folder.is_empty() && folder != DEFAULT_SESSION_FOLDER)
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
        rows.push(sidebar_group(tr(lang, Key::SidebarPinned), true, p));
        for session in pinned {
            rows.push(session_row(&app, row_ix, session, session.id == active_id, &folders, p, cx).into_any_element());
            row_ix += 1;
        }
    }
    let mut current_group = "";
    for session in rest {
        let group = date_group_label(&session.created_at, lang);
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
                .child(tr(lang, Key::SidebarNoMatch))
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
                        .label(tr(lang, Key::NewChat))
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
                                tr(lang, Key::SidebarAll),
                                folder_filter.is_empty() && !favorites_only,
                                cx.listener(|this, _, _, cx| {
                                    this.clear_session_filters(cx);
                                }),
                            ))
                            .child(filter_chip(
                                "folder-fav",
                                tr(lang, Key::SidebarFavorite),
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
    let lang = app.read(cx).language();
    let session_id = session.id.clone();
    // 新建后还没用过、也没被自动命名过的会话，标题仍是数据层的占位值
    // （`DEFAULT_SESSION_TITLE`）。这类占位标题在显示层换成当前语言的「新对话」，
    // 而数据层保持固定值——否则标题会随界面语言变化，搜索和比较都会跟着变。
    let title = if session.title_auto && session.title == DEFAULT_SESSION_TITLE {
        tr(lang, Key::NewChat).to_string()
    } else {
        session.title.clone()
    };
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
                    if pinned {
                        tr(lang, Key::SidebarUnpin)
                    } else {
                        tr(lang, Key::SidebarPinned)
                    },
                    {
                        let id = session_id.clone();
                        cx.listener(move |this, _, _, cx| this.toggle_session_pin(&id, cx))
                    },
                ))
                .child(session_icon_button(
                    ("fav-session", ix),
                    if favorite { IconName::StarOff } else { IconName::Star },
                    if favorite {
                        tr(lang, Key::SidebarUnfavorite)
                    } else {
                        tr(lang, Key::SidebarFavorite)
                    },
                    {
                        let id = session_id.clone();
                        cx.listener(move |this, _, _, cx| this.toggle_session_favorite(&id, cx))
                    },
                ))
                .child(session_icon_button(
                    ("rename-session", ix),
                    IconName::Pencil,
                    tr(lang, Key::Rename),
                    {
                        let rename_id = session_id.clone();
                        let rename_title = title.clone();
                        cx.listener(move |this, _, window, cx| {
                            dialogs::open_rename_dialog(cx.entity(), this.rename_input.clone(), window, cx);
                            this.start_rename_session(rename_id.clone(), &rename_title, window, cx);
                        })
                    },
                ))
                .child(session_icon_button(
                    ("delete-session", ix),
                    IconName::Trash,
                    tr(lang, Key::Delete),
                    {
                        let delete_id = session_id.clone();
                        let delete_title = title.clone();
                        cx.listener(move |_, _, window, cx| {
                            dialogs::confirm_delete_session(cx.entity(), delete_id.clone(), &delete_title, window, cx);
                        })
                    },
                )),
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
                    PopupMenuItem::new(if pinned {
                        tr(lang, Key::SidebarUnpin)
                    } else {
                        tr(lang, Key::SidebarPinned)
                    })
                    .icon(IconName::Pin)
                    .on_click(move |_, _, cx| {
                        pin_app.update(cx, |this, cx| this.toggle_session_pin(&pin_id, cx));
                    }),
                )
                .item(
                    PopupMenuItem::new(if favorite {
                        tr(lang, Key::SidebarUnfavorite)
                    } else {
                        tr(lang, Key::SidebarFavorite)
                    })
                    .icon(IconName::Star)
                    .on_click(move |_, _, cx| {
                        fav_app.update(cx, |this, cx| this.toggle_session_favorite(&fav_id, cx));
                    }),
                )
                .separator();
            if current_folder != DEFAULT_SESSION_FOLDER {
                let app = menu_app.clone();
                let id = menu_id.clone();
                menu = menu.item(
                    PopupMenuItem::new(tr(lang, Key::RemoveFromFolder))
                        .icon(IconName::Folder)
                        .on_click(move |_, _, cx| {
                            app.update(cx, |this, cx| this.set_session_folder(&id, DEFAULT_SESSION_FOLDER, cx));
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
                    PopupMenuItem::new(tr_args(lang, Key::MoveToFolder, &[folder_name]))
                        .icon(IconName::Folder)
                        .on_click(move |_, _, cx| {
                            app.update(cx, |this, cx| this.set_session_folder(&id, &target, cx));
                        }),
                );
            }
            menu.item(
                PopupMenuItem::new(tr(lang, Key::NewFolder))
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
                PopupMenuItem::new(tr(lang, Key::Rename))
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
                PopupMenuItem::new(tr(lang, Key::Delete))
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

fn date_group_label(created_at: &str, lang: AppLanguage) -> &'static str {
    let today = chrono::Local::now().date_naive();
    match chrono::NaiveDate::parse_from_str(created_at.get(..10).unwrap_or(""), "%Y-%m-%d") {
        Ok(date) => match (today - date).num_days() {
            ..=0 => tr(lang, Key::DateToday),
            1 => tr(lang, Key::DateYesterday),
            2..=6 => tr(lang, Key::DateLast7Days),
            7..=29 => tr(lang, Key::DateLast30Days),
            _ => tr(lang, Key::DateEarlier),
        },
        Err(_) => tr(lang, Key::DateEarlier),
    }
}
