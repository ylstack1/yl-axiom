//! Floating elevated overlays matching `CommandPalette.kt`, `ParchmentModalsAndNavigation.kt`,
//! `ParchmentModelPickerAndStats.kt`, and `AppDialogs.kt`:
//! - Anchored Quick Model Popover (top-left below model capsule, provider + vendor filter pills,
//!   `ModelLabSvgIcon`, `ModelPrivacyBadge`, context window badge)
//! - Command Palette (`Ctrl+K`) with fuzzy search, `ShortcutKeycapRow`, keyboard selection, all 8
//!   settings categories, Retry now, and open chats
//! - Prompt Jump (`Ctrl+G`) with live filter & keyboard selection
//! - Tab Overview Switcher (2-column cards, search filter when > 3 tabs, `Close empty (N)`)
//! - Built-in Zoomable Image & Text/Code/SVG/Markdown Attachment Viewer Modal
//! - Delete & Regenerate Confirmation Dialogs with inline opt-out switches.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement as _, MouseButton,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    img, px,
};
use gpui_component::Icon;
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::text::TextView;
use gpui_component::{StyledExt as _, h_flex, v_flex};
use parchment_core::model::{ChatStub, ProviderProfile};
use parchment_core::prose::parse_svg_bounded;

use crate::icons::{KissIcon, kiss_icon, model_lab_icon};
use crate::prose_view::DisplayMessageRow;
use crate::strings;
use crate::theme::ParchmentPalette;
use crate::ui_kit::{
    SepiaButtonStyle, format_tokens_short, fuzzy_match_score, h_divider, icon_button,
    kiss_bounce_subtle, model_privacy_badge_chip, primary_model_privacy_badge, rubric_mark_overlay,
    sepia_button, setting_toggle_row, shortcut_keycap_row, spring_pop_enter, v_divider,
};

mod dialogs;

pub use dialogs::*;

#[derive(Debug, Clone)]
pub enum ActiveMediaViewerItem {
    Image {
        title: String,
        local_file_path: String,
        remote_url: String,
        mime_type: String,
    },
    TextOrCode {
        title: String,
        text_content: String,
        mime_type: String,
        local_file_path: String,
    },
}

#[derive(Debug, Clone)]
pub enum ConfirmDialogKind {
    DeleteSingleChat {
        chat_id: String,
        title: String,
    },
    ClearAllChats {
        assistant_name: String,
        count: usize,
    },
    ClearCurrentMessages {
        title: String,
    },
    RegenerateTurn {
        index: usize,
        replaces_reply: bool,
    },
}

