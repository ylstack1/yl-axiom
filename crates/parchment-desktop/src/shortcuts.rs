//! Customizable keyboard shortcut engine matching `AppShortcuts.kt`:
//! - Persisted in `PreferencesConfig.custom_shortcuts` as canonical chords (`Ctrl+K`, `Ctrl+Shift+Tab`)
//! - Interactive capture with modifier/reserved-chord validation and conflict detection + swap.

use std::collections::BTreeMap;

use gpui::Keystroke;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutBinding {
    pub key_code: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl ShortcutBinding {
    pub fn new(key_code: &str, ctrl: bool, alt: bool, shift: bool) -> Self {
        Self {
            key_code: key_code.to_string(),
            ctrl,
            alt,
            shift,
        }
    }

    pub fn keycap_tokens(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(4);
        if self.ctrl {
            out.push("Ctrl".into());
        }
        if self.alt {
            out.push("Alt".into());
        }
        if self.shift {
            out.push("Shift".into());
        }
        out.push(self.key_code.clone());
        out
    }

    pub fn format(&self) -> String {
        self.keycap_tokens().join("+")
    }

    pub fn is_function_key(&self) -> bool {
        matches!(
            self.key_code.as_str(),
            "F1" | "F2" | "F3" | "F4" | "F5" | "F6" | "F7" | "F8" | "F9" | "F10" | "F11" | "F12"
        )
    }

    pub fn has_allowed_modifiers(&self) -> bool {
        self.ctrl || self.alt || self.is_function_key()
    }

    pub fn is_reserved(&self) -> bool {
        let k = self.key_code.as_str();
        (self.ctrl && !self.alt && !self.shift && matches!(k, "C" | "V" | "X" | "A" | "Z" | "Y"))
            || (self.ctrl && !self.alt && self.shift && k == "Z")
            || (!self.ctrl && self.alt && !self.shift && k == "F4")
            || (!self.ctrl && !self.alt && self.shift && k == "Insert")
    }

    pub fn is_valid(&self) -> bool {
        self.has_allowed_modifiers() && !self.is_reserved()
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut key_code: Option<String> = None;
        for part in trimmed.split('+').map(str::trim).filter(|s| !s.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "meta" | "mod" => ctrl = true,
                "alt" | "option" => alt = true,
                "shift" => shift = true,
                other => {
                    if key_code.is_some() {
                        return None;
                    }
                    key_code = normalize_key_code(other).map(str::to_string);
                    key_code.as_ref()?;
                }
            }
        }
        Some(Self {
            key_code: key_code?,
            ctrl,
            alt,
            shift,
        })
    }

    pub fn matches(&self, ks: &Keystroke) -> bool {
        let m = &ks.modifiers;
        let ev_ctrl = m.control || m.platform;
        if ev_ctrl != self.ctrl || m.alt != self.alt || m.shift != self.shift {
            return false;
        }
        normalize_key_code(&ks.key).is_some_and(|code| code == self.key_code)
    }
}

pub enum ShortcutCaptureResult {
    Accepted(ShortcutBinding),
    Cancelled,
    IgnoredModifier,
    Rejected(String),
}

pub fn capture_shortcut_event(ks: &Keystroke) -> ShortcutCaptureResult {
    let key = ks.key.as_str();
    let m = &ks.modifiers;
    let ctrl = m.control || m.platform;
    if key == "escape" && !ctrl && !m.alt {
        return ShortcutCaptureResult::Cancelled;
    }
    if matches!(
        key,
        "control" | "shift" | "alt" | "platform" | "meta" | "function" | "capslock"
    ) {
        return ShortcutCaptureResult::IgnoredModifier;
    }
    let Some(code) = normalize_key_code(key) else {
        return ShortcutCaptureResult::Rejected("That key cannot be used as a shortcut.".into());
    };
    let binding = ShortcutBinding::new(code, ctrl, m.alt, m.shift);
    if !binding.has_allowed_modifiers() {
        return ShortcutCaptureResult::Rejected(
            "Include Ctrl, Alt, or use a function key (F1–F12).".into(),
        );
    }
    if binding.is_reserved() {
        return ShortcutCaptureResult::Rejected(format!(
            "{} is reserved for standard system editing.",
            binding.format()
        ));
    }
    ShortcutCaptureResult::Accepted(binding)
}

