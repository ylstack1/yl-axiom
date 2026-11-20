//! Parchment's crisp 24-unit stroke SVG icon family (`KissIconGlyphs.kt`) and built-in
//! AI Lab SVG brand icons (`ParchmentIcons.kt`).
//!
//! Rendered natively via GPUI SVG (`gpui_component::Icon::default().data(...)`), inheriting
//! exact theme ink colors via `currentColor` with zero raw emoji in controls.

use gpui::{AnyElement, Hsla, IntoElement as _, Styled as _, px};
use gpui_component::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KissIcon {
    Menu,
    SidebarClose,
    Settings,
    Search,
    Plus,
    Close,
    Trash,
    Copy,
    Markdown,
    Edit,
    Sparkles,
    Refresh,
    Rewind,
    ArrowLeft,
    ArrowRight,
    ArrowDown,
    ChevronsUp,
    ChevronsDown,
    ChevronDown,
    ChevronUp,
    ChevronRight,
    MapTree,
    GitBranch,
    Brain,
    Paperclip,
    Globe,
    Image,
    Video,
    Audio,
    Translate,
    Code,
    Diff,
    WrapText,
    Clock,
    Zap,
    SendEnter,
    Eye,
    EyeOff,
    HeartPulse,
    Pin,
    Folder,
    BookOpen,
    Cloud,
    Download,
    Upload,
    Monitor,
    ServerNetwork,
    Bot,
    MessageCircle,
    Flask,
    Compass,
    Theater,
    Palette,
    Crown,
    Flame,
    Check,
    Play,
    Stop,
    CircleDot,
    CircleEmpty,
    Target,
    FileText,
    Tabs,
    Mic,
    VolumeOn,
    VolumeOff,
    Terminal,
    ShieldCheck,
    Cpu,
    User,
    Wrench,
    Calculator,
    Dice,
    ExternalLink,
    ZoomIn,
    ZoomOut,
    Keyboard,
    MoreHorizontal,
}

/// The 18 canonical assistant icon choices stored as legacy-compatible emoji keys
/// (rendered exclusively as crisp `KissIcon` vectors, never raw OS emoji).
pub const ASSISTANT_ICON_CHOICES: [(&str, KissIcon); 18] = [
    ("💬", KissIcon::MessageCircle),
    ("🤖", KissIcon::Bot),
    ("🧪", KissIcon::Flask),
    ("✏️", KissIcon::Edit),
    ("📚", KissIcon::BookOpen),
    ("⚡", KissIcon::Zap),
    ("🎭", KissIcon::Theater),
    ("🧭", KissIcon::Compass),
    ("🧠", KissIcon::Brain),
    ("🌐", KissIcon::Globe),
    ("🎯", KissIcon::Target),
    ("💻", KissIcon::Monitor),
    ("🔧", KissIcon::Wrench),
    ("✦", KissIcon::Sparkles),
    ("🛡", KissIcon::ShieldCheck),
    ("🎨", KissIcon::Palette),
    ("👑", KissIcon::Crown),
    ("🔥", KissIcon::Flame),
];

pub fn assistant_icon_from_symbol(symbol: &str) -> KissIcon {
    let trimmed = symbol.trim();
    for &(key, icon) in &ASSISTANT_ICON_CHOICES {
        if trimmed == key || trimmed.starts_with(key) {
            return icon;
        }
    }
    match trimmed {
        "👤" => KissIcon::User,
        "✎" | "✏" => KissIcon::Edit,
        "⚙" | "⚙️" => KissIcon::Settings,
        _ => KissIcon::MessageCircle,
    }
}

pub fn tool_kiss_icon(tool_name: &str) -> KissIcon {
    match tool_name.trim().to_ascii_lowercase().as_str() {
        "fs_read" => KissIcon::FileText,
        "fs_edit" | "fs_write" | "fs_multi_edit" => KissIcon::Diff,
        "fs_list" | "fs_glob" | "fs_move" => KissIcon::Folder,
        "fs_delete" => KissIcon::Trash,
        "fs_search" => KissIcon::Search,
        "web_fetch" | "web_search" => KissIcon::Globe,
        "calc_eval" => KissIcon::Calculator,
        "shell_exec" => KissIcon::Terminal,
        "todo_write" => KissIcon::Target,
        "goal_complete" => KissIcon::Check,
        "ask_user" => KissIcon::User,
        "skill_load" | "agent_memory" => KissIcon::BookOpen,
        "agent_delegate" => KissIcon::Sparkles,
        "dice_roll" => KissIcon::Dice,
        _ => KissIcon::Wrench,
    }
}