#[derive(Debug, Clone)]
pub enum OverlayAction {
    Dismiss,
    RunCommand(&'static str),
    SelectChat(String),
    JumpToMessageIndex(usize),
    SelectModelQuick {
        profile_id: String,
        model_id: String,
    },
    SetQuickModelProviderFilter(Option<String>),
    OpenProviderSettings,
    CloseTab(String),
    CloseEmptyTabs,
    NewTab,
    // Viewer modal actions
    ViewerZoomIn,
    ViewerZoomOut,
    ViewerZoomReset,
    ViewerTogglePreview,
    ViewerToggleWrap,
    ViewerCopyText(String),
    ViewerSaveFile {
        title: String,
        local_path: String,
        text_content: Option<String>,
    },
    ViewerOpenLocal(String),
    ViewerOpenUrl(String),
    // Confirm dialog actions
    ConfirmDialogAccept,
    SetConfirmDeletePref(bool),
    SetConfirmRegeneratePref(bool),
}

pub type OverlayActionHandler = Rc<dyn Fn(OverlayAction, &mut Window, &mut App)>;

fn modal_shell(
    id: &'static str,
    width_px: f32,
    palette: &ParchmentPalette,
    on_dismiss: OverlayActionHandler,
    content: AnyElement,
) -> AnyElement {
    let scrim = gpui::hsla(0.0, 0.0, 0.0, 0.42);
    div()
        .id(id)
        .absolute()
        .inset_0()
        .occlude()
        .bg(scrim)
        .flex()
        .items_start()
        .justify_center()
        .pt(px(56.0))
        .on_mouse_down(MouseButton::Left, move |_, w, cx| {
            cx.stop_propagation();
            on_dismiss(OverlayAction::Dismiss, w, cx);
        })
        .child(spring_pop_enter(
            SharedString::from(format!("modal-anim-{id}")),
            div()
                .id(SharedString::from(format!("modal-card-{id}")))
                .w(px(width_px))
                .max_h(px(620.0))
                .rounded(px(12.0))
                .bg(palette.elevated_bg.to_hsla())
                .border_1()
                .border_color(palette.border.to_hsla())
                .shadow_lg()
                .overflow_hidden()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(content),
        ))
        .into_any_element()
}

pub fn command_palette_entries(
    query: &str,
    stubs: &[ChatStub],
    can_retry_now: bool,
    shortcut_hints: &dyn Fn(&str) -> Vec<String>,
) -> Vec<PaletteEntry> {
    let mut builtin: Vec<(&'static str, &'static str, Vec<String>)> = vec![
        ("new_chat", "New chat", shortcut_hints("new_chat")),
        (
            "open_model_picker",
            "Choose a model",
            shortcut_hints("quick_model"),
        ),
        (
            "toggle_sidebar",
            "Toggle left sidebar",
            shortcut_hints("toggle_sidebar"),
        ),
        (
            "toggle_vault",
            "Toggle Chat Files sidebar",
            shortcut_hints("toggle_vault"),
        ),
        (
            "toggle_notes",
            "Toggle Chat Notes",
            shortcut_hints("toggle_notes"),
        ),
        ("toggle_view", "Switch Bubbles / Page view", vec![]),
        (
            "prompt_jump",
            "Jump to a prompt in this chat",
            shortcut_hints("prompt_jump"),
        ),
        (
            "tab_switcher",
            "Show all open tabs",
            shortcut_hints("tab_switcher"),
        ),
        (
            "open_settings",
            "Settings: Preferences",
            shortcut_hints("open_settings"),
        ),
        (
            "settings_providers",
            "Settings: Providers & API keys",
            vec![],
        ),
        ("settings_assistants", "Settings: Assistant", vec![]),
        ("settings_default_model", "Settings: Default Model", vec![]),
        (
            "settings_workspace",
            "Settings: Workspace, Tools & MCP",
            vec![],
        ),
        (
            "settings_world_book",
            "Settings: World Book & Cards",
            vec![],
        ),
        ("settings_shortcuts", "Settings: Keyboard Shortcuts", vec![]),
        (
            "settings_storage",
            "Settings: Storage, Backup & Updates",
            vec![],
        ),
        ("stream_bench", "Run 87 KB streaming benchmark", vec![]),
    ];
    if can_retry_now {
        builtin.insert(0, ("retry_now", "Retry waiting request now", vec![]));
    }

    let mut scored: Vec<(i32, PaletteEntry)> = Vec::new();
    for (id, label, keys) in builtin {
        if let Some(sc) = fuzzy_match_score(label, query) {
            scored.push((sc + 20, PaletteEntry::Command { id, label, keys }));
        }
    }
    for stub in stubs.iter().take(40) {
        let lbl = format!("Open chat: {}", stub.title);
        if let Some(sc) = fuzzy_match_score(&lbl, query) {
            scored.push((
                sc,
                PaletteEntry::Chat {
                    id: stub.id.clone(),
                    title: stub.title.clone(),
                },
            ));
        }
    }
    if !query.trim().is_empty() {
        scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    }
    scored.into_iter().take(28).map(|(_, e)| e).collect()
}

#[derive(Debug, Clone)]
pub enum PaletteEntry {
    Command {
        id: &'static str,
        label: &'static str,
        keys: Vec<String>,
    },
    Chat {
        id: String,
        title: String,
    },
}

#[allow(clippy::too_many_arguments)]
pub fn render_command_palette(
    filter_input: &Entity<InputState>,
    entries: &[PaletteEntry],
    selected_idx: usize,
    palette: &ParchmentPalette,
    on_action: &OverlayActionHandler,
) -> AnyElement {
    let p = palette;
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    let hover_bg = p.hover_tint();

    let h_close = on_action.clone();
    let mut rows = v_flex()
        .id("cmd-pal-scroll")
        .max_h(px(390.0))
        .w_full()
        .gap(px(2.0))
        .overflow_y_scrollbar();

    if entries.is_empty() {
        rows = rows.child(
            div()
                .p(px(12.0))
                .text_size(px(12.5))
                .text_color(muted)
                .child("No matching commands or chats."),
        );
    } else {
        for (ix, item) in entries.iter().enumerate() {
            let is_sel = ix == selected_idx;
            match item {
                PaletteEntry::Command { id, label, keys } => {
                    let cmd_id = *id;
                    let h = on_action.clone();
                    let mut row = h_flex()
                        .id(("pal-cmd", ix))
                        .relative()
                        .overflow_hidden()
                        .w_full()
                        .px(px(12.0))
                        .py(px(8.0))
                        .rounded(px(8.0))
                        .bg(if is_sel {
                            p.accent.with_alpha(0.12)
                        } else {
                            p.canvas_bg.with_alpha(0.0)
                        })
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .justify_between()
                        .items_center()
                        .on_click(move |_, w, cx| h(OverlayAction::RunCommand(cmd_id), w, cx));
                    if is_sel {
                        row = row.child(rubric_mark_overlay(6.0, p));
                    }
                    rows = rows.child(kiss_bounce_subtle(
                        ("pal-cmd-kb", ix),
                        row.child(
                            div()
                                .text_size(px(13.0))
                                .text_color(if is_sel { accent } else { ink })
                                .child(*label),
                        )
                        .children((!keys.is_empty()).then(|| shortcut_keycap_row(keys, is_sel, p))),
                    ));
                }
                PaletteEntry::Chat { id, title } => {
                    let cid = id.clone();
                    let h = on_action.clone();
                    let mut row = h_flex()
                        .id(("pal-chat", ix))
                        .relative()
                        .overflow_hidden()
                        .w_full()
                        .px(px(12.0))
                        .py(px(8.0))
                        .rounded(px(8.0))
                        .bg(if is_sel {
                            p.accent.with_alpha(0.12)
                        } else {
                            p.canvas_bg.with_alpha(0.0)
                        })
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .justify_between()
                        .items_center()
                        .on_click(move |_, w, cx| h(OverlayAction::SelectChat(cid.clone()), w, cx));
                    if is_sel {
                        row = row.child(rubric_mark_overlay(6.0, p));
                    }
                    rows = rows.child(
                        row.child(
                            h_flex()
                                .items_center()
                                .gap(px(7.0))
                                .child(kiss_icon(KissIcon::MessageCircle, 13.0, accent))
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .text_color(if is_sel { accent } else { ink })
                                        .child(title.clone()),
                                ),
                        )
                        .child(div().text_size(px(11.0)).text_color(muted).child("Chat")),
                    );
                }
            }
        }
    }

