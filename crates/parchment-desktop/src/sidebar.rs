//! Left navigation sidebar matching `ParchmentSidebar.kt`:
//! - Search bar (`Input`) + `+` New chat / New assistant button
//! - Segmented pill switcher: `Assistants` | `Chats` | `Pinned`
//! - Smoothly scrollable lists with rubric mark on active selection, pin/title-regen/delete actions,
//!   and combined title + FTS5 full-text search across stored history.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement as _, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle, Window,
    div, px, uniform_list,
};
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{StyledExt as _, h_flex, v_flex};
use parchment_core::model::{AssistantAgent, ChatStub, PinnedMessageStub};
use parchment_core::storage::SearchHit;

use crate::icons::{KissIcon, assistant_icon_from_symbol, kiss_icon};
use crate::strings;
use crate::theme::ParchmentPalette;
use crate::ui_kit::{
    SepiaButtonStyle, field_label, format_compact_timestamp, h_divider, icon_button,
    kiss_bounce_subtle, rubric_mark_overlay, sepia_avatar_circle, sepia_button, sepia_chip,
    streaming_pulse_dot, with_tooltip,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Assistants,
    Chats,
    Pinned,
}

#[derive(Debug, Clone)]
pub enum SidebarAction {
    SelectTab(SidebarTab),
    NewChat,
    NewAssistant,
    SelectAssistant(String),
    EditAssistant(String),
    SelectChat(String),
    TogglePinChat(String),
    RegenerateChatTitle(String),
    DeleteChat(String),
    ClearAllForAssistant,
    SetPinnedScopeAll(bool),
    OpenPinned { chat_id: String, message_id: String },
    UnpinReply { chat_id: String, message_id: String },
}
pub type SidebarActionHandler = Rc<dyn Fn(SidebarAction, &mut Window, &mut App)>;

pub struct SidebarRenderProps<'a> {
    pub palette: &'a ParchmentPalette,
    pub selected_tab: SidebarTab,
    pub search_input: &'a Entity<InputState>,
    pub search_query: &'a str,
    pub assistants: &'a [AssistantAgent],
    pub active_assistant_id: &'a str,
    pub stubs: &'a [ChatStub],
    pub active_chat_id: &'a str,
    pub streaming_chat_ids: &'a [String],
    pub search_hits: &'a [SearchHit],
    pub pinned_stubs: &'a [PinnedMessageStub],
    pub pinned_all_chats: bool,
    pub show_dates: bool,
    pub chats_scroll: &'a UniformListScrollHandle,
    pub on_action: &'a SidebarActionHandler,
}