fn svg_bytes(icon: KissIcon) -> &'static [u8] {
    macro_rules! icon_svg {
        ($path:literal) => {
            concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.85" stroke-linecap="round" stroke-linejoin="round">"#,
                $path,
                r#"</svg>"#
            )
            .as_bytes()
        };
    }

    match icon {
        KissIcon::Menu => icon_svg!(r#"<path d="M4 6h16M4 12h16M4 18h16"/>"#),
        KissIcon::SidebarClose => icon_svg!(
            r#"<rect x="3" y="3" width="18" height="18" rx="2.5"/><path d="M9 3v18M16 9l-3 3 3 3"/>"#
        ),
        KissIcon::Settings => icon_svg!(
            r#"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>"#
        ),
        KissIcon::Search => icon_svg!(r#"<circle cx="11" cy="11" r="7"/><path d="M16 16l5 5"/>"#),
        KissIcon::Plus => icon_svg!(r#"<path d="M12 5v14M5 12h14"/>"#),
        KissIcon::Close => icon_svg!(r#"<path d="M6 6l12 12M18 6L6 18"/>"#),
        KissIcon::Trash => icon_svg!(
            r#"<path d="M3.5 6h17M8.5 6V4.5A1.5 1.5 0 0 1 10 3h4a1.5 1.5 0 0 1 1.5 1.5V6M18.5 6l-.8 13a2 2 0 0 1-2 2H8.3a2 2 0 0 1-2-2L5.5 6M10 11v5.5M14 11v5.5"/>"#
        ),
        KissIcon::Copy => icon_svg!(
            r#"<rect x="9" y="9" width="12" height="12" rx="2.5"/><path d="M5.5 15A2.5 2.5 0 0 1 3 12.5v-7A2.5 2.5 0 0 1 5.5 3h7A2.5 2.5 0 0 1 15 5.5"/>"#
        ),
        KissIcon::Markdown => icon_svg!(
            r#"<rect x="2.5" y="5" width="19" height="14" rx="2.5"/><path d="M6.5 15V9l3 3.5 3-3.5v6M17 9v6M15 13l2 2 2-2"/>"#
        ),
        KissIcon::Edit => icon_svg!(
            r#"<path d="M16.5 3.6a2.1 2.1 0 0 1 3 0l.9.9a2.1 2.1 0 0 1 0 3L8 20l-4.5.5L4 16ZM14.5 5.5l4 4"/>"#
        ),
        KissIcon::Sparkles => icon_svg!(
            r#"<path d="M12 3c.6 4.9 3.1 7.4 9 9-5.9 1.6-8.4 4.1-9 9-.6-4.9-3.1-7.4-9-9 5.9-1.6 8.4-4.1 9-9Z"/>"#
        ),
        KissIcon::Refresh => {
            icon_svg!(r#"<path d="M20.5 12a8.5 8.5 0 1 1-2.6-6.1L20.5 8M20.5 3.5V8H16"/>"#)
        }
        KissIcon::Rewind => {
            icon_svg!(r#"<path d="M3.5 12a8.5 8.5 0 1 0 2.6-6.1L3.5 8M3.5 3.5V8H8"/>"#)
        }
        KissIcon::ArrowLeft => icon_svg!(r#"<path d="M20 12H4M10 6l-6 6 6 6"/>"#),
        KissIcon::ArrowRight => icon_svg!(r#"<path d="M4 12h16M14 6l6 6-6 6"/>"#),
        KissIcon::ArrowDown => icon_svg!(r#"<path d="M12 5v14M6 13l6 6 6-6"/>"#),
        KissIcon::ChevronsUp => icon_svg!(r#"<path d="M6 17l6-6 6 6M6 11l6-6 6 6"/>"#),
        KissIcon::ChevronsDown => icon_svg!(r#"<path d="M6 7l6 6 6-6M6 13l6 6 6-6"/>"#),
        KissIcon::ChevronDown => icon_svg!(r#"<path d="M6 9l6 6 6-6"/>"#),
        KissIcon::ChevronUp => icon_svg!(r#"<path d="M6 15l6-6 6 6"/>"#),
        KissIcon::ChevronRight => icon_svg!(r#"<path d="M9 6l6 6-6 6"/>"#),
        KissIcon::MapTree => icon_svg!(
            r#"<circle cx="6" cy="5.5" r="2.5"/><circle cx="18" cy="11" r="2.5"/><circle cx="18" cy="19" r="2.5"/><path d="M6 8v8a3 3 0 0 0 3 3h6.5M6 8a3 3 0 0 0 3 3h6.5"/>"#
        ),
        KissIcon::GitBranch => icon_svg!(
            r#"<circle cx="6" cy="5.5" r="2.5"/><circle cx="6" cy="18.5" r="2.5"/><circle cx="18" cy="5.5" r="2.5"/><path d="M6 8v8M18 8a6 6 0 0 1-6 6H8.5"/>"#
        ),
        KissIcon::Brain => icon_svg!(
            r#"<path d="M12 5a3 3 0 0 0-5.7-1.3A3.5 3.5 0 0 0 4 10a3.5 3.5 0 0 0 .8 6 3.5 3.5 0 0 0 6.2 2.5c.6-.4 1-1 1-1.7ZM12 5a3 3 0 0 1 5.7-1.3A3.5 3.5 0 0 1 20 10a3.5 3.5 0 0 1-.8 6 3.5 3.5 0 0 1-6.2 2.5c-.6-.4-1-1-1-1.7Z"/>"#
        ),
        KissIcon::Paperclip => icon_svg!(
            r#"<path d="M20.5 11.5l-8.6 8.6a5.5 5.5 0 0 1-7.8-7.8l8.9-8.9a3.7 3.7 0 0 1 5.2 5.2l-8.9 8.9a1.8 1.8 0 0 1-2.6-2.6l8.1-8.1"/>"#
        ),
        KissIcon::Globe => icon_svg!(
            r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.6 2.6 4 5.6 4 9s-1.4 6.4-4 9c-2.6-2.6-4-5.6-4-9s1.4-6.4 4-9Z"/>"#
        ),
        KissIcon::Image => icon_svg!(
            r#"<rect x="3" y="4" width="18" height="16" rx="2.5"/><circle cx="8.5" cy="9.5" r="1.5"/><path d="M21 15l-4.5-4.5L7 20"/>"#
        ),
        KissIcon::Video => icon_svg!(
            r#"<rect x="2.5" y="6" width="13.5" height="12" rx="2.5"/><path d="M16 10l5.5-3v10L16 14"/>"#
        ),
        KissIcon::Audio => icon_svg!(
            r#"<path d="M9 18V6l10.5-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="17" cy="16" r="2.5"/>"#
        ),
        KissIcon::Translate => icon_svg!(
            r#"<path d="M4 5.5h9M8.5 3.5v2M11 5.5c-.8 3.7-3.4 6.7-7 8.5M6 8.5c1.2 2.5 3.2 4.3 6 5.5M13.5 20.5l3.5-9 3.5 9M14.7 17.5h4.6"/>"#
        ),
        KissIcon::Code => icon_svg!(r#"<path d="M8 7l-5 5 5 5M16 7l5 5-5 5M13.5 5l-3 14"/>"#),
        KissIcon::Diff => icon_svg!(r#"<path d="M12 4v8M8 8h8M8 18h8"/>"#),
        KissIcon::WrapText => icon_svg!(
            r#"<path d="M3.5 6h17M3.5 12h13a3 3 0 0 1 0 6h-4M14 15.5L11.5 18 14 20.5M3.5 18h4.5"/>"#
        ),
        KissIcon::Clock => {
            icon_svg!(r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/>"#)
        }
        KissIcon::Zap => icon_svg!(r#"<path d="M13 2.5L4.5 13h6.5l-1 8.5L19.5 11H13Z"/>"#),
        KissIcon::SendEnter => icon_svg!(r#"<path d="M19.5 5v6a3 3 0 0 1-3 3H5M9 10l-4 4 4 4"/>"#),
        KissIcon::Eye => icon_svg!(
            r#"<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12Z"/><circle cx="12" cy="12" r="3"/>"#
        ),
        KissIcon::EyeOff => icon_svg!(
            r#"<path d="M4 4l16 16M9.6 5.9A9 9 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a17 17 0 0 1-2.7 3.5M6.2 7.6C3.7 9.3 2.5 12 2.5 12S6 18.5 12 18.5c1.9 0 3.6-.6 5-1.5M10 10a3 3 0 0 0 4.1 4.1"/>"#
        ),
        KissIcon::HeartPulse => icon_svg!(
            r#"<path d="M12 20s-7.5-4.8-9-9.5A5.2 5.2 0 0 1 12 6.6a5.2 5.2 0 0 1 9 3.9c-1.5 4.7-9 9.5-9 9.5Z"/>"#
        ),
        KissIcon::Pin => {
            icon_svg!(r#"<path d="M9 3.5h6M10 3.5v6.7L6.5 14v1.5h11V14L14 10.2V3.5M12 15.5V21"/>"#)
        }
        KissIcon::Folder => icon_svg!(
            r#"<path d="M3 7.5A2.5 2.5 0 0 1 5.5 5h3.4a2 2 0 0 1 1.6.8l1 1.4a2 2 0 0 0 1.6.8h5.4A2.5 2.5 0 0 1 21 10.5v6a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 16.5Z"/>"#
        ),
        KissIcon::BookOpen => icon_svg!(
            r#"<path d="M12 7c-1.8-1.6-4.5-2.5-9-2.5v14c4.5 0 7.2.9 9 2.5 1.8-1.6 4.5-2.5 9-2.5v-14c-4.5 0-7.2.9-9 2.5ZM12 7v14"/>"#
        ),
        KissIcon::Cloud => icon_svg!(
            r#"<path d="M7 19a4.5 4.5 0 0 1-.7-8.9A6 6 0 0 1 17.8 9.5 4.8 4.8 0 0 1 17.5 19Z"/>"#
        ),
        KissIcon::Download => icon_svg!(r#"<path d="M12 4v12M7 11.5l5 5 5-5M4 20.5h16"/>"#),
        KissIcon::Upload => icon_svg!(r#"<path d="M12 16.5V4.5M7 9.5l5-5 5 5M4 20.5h16"/>"#),
        KissIcon::Monitor => icon_svg!(
            r#"<rect x="2.5" y="3.5" width="19" height="13.5" rx="2.5"/><path d="M8 21h8M12 17v4"/>"#
        ),
        KissIcon::ServerNetwork => icon_svg!(
            r#"<circle cx="12" cy="5.5" r="2.75"/><circle cx="5.5" cy="18" r="2.75"/><circle cx="18.5" cy="18" r="2.75"/><path d="M10.6 7.9l-3.8 7.7M13.4 7.9l3.8 7.7M8.25 18h7.5"/>"#
        ),
        KissIcon::Bot => icon_svg!(
            r#"<rect x="4" y="8" width="16" height="12" rx="3"/><path d="M12 3.5V8M8.5 13v2M15.5 13v2M2 13.5v2M22 13.5v2"/>"#
        ),
        KissIcon::MessageCircle => icon_svg!(
            r#"<path d="M6.5 4h11A3.5 3.5 0 0 1 21 7.5V14a3.5 3.5 0 0 1-3.5 3.5H12l-5 3.5v-3.5h-.5A3.5 3.5 0 0 1 3 14V7.5A3.5 3.5 0 0 1 6.5 4Z"/>"#
        ),
        KissIcon::Flask => icon_svg!(
            r#"<path d="M9 3.5h6M10 3.5v5.2L5 18a2 2 0 0 0 1.7 3h10.6A2 2 0 0 0 19 18l-5-9.3V3.5M7.5 15h9"/>"#
        ),
        KissIcon::Compass => {
            icon_svg!(r#"<circle cx="12" cy="12" r="9"/><path d="M15.5 8.5l-2 5-5 2 2-5Z"/>"#)
        }
        KissIcon::Theater => icon_svg!(
            r#"<path d="M4 5.5c2.5-1 5.5-1 8 0v6a4 4 0 0 1-8 0ZM12 8.5c2.5-1 5.5-1 8 0v6a4 4 0 0 1-8 0Z"/>"#
        ),
        KissIcon::Palette => icon_svg!(
            r#"<path d="M12 3a9 9 0 1 0 0 18h1.5a2 2 0 0 0 1.5-3.3 1.8 1.8 0 0 1 1.4-3h1.6A4 4 0 0 0 21 11a8 8 0 0 0-9-8Z"/><circle cx="7.5" cy="10" r="1"/><circle cx="10.5" cy="7" r="1"/><circle cx="15" cy="8" r="1"/>"#
        ),
        KissIcon::Crown => {
            icon_svg!(r#"<path d="M3.5 18l1.5-10 4.5 4L12 6l2.5 6 4.5-4 1.5 10Z"/>"#)
        }
        KissIcon::Flame => icon_svg!(
            r#"<path d="M12 3c2.5 3 6 5.5 6 10a6 6 0 0 1-12 0c0-2.5 1.2-4.7 2.5-6.2.3 1.8 1.3 2.8 2.2 3.2C10.5 7.5 11 5 12 3Z"/>"#
        ),
        KissIcon::Check => icon_svg!(r#"<path d="M4.5 12.5l5 5 10-11"/>"#),
        KissIcon::Play => icon_svg!(r#"<path d="M7 4.5L19 12 7 19.5Z"/>"#),
        KissIcon::Stop => {
            icon_svg!(
                r#"<rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none"/>"#
            )
        }
        KissIcon::CircleDot => {
            icon_svg!(r#"<circle cx="12" cy="12" r="4.5" fill="currentColor" stroke="none"/>"#)
        }
        KissIcon::CircleEmpty => icon_svg!(r#"<circle cx="12" cy="12" r="7.5"/>"#),
        KissIcon::Target => icon_svg!(
            r#"<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="5"/><circle cx="12" cy="12" r="1.5"/>"#
        ),
        KissIcon::FileText => icon_svg!(
            r#"<path d="M14 3H7.5A2.5 2.5 0 0 0 5 5.5v13A2.5 2.5 0 0 0 7.5 21h9a2.5 2.5 0 0 0 2.5-2.5V8ZM14 3v5h5M9 13h6M9 17h4.5"/>"#
        ),
        KissIcon::Tabs => icon_svg!(
            r#"<rect x="3.5" y="7" width="13.5" height="13.5" rx="2.5"/><path d="M7.5 3.5H18A2.5 2.5 0 0 1 20.5 6v10.5"/>"#
        ),
        KissIcon::Mic => icon_svg!(
            r#"<rect x="9" y="3.5" width="6" height="10" rx="3"/><path d="M6 10.5a6 6 0 0 0 12 0M12 16.5V21M9 21h6"/>"#
        ),
        KissIcon::VolumeOn => icon_svg!(
            r#"<path d="M11 5.5L6.5 9H3.5v6h3L11 18.5ZM15.5 9a4.5 4.5 0 0 1 0 6M18.2 6.5a8 8 0 0 1 0 11"/>"#
        ),
        KissIcon::VolumeOff => {
            icon_svg!(r#"<path d="M11 5.5L6.5 9H3.5v6h3L11 18.5ZM16 9.5l5 5M21 9.5l-5 5"/>"#)
        }
        KissIcon::Terminal => icon_svg!(
            r#"<rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M7 9l3.5 3L7 15M12.5 15H17"/>"#
        ),
        KissIcon::ShieldCheck => icon_svg!(
            r#"<path d="M12 3l7.5 2.8v5.7c0 4.7-3.1 7.9-7.5 9.5-4.4-1.6-7.5-4.8-7.5-9.5V5.8ZM8.8 12l2.3 2.3 4.2-4.6"/>"#
        ),
        KissIcon::Cpu => icon_svg!(
            r#"<rect x="6" y="6" width="12" height="12" rx="2"/><rect x="9.5" y="9.5" width="5" height="5" rx="1"/><path d="M9.5 2.5V6M14.5 2.5V6M9.5 18v3.5M14.5 18v3.5M2.5 9.5H6M2.5 14.5H6M18 9.5h3.5M18 14.5h3.5"/>"#
        ),
        KissIcon::User => {
            icon_svg!(r#"<circle cx="12" cy="8.5" r="3.5"/><path d="M5 20a7 7 0 0 1 14 0"/>"#)
        }
        KissIcon::Wrench => icon_svg!(
            r#"<path d="M14.5 6.5a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.3-3.3a6 6 0 0 1-7.9 7.9l-6.8 6.8a2.1 2.1 0 0 1-3-3l6.8-6.8a6 6 0 0 1 7.9-7.9Z"/>"#
        ),
        KissIcon::Calculator => icon_svg!(
            r#"<rect x="5" y="3" width="14" height="18" rx="2"/><path d="M8.5 7h7M8.5 12h.01M12 12h.01M15.5 12h.01M8.5 16h.01M12 16h.01M15.5 16h.01"/>"#
        ),
        KissIcon::Dice => icon_svg!(
            r#"<rect x="4" y="4" width="16" height="16" rx="2.5"/><circle cx="9" cy="9" r="1"/><circle cx="15" cy="9" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="9" cy="15" r="1"/><circle cx="15" cy="15" r="1"/>"#
        ),
        KissIcon::ExternalLink => icon_svg!(
            r#"<path d="M14 4h6v6M20 4l-9.5 9.5M18 14v4a2.5 2.5 0 0 1-2.5 2.5h-9A2.5 2.5 0 0 1 4 18V8.5A2.5 2.5 0 0 1 6.5 6H10"/>"#
        ),
        KissIcon::ZoomIn => {
            icon_svg!(r#"<circle cx="11" cy="11" r="7"/><path d="M16 16l5 5M11 8v6M8 11h6"/>"#)
        }
        KissIcon::ZoomOut => {
            icon_svg!(r#"<circle cx="11" cy="11" r="7"/><path d="M16 16l5 5M8 11h6"/>"#)
        }
        KissIcon::Keyboard => icon_svg!(
            r#"<rect x="2.5" y="5.5" width="19" height="13" rx="2.5"/><path d="M6 9.5h.01M10 9.5h.01M14 9.5h.01M18 9.5h.01M7 14h10"/>"#
        ),
        KissIcon::MoreHorizontal => icon_svg!(
            r#"<circle cx="5.5" cy="12" r="1.6" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none"/><circle cx="18.5" cy="12" r="1.6" fill="currentColor" stroke="none"/>"#
        ),
    }
}

/// Renders one `KissIcon` at an even pixel size (`10`, `12`, `14`, `16`, `20`) tinted to `color`.
pub fn kiss_icon(icon: KissIcon, size_px: f32, color: impl Into<Hsla>) -> AnyElement {
    Icon::default()
        .data(svg_bytes(icon))
        .size(px(size_px))
        .text_color(color.into())
        .into_any_element()
}

fn match_known_lab_slug(raw: &str) -> Option<&'static str> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    if s.contains("gemini")
        || s.contains("gemma")
        || s.contains("learnlm")
        || s.contains("deep-research")
        || s.contains("nano-banana")
        || s.contains("palm")
    {
        return Some("gemini");
    }
    if s.contains("claude") {
        return Some("claude");
    }
    if s.contains("anthropic") {
        return Some("anthropic");
    }
    if s.contains("doubao") || s.contains("seedream") || s.contains("seedance") {
        return Some("doubao");
    }
    if s.contains("bytedance") || s.contains("volcengine") || s.contains("byteplus") {
        return Some("bytedance");
    }
    if s.contains("deepseek") {
        return Some("deepseek");
    }
    if s.contains("grok") || s.contains("xai") || s.contains("x-ai") {
        return Some("grok");
    }
    if s.contains("qwen") || s.contains("qwq") || s.contains("tongyi") || s.contains("alibaba") {
        return Some("qwen");
    }
    if s.contains("mistral") || s.contains("mixtral") || s.contains("codestral") {
        return Some("mistral");
    }
    if s.contains("llama") || s.contains("meta") {
        return Some("meta");
    }
    if s.contains("glm") || s.contains("zhipu") || s.contains("zai") {
        return Some("zhipu");
    }
    if s.contains("kimi") || s.contains("moonshot") {
        return Some("kimi");
    }
    if s.contains("minimax") || s.contains("hailuo") {
        return Some("minimax");
    }
    if s.contains("perplexity") || s.contains("sonar") {
        return Some("perplexity");
    }
    if s.contains("cohere") || s.contains("command-r") {
        return Some("cohere");
    }
    if s.contains("google") || s.contains("imagen") || s.contains("veo") {
        return Some("google");
    }
    if s.contains("gpt")
        || s.contains("openai")
        || s.contains("chatgpt")
        || s.contains("codex")
        || s.contains("dall-e")
        || s.contains("sora")
        || s.contains("whisper")
        || s.starts_with("o1")
        || s.starts_with("o3")
        || s.starts_with("o4")
        || s.contains("/o1")
        || s.contains("/o3")
        || s.contains("/o4")
    {
        return Some("openai");
    }
    if s.contains("openrouter") {
        return Some("openrouter");
    }
    None
}

fn lab_svg_bytes(slug: &str) -> Option<&'static [u8]> {
    macro_rules! lab_svg {
        ($body:literal) => {
            concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd">"#,
                $body,
                r#"</svg>"#
            )
            .as_bytes()
        };
    }

    Some(match slug {
        "gemini" => lab_svg!(
            r#"<path d="M20.616 10.835a14.147 14.147 0 01-4.45-3.001 14.111 14.111 0 01-3.678-6.452.503.503 0 00-.975 0 14.134 14.134 0 01-3.679 6.452 14.155 14.155 0 01-4.45 3.001c-.65.28-1.318.505-2.002.678a.502.502 0 000 .975c.684.172 1.35.397 2.002.677a14.147 14.147 0 014.45 3.001 14.112 14.112 0 013.679 6.453.502.502 0 00.975 0c.172-.685.397-1.351.677-2.003a14.145 14.145 0 013.001-4.45 14.113 14.113 0 016.453-3.678.503.503 0 000-.975 13.245 13.245 0 01-2.003-.678z"/>"#
        ),
        "openai" => lab_svg!(
            r#"<path d="M9.205 8.658v-2.26c0-.19.072-.333.238-.428l4.543-2.616c.619-.357 1.356-.523 2.117-.523 2.854 0 4.662 2.212 4.662 4.566 0 .167 0 .357-.024.547l-4.71-2.759a.797.797 0 00-.856 0l-5.97 3.473zm10.609 8.8V12.06c0-.333-.143-.57-.429-.737l-5.97-3.473 1.95-1.118a.433.433 0 01.476 0l4.543 2.617c1.309.76 2.189 2.378 2.189 3.948 0 1.808-1.07 3.473-2.76 4.163zM7.802 12.703l-1.95-1.142c-.167-.095-.239-.238-.239-.428V5.899c0-2.545 1.95-4.472 4.591-4.472 1 0 1.927.333 2.712.928L8.23 5.067c-.285.166-.428.404-.428.737v6.898zM12 15.128l-2.795-1.57v-3.33L12 8.658l2.795 1.57v3.33L12 15.128zm1.796 7.23c-1 0-1.927-.332-2.712-.927l4.686-2.712c.285-.166.428-.404.428-.737v-6.898l1.974 1.142c.167.095.238.238.238.428v5.233c0 2.545-1.974 4.472-4.614 4.472zm-5.637-5.303l-4.544-2.617c-1.308-.761-2.188-2.378-2.188-3.948A4.482 4.482 0 014.21 6.327v5.423c0 .333.143.571.428.738l5.947 3.449-1.95 1.118a.432.432 0 01-.476 0zm-.262 3.9c-2.688 0-4.662-2.021-4.662-4.519 0-.19.024-.38.047-.57l4.686 2.71c.286.167.571.167.856 0l5.97-3.448v2.26c0 .19-.07.333-.237.428l-4.543 2.616c-.619.357-1.356.523-2.117.523zm5.899 2.83a5.947 5.947 0 005.827-4.756C22.287 18.339 24 15.84 24 13.296c0-1.665-.713-3.282-1.998-4.448.119-.5.19-.999.19-1.498 0-3.401-2.759-5.947-5.946-5.947-.642 0-1.26.095-1.88.31A5.962 5.962 0 0010.205 0a5.947 5.947 0 00-5.827 4.757C1.713 5.447 0 7.945 0 10.49c0 1.666.713 3.283 1.998 4.448-.119.5-.19 1-.19 1.499 0 3.401 2.759 5.946 5.946 5.946.642 0 1.26-.095 1.88-.309a5.96 5.96 0 004.162 1.713z"/>"#
        ),
        "claude" => lab_svg!(
            r#"<path d="M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z"/>"#
        ),
        "anthropic" => lab_svg!(
            r#"<path d="M13.827 3.52h3.603L24 20h-3.603l-6.57-16.48zm-7.258 0h3.767L16.906 20h-3.674l-1.343-3.461H5.017l-1.344 3.46H0L6.57 3.522zm4.132 9.959L8.453 7.687 6.205 13.48H10.7z"/>"#
        ),
        "deepseek" => lab_svg!(
            r#"<path d="M23.748 4.482c-.254-.124-.364.113-.512.234-.051.039-.094.09-.137.136-.372.397-.806.657-1.373.626-.829-.046-1.537.214-2.163.848-.133-.782-.575-1.248-1.247-1.548-.352-.156-.708-.311-.955-.65-.172-.241-.219-.51-.305-.774-.055-.16-.11-.323-.293-.35-.2-.031-.278.136-.356.276-.313.572-.434 1.202-.422 1.84.027 1.436.633 2.58 1.838 3.393.137.093.172.187.129.323-.082.28-.18.552-.266.833-.055.179-.137.217-.329.14a5.526 5.526 0 01-1.736-1.18c-.857-.828-1.631-1.742-2.597-2.458a11.365 11.365 0 00-.689-.471c-.985-.957.13-1.743.388-1.836.27-.098.093-.432-.779-.428-.872.004-1.67.295-2.687.684a3.055 3.055 0 01-.465.137 9.597 9.597 0 00-2.883-.102c-1.885.21-3.39 1.102-4.497 2.623C.082 8.606-.231 10.684.152 12.85c.403 2.284 1.569 4.175 3.36 5.653 1.858 1.533 3.997 2.284 6.438 2.14 1.482-.085 3.133-.284 4.994-1.86.47.234.962.327 1.78.397.63.059 1.236-.03 1.705-.128.735-.156.684-.837.419-.961-2.155-1.004-1.682-.595-2.113-.926 1.096-1.296 2.746-2.642 3.392-7.003.05-.347.007-.565 0-.845-.004-.17.035-.237.23-.256a4.173 4.173 0 001.545-.475c1.396-.763 1.96-2.015 2.093-3.517.02-.23-.004-.467-.247-.588zM11.581 18c-2.089-1.642-3.102-2.183-3.52-2.16-.392.024-.321.471-.235.763.09.288.207.486.371.739.114.167.192.416-.113.603-.673.416-1.842-.14-1.897-.167-1.361-.802-2.5-1.86-3.301-3.307-.774-1.393-1.224-2.887-1.298-4.482-.02-.386.093-.522.477-.592a4.696 4.696 0 011.529-.039c2.132.312 3.946 1.265 5.468 2.774.868.86 1.525 1.887 2.202 2.891.72 1.066 1.494 2.082 2.48 2.914.348.292.625.514.891.677-.802.09-2.14.11-3.054-.614z"/>"#
        ),
        "grok" => lab_svg!(
            r#"<path d="M9.27 15.29l7.978-5.897c.391-.29.95-.177 1.137.272.98 2.369.542 5.215-1.41 7.169-1.951 1.954-4.667 2.382-7.149 1.406l-2.711 1.257c3.889 2.661 8.611 2.003 11.562-.953 2.341-2.344 3.066-5.539 2.388-8.42l.006.007c-.983-4.232.242-5.924 2.75-9.383.06-.082.12-.164.179-.248l-3.301 3.305v-.01L9.267 15.292M7.623 16.723c-2.792-2.67-2.31-6.801.071-9.184 1.761-1.763 4.647-2.483 7.166-1.425l2.705-1.25a7.808 7.808 0 00-1.829-1A8.975 8.975 0 005.984 5.83c-2.533 2.536-3.33 6.436-1.962 9.764 1.022 2.487-.653 4.246-2.34 6.022-.599.63-1.199 1.259-1.682 1.925l7.62-6.815"/>"#
        ),
        "qwen" => lab_svg!(
            r#"<path d="M19.713 2.825h-5.907c0-.251-.066-.503-.196-.728L12.819.728A1.46 1.46 0 0 0 11.559 0h-1.58c-.52 0-1 .275-1.26.728l-2.42 4.19a1.45 1.45 0 0 0 0 1.453l.791 1.37c.26.448.74.727 1.26.727h11.36c.52 0 1-.275 1.26-.728l.79-1.37a1.45 1.45 0 0 0 0-1.451l-.79-1.37a1.46 1.46 0 0 0-1.26-.728zM.195 9.905l2.951 5.115a1.46 1.46 0 0 0-.531.531l-.791 1.37a1.45 1.45 0 0 0 0 1.452l.79 1.37c.26.448.74.727 1.26.727h4.84c.52 0 1-.275 1.26-.728l.79-1.37a1.45 1.45 0 0 0 0-1.451L5.086 7.083a1.46 1.46 0 0 0-1.26-.728h-1.58c-.52 0-1 .276-1.26.728l-.79 1.37a1.45 1.45 0 0 0 0 1.452m15.888 13.367 2.951-5.115c.217.125.469.196.728.196h1.582c.52 0 1-.275 1.26-.728l.79-1.37a1.45 1.45 0 0 0 0-1.451l-2.42-4.191a1.46 1.46 0 0 0-1.259-.728h-1.582c-.52 0-1 .276-1.259.728l-5.678 9.838a1.45 1.45 0 0 0 0 1.452l.79 1.37c.26.448.74.727 1.26.727h1.582c.52 0 1-.275 1.26-.728z"/>"#
        ),
        "mistral" => lab_svg!(
            r#"<path d="M3.428 3.4h3.429v3.428h3.429v3.429h-.002 3.431V6.828h3.427V3.4h3.43v13.714H24v3.429H13.714v-3.428h-3.428v-3.429h-3.43v3.428h3.43v3.429H0v-3.429h3.428V3.4zm10.286 13.715h3.428v-3.429h-3.427v3.429z"/>"#
        ),
        "meta" => lab_svg!(
            r#"<path d="M6.897 4c1.915 0 3.516.932 5.43 3.376l.282-.373c.19-.246.383-.484.58-.71l.313-.35C14.588 4.788 15.792 4 17.225 4c1.273 0 2.469.557 3.491 1.516l.218.213c1.73 1.765 2.917 4.71 3.053 8.026l.011.392.002.25c0 1.501-.28 2.759-.818 3.7l-.14.23-.108.153c-.301.42-.664.758-1.086 1.009l-.265.142-.087.04a3.493 3.493 0 01-.302.118 4.117 4.117 0 01-1.33.208c-.524 0-.996-.067-1.438-.215-.614-.204-1.163-.56-1.726-1.116l-.227-.235c-.753-.812-1.534-1.976-2.493-3.586l-1.43-2.41-.544-.895-1.766 3.13-.343.592C7.597 19.156 6.227 20 4.356 20c-1.21 0-2.205-.42-2.936-1.182l-.168-.184c-.484-.573-.837-1.311-1.043-2.189l-.067-.32a8.69 8.69 0 01-.136-1.288L0 14.468c.002-.745.06-1.49.174-2.23l.1-.573c.298-1.53.828-2.958 1.536-4.157l.209-.34c1.177-1.83 2.789-3.053 4.615-3.16L6.897 4zm-.033 2.615l-.201.01c-.83.083-1.606.673-2.252 1.577l-.138.199-.01.018c-.67 1.017-1.185 2.378-1.456 3.845l-.004.022a12.591 12.591 0 00-.207 2.254l.002.188c.004.18.017.36.04.54l.043.291c.092.503.257.908.486 1.208l.117.137c.303.323.698.492 1.17.492 1.1 0 1.796-.676 3.696-3.641l2.175-3.4.454-.701-.139-.198C9.11 7.3 8.084 6.616 6.864 6.616zm10.196-.552l-.176.007c-.635.048-1.223.359-1.82.933l-.196.198c-.439.462-.887 1.064-1.367 1.807l.266.398c.18.274.362.56.55.858l.293.475 1.396 2.335.695 1.114c.583.926 1.03 1.6 1.408 2.082l.213.262c.282.326.529.54.777.673l.102.05c.227.1.457.138.718.138.176.002.35-.023.518-.073.338-.104.61-.32.813-.637l.095-.163.077-.162c.194-.459.29-1.06.29-1.785l-.006-.449c-.08-2.871-.938-5.372-2.2-6.798l-.176-.189c-.67-.683-1.444-1.074-2.27-1.074z"/>"#
        ),
        "bytedance" => lab_svg!(
            r#"<path d="M14.944 18.587l-1.704-.445V10.01l1.824-.462c1-.254 1.84-.461 1.88-.453.032 0 .056 2.235.056 4.972v4.973l-.176-.008c-.104 0-.952-.207-1.88-.446z"/><path d="M7 16.542c0-2.736.024-4.98.064-4.98.032-.008.872.2 1.88.454l1.816.461-.016 4.05-.024 4.049-1.632.422c-.896.23-1.736.445-1.856.469L7 21.523v-4.98z"/><path d="M19.24 12.477c0-9.03.008-9.515.144-9.475.072.024.784.207 1.576.406.792.207 1.576.405 1.744.445l.296.08-.016 8.56-.024 8.568-1.624.414c-.888.23-1.728.437-1.856.47l-.24.055v-9.523z"/><path d="M1 12.509c0-4.678.024-8.505.064-8.505.032 0 .872.207 1.872.454l1.824.461v7.582c0 4.16-.016 7.574-.032 7.574-.024 0-.872.215-1.88.47L1 21.013v-8.505z"/>"#
        ),
        "doubao" => lab_svg!(
            r#"<path d="M5.31 15.756c.172-3.75 1.883-5.999 2.549-6.739-3.26 2.058-5.425 5.658-6.358 8.308v1.12C1.501 21.513 4.226 24 7.59 24a6.59 6.59 0 002.2-.375c.353-.12.7-.248 1.039-.378.913-.899 1.65-1.91 2.243-2.992-4.877 2.431-7.974.072-7.763-4.5l.002.001z"/><path d="M22.57 10.283c-1.212-.901-4.109-2.404-7.397-2.8.295 3.792.093 8.766-2.1 12.773a12.782 12.782 0 01-2.244 2.992c3.764-1.448 6.746-3.457 8.596-5.219 2.82-2.683 3.353-5.178 3.361-6.66a2.737 2.737 0 00-.216-1.084v-.002zM14.303 1.867C12.955.7 11.248 0 9.39 0 7.532 0 5.883.677 4.545 1.807 2.791 3.29 1.627 5.557 1.5 8.125v9.201c.932-2.65 3.097-6.25 6.357-8.307.5-.318 1.025-.595 1.569-.829 1.883-.801 3.878-.932 5.746-.706-.222-2.83-.718-5.002-.87-5.617h.001z"/>"#
        ),
        "zhipu" => lab_svg!(
            r#"<path d="M9.671 5.365a1.697 1.697 0 011.099 2.132l-.071.172-.016.04-.018.054c-.07.16-.104.32-.104.498-.035.71.47 1.279 1.186 1.314h.366c1.309.053 2.338 1.173 2.286 2.523-.052 1.332-1.152 2.38-2.478 2.327h-.174c-.715.018-1.274.64-1.239 1.368 0 .124.018.23.053.337.209.373.54.658.96.8.75.23 1.517-.125 1.9-.782l.018-.035c.402-.64 1.17-.96 1.92-.711.854.284 1.378 1.226 1.099 2.167a1.661 1.661 0 01-2.077 1.102 1.711 1.711 0 01-.907-.711l-.017-.035c-.2-.323-.463-.58-.851-.711l-.056-.018a1.646 1.646 0 00-1.954.746 1.66 1.66 0 01-1.065.764 1.677 1.677 0 01-1.989-1.279c-.209-.906.332-1.83 1.257-2.043a1.51 1.51 0 01.296-.035h.018c.68-.071 1.151-.622 1.116-1.333a1.307 1.307 0 00-.227-.693 2.515 2.515 0 01-.366-1.403 2.39 2.39 0 01.366-1.208c.14-.195.21-.444.227-.693.018-.71-.506-1.261-1.186-1.332l-.07-.018a1.43 1.43 0 01-.299-.07l-.05-.019a1.7 1.7 0 01-1.047-2.114 1.68 1.68 0 012.094-1.101zm10.031-1.475c.925 0 1.675.764 1.675 1.706s-.75 1.705-1.675 1.705-1.674-.763-1.674-1.705c0-.942.75-1.706 1.674-1.706z"/>"#
        ),
        "kimi" => lab_svg!(
            r#"<path d="M21.846 0a1.923 1.923 0 110 3.846H20.15a.226.226 0 01-.227-.226V1.923C19.923.861 20.784 0 21.846 0z"/><path d="M11.065 11.199l7.257-7.2c.137-.136.06-.41-.116-.41H14.3a.164.164 0 00-.117.051l-7.82 7.756c-.122.12-.302.013-.302-.179V3.82c0-.127-.083-.23-.185-.23H3.186c-.103 0-.186.103-.186.23V19.77c0 .128.083.23.186.23h2.69c.103 0 .186-.102.186-.23v-3.25c0-.069.025-.135.069-.178l2.424-2.406a.158.158 0 01.205-.023l6.484 4.772a7.677 7.677 0 003.453 1.283c.108.012.2-.095.2-.23v-3.06c0-.117-.07-.212-.164-.227a5.028 5.028 0 01-2.027-.807l-5.613-4.064c-.117-.078-.132-.279-.028-.381z"/>"#
        ),
        "minimax" => lab_svg!(
            r#"<path d="M16.278 2c1.156 0 2.093.927 2.093 2.07v12.501a.74.74 0 00.744.709.74.74 0 00.743-.709V9.099a2.06 2.06 0 012.071-2.049A2.06 2.06 0 0124 9.1v6.561a.649.649 0 01-.652.645.649.649 0 01-.653-.645V9.1a.762.762 0 00-.766-.758.762.762 0 00-.766.758v7.472a2.037 2.037 0 01-2.048 2.026 2.037 2.037 0 01-2.048-2.026v-12.5a.785.785 0 00-.788-.753.785.785 0 00-.789.752l-.001 15.904A2.037 2.037 0 0113.441 22a2.037 2.037 0 01-2.048-2.026V18.04c0-.356.292-.645.652-.645.36 0 .652.289.652.645v1.934c0 .263.142.506.372.638.23.131.514.131.744 0a.734.734 0 00.372-.638V4.07c0-1.143.937-2.07 2.093-2.07zm-5.674 0c1.156 0 2.093.927 2.093 2.07v11.523a.648.648 0 01-.652.645.648.648 0 01-.652-.645V4.07a.785.785 0 00-.789-.78.785.785 0 00-.789.78v14.013a2.06 2.06 0 01-2.07 2.048 2.06 2.06 0 01-2.071-2.048V9.1a.762.762 0 00-.766-.758.762.762 0 00-.766.758v3.8a2.06 2.06 0 01-2.071 2.049A2.06 2.06 0 010 12.9v-1.378c0-.357.292-.646.652-.646.36 0 .653.29.653.646V12.9c0 .418.343.757.766.757s.766-.339.766-.757V9.099a2.06 2.06 0 012.07-2.048 2.06 2.06 0 012.071 2.048v8.984c0 .419.343.758.767.758.423 0 .766-.339.766-.758V4.07c0-1.143.937-2.07 2.093-2.07z"/>"#
        ),
        "perplexity" => lab_svg!(
            r#"<path d="M19.785 0v7.272H22.5V17.62h-2.935V24l-7.037-6.194v6.145h-1.091v-6.152L4.392 24v-6.465H1.5V7.188h2.884V0l7.053 6.494V.19h1.09v6.49L19.786 0zm-7.257 9.044v7.319l5.946 5.234V14.44l-5.946-5.397zm-1.099-.08l-5.946 5.398v7.235l5.946-5.234V8.965zm8.136 7.58h1.844V8.349H13.46l6.105 5.54v2.655zm-8.982-8.28H2.59v8.195h1.8v-2.576l6.192-5.62zM5.475 2.476v4.71h5.115l-5.115-4.71zm13.219 0l-5.115 4.71h5.115v-4.71z"/>"#
        ),
        "cohere" => lab_svg!(
            r#"<path d="M8.128 14.099c.592 0 1.77-.033 3.398-.703 1.897-.781 5.672-2.2 8.395-3.656 1.905-1.018 2.74-2.366 2.74-4.18A4.56 4.56 0 0018.1 1H7.549A6.55 6.55 0 001 7.55c0 3.617 2.745 6.549 7.128 6.549z"/><path d="M9.912 18.61a4.387 4.387 0 012.705-4.052l3.323-1.38c3.361-1.394 7.06 1.076 7.06 4.715a5.104 5.104 0 01-5.105 5.104l-3.597-.001a4.386 4.386 0 01-4.386-4.387z"/><path d="M4.776 14.962A3.775 3.775 0 001 18.738v.489a3.776 3.776 0 007.551 0v-.49a3.775 3.775 0 00-3.775-3.775z"/>"#
        ),
        "openrouter" => lab_svg!(
            r#"<path d="M18.654 3.87a5.087 5.087 0 110 10.174L23.7 19.09c.64.641.187 1.737-.72 1.737H8.48a8.479 8.479 0 010-16.958h10.175zM8.479 7.26a5.087 5.087 0 100 10.176 5.087 5.087 0 000-10.175z"/>"#
        ),
        "google" => lab_svg!(
            r#"<path d="M23 12.245c0-.905-.075-1.565-.236-2.25h-10.54v4.083h6.186c-.124 1.014-.797 2.542-2.294 3.569l-.021.136 3.332 2.53.23.022C21.779 18.417 23 15.593 23 12.245z"/><path d="M12.225 23c3.03 0 5.574-.978 7.433-2.665l-3.542-2.688c-.948.648-2.22 1.1-3.891 1.1a6.745 6.745 0 01-6.386-4.572l-.132.011-3.465 2.628-.045.124C4.043 20.531 7.835 23 12.225 23z"/><path d="M5.84 14.175A6.65 6.65 0 015.463 12c0-.758.138-1.491.361-2.175l-.006-.147-3.508-2.67-.115.054A10.831 10.831 0 001 12c0 1.772.436 3.447 1.197 4.938l3.642-2.763z"/><path d="M12.225 5.253c2.108 0 3.529.892 4.34 1.638l3.167-3.031C17.787 2.088 15.255 1 12.225 1 7.834 1 4.043 3.469 2.197 7.062l3.63 2.763a6.77 6.77 0 016.398-4.572z"/>"#
        ),
        _ => return None,
    })
}

/// Renders the official AI Lab / Model vector logo (`ModelLabSvgIcon` parity) tinted to `color`.
pub fn model_lab_icon(
    model_or_provider: &str,
    size_px: f32,
    color: impl Into<Hsla>,
    fallback_protocol: &str,
) -> AnyElement {
    let color = color.into();
    let slug = match_known_lab_slug(model_or_provider)
        .or_else(|| {
            model_or_provider
                .split_once('/')
                .and_then(|(vendor, _)| match_known_lab_slug(vendor))
        })
        .or_else(|| match_known_lab_slug(fallback_protocol));

    if let Some(bytes) = slug.and_then(lab_svg_bytes) {
        Icon::default()
            .data(bytes)
            .size(px(size_px))
            .text_color(color)
            .into_any_element()
    } else {
        kiss_icon(KissIcon::Sparkles, size_px, color)
    }
}