    let body = v_flex()
        .w_full()
        .p(px(14.0))
        .gap(px(10.0))
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_size(px(14.5))
                        .font_semibold()
                        .text_color(ink)
                        .child(strings::COMMAND_PALETTE_TITLE),
                )
                .child(icon_button(
                    "pal-close",
                    KissIcon::Close,
                    false,
                    p,
                    move |_, w, cx| h_close(OverlayAction::Dismiss, w, cx),
                )),
        )
        .child(Input::new(filter_input).prefix(kiss_icon(KissIcon::Search, 13.0, muted)))
        .child(h_divider(p))
        .child(rows)
        .into_any_element();

    modal_shell("overlay-cmd-pal", 560.0, p, on_action.clone(), body)
}

pub fn prompt_jump_targets(
    query: &str,
    rows_ref: &[DisplayMessageRow],
) -> Vec<(usize, usize, String)> {
    let mut prompt_num = 0usize;
    let mut targets: Vec<(usize, usize, String)> = Vec::new();
    for (ix, m) in rows_ref.iter().enumerate() {
        if m.is_user {
            prompt_num += 1;
            let preview: String = m.content.replace('\n', " ").chars().take(140).collect();
            if query.trim().is_empty() || fuzzy_match_score(&preview, query).is_some() {
                targets.push((ix, prompt_num, preview));
            }
        }
    }
    targets.reverse();
    targets.truncate(40);
    targets
}