pub fn render_sidebar(props: SidebarRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let sheet = p.sheet_bg.to_hsla();
    let card = p.card_bg.to_hsla();
    let border = p.subtle_border();
    let ink = p.primary_ink.to_hsla();
    let narration = p.narration_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    let hover_bg = p.hover_tint();

    let active_assistant = props
        .assistants
        .iter()
        .find(|a| a.id == props.active_assistant_id)
        .or_else(|| props.assistants.first());

    let search_placeholder_icon = kiss_icon(KissIcon::Search, 13.0, muted);
    let h_plus = props.on_action.clone();
    let tab_now = props.selected_tab;

    // 1. Search bar + primary `+` button
    let search_row = h_flex()
        .w_full()
        .gap(px(6.0))
        .items_center()
        .child(
            div()
                .flex_1()
                .child(Input::new(props.search_input).prefix(search_placeholder_icon)),
        )
        .child(with_tooltip(
            sepia_button(
                "sidebar-plus",
                "",
                Some(KissIcon::Plus),
                SepiaButtonStyle::Primary,
                true,
                false,
                p,
                move |_, w, cx| {
                    let act = if tab_now == SidebarTab::Assistants {
                        SidebarAction::NewAssistant
                    } else {
                        SidebarAction::NewChat
                    };
                    h_plus(act, w, cx);
                },
            ),
            if tab_now == SidebarTab::Assistants {
                "New assistant"
            } else {
                "New chat"
            },
        ));

    // 2. Segmented Pill Switcher (Assistants | Chats | Pinned)
    let tabs = [
        (SidebarTab::Assistants, strings::SIDEBAR_ASSISTANTS),
        (SidebarTab::Chats, strings::SIDEBAR_CHATS),
        (SidebarTab::Pinned, strings::SIDEBAR_PINNED),
    ];
    let segmented = h_flex()
        .w_full()
        .p(px(2.0))
        .rounded(px(999.0))
        .bg(p.card_bg.with_alpha(0.70))
        .border_1()
        .border_color(p.border.with_alpha(0.65))
        .gap(px(4.0))
        .children(tabs.into_iter().enumerate().map(|(ix, (tab, label))| {
            let is_sel = props.selected_tab == tab;
            let h = props.on_action.clone();
            let el = div()
                .id(("sb-tab", ix))
                .flex_1()
                .h(px(28.0))
                .rounded(px(999.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(if is_sel {
                    sheet
                } else {
                    p.canvas_bg.with_alpha(0.0)
                })
                .hover(move |s| {
                    s.bg(if is_sel { sheet } else { hover_bg })
                        .text_color(accent)
                })
                .text_size(px(13.0))
                .text_color(if is_sel { accent } else { narration })
                .on_click(move |_, w, cx| h(SidebarAction::SelectTab(tab), w, cx))
                .child(label);
            let styled = if is_sel {
                el.font_bold()
            } else {
                el.font_medium()
            };
            kiss_bounce_subtle(("sb-tab-kb", ix), styled).flex_1()
        }));

    // 3. Scrollable Tab Body
    let q_lower = props.search_query.trim().to_ascii_lowercase();
    let list_body: AnyElement = match props.selected_tab {
        SidebarTab::Assistants => {
            let filtered: Vec<&AssistantAgent> = props
                .assistants
                .iter()
                .filter(|a| {
                    q_lower.is_empty()
                        || a.name.to_ascii_lowercase().contains(&q_lower)
                        || a.system_prompt.to_ascii_lowercase().contains(&q_lower)
                })
                .collect();

            let mut list = v_flex()
                .id("sidebar-assistants-scroll")
                .flex_1()
                .w_full()
                .gap(px(6.0))
                .overflow_y_scrollbar();

            if filtered.is_empty() {
                list = list.child(
                    div()
                        .p(px(8.0))
                        .text_size(px(12.5))
                        .text_color(muted)
                        .child("No matching assistants found. Click + to create one."),
                );
            } else {
                for (ix, asst) in filtered.into_iter().enumerate() {
                    let is_sel = asst.id == props.active_assistant_id;
                    let aid = asst.id.clone();
                    let aid_edit = asst.id.clone();
                    let h_sel = props.on_action.clone();
                    let h_edit = props.on_action.clone();
                    let mut row = h_flex()
                        .id(("sb-asst", ix))
                        .relative()
                        .overflow_hidden()
                        .w_full()
                        .px(px(10.0))
                        .py(px(8.0))
                        .rounded(px(8.0))
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .cursor_pointer()
                        .bg(if is_sel {
                            card
                        } else {
                            p.canvas_bg.with_alpha(0.0)
                        })
                        .hover(move |s| s.bg(if is_sel { card } else { hover_bg }))
                        .on_click(move |_, w, cx| {
                            h_sel(SidebarAction::SelectAssistant(aid.clone()), w, cx);
                        });
                    if is_sel {
                        row = row.child(rubric_mark_overlay(8.0, p));
                    }
                    let avatar = sepia_avatar_circle(&asst.icon_emoji, is_sel, 32.0, p);
                    let mut name_el = div()
                        .flex_1()
                        .truncate()
                        .text_size(px(14.0))
                        .text_color(if is_sel { accent } else { ink })
                        .child(asst.name.clone());
                    if is_sel {
                        name_el = name_el.font_semibold();
                    }

                    row = row
                        .child(
                            h_flex()
                                .flex_1()
                                .overflow_hidden()
                                .items_center()
                                .gap(px(10.0))
                                .child(avatar)
                                .child(name_el),
                        )
                        .child(icon_button(
                            ("sb-asst-edit", ix),
                            KissIcon::Settings,
                            false,
                            p,
                            move |_, w, cx| {
                                cx.stop_propagation();
                                h_edit(SidebarAction::EditAssistant(aid_edit.clone()), w, cx);
                            },
                        ));
                    list = list.child(kiss_bounce_subtle(("sb-asst-kb", ix), row));
                }
            }
            list.into_any_element()
        }
        SidebarTab::Chats => {
            let active_asst_name = active_assistant
                .map(|a| a.name.as_str())
                .unwrap_or(strings::DEFAULT_ASSISTANT);
            let hdr_icon = active_assistant
                .map(|a| assistant_icon_from_symbol(&a.icon_emoji))
                .unwrap_or(KissIcon::MessageCircle);
            let fts_chat_ids: Vec<&str> = props
                .search_hits
                .iter()
                .map(|h| h.chat_id.as_str())
                .collect();

            let visible_stubs: Vec<&ChatStub> = props
                .stubs
                .iter()
                .filter(|s| {
                    let matches_asst = props.assistants.len() <= 1
                        || s.assistant_id.is_empty()
                        || s.assistant_id == props.active_assistant_id
                        || !props.assistants.iter().any(|a| a.id == s.assistant_id);
                    let matches_q = q_lower.is_empty()
                        || s.title.to_ascii_lowercase().contains(&q_lower)
                        || fts_chat_ids.contains(&s.id.as_str());
                    matches_asst && matches_q
                })
                .collect();

            let show_clear = visible_stubs.len() > 1
                || visible_stubs
                    .iter()
                    .any(|s| s.message_count > 0 || !s.title.eq_ignore_ascii_case("New Chat"));
            let h_clear = props.on_action.clone();
            let sub_header = h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .px(px(2.0))
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(6.0))
                        .flex_1()
                        .overflow_hidden()
                        .child(kiss_icon(hdr_icon, 14.0, muted))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_semibold()
                                .text_color(muted)
                                .truncate()
                                .child(format!("{active_asst_name} ({})", visible_stubs.len())),
                        ),
                )
                .children(show_clear.then(|| {
                    sepia_button(
                        "sb-clear-all",
                        strings::CLEAR_ALL,
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| h_clear(SidebarAction::ClearAllForAssistant, w, cx),
                    )
                }));

            let list_el: AnyElement = if visible_stubs.is_empty() {
                div()
                    .id("sidebar-chats-scroll")
                    .flex_1()
                    .w_full()
                    .p(px(10.0))
                    .text_size(px(12.5))
                    .text_color(muted)
                    .child(if q_lower.is_empty() {
                        "No chats yet. Click + to begin."
                    } else {
                        "No matching chats found."
                    })
                    .into_any_element()
            } else {
                // Virtualize with `uniform_list` so 300+ chats only lay out & paint visible rows.
                let item_count = visible_stubs.len();
                let rows_data: Vec<ChatStub> = visible_stubs.into_iter().cloned().collect();
                let palette_owned = p.clone();
                let active_id_owned = props.active_chat_id.to_string();
                let streaming_owned = props.streaming_chat_ids.to_vec();
                let show_dates = props.show_dates;
                let has_query = !q_lower.is_empty();
                let search_hits_owned: Vec<SearchHit> = if has_query {
                    props.search_hits.to_vec()
                } else {
                    Vec::new()
                };
                let on_action_owned = props.on_action.clone();
                let row_h = if has_query {
                    px(60.0)
                } else if show_dates {
                    px(48.0)
                } else {
                    px(36.0)
                };

                uniform_list(
                    "sidebar-chats-scroll",
                    item_count,
                    move |range, _window, _cx| {
                        let p = &palette_owned;
                        let card = p.card_bg.to_hsla();
                        let ink = p.primary_ink.to_hsla();
                        let narration = p.narration_ink.to_hsla();
                        let muted = p.muted_ink.to_hsla();
                        let accent = p.accent.to_hsla();
                        let subtle_hover = p.accent.with_alpha(0.06);

                        range
                            .filter_map(|ix| {
                                let stub = rows_data.get(ix)?;
                                let is_active = stub.id == active_id_owned;
                                let is_streaming = streaming_owned.iter().any(|id| id == &stub.id);
                                let cid_sel = stub.id.clone();
                                let cid_pin = stub.id.clone();
                                let cid_regen = stub.id.clone();
                                let cid_del = stub.id.clone();
                                let h_sel = on_action_owned.clone();
                                let h_pin = on_action_owned.clone();
                                let h_regen = on_action_owned.clone();
                                let h_del = on_action_owned.clone();
                                let grp = SharedString::from(format!("sb-chat-grp-{ix}"));

                                let date_label = if show_dates && stub.updated_at > 0 {
                                    format_compact_timestamp(stub.updated_at)
                                } else {
                                    String::new()
                                };

                                let fts_snippet = if has_query {
                                    search_hits_owned
                                        .iter()
                                        .find(|h| h.chat_id == stub.id)
                                        .map(|h| h.snippet.clone())
                                } else {
                                    None
                                };

                                let mut row = h_flex()
                                    .id(("sb-chat", ix))
                                    .group(grp.clone())
                                    .relative()
                                    .overflow_hidden()
                                    .w_full()
                                    .h(row_h)
                                    .px(px(10.0))
                                    .py(px(5.0))
                                    .rounded(px(8.0))
                                    .items_center()
                                    .justify_between()
                                    .gap(px(4.0))
                                    .cursor_pointer()
                                    .bg(if is_active {
                                        card
                                    } else {
                                        p.canvas_bg.with_alpha(0.0)
                                    })
                                    .hover(move |s| {
                                        s.bg(if is_active { card } else { subtle_hover })
                                    })
                                    .on_click(move |_, w, cx| {
                                        h_sel(SidebarAction::SelectChat(cid_sel.clone()), w, cx);
                                    });
                                if is_active {
                                    row = row.child(rubric_mark_overlay(9.0, p));
                                }

                                let mut title_el = div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(13.5))
                                    .text_color(ink)
                                    .child(stub.title.clone());
                                if is_active {
                                    title_el = title_el.font_medium();
                                }

                                let title_col = v_flex()
                                    .flex_1()
                                    .overflow_hidden()
                                    .gap(px(2.0))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap(px(5.0))
                                            .children(is_streaming.then(|| {
                                                streaming_pulse_dot(("sb-chat-pulse", ix), accent)
                                            }))
                                            .children(
                                                stub.is_pinned.then(|| {
                                                    kiss_icon(KissIcon::Pin, 11.0, accent)
                                                }),
                                            )
                                            .child(title_el),
                                    )
                                    .children((!date_label.is_empty()).then(|| {
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(muted)
                                            .truncate()
                                            .child(date_label)
                                    }))
                                    .children(fts_snippet.map(|snip| {
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(narration)
                                            .truncate()
                                            .child(snip)
                                    }));

                                let row_actions = h_flex()
                                    .items_center()
                                    .gap(px(2.0))
                                    .flex_shrink_0()
                                    .opacity(0.0)
                                    .group_hover(grp, |s| s.opacity(1.0))
                                    .child(with_tooltip(
                                        icon_button(
                                            ("sb-chat-pin", ix),
                                            KissIcon::Pin,
                                            stub.is_pinned,
                                            p,
                                            move |_, w, cx| {
                                                cx.stop_propagation();
                                                h_pin(
                                                    SidebarAction::TogglePinChat(cid_pin.clone()),
                                                    w,
                                                    cx,
                                                );
                                            },
                                        ),
                                        if stub.is_pinned {
                                            "Unpin chat"
                                        } else {
                                            "Pin chat"
                                        },
                                    ))
                                    .children((stub.message_count > 0).then(|| {
                                        with_tooltip(
                                            icon_button(
                                                ("sb-chat-regen", ix),
                                                KissIcon::Sparkles,
                                                false,
                                                p,
                                                move |_, w, cx| {
                                                    cx.stop_propagation();
                                                    h_regen(
                                                        SidebarAction::RegenerateChatTitle(
                                                            cid_regen.clone(),
                                                        ),
                                                        w,
                                                        cx,
                                                    );
                                                },
                                            ),
                                            "Regenerate title",
                                        )
                                    }))
                                    .child(with_tooltip(
                                        icon_button(
                                            ("sb-chat-del", ix),
                                            KissIcon::Trash,
                                            false,
                                            p,
                                            move |_, w, cx| {
                                                cx.stop_propagation();
                                                h_del(
                                                    SidebarAction::DeleteChat(cid_del.clone()),
                                                    w,
                                                    cx,
                                                );
                                            },
                                        ),
                                        "Delete chat",
                                    ));

                                Some(div().w_full().pb(px(2.0)).child(kiss_bounce_subtle(
                                    ("sb-chat-kb", ix),
                                    row.child(title_col).child(row_actions),
                                )))
                            })
                            .collect()
                    },
                )
                .track_scroll(props.chats_scroll)
                .flex_1()
                .w_full()
                .into_any_element()
            };

            v_flex()
                .flex_1()
                .w_full()
                .gap(px(6.0))
                .overflow_hidden()
                .child(sub_header)
                .child(list_el)
                .into_any_element()
        }
        SidebarTab::Pinned => {
            let pinned_chats: Vec<&ChatStub> = props
                .stubs
                .iter()
                .filter(|s| {
                    s.is_pinned
                        && (q_lower.is_empty() || s.title.to_ascii_lowercase().contains(&q_lower))
                })
                .collect();

            let h_this = props.on_action.clone();
            let h_all = props.on_action.clone();
            let scope_row = h_flex()
                .gap(px(6.0))
                .child(sepia_chip(
                    "sb-pin-this",
                    "This chat",
                    None,
                    !props.pinned_all_chats,
                    p,
                    move |_, w, cx| h_this(SidebarAction::SetPinnedScopeAll(false), w, cx),
                ))
                .child(sepia_chip(
                    "sb-pin-all",
                    "All chats",
                    None,
                    props.pinned_all_chats,
                    p,
                    move |_, w, cx| h_all(SidebarAction::SetPinnedScopeAll(true), w, cx),
                ));

            let filtered: Vec<&PinnedMessageStub> = props
                .pinned_stubs
                .iter()
                .filter(|pin| {
                    let scope_ok = props.pinned_all_chats || pin.chat_id == props.active_chat_id;
                    let q_ok = q_lower.is_empty()
                        || pin.chat_title.to_ascii_lowercase().contains(&q_lower)
                        || pin.preview.to_ascii_lowercase().contains(&q_lower);
                    scope_ok && q_ok
                })
                .collect();

            let mut list = v_flex()
                .id("sidebar-pinned-scroll")
                .flex_1()
                .w_full()
                .gap(px(6.0))
                .overflow_y_scrollbar();

            if !pinned_chats.is_empty() {
                list = list.child(field_label(
                    &format!("Pinned chats ({})", pinned_chats.len()),
                    p,
                ));
                for (ix, stub) in pinned_chats.into_iter().enumerate() {
                    let is_active = stub.id == props.active_chat_id;
                    let cid_sel = stub.id.clone();
                    let cid_unpin = stub.id.clone();
                    let h_sel = props.on_action.clone();
                    let h_unpin = props.on_action.clone();
                    let active_bg = p.active_tint();
                    let mut row = h_flex()
                        .id(("sb-pin-chat", ix))
                        .relative()
                        .overflow_hidden()
                        .w_full()
                        .pl(px(10.0))
                        .pr(px(4.0))
                        .py(px(7.0))
                        .rounded(px(8.0))
                        .bg(if is_active {
                            card
                        } else {
                            p.card_bg.with_alpha(0.65)
                        })
                        .border_1()
                        .border_color(if is_active { accent } else { border })
                        .items_center()
                        .gap(px(6.0))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .active(move |s| s.bg(active_bg))
                        .on_click(move |_, w, cx| {
                            h_sel(SidebarAction::SelectChat(cid_sel.clone()), w, cx)
                        });
                    if is_active {
                        row = row.child(rubric_mark_overlay(7.0, p));
                    }
                    list = list.child(
                        row.child(kiss_icon(KissIcon::Pin, 12.0, accent))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(13.0))
                                            .font_medium()
                                            .text_color(if is_active { accent } else { ink })
                                            .child(stub.title.clone()),
                                    )
                                    .children((props.show_dates && stub.updated_at > 0).then(
                                        || {
                                            div()
                                                .text_size(px(10.5))
                                                .text_color(muted)
                                                .child(format_compact_timestamp(stub.updated_at))
                                        },
                                    )),
                            )
                            .child(with_tooltip(
                                icon_button(
                                    ("sb-unpin-chat", ix),
                                    KissIcon::Pin,
                                    true,
                                    p,
                                    move |_, w, cx| {
                                        cx.stop_propagation();
                                        h_unpin(
                                            SidebarAction::TogglePinChat(cid_unpin.clone()),
                                            w,
                                            cx,
                                        );
                                    },
                                ),
                                "Unpin chat",
                            )),
                    );
                }
                list = list.child(h_divider(p));
            }

            list = list
                .child(field_label("Pinned replies", p))
                .child(scope_row);

            if filtered.is_empty() {
                let empty_msg = if !q_lower.is_empty() {
                    "No pinned replies match your search."
                } else if props.pinned_all_chats {
                    "No pinned replies yet. Pin any assistant reply to keep it here."
                } else {
                    "No pinned replies in this chat."
                };
                list = list.child(
                    div()
                        .px(px(4.0))
                        .py(px(6.0))
                        .text_size(px(12.5))
                        .text_color(muted)
                        .child(empty_msg),
                );
            } else {
                for (ix, pin) in filtered.into_iter().enumerate() {
                    let cid = pin.chat_id.clone();
                    let mid = pin.message_id.clone();
                    let cid_u = pin.chat_id.clone();
                    let mid_u = pin.message_id.clone();
                    let h_open = props.on_action.clone();
                    let h_unpin = props.on_action.clone();
                    let active_bg = p.active_tint();

                    list = list.child(
                        h_flex()
                            .id(("sb-pin", ix))
                            .w_full()
                            .pl(px(10.0))
                            .pr(px(4.0))
                            .py(px(7.0))
                            .rounded(px(8.0))
                            .bg(card)
                            .border_1()
                            .border_color(border)
                            .items_center()
                            .gap(px(6.0))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .active(move |s| s.bg(active_bg))
                            .on_click(move |_, w, cx| {
                                h_open(
                                    SidebarAction::OpenPinned {
                                        chat_id: cid.clone(),
                                        message_id: mid.clone(),
                                    },
                                    w,
                                    cx,
                                );
                            })
                            .child(
                                v_flex()
                                    .flex_1()
                                    .overflow_hidden()
                                    .gap(px(2.0))
                                    .children(props.pinned_all_chats.then(|| {
                                        div()
                                            .truncate()
                                            .text_size(px(11.0))
                                            .font_semibold()
                                            .text_color(accent)
                                            .child(pin.chat_title.clone())
                                    }))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .text_color(narration)
                                            .child(pin.preview.clone()),
                                    ),
                            )
                            .child(with_tooltip(
                                icon_button(
                                    ("sb-unpin", ix),
                                    KissIcon::Pin,
                                    true,
                                    p,
                                    move |_, w, cx| {
                                        cx.stop_propagation();
                                        h_unpin(
                                            SidebarAction::UnpinReply {
                                                chat_id: cid_u.clone(),
                                                message_id: mid_u.clone(),
                                            },
                                            w,
                                            cx,
                                        );
                                    },
                                ),
                                "Unpin reply",
                            )),
                    );
                }
            }
            list.into_any_element()
        }
    };

    // Desktop sidebar has no bottom Settings footer (Settings lives in TopEditorialBar).
    v_flex()
        .w(px(285.0))
        .h_full()
        .flex_shrink_0()
        .bg(sheet)
        .p(px(12.0))
        .gap(px(10.0))
        .child(search_row)
        .child(segmented)
        .child(list_body)
        .into_any_element()
}