fn normalize_key_code(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "a" => Some("A"),
        "b" => Some("B"),
        "c" => Some("C"),
        "d" => Some("D"),
        "e" => Some("E"),
        "f" => Some("F"),
        "g" => Some("G"),
        "h" => Some("H"),
        "i" => Some("I"),
        "j" => Some("J"),
        "k" => Some("K"),
        "l" => Some("L"),
        "m" => Some("M"),
        "n" => Some("N"),
        "o" => Some("O"),
        "p" => Some("P"),
        "q" => Some("Q"),
        "r" => Some("R"),
        "s" => Some("S"),
        "t" => Some("T"),
        "u" => Some("U"),
        "v" => Some("V"),
        "w" => Some("W"),
        "x" => Some("X"),
        "y" => Some("Y"),
        "z" => Some("Z"),
        "0" => Some("0"),
        "1" => Some("1"),
        "2" => Some("2"),
        "3" => Some("3"),
        "4" => Some("4"),
        "5" => Some("5"),
        "6" => Some("6"),
        "7" => Some("7"),
        "8" => Some("8"),
        "9" => Some("9"),
        "f1" => Some("F1"),
        "f2" => Some("F2"),
        "f3" => Some("F3"),
        "f4" => Some("F4"),
        "f5" => Some("F5"),
        "f6" => Some("F6"),
        "f7" => Some("F7"),
        "f8" => Some("F8"),
        "f9" => Some("F9"),
        "f10" => Some("F10"),
        "f11" => Some("F11"),
        "f12" => Some("F12"),
        "," | "comma" => Some(","),
        "." | "period" => Some("."),
        "/" | "slash" => Some("/"),
        ";" | "semicolon" => Some(";"),
        "'" | "apostrophe" => Some("'"),
        "[" => Some("["),
        "]" => Some("]"),
        "\\" | "backslash" => Some("\\"),
        "-" | "minus" => Some("-"),
        "=" | "equals" => Some("="),
        "`" | "grave" => Some("`"),
        "tab" => Some("Tab"),
        "space" | " " => Some("Space"),
        "enter" | "return" => Some("Enter"),
        "backspace" => Some("Backspace"),
        "delete" => Some("Delete"),
        "insert" => Some("Insert"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "pageup" => Some("PageUp"),
        "pagedown" => Some("PageDown"),
        "up" => Some("Up"),
        "down" => Some("Down"),
        "left" => Some("Left"),
        "right" => Some("Right"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppShortcutAction {
    CommandPalette,
    PromptJump,
    NewChat,
    ToggleSettings,
    ToggleSidebar,
    CloseTab,
    NextTab,
    PrevTab,
}

impl AppShortcutAction {
    pub const ALL: [Self; 8] = [
        Self::CommandPalette,
        Self::PromptJump,
        Self::NewChat,
        Self::ToggleSettings,
        Self::ToggleSidebar,
        Self::CloseTab,
        Self::NextTab,
        Self::PrevTab,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::CommandPalette => "command_palette",
            Self::PromptJump => "prompt_jump",
            Self::NewChat => "new_chat",
            Self::ToggleSettings => "open_settings",
            Self::ToggleSidebar => "toggle_sidebar",
            Self::CloseTab => "close_tab",
            Self::NextTab => "next_tab",
            Self::PrevTab => "prev_tab",
        }
    }

    pub fn default_binding(self) -> ShortcutBinding {
        match self {
            Self::CommandPalette => ShortcutBinding::new("K", true, false, false),
            Self::PromptJump => ShortcutBinding::new("G", true, false, false),
            Self::NewChat => ShortcutBinding::new("N", true, false, false),
            Self::ToggleSettings => ShortcutBinding::new(",", true, false, false),
            Self::ToggleSidebar => ShortcutBinding::new("B", true, false, false),
            Self::CloseTab => ShortcutBinding::new("W", true, false, false),
            Self::NextTab => ShortcutBinding::new("Tab", true, false, false),
            Self::PrevTab => ShortcutBinding::new("Tab", true, false, true),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::CommandPalette => "Command palette",
            Self::PromptJump => "Jump to a prompt in this chat",
            Self::NewChat => "New chat",
            Self::ToggleSettings => "Open Settings",
            Self::ToggleSidebar => "Show or hide the sidebar",
            Self::CloseTab => "Close this tab",
            Self::NextTab => "Next tab",
            Self::PrevTab => "Previous tab",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::CommandPalette => "Search commands, settings pages, and recent chats",
            Self::PromptJump => "Filter and jump straight to any prompt in the current chat",
            Self::NewChat => "Open a fresh conversation tab",
            Self::ToggleSettings => "Open or return from the Settings workspace",
            Self::ToggleSidebar => "Toggle the left Assistants, Chats & Pinned panel",
            Self::CloseTab => "Close the active chat tab",
            Self::NextTab => "Switch to the next open tab",
            Self::PrevTab => "Switch to the previous open tab",
        }
    }
}

pub fn resolve_shortcut(
    action: AppShortcutAction,
    custom: &BTreeMap<String, String>,
) -> ShortcutBinding {
    custom
        .get(action.id())
        .and_then(|raw| ShortcutBinding::parse(raw))
        .filter(ShortcutBinding::is_valid)
        .unwrap_or_else(|| action.default_binding())
}

pub fn is_shortcut_customized(
    action: AppShortcutAction,
    custom: &BTreeMap<String, String>,
) -> bool {
    resolve_shortcut(action, custom) != action.default_binding()
}

pub fn find_shortcut_conflict(
    target: AppShortcutAction,
    candidate: &ShortcutBinding,
    custom: &BTreeMap<String, String>,
) -> Option<AppShortcutAction> {
    AppShortcutAction::ALL
        .into_iter()
        .find(|other| *other != target && &resolve_shortcut(*other, custom) == candidate)
}

pub fn update_custom_shortcut(
    custom: &mut BTreeMap<String, String>,
    action: AppShortcutAction,
    binding: ShortcutBinding,
) {
    if binding == action.default_binding() {
        custom.remove(action.id());
    } else {
        custom.insert(action.id().to_string(), binding.format());
    }
}

pub fn swap_custom_shortcuts(
    custom: &mut BTreeMap<String, String>,
    target: AppShortcutAction,
    new_binding: ShortcutBinding,
    conflict: AppShortcutAction,
) {
    let prev = resolve_shortcut(target, custom);
    update_custom_shortcut(custom, target, new_binding);
    update_custom_shortcut(custom, conflict, prev);
}

pub fn match_shortcut_action(
    ks: &Keystroke,
    custom: &BTreeMap<String, String>,
) -> Option<AppShortcutAction> {
    AppShortcutAction::ALL
        .into_iter()
        .find(|action| resolve_shortcut(*action, custom).matches(ks))
}