pub fn render_prompt_jump(
    filter_input: &Entity<InputState>,
    targets: &[(usize, usize, String)],
    selected_idx: usize,
    palette: &ParchmentPalette,
    on_action: &OverlayActionHandler,
) -> AnyElement {
    let p = palette;
    let card = p.card_bg.to_hsla();
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    let hover_bg = p.hover_tint();

    let h_close = on_action.clone();
    let mut list = v_flex()
        .id("prompt-jump-scroll")
        .max_h(px(380.0))
        .w_full()
        .gap(px(4.0))
        .overflow_y_scrollbar();

    if targets.is_empty() {
        list = list.child(
            div()
                .p(px(10.0))
                .text_size(px(12.5))
                .text_color(muted)
                .child("No matching prompts in this chat."),
        );
    } else {
        for (row_i, (msg_ix, p_num, preview)) in targets.iter().enumerate() {
            let is_sel = row_i == selected_idx;
            let m_ix = *msg_ix;
            let h = on_action.clone();
            let mut row = h_flex()
                .id(("jump-row", row_i))
                .relative()
                .w_full()
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .bg(if is_sel {
                    p.accent.with_alpha(0.12)
                } else {
                    card
                })
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .items_center()
                .gap(px(8.0))
                .on_click(move |_, w, cx| h(OverlayAction::JumpToMessageIndex(m_ix), w, cx));
            if is_sel {
                row = row.child(rubric_mark_overlay(6.0, p));
            }
            list = list.child(
                row.child(
                    div()
                        .font_family(p.code_font_family.clone())
                        .text_size(px(11.5))
                        .font_semibold()
                        .text_color(accent)
                        .child(format!("#{p_num}")),
                )
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_size(px(13.0))
                        .text_color(ink)
                        .child(preview.clone()),
                ),
            );
        }
    }

    let body = v_flex()
        .w_full()
        .p(px(14.0))
        .gap(px(10.0))
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_size(px(14.5))
                        .font_semibold()
                        .text_color(ink)
                        .child(strings::PROMPT_JUMP_TITLE),
                )
                .child(icon_button(
                    "jump-close",
                    KissIcon::Close,
                    false,
                    p,
                    move |_, w, cx| h_close(OverlayAction::Dismiss, w, cx),
                )),
        )
        .child(Input::new(filter_input).prefix(kiss_icon(KissIcon::Search, 13.0, muted)))
        .child(h_divider(p))
        .child(list)
        .into_any_element();

    modal_shell("overlay-prompt-jump", 560.0, p, on_action.clone(), body)
}

