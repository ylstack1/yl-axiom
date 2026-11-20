//! Right Chat Vault sidebar (`ParchmentRightFilesSidebar` from `ChatFilesSidebar.kt`)
//! and Chat Notes panel (`ChatNotesPanel` from `EmptyChatAndNotes.kt`, `desktop-notes.png`).

use std::rc::Rc;

use gpui::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{StyledExt as _, h_flex, v_flex};
use parchment_core::model::{
    ChatSession, GeneratedMediaItem, MediaKind, PromptAttachment, WebSourceCitation,
};

use crate::icons::{KissIcon, kiss_icon};
use crate::strings;
use crate::theme::ParchmentPalette;
use crate::ui_kit::{
    SepiaButtonStyle, field_label, h_divider, icon_button, sepia_button, sepia_chip,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultFilter {
    All,
    AiFiles,
    Uploads,
    Sources,
}

#[derive(Debug, Clone)]
pub enum VaultAiArtifact {
    WorkspaceFile {
        turn: usize,
        path: String,
        badge: String,
        fallback_content: String,
    },
    CodeSnippet {
        turn: usize,
        title: String,
        badge: String,
        code: String,
    },
    GeneratedMedia {
        turn: usize,
        item: GeneratedMediaItem,
    },
}

#[derive(Debug, Clone)]
pub enum VaultAction {
    SetFilter(VaultFilter),
    InspectWorkspaceFile {
        path: String,
        fallback_content: String,
    },
    InspectCodeSnippet {
        title: String,
        code: String,
        mime_type: String,
    },
    InspectGeneratedMedia(GeneratedMediaItem),
    InspectAttachment(PromptAttachment),
    ReattachAttachment(PromptAttachment),
    OpenUrl(String),
    CopySnippet(String),
    CloseVault,
}

pub type VaultActionHandler = Rc<dyn Fn(VaultAction, &mut Window, &mut App)>;

#[derive(Debug, Clone)]
pub enum NotesAction {
    SummarizeChat,
    CompactContext,
    ExportMarkdown,
    ClearMessages,
    CloseNotes,
}

pub type NotesActionHandler = Rc<dyn Fn(NotesAction, &mut Window, &mut App)>;

pub struct ExtractedVaultItems {
    pub ai_files: Vec<VaultAiArtifact>,
    pub uploads: Vec<(usize, PromptAttachment)>,
    pub sources: Vec<(usize, WebSourceCitation)>,
}

pub fn count_vault_items(session: &ChatSession, pending: &[PromptAttachment]) -> usize {
    let items = extract_vault_items(session, pending);
    items.ai_files.len() + items.uploads.len() + items.sources.len()
}

pub fn extract_vault_items(
    session: &ChatSession,
    pending: &[PromptAttachment],
) -> ExtractedVaultItems {
    let mut ai_files = Vec::new();
    let mut uploads = Vec::new();
    let mut sources = Vec::new();

    for att in pending {
        uploads.push((0, att.clone()));
    }

    for (idx, node) in session.messages.iter().enumerate() {
        let turn = idx + 1;
        let Some(v) = node.active_variant() else {
            continue;
        };
        for tc in &v.tool_calls {
            if tc.name == "fs_write" || tc.name == "fs_edit" {
                let parsed = serde_json::from_str::<serde_json::Value>(&tc.arguments_json).ok();
                let path = parsed
                    .as_ref()
                    .and_then(|j| {
                        j.get("path")
                            .or_else(|| j.get("file_path"))
                            .and_then(|s| s.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| tc.name.clone());
                let fallback = parsed
                    .as_ref()
                    .and_then(|j| {
                        j.get("content")
                            .or_else(|| j.get("new_text"))
                            .or_else(|| j.get("new_string"))
                            .and_then(|s| s.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| tc.arguments_json.clone());
                let badge = if tc.name == "fs_write" {
                    "Written"
                } else {
                    "Edited"
                };
                ai_files.push(VaultAiArtifact::WorkspaceFile {
                    turn,
                    path,
                    badge: badge.to_string(),
                    fallback_content: fallback,
                });
            }
        }
        for media in &v.media_items {
            ai_files.push(VaultAiArtifact::GeneratedMedia {
                turn,
                item: media.clone(),
            });
        }
        // Extract assistant fenced code blocks (>= 6 lines) and inline SVGs as inspectable artifacts.
        let mut in_fence = false;
        let mut lang = String::new();
        let mut code_buf = String::new();
        let mut fence_idx = 0usize;
        for line in v.content.lines() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("```") {
                if !in_fence {
                    in_fence = true;
                    lang = rest.trim().to_string();
                    code_buf.clear();
                } else {
                    in_fence = false;
                    let is_svg = lang.eq_ignore_ascii_case("svg")
                        || (code_buf.contains("<svg") && code_buf.contains("</svg>"));
                    if code_buf.lines().count() >= 6 || is_svg {
                        fence_idx += 1;
                        let ext = if is_svg {
                            "svg"
                        } else if lang.is_empty() {
                            "txt"
                        } else {
                            lang.as_str()
                        };
                        let title = format!("artifact-{turn}-{fence_idx}.{ext}");
                        ai_files.push(VaultAiArtifact::CodeSnippet {
                            turn,
                            title,
                            badge: if lang.is_empty() {
                                "Code".into()
                            } else {
                                lang.clone()
                            },
                            code: code_buf.clone(),
                        });
                    }
                }
            } else if in_fence {
                if !code_buf.is_empty() {
                    code_buf.push('\n');
                }
                code_buf.push_str(line);
            }
        }

        for att in &v.attachments {
            uploads.push((turn, att.clone()));
        }
        for src in &v.web_sources {
            sources.push((turn, src.clone()));
        }
    }

    ExtractedVaultItems {
        ai_files,
        uploads,
        sources,
    }
}

pub fn render_right_vault_sidebar(
    session: &ChatSession,
    pending_attachments: &[PromptAttachment],
    filter: VaultFilter,
    palette: &ParchmentPalette,
    on_action: &VaultActionHandler,
) -> AnyElement {
    let p = palette;
    let sheet = p.sheet_bg.to_hsla();
    let card = p.card_bg.to_hsla();
    let border = p.subtle_border();
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    let hover_bg = p.hover_tint();

    let items = extract_vault_items(session, pending_attachments);
    let total = items.ai_files.len() + items.uploads.len() + items.sources.len();

    let h_close = on_action.clone();
    let header = h_flex()
        .w_full()
        .justify_between()
        .items_center()
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .child(kiss_icon(KissIcon::Folder, 15.0, accent))
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_semibold()
                        .text_color(ink)
                        .child(format!("Chat files ({total})")),
                ),
        )
        .child(icon_button(
            "vault-close",
            KissIcon::Close,
            false,
            p,
            move |_, w, cx| h_close(VaultAction::CloseVault, w, cx),
        ));

    let filter_chips = h_flex().flex_wrap().gap(px(5.0)).children(
        [
            (VaultFilter::All, "All"),
            (VaultFilter::AiFiles, "AI files"),
            (VaultFilter::Uploads, "Uploads"),
            (VaultFilter::Sources, "Sources"),
        ]
        .into_iter()
        .enumerate()
        .map(|(ix, (f, label))| {
            let h = on_action.clone();
            sepia_chip(
                ("vault-flt", ix),
                label,
                None,
                filter == f,
                p,
                move |_, w, cx| h(VaultAction::SetFilter(f), w, cx),
            )
        }),
    );

    let mut list = v_flex()
        .id("right-vault-scroll")
        .flex_1()
        .w_full()
        .gap(px(8.0))
        .overflow_y_scrollbar();

    let show_ai = matches!(filter, VaultFilter::All | VaultFilter::AiFiles);
    let show_up = matches!(filter, VaultFilter::All | VaultFilter::Uploads);
    let show_src = matches!(filter, VaultFilter::All | VaultFilter::Sources);

    if total == 0 {
        list = list.child(
            div()
                .p(px(10.0))
                .text_size(px(12.5))
                .text_color(muted)
                .child("Files written by the assistant, generated media, attachments you share, and grounded web sources appear here."),
        );
    } else {
        if show_ai && !items.ai_files.is_empty() {
            list = list.child(field_label("AI-generated files & code", p));
            for (ix, art) in items.ai_files.into_iter().enumerate() {
                match art {
                    VaultAiArtifact::WorkspaceFile {
                        turn,
                        path,
                        badge,
                        fallback_content,
                    } => {
                        let p_open = path.clone();
                        let fb_open = fallback_content.clone();
                        let fb_copy = fallback_content.clone();
                        let h_open = on_action.clone();
                        let h_copy = on_action.clone();
                        list = list.child(
                            h_flex()
                                .id(("vault-ai-ws", ix))
                                .w_full()
                                .p(px(8.0))
                                .rounded(px(8.0))
                                .bg(card)
                                .border_1()
                                .border_color(border)
                                .items_center()
                                .justify_between()
                                .gap(px(6.0))
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
                                .on_click(move |_, w, cx| {
                                    h_open(
                                        VaultAction::InspectWorkspaceFile {
                                            path: p_open.clone(),
                                            fallback_content: fb_open.clone(),
                                        },
                                        w,
                                        cx,
                                    );
                                })
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .overflow_hidden()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(kiss_icon(KissIcon::FileText, 13.0, accent))
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .overflow_hidden()
                                                .gap(px(2.0))
                                                .child(
                                                    div()
                                                        .text_size(px(12.5))
                                                        .font_medium()
                                                        .text_color(ink)
                                                        .truncate()
                                                        .child(path),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .text_color(muted)
                                                        .child(format!("{badge} · Turn {turn}")),
                                                ),
                                        ),
                                )
                                .child(icon_button(
                                    ("vault-ai-copy", ix),
                                    KissIcon::Copy,
                                    false,
                                    p,
                                    move |_, w, cx| {
                                        cx.stop_propagation();
                                        h_copy(VaultAction::CopySnippet(fb_copy.clone()), w, cx);
                                    },
                                )),
                        );
                    }
                    VaultAiArtifact::CodeSnippet {
                        turn,
                        title,
                        badge,
                        code,
                    } => {
                        let t_open = title.clone();
                        let c_open = code.clone();
                        let c_copy = code.clone();
                        let mime = if title.ends_with(".svg") {
                            "image/svg+xml".to_string()
                        } else if title.ends_with(".md") {
                            "text/markdown".to_string()
                        } else {
                            "text/plain".to_string()
                        };
                        let h_open = on_action.clone();
                        let h_copy = on_action.clone();
                        list = list.child(
                            h_flex()
                                .id(("vault-ai-code", ix))
                                .w_full()
                                .p(px(8.0))
                                .rounded(px(8.0))
                                .bg(card)
                                .border_1()
                                .border_color(border)
                                .items_center()
                                .justify_between()
                                .gap(px(6.0))
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
                                .on_click(move |_, w, cx| {
                                    h_open(
                                        VaultAction::InspectCodeSnippet {
                                            title: t_open.clone(),
                                            code: c_open.clone(),
                                            mime_type: mime.clone(),
                                        },
                                        w,
                                        cx,
                                    );
                                })
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .overflow_hidden()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(kiss_icon(KissIcon::Code, 13.0, accent))
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .overflow_hidden()
                                                .gap(px(2.0))
                                                .child(
                                                    div()
                                                        .text_size(px(12.5))
                                                        .font_medium()
                                                        .text_color(ink)
                                                        .truncate()
                                                        .child(title),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .text_color(muted)
                                                        .child(format!("{badge} · Turn {turn}")),
                                                ),
                                        ),
                                )
                                .child(icon_button(
                                    ("vault-code-copy", ix),
                                    KissIcon::Copy,
                                    false,
                                    p,
                                    move |_, w, cx| {
                                        cx.stop_propagation();
                                        h_copy(VaultAction::CopySnippet(c_copy.clone()), w, cx);
                                    },
                                )),
                        );
                    }
                    VaultAiArtifact::GeneratedMedia { turn, item } => {
                        let ic = match item.kind {
                            MediaKind::Image => KissIcon::Image,
                            MediaKind::Video => KissIcon::Video,
                            MediaKind::Audio => KissIcon::Audio,
                        };
                        let label = match item.kind {
                            MediaKind::Image => "Generated image",
                            MediaKind::Video => "Generated video",
                            MediaKind::Audio => "Generated audio",
                        };
                        let item_open = item.clone();
                        let h_open = on_action.clone();
                        list = list.child(
                            h_flex()
                                .id(("vault-ai-media", ix))
                                .w_full()
                                .p(px(8.0))
                                .rounded(px(8.0))
                                .bg(card)
                                .border_1()
                                .border_color(border)
                                .items_center()
                                .gap(px(8.0))
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
                                .on_click(move |_, w, cx| {
                                    h_open(
                                        VaultAction::InspectGeneratedMedia(item_open.clone()),
                                        w,
                                        cx,
                                    );
                                })
                                .child(kiss_icon(ic, 14.0, accent))
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .overflow_hidden()
                                        .gap(px(2.0))
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .font_medium()
                                                .text_color(ink)
                                                .truncate()
                                                .child(label),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(muted)
                                                .child(format!("{} · Turn {turn}", item.mime_type)),
                                        ),
                                ),
                        );
                    }
                }
            }
        }

        if show_up && !items.uploads.is_empty() {
            list = list.child(field_label("Uploaded attachments", p));
            for (ix, (turn, att)) in items.uploads.into_iter().enumerate() {
                let att_open = att.clone();
                let att_re = att.clone();
                let h_open = on_action.clone();
                let h_re = on_action.clone();
                let turn_lbl = if turn == 0 {
                    "Staged in composer".to_string()
                } else {
                    format!("Turn {turn}")
                };
                list = list.child(
                    h_flex()
                        .id(("vault-up", ix))
                        .w_full()
                        .p(px(8.0))
                        .rounded(px(8.0))
                        .bg(card)
                        .border_1()
                        .border_color(border)
                        .items_center()
                        .justify_between()
                        .gap(px(6.0))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(move |_, w, cx| {
                            h_open(VaultAction::InspectAttachment(att_open.clone()), w, cx);
                        })
                        .child(
                            v_flex()
                                .flex_1()
                                .overflow_hidden()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_medium()
                                        .text_color(ink)
                                        .truncate()
                                        .child(att.file_name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(muted)
                                        .child(format!("{} · {turn_lbl}", att.mime_type)),
                                ),
                        )
                        .children((turn > 0).then(|| {
                            icon_button(
                                ("vault-reatt", ix),
                                KissIcon::Plus,
                                false,
                                p,
                                move |_, w, cx| {
                                    cx.stop_propagation();
                                    h_re(VaultAction::ReattachAttachment(att_re.clone()), w, cx);
                                },
                            )
                        })),
                );
            }
        }

        if show_src && !items.sources.is_empty() {
            list = list.child(field_label("Web sources", p));
            for (ix, (turn, src)) in items.sources.into_iter().enumerate() {
                let url = src.url.clone();
                let h_open = on_action.clone();
                list = list.child(
                    h_flex()
                        .id(("vault-src", ix))
                        .w_full()
                        .p(px(8.0))
                        .rounded(px(8.0))
                        .bg(card)
                        .border_1()
                        .border_color(border)
                        .items_center()
                        .justify_between()
                        .gap(px(6.0))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(move |_, w, cx| h_open(VaultAction::OpenUrl(url.clone()), w, cx))
                        .child(
                            v_flex()
                                .flex_1()
                                .overflow_hidden()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_medium()
                                        .text_color(accent)
                                        .truncate()
                                        .child(if src.title.is_empty() {
                                            src.url.clone()
                                        } else {
                                            src.title.clone()
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(muted)
                                        .truncate()
                                        .child(format!("Turn {turn} · {}", src.url)),
                                ),
                        )
                        .child(kiss_icon(KissIcon::ExternalLink, 12.0, muted)),
                );
            }
        }
    }

    v_flex()
        .w(px(300.0))
        .h_full()
        .flex_shrink_0()
        .bg(sheet)
        .p(px(12.0))
        .gap(px(10.0))
        .child(header)
        .child(filter_chips)
        .child(h_divider(p))
        .child(list)
        .into_any_element()
}

/// Chat Notes & Memory collapsible drawer matching `EmptyChatAndNotes.kt` & `desktop-notes.png` 1:1.
#[allow(clippy::too_many_arguments)]
pub fn render_chat_notes_panel(
    system_prompt_input: &Entity<TextareaState>,
    authors_note_input: &Entity<TextareaState>,
    story_summary_input: &Entity<TextareaState>,
    is_streaming: bool,
    has_messages: bool,
    can_compact: bool,
    palette: &ParchmentPalette,
    on_action: &NotesActionHandler,
) -> AnyElement {
    let p = palette;
    let h_close = on_action.clone();
    let h_sum = on_action.clone();
    let h_comp = on_action.clone();
    let h_md = on_action.clone();
    let h_clr = on_action.clone();

    div()
        .w_full()
        .flex()
        .justify_center()
        .bg(p.elevated_bg.with_alpha(0.45))
        .border_b_1()
        .border_color(p.subtle_border())
        .px(px(16.0))
        .py(px(10.0))
        .child(
            v_flex()
                .id("chat-notes-scroll")
                .w_full()
                .max_w(px(p.max_reading_width_px))
                .max_h(px(340.0))
                .p(px(14.0))
                .rounded(px(12.0))
                .bg(p.card_bg.to_hsla())
                .border_1()
                .border_color(p.subtle_border())
                .gap(px(8.0))
                .overflow_y_scrollbar()
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(15.0))
                                .font_semibold()
                                .text_color(p.primary_ink.to_hsla())
                                .child("Chat notes & memory"),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(6.0))
                                .children(has_messages.then(|| {
                                    sepia_button(
                                        "notes-export-md",
                                        "Export .MD",
                                        None,
                                        SepiaButtonStyle::Outline,
                                        true,
                                        false,
                                        p,
                                        move |_, w, cx| h_md(NotesAction::ExportMarkdown, w, cx),
                                    )
                                }))
                                .children(has_messages.then(|| {
                                    sepia_button(
                                        "notes-clear",
                                        "Clear chat",
                                        None,
                                        SepiaButtonStyle::Ghost,
                                        true,
                                        false,
                                        p,
                                        move |_, w, cx| h_clr(NotesAction::ClearMessages, w, cx),
                                    )
                                }))
                                .child(icon_button(
                                    "notes-close",
                                    KissIcon::Close,
                                    false,
                                    p,
                                    move |_, w, cx| h_close(NotesAction::CloseNotes, w, cx),
                                )),
                        ),
                )
                .child(h_divider(p))
                .child(field_label("Extra instructions for this chat", p))
                .child(Textarea::new(system_prompt_input).h(px(64.0)))
                .child(field_label("Style & tone reminder", p))
                .child(Textarea::new(authors_note_input).h(px(52.0)))
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_semibold()
                                .text_color(p.narration_ink.to_hsla())
                                .child("Chat summary & memory"),
                        )
                        .child(
                            h_flex()
                                .gap(px(6.0))
                                .child(sepia_button(
                                    "notes-summarize",
                                    if is_streaming {
                                        "Summarize & resume"
                                    } else {
                                        "Summarize"
                                    },
                                    Some(KissIcon::Sparkles),
                                    SepiaButtonStyle::Outline,
                                    true,
                                    !has_messages,
                                    p,
                                    move |_, w, cx| h_sum(NotesAction::SummarizeChat, w, cx),
                                ))
                                .child(sepia_button(
                                    "notes-compact",
                                    if is_streaming {
                                        "Shorten & continue"
                                    } else {
                                        strings::SHORTEN_CONVERSATION
                                    },
                                    Some(KissIcon::Zap),
                                    SepiaButtonStyle::Secondary,
                                    true,
                                    !can_compact,
                                    p,
                                    move |_, w, cx| h_comp(NotesAction::CompactContext, w, cx),
                                )),
                        ),
                )
                .child(Textarea::new(story_summary_input).h(px(96.0))),
        )
        .into_any_element()
}