/// Anchored top-left Quick Model Popover (`ParchmentModalsAndNavigation.kt` vertical list design).
#[allow(clippy::too_many_arguments)]
pub fn render_quick_model_picker(
    filter_input: &Entity<InputState>,
    query: &str,
    profiles: &[ProviderProfile],
    active_profile_id: &str,
    active_model_id: &str,
    selected_provider_filter: Option<&str>,
    palette: &ParchmentPalette,
    on_action: &OverlayActionHandler,
) -> AnyElement {
    let p = palette;
    let card = p.card_bg.to_hsla();
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    let hover_bg = p.hover_tint();

    let mut all_entries: Vec<(&ProviderProfile, String)> = Vec::new();
    let mut providers_with_models: Vec<(&ProviderProfile, usize)> = Vec::new();
    for prof in profiles {
        let mut count = 0usize;
        if !prof.saved_models.is_empty() {
            for m in &prof.saved_models {
                all_entries.push((prof, m.clone()));
                count += 1;
            }
        } else if !prof.default_model.trim().is_empty() {
            all_entries.push((prof, prof.default_model.clone()));
            count += 1;
        }
        if count > 0 {
            providers_with_models.push((prof, count));
        }
    }
    let total_count = all_entries.len();

    let q_low = query.trim().to_ascii_lowercase();
    let tokens: Vec<&str> = q_low.split_whitespace().collect();
    let mut filtered: Vec<(&ProviderProfile, String)> = all_entries
        .into_iter()
        .filter(|(prof, mid)| {
            let prov_ok = selected_provider_filter
                .map(|fid| prof.id == fid)
                .unwrap_or(true);
            let q_ok = tokens.is_empty()
                || tokens.iter().all(|tok| {
                    mid.to_ascii_lowercase().contains(tok)
                        || prof.name.to_ascii_lowercase().contains(tok)
                });
            prov_ok && q_ok
        })
        .collect();

    // Pin active model first when unfiltered (`ParchmentModelPickerAndStats.kt:429`)
    if tokens.is_empty()
        && let Some(pos) = filtered
            .iter()
            .position(|(prof, mid)| prof.id == active_profile_id && mid == active_model_id)
        && pos > 0
    {
        let active_item = filtered.remove(pos);
        filtered.insert(0, active_item);
    }
    filtered.truncate(120);

    let h_manage = on_action.clone();
    let h_close = on_action.clone();
    let h_dismiss = on_action.clone();

    // Vertical provider filter rail (ONLY when >1 provider actually has models — zero horizontal strip)
    let provider_rail = (providers_with_models.len() > 1).then(|| {
        let h_all = on_action.clone();
        let all_sel = selected_provider_filter.is_none();
        let mut rail = v_flex()
            .id("qm-prov-rail")
            .w(px(148.0))
            .max_h(px(330.0))
            .pr(px(8.0))
            .gap(px(3.0))
            .flex_shrink_0()
            .overflow_y_scrollbar()
            .child({
                let mut r = h_flex()
                    .id("qm-prov-all")
                    .relative()
                    .w_full()
                    .px(px(8.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .bg(if all_sel {
                        p.accent.with_alpha(0.12)
                    } else {
                        p.canvas_bg.with_alpha(0.0)
                    })
                    .justify_between()
                    .items_center()
                    .gap(px(4.0))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, w, cx| {
                        h_all(OverlayAction::SetQuickModelProviderFilter(None), w, cx)
                    })
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_medium()
                            .text_color(if all_sel { accent } else { ink })
                            .child("All"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(muted)
                            .child(total_count.to_string()),
                    );
                if all_sel {
                    r = r.child(rubric_mark_overlay(5.0, p));
                }
                r
            });

        for (ix, (prof, count)) in providers_with_models.iter().enumerate() {
            let pid = prof.id.clone();
            let h_p = on_action.clone();
            let sel = selected_provider_filter == Some(prof.id.as_str());
            let mut row = h_flex()
                .id(("qm-prov", ix))
                .relative()
                .w_full()
                .px(px(8.0))
                .py(px(7.0))
                .rounded(px(8.0))
                .bg(if sel {
                    p.accent.with_alpha(0.12)
                } else {
                    p.canvas_bg.with_alpha(0.0)
                })
                .justify_between()
                .items_center()
                .gap(px(5.0))
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .on_click(move |_, w, cx| {
                    let next = if sel { None } else { Some(pid.clone()) };
                    h_p(OverlayAction::SetQuickModelProviderFilter(next), w, cx);
                })
                .child(
                    h_flex()
                        .flex_1()
                        .overflow_hidden()
                        .items_center()
                        .gap(px(6.0))
                        .child(model_lab_icon(
                            &prof.name,
                            13.0,
                            if sel {
                                accent
                            } else {
                                p.narration_ink.to_hsla()
                            },
                            prof.protocol.as_db_str(),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_size(px(12.0))
                                .font_medium()
                                .text_color(if sel { accent } else { ink })
                                .child(prof.name.clone()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(muted)
                        .child(count.to_string()),
                );
            if sel {
                row = row.child(rubric_mark_overlay(5.0, p));
            }
            rail = rail.child(kiss_bounce_subtle(("qm-prov-kb", ix), row));
        }
        rail
    });

    let mut list = v_flex()
        .id("quick-model-scroll")
        .flex_1()
        .max_h(px(330.0))
        .w_full()
        .gap(px(4.0))
        .overflow_y_scrollbar();

    if filtered.is_empty() {
        list = list.child(
            div()
                .w_full()
                .p(px(12.0))
                .rounded(px(8.0))
                .bg(p.sheet_bg.with_alpha(0.45))
                .text_size(px(12.5))
                .text_color(muted)
                .child(if total_count == 0 {
                    "No models configured yet. Open Manage providers."
                } else {
                    "No matching models."
                }),
        );
    } else {
        let show_provider_sub =
            providers_with_models.len() > 1 && selected_provider_filter.is_none();
        for (ix, (prof, mid)) in filtered.into_iter().enumerate() {
            let is_sel = prof.id == active_profile_id && mid == active_model_id;
            let pid = prof.id.clone();
            let mid_click = mid.clone();
            let h = on_action.clone();
            let spec = prof.custom_model_specs.get(&mid);
            let ctx_tokens = spec
                .map(|s| s.context_window_tokens)
                .filter(|&v| v > 0)
                .unwrap_or(prof.context_window_tokens);
            let privacy_badge = primary_model_privacy_badge(prof, &mid, spec);

            let mut row = h_flex()
                .id(("qm-row", ix))
                .relative()
                .w_full()
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(8.0))
                .bg(if is_sel {
                    p.accent.with_alpha(0.13)
                } else {
                    card
                })
                .border_1()
                .border_color(if is_sel {
                    accent
                } else {
                    p.border.with_alpha(0.45)
                })
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .justify_between()
                .items_center()
                .gap(px(8.0))
                .on_click(move |_, w, cx| {
                    h(
                        OverlayAction::SelectModelQuick {
                            profile_id: pid.clone(),
                            model_id: mid_click.clone(),
                        },
                        w,
                        cx,
                    );
                });
            if is_sel {
                row = row.child(rubric_mark_overlay(6.0, p));
            }
            list = list.child(kiss_bounce_subtle(
                ("qm-mod-kb", ix),
                row.child(
                    h_flex()
                        .flex_1()
                        .overflow_hidden()
                        .items_center()
                        .gap(px(8.0))
                        .child(model_lab_icon(
                            &mid,
                            14.0,
                            if is_sel {
                                accent
                            } else {
                                p.narration_ink.to_hsla()
                            },
                            prof.protocol.as_db_str(),
                        ))
                        .child(
                            v_flex()
                                .flex_1()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.5))
                                        .font_medium()
                                        .text_color(if is_sel { accent } else { ink })
                                        .child(mid),
                                )
                                .children(show_provider_sub.then(|| {
                                    div()
                                        .truncate()
                                        .text_size(px(11.0))
                                        .text_color(muted)
                                        .child(prof.name.clone())
                                })),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(5.0))
                        .flex_shrink_0()
                        .children(privacy_badge.map(|b| model_privacy_badge_chip(&b, p)))
                        .children((ctx_tokens > 0).then(|| {
                            div()
                                .px(px(5.0))
                                .py(px(2.0))
                                .rounded(px(5.0))
                                .bg(p.sheet_bg.to_hsla())
                                .font_family(p.code_font_family.clone())
                                .text_size(px(10.5))
                                .text_color(muted)
                                .child(format_tokens_short(ctx_tokens as i64))
                        }))
                        .children(is_sel.then(|| kiss_icon(KissIcon::Check, 13.0, accent))),
                ),
            ));
        }
    }

    let body_content: AnyElement = if let Some(rail) = provider_rail {
        h_flex()
            .w_full()
            .items_start()
            .gap(px(8.0))
            .child(rail)
            .child(v_divider(p))
            .child(list)
            .into_any_element()
    } else {
        list.into_any_element()
    };

    let pop_w = if providers_with_models.len() > 1 {
        530.0
    } else {
        440.0
    };
    let popover_card = v_flex()
        .id("quick-model-popover-card")
        .w(px(pop_w))
        .max_h(px(500.0))
        .p(px(12.0))
        .rounded(px(10.0))
        .bg(p.elevated_bg.to_hsla())
        .border_1()
        .border_color(p.border.to_hsla())
        .shadow_lg()
        .gap(px(8.0))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(kiss_icon(KissIcon::Cpu, 14.0, accent))
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_bold()
                                .text_color(ink)
                                .child(strings::CHOOSE_MODEL),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(muted)
                                .child(format!("({total_count})")),
                        ),
                )
                .child(
                    h_flex()
                        .gap(px(4.0))
                        .child(sepia_button(
                            "qm-manage",
                            strings::MANAGE_PROVIDERS,
                            None,
                            SepiaButtonStyle::Ghost,
                            true,
                            false,
                            p,
                            move |_, w, cx| h_manage(OverlayAction::OpenProviderSettings, w, cx),
                        ))
                        .child(icon_button(
                            "qm-close",
                            KissIcon::Close,
                            false,
                            p,
                            move |_, w, cx| h_close(OverlayAction::Dismiss, w, cx),
                        )),
                ),
        )
        .child(Input::new(filter_input).prefix(kiss_icon(KissIcon::Search, 13.0, muted)))
        .child(h_divider(p))
        .child(body_content);

    // Anchored top-left right beneath TopEditorialBar's model capsule
    div()
        .id("overlay-quick-model")
        .absolute()
        .inset_0()
        .occlude()
        .on_mouse_down(MouseButton::Left, move |_, w, cx| {
            cx.stop_propagation();
            h_dismiss(OverlayAction::Dismiss, w, cx);
        })
        .child(
            div()
                .absolute()
                .top(px(54.0))
                .left(px(54.0))
                .child(spring_pop_enter("qm-pop-anim", popover_card)),
        )
        .into_any_element()
}
