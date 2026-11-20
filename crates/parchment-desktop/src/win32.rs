//! Native Windows OS integration via the official Microsoft `windows` crate:
//! - DWM native caption tinting (`DwmSetWindowAttribute` dark mode, rounded corners, caption/text/border color)
//! - Native COM `IFileOpenDialog` (multi-file & `FOS_PICKFOLDERS` folder picker) and `IFileSaveDialog`
//! - Dedicated keep-awake `WindowsPowerLease` (`SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`)
//! - Native Win32 `ComputerUseBackend` (`SendInput` mouse/keyboard + screen dimensions)
//! - Safe `open_web_link` (`http`/`https` only — never executes local paths/binaries).

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parchment_core::harness::{ComputerUseAction, ComputerUseBackend};
use parchment_core::model::is_web_link;

use crate::theme::ParchmentPalette;

static ACTIVE_TTS: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
static ACTIVE_STT: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

const BLOCKED_OPEN_EXTS: &[&str] = &[
    "exe", "msi", "msp", "bat", "cmd", "com", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf",
    "wsh", "scr", "pif", "lnk", "hta", "jar", "dll", "cpl", "reg", "msc", "sh", "appimage",
];

/// Opens a local folder or safe non-executable file in the OS viewer (blocks executable extensions).
pub fn open_local_file_or_folder(raw: &str) -> Result<(), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Empty path.".into());
    }
    if is_web_link(trimmed) {
        return open_web_link(trimmed);
    }
    let path = Path::new(trimmed);
    if !path.exists() {
        return Err(format!("Path does not exist: {trimmed}"));
    }
    if path.is_file() {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if BLOCKED_OPEN_EXTS.contains(&ext.as_str()) {
            return Err(format!("Blocked opening executable file type '.{ext}'."));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if path.is_dir() {
            std::process::Command::new("explorer.exe")
                .arg(path)
                .spawn()
                .map_err(|e| format!("Failed opening folder: {e}"))?;
        } else {
            std::process::Command::new("rundll32.exe")
                .args(["url.dll,FileProtocolHandler", trimmed])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| format!("Failed opening file: {e}"))?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Opening local files is implemented on Windows.".into())
    }
}

/// Speaks plain text aloud via Windows `System.Speech` on a background thread.
pub fn speak_text_aloud(text: &str, on_done: impl FnOnce() + Send + 'static) {
    stop_speaking_aloud();
    let cleaned: String = text.trim().chars().take(6000).collect();
    if cleaned.is_empty() {
        on_done();
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::thread::spawn(move || {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let tmp = std::env::temp_dir().join(format!("parchment-tts-{stamp}.txt"));
            if std::fs::write(&tmp, &cleaned).is_ok() {
                let escaped = tmp.to_string_lossy().replace('\'', "''");
                let script = format!(
                    "Add-Type -AssemblyName System.Speech; \
                     $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
                     $t = Get-Content -LiteralPath '{escaped}' -Raw -Encoding UTF8; \
                     $s.Speak($t); \
                     Remove-Item -LiteralPath '{escaped}' -Force -ErrorAction SilentlyContinue"
                );
                if let Ok(child) = std::process::Command::new("powershell.exe")
                    .args([
                        "-NoProfile",
                        "-NonInteractive",
                        "-WindowStyle",
                        "Hidden",
                        "-Command",
                        &script,
                    ])
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
                {
                    let slot = ACTIVE_TTS.get_or_init(|| Mutex::new(None));
                    let pid = child.id();
                    if let Ok(mut guard) = slot.lock() {
                        *guard = Some(child);
                    }
                    // Poll-free wait by taking the child only if still our handle, or waiting on a helper.
                    loop {
                        let done = slot
                            .lock()
                            .ok()
                            .map(|mut g| match g.as_mut() {
                                Some(c) if c.id() == pid => match c.try_wait() {
                                    Ok(Some(_)) | Err(_) => {
                                        *g = None;
                                        true
                                    }
                                    Ok(None) => false,
                                },
                                _ => true,
                            })
                            .unwrap_or(true);
                        if done {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(80));
                    }
                    let _ = std::fs::remove_file(&tmp);
                }
            }
            on_done();
        });
    }
    #[cfg(not(windows))]
    {
        on_done();
    }
}

pub fn stop_speaking_aloud() {
    if let Some(slot) = ACTIVE_TTS.get()
        && let Ok(mut guard) = slot.lock()
        && let Some(mut child) = guard.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Captures a single spoken utterance via Windows `System.Speech.Recognition` on a background thread.
pub fn start_voice_input(on_result: impl FnOnce(Result<String, String>) + Send + 'static) {
    stop_voice_input();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::thread::spawn(move || {
            let script = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
                Add-Type -AssemblyName System.Speech; \
                $r = New-Object System.Speech.Recognition.SpeechRecognitionEngine; \
                $r.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar)); \
                $r.SetInputToDefaultAudioDevice(); \
                $res = $r.Recognize([TimeSpan]::FromSeconds(10)); \
                if ($res -ne $null -and $res.Text) { Write-Output $res.Text }";
            let spawned = std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    script,
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
            let child = match spawned {
                Ok(c) => c,
                Err(e) => {
                    on_result(Err(format!("Voice input unavailable: {e}")));
                    return;
                }
            };
            let pid = child.id();
            let slot = ACTIVE_STT.get_or_init(|| Mutex::new(None));
            if let Ok(mut g) = slot.lock() {
                *g = Some(child);
            }
            let out = slot.lock().ok().and_then(|mut g| {
                if g.as_ref().is_some_and(|c| c.id() == pid) {
                    g.take()
                } else {
                    None
                }
            });
            let Some(child) = out else {
                on_result(Err("Voice input stopped.".into()));
                return;
            };
            match child.wait_with_output() {
                Ok(output) => {
                    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if text.is_empty() {
                        on_result(Err("No speech detected from microphone.".into()));
                    } else {
                        on_result(Ok(text));
                    }
                }
                Err(e) => on_result(Err(format!("Voice input error: {e}"))),
            }
        });
    }
    #[cfg(not(windows))]
    {
        on_result(Err("Voice input is implemented on Windows.".into()));
    }
}

pub fn stop_voice_input() {
    if let Some(slot) = ACTIVE_STT.get()
        && let Ok(mut guard) = slot.lock()
        && let Some(mut child) = guard.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Opens a web URL in the system browser ONLY if it is `http://` or `https://`.
pub fn open_web_link(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if !is_web_link(trimmed) {
        return Err("Only http:// and https:// links can be opened.".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", trimmed])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed opening link: {e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = trimmed;
        Err("Opening links is implemented on Windows.".into())
    }
}

/// Applies Parchment's theme palette to the active native Windows 10/11 window caption via DWM.
#[cfg(windows)]
pub fn apply_dwm_chrome_to_active_window(palette: &ParchmentPalette) {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DwmSetWindowAttribute,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetForegroundWindow, GetWindow, GetWindowThreadProcessId,
        IsWindowVisible,
    };
    use windows::core::BOOL;

    unsafe extern "system" fn enum_process_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == std::process::id()
            && unsafe { IsWindowVisible(hwnd) }.as_bool()
            && unsafe { GetWindow(hwnd, GW_OWNER) }
                .map(|owner| owner.0.is_null())
                .unwrap_or(true)
        {
            let slot = lparam.0 as *mut HWND;
            if !slot.is_null() {
                unsafe { *slot = hwnd };
            }
            return BOOL(0);
        }
        BOOL(1)
    }

    unsafe {
        let mut hwnd = GetActiveWindow();
        if hwnd.0.is_null() {
            let fg = GetForegroundWindow();
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            if pid == std::process::id() {
                hwnd = fg;
            }
        }
        if hwnd.0.is_null() {
            let mut found = HWND::default();
            let _ = EnumWindows(Some(enum_process_window), LPARAM((&raw mut found) as isize));
            hwnd = found;
        }
        if hwnd.0.is_null() {
            return;
        }
        apply_dwm_attributes(hwnd, palette);
    }

    unsafe fn apply_dwm_attributes(hwnd: HWND, palette: &ParchmentPalette) {
        let dark: u32 = if palette.is_dark { 1 } else { 0 };
        let corners: u32 = 2; // DWMWCP_ROUND
        let caption = palette.canvas_bg.to_colorref();
        let text = palette.primary_ink.to_colorref();
        let border = palette.border.to_colorref();

        unsafe { apply_embedded_window_icon(hwnd) };

        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw const dark).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const corners).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                (&raw const caption).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_TEXT_COLOR,
                (&raw const text).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                (&raw const border).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
    }

    unsafe fn apply_embedded_window_icon(hwnd: HWND) {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON,
            SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
        };
        use windows::core::PCWSTR;

        static ICON_APPLIED_HWND: Mutex<isize> = Mutex::new(0);
        let cur = hwnd.0 as isize;
        if let Ok(guard) = ICON_APPLIED_HWND.lock()
            && *guard == cur
        {
            return;
        }

        let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
            return;
        };
        let small_w = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
        let small_h = unsafe { GetSystemMetrics(SM_CYSMICON) }.max(16);
        let big_w = unsafe { GetSystemMetrics(SM_CXICON) }.max(32);
        let big_h = unsafe { GetSystemMetrics(SM_CYICON) }.max(32);
        let mut applied_any = false;

        if let Ok(handle) = unsafe {
            LoadImageW(
                Some(module.into()),
                PCWSTR(1 as _),
                IMAGE_ICON,
                small_w,
                small_h,
                LR_SHARED,
            )
        } {
            unsafe {
                let _ = SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(WPARAM(ICON_SMALL as usize)),
                    Some(LPARAM(handle.0 as isize)),
                );
            }
            applied_any = true;
        }
        if let Ok(handle) = unsafe {
            LoadImageW(
                Some(module.into()),
                PCWSTR(1 as _),
                IMAGE_ICON,
                big_w,
                big_h,
                LR_SHARED,
            )
        } {
            unsafe {
                let _ = SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(WPARAM(ICON_BIG as usize)),
                    Some(LPARAM(handle.0 as isize)),
                );
            }
            applied_any = true;
        }
        if applied_any && let Ok(mut guard) = ICON_APPLIED_HWND.lock() {
            *guard = cur;
        }
    }
}

#[cfg(not(windows))]
pub fn apply_dwm_chrome_to_active_window(_palette: &ParchmentPalette) {}

/// Opens a native Windows COM `IFileOpenDialog` configured with `FOS_PICKFOLDERS` on an STA thread.
#[cfg(windows)]
pub fn pick_folder_dialog(title: &str) -> Option<PathBuf> {
    let title = title.to_string();
    std::thread::spawn(move || pick_folder_sta(&title))
        .join()
        .ok()
        .flatten()
}

#[cfg(windows)]
fn pick_folder_sta(title: &str) -> Option<PathBuf> {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
        CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
        SIGDN_FILESYSPATH,
    };
    use windows::core::HSTRING;

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let result = (|| -> Option<PathBuf> {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let opts = dialog.GetOptions().ok()?;
            dialog
                .SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
                .ok()?;
            dialog.SetTitle(&HSTRING::from(title)).ok()?;
            dialog.Show(None).ok()?;
            let item = dialog.GetResult().ok()?;
            let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let path = pwstr.to_string().ok()?;
            Some(PathBuf::from(path))
        })();
        CoUninitialize();
        result
    }
}

#[cfg(not(windows))]
pub fn pick_folder_dialog(_title: &str) -> Option<PathBuf> {
    None
}

/// Opens a native Windows COM `IFileOpenDialog` for selecting a file (e.g. `.parchment`, `.json`, `.png`).
#[cfg(windows)]
pub fn pick_open_file_dialog(
    title: &str,
    filter_label: &str,
    filter_spec: &str,
) -> Option<PathBuf> {
    let t = title.to_string();
    let fl = filter_label.to_string();
    let fs = filter_spec.to_string();
    std::thread::spawn(move || pick_open_file_sta(&t, &fl, &fs))
        .join()
        .ok()
        .flatten()
}

#[cfg(windows)]
fn pick_open_file_sta(title: &str, filter_label: &str, filter_spec: &str) -> Option<PathBuf> {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
        CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };
    use windows::core::{HSTRING, PCWSTR};

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let result = (|| -> Option<PathBuf> {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let opts = dialog.GetOptions().ok()?;
            dialog
                .SetOptions(opts | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST)
                .ok()?;
            dialog.SetTitle(&HSTRING::from(title)).ok()?;
            let label_w: Vec<u16> = filter_label.encode_utf16().chain(Some(0)).collect();
            let spec_w: Vec<u16> = filter_spec.encode_utf16().chain(Some(0)).collect();
            let specs = [COMDLG_FILTERSPEC {
                pszName: PCWSTR(label_w.as_ptr()),
                pszSpec: PCWSTR(spec_w.as_ptr()),
            }];
            let _ = dialog.SetFileTypes(&specs);
            dialog.Show(None).ok()?;
            let item = dialog.GetResult().ok()?;
            let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            Some(PathBuf::from(pwstr.to_string().ok()?))
        })();
        CoUninitialize();
        result
    }
}

#[cfg(not(windows))]
pub fn pick_open_file_dialog(
    _title: &str,
    _filter_label: &str,
    _filter_spec: &str,
) -> Option<PathBuf> {
    None
}

/// Opens a native Windows COM `IFileSaveDialog` for saving exports (e.g. `.parchment`).
#[cfg(windows)]
pub fn pick_save_file_dialog(
    title: &str,
    default_name: &str,
    default_ext: &str,
) -> Option<PathBuf> {
    let t = title.to_string();
    let dn = default_name.to_string();
    let de = default_ext.to_string();
    std::thread::spawn(move || pick_save_file_sta(&t, &dn, &de))
        .join()
        .ok()
        .flatten()
}

#[cfg(windows)]
fn pick_save_file_sta(title: &str, default_name: &str, default_ext: &str) -> Option<PathBuf> {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
        CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FileSaveDialog, IFileSaveDialog,
        SIGDN_FILESYSPATH,
    };
    use windows::core::HSTRING;

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let result = (|| -> Option<PathBuf> {
            let dialog: IFileSaveDialog =
                CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let opts = dialog.GetOptions().ok()?;
            dialog
                .SetOptions(opts | FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT)
                .ok()?;
            dialog.SetTitle(&HSTRING::from(title)).ok()?;
            dialog.SetFileName(&HSTRING::from(default_name)).ok()?;
            dialog
                .SetDefaultExtension(&HSTRING::from(default_ext))
                .ok()?;
            dialog.Show(None).ok()?;
            let item = dialog.GetResult().ok()?;
            let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            Some(PathBuf::from(pwstr.to_string().ok()?))
        })();
        CoUninitialize();
        result
    }
}

#[cfg(not(windows))]
pub fn pick_save_file_dialog(
    _title: &str,
    _default_name: &str,
    _default_ext: &str,
) -> Option<PathBuf> {
    None
}

/// One dedicated thread holds `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` while
/// `owners > 0`, clearing on `owners == 0` and retrying failed calls after `5 s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerLeaseStatus {
    Idle,
    Active,
    Unavailable,
}

struct LeaseState {
    owners: usize,
    closed: bool,
    status: PowerLeaseStatus,
    last_failure: Option<Instant>,
}

pub struct WindowsPowerLease {
    shared: Arc<(Mutex<LeaseState>, Condvar)>,
}

impl WindowsPowerLease {
    pub fn new() -> Self {
        let shared = Arc::new((
            Mutex::new(LeaseState {
                owners: 0,
                closed: false,
                status: PowerLeaseStatus::Idle,
                last_failure: None,
            }),
            Condvar::new(),
        ));
        let bg = shared.clone();
        let _ = std::thread::Builder::new()
            .name("parchment-keep-awake".into())
            .spawn(move || Self::worker_loop(bg));
        Self { shared }
    }

    pub fn acquire(&self) {
        let (lock, cvar) = &*self.shared;
        if let Ok(mut st) = lock.lock() {
            st.owners += 1;
            cvar.notify_one();
        }
    }

    pub fn release(&self) {
        let (lock, cvar) = &*self.shared;
        if let Ok(mut st) = lock.lock() {
            st.owners = st.owners.saturating_sub(1);
            cvar.notify_one();
        }
    }

    pub fn status(&self) -> PowerLeaseStatus {
        self.shared
            .0
            .lock()
            .map(|s| s.status)
            .unwrap_or(PowerLeaseStatus::Unavailable)
    }

    fn worker_loop(shared: Arc<(Mutex<LeaseState>, Condvar)>) {
        let mut holding = false;
        let (lock, cvar) = &*shared;
        loop {
            let (want_hold, closed) = {
                let mut st = match lock.lock() {
                    Ok(g) => g,
                    Err(_) => break,
                };
                loop {
                    if st.closed {
                        break (false, true);
                    }
                    let want = st.owners > 0;
                    if want != holding {
                        if want
                            && let Some(failed_at) = st.last_failure
                            && failed_at.elapsed() < Duration::from_secs(5)
                        {
                            let wait = Duration::from_secs(5).saturating_sub(failed_at.elapsed());
                            st = match cvar.wait_timeout(st, wait) {
                                Ok((guard, _)) => guard,
                                Err(poisoned) => poisoned.into_inner().0,
                            };
                            continue;
                        }
                        break (want, false);
                    }
                    st = match cvar.wait(st) {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                }
            };

            if want_hold && !holding {
                let ok = set_thread_keep_awake(true);
                if let Ok(mut st) = lock.lock() {
                    if ok {
                        holding = true;
                        st.status = PowerLeaseStatus::Active;
                        st.last_failure = None;
                    } else {
                        st.status = PowerLeaseStatus::Unavailable;
                        st.last_failure = Some(Instant::now());
                    }
                }
            } else if !want_hold && holding {
                let _ = set_thread_keep_awake(false);
                holding = false;
                if let Ok(mut st) = lock.lock() {
                    st.status = PowerLeaseStatus::Idle;
                }
            }

            if closed {
                if holding {
                    let _ = set_thread_keep_awake(false);
                }
                break;
            }
        }
    }
}

impl Default for WindowsPowerLease {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WindowsPowerLease {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.shared;
        if let Ok(mut st) = lock.lock() {
            st.closed = true;
            st.owners = 0;
            cvar.notify_one();
        }
    }
}

#[cfg(windows)]
fn set_thread_keep_awake(enable: bool) -> bool {
    use windows::Win32::System::Power::{
        ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
    };
    let flags = if enable {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    unsafe { SetThreadExecutionState(flags).0 != 0 }
}

#[cfg(not(windows))]
fn set_thread_keep_awake(_enable: bool) -> bool {
    true
}

/// Native Win32 desktop `ComputerUseBackend` (`SendInput` + screen bounds).
pub struct Win32ComputerUse;

#[async_trait]
impl ComputerUseBackend for Win32ComputerUse {
    async fn execute(&self, action: &ComputerUseAction) -> Result<(String, String), String> {
        #[cfg(windows)]
        {
            let act = action.clone();
            tokio::task::spawn_blocking(move || execute_win32_computer_action(&act))
                .await
                .map_err(|e| format!("Computer control task error: {e}"))?
        }
        #[cfg(not(windows))]
        {
            let _ = action;
            Err("Win32 computer control is only available on Windows.".into())
        }
    }
}

#[cfg(windows)]
fn execute_win32_computer_action(action: &ComputerUseAction) -> Result<(String, String), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
        MOUSEINPUT, SendInput,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let (w, h) = unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN).max(1),
            GetSystemMetrics(SM_CYSCREEN).max(1),
        )
    };
    let to_abs = |x: i32, y: i32| -> (i32, i32) {
        let nx = ((x.clamp(0, w - 1) as i64) * 65535 / (w - 1).max(1) as i64) as i32;
        let ny = ((y.clamp(0, h - 1) as i64) * 65535 / (h - 1).max(1) as i64) as i32;
        (nx, ny)
    };

    let send_mouse = |dx: i32, dy: i32, data: u32, flags| {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    };

    match action.action.as_str() {
        "screenshot" => Ok((format!("Screen dimensions: {w}x{h} px."), String::new())),
        "move" => {
            let (nx, ny) = to_abs(action.x.unwrap_or(0), action.y.unwrap_or(0));
            send_mouse(nx, ny, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE);
            Ok((
                format!(
                    "Moved pointer to ({}, {}).",
                    action.x.unwrap_or(0),
                    action.y.unwrap_or(0)
                ),
                String::new(),
            ))
        }
        "click" | "double_click" => {
            let (nx, ny) = to_abs(action.x.unwrap_or(0), action.y.unwrap_or(0));
            let clicks = if action.action == "double_click" {
                2
            } else {
                1
            };
            for _ in 0..clicks {
                send_mouse(
                    nx,
                    ny,
                    0,
                    MOUSEEVENTF_MOVE
                        | MOUSEEVENTF_ABSOLUTE
                        | MOUSEEVENTF_LEFTDOWN
                        | MOUSEEVENTF_LEFTUP,
                );
            }
            Ok((
                format!(
                    "Clicked ({}, {}).",
                    action.x.unwrap_or(0),
                    action.y.unwrap_or(0)
                ),
                String::new(),
            ))
        }
        "right_click" => {
            let (nx, ny) = to_abs(action.x.unwrap_or(0), action.y.unwrap_or(0));
            send_mouse(
                nx,
                ny,
                0,
                MOUSEEVENTF_MOVE
                    | MOUSEEVENTF_ABSOLUTE
                    | MOUSEEVENTF_RIGHTDOWN
                    | MOUSEEVENTF_RIGHTUP,
            );
            Ok((
                format!(
                    "Right-clicked ({}, {}).",
                    action.x.unwrap_or(0),
                    action.y.unwrap_or(0)
                ),
                String::new(),
            ))
        }
        "scroll" => {
            let delta = (-action.amount.unwrap_or(3) * 120) as u32;
            send_mouse(0, 0, delta, MOUSEEVENTF_WHEEL);
            Ok((
                format!("Scrolled {}.", action.amount.unwrap_or(3)),
                String::new(),
            ))
        }
        "type" => {
            let text = action.text.as_deref().unwrap_or("");
            for unit in text.encode_utf16() {
                let down = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: Default::default(),
                            wScan: unit,
                            dwFlags: KEYEVENTF_UNICODE,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                let up = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: Default::default(),
                            wScan: unit,
                            dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                unsafe {
                    SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
                }
            }
            Ok((
                format!("Typed {} characters.", text.chars().count()),
                String::new(),
            ))
        }
        other => Err(format!("Unsupported computer_use action '{other}'.")),
    }
}

/// Reads copied files (`CF_HDROP`) or clipboard images (registered `"PNG"` or `CF_DIB` encoded to
/// PNG) into `dest_dir`. Returns empty when the clipboard contains only text so normal text paste
/// is never intercepted.
#[cfg(windows)]
pub fn read_clipboard_attachments(dest_dir: &Path) -> Vec<PathBuf> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::Graphics::Gdi::BITMAPINFOHEADER;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
    use windows::core::w;

    const CF_DIB: u32 = 8;
    const CF_HDROP: u32 = 15;
    const MAX_CLIPBOARD_FILES: u32 = 32;
    const MAX_CLIPBOARD_IMAGE_BYTES: usize = 128 * 1024 * 1024;

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    unsafe {
        if OpenClipboard(None).is_err() {
            return Vec::new();
        }
        let _guard = ClipboardGuard;

        // 1. Copied files (Explorer Ctrl+C)
        if IsClipboardFormatAvailable(CF_HDROP).is_ok()
            && let Ok(handle) = GetClipboardData(CF_HDROP)
        {
            let hdrop = HDROP(handle.0);
            let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None).min(MAX_CLIPBOARD_FILES);
            let mut paths = Vec::new();
            for idx in 0..count {
                let needed = DragQueryFileW(hdrop, idx, None);
                if needed == 0 {
                    continue;
                }
                let mut wide = vec![0u16; (needed as usize) + 1];
                let written = DragQueryFileW(hdrop, idx, Some(&mut wide));
                if written > 0 {
                    let s = String::from_utf16_lossy(&wide[..written as usize]);
                    let p = PathBuf::from(s);
                    if p.is_file() {
                        paths.push(p);
                    }
                }
            }
            if !paths.is_empty() {
                return paths;
            }
        }

        let write_png_temp = |png_bytes: &[u8]| -> Option<PathBuf> {
            std::fs::create_dir_all(dest_dir).ok()?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let out_path = dest_dir.join(format!("clipboard-{stamp}.png"));
            std::fs::write(&out_path, png_bytes).ok()?;
            Some(out_path)
        };

        // 2. Registered "PNG" format verbatim
        let png_fmt = RegisterClipboardFormatW(w!("PNG"));
        if png_fmt != 0
            && IsClipboardFormatAvailable(png_fmt).is_ok()
            && let Ok(handle) = GetClipboardData(png_fmt)
        {
            let hglob = HGLOBAL(handle.0);
            let size = GlobalSize(hglob);
            if (8..=MAX_CLIPBOARD_IMAGE_BYTES).contains(&size) {
                let ptr = GlobalLock(hglob) as *const u8;
                if !ptr.is_null() {
                    let slice = std::slice::from_raw_parts(ptr, size);
                    let res = if slice.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
                        write_png_temp(slice)
                    } else {
                        None
                    };
                    let _ = GlobalUnlock(hglob);
                    if let Some(p) = res {
                        return vec![p];
                    }
                }
            }
        }

        // 3. Fallback CF_DIB -> decode 24/32-bit DIB pixels and encode to PNG (never write .bmp)
        if IsClipboardFormatAvailable(CF_DIB).is_ok()
            && let Ok(handle) = GetClipboardData(CF_DIB)
        {
            let hglob = HGLOBAL(handle.0);
            let size = GlobalSize(hglob);
            if size >= std::mem::size_of::<BITMAPINFOHEADER>() && size <= MAX_CLIPBOARD_IMAGE_BYTES
            {
                let ptr = GlobalLock(hglob) as *const u8;
                if !ptr.is_null() {
                    let slice = std::slice::from_raw_parts(ptr, size);
                    let png = decode_dib_to_png(slice);
                    let _ = GlobalUnlock(hglob);
                    if let Ok(png_bytes) = png
                        && let Some(p) = write_png_temp(&png_bytes)
                    {
                        return vec![p];
                    }
                }
            }
        }
    }
    Vec::new()
}

#[cfg(windows)]
fn decode_dib_to_png(dib: &[u8]) -> Result<Vec<u8>, String> {
    if dib.len() < 40 {
        return Err("DIB header truncated".into());
    }
    let bi_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    if bi_size < 40 || bi_size > dib.len() {
        return Err("Invalid DIB header size".into());
    }
    let width_i32 = i32::from_le_bytes([dib[4], dib[5], dib[6], dib[7]]);
    let height_i32 = i32::from_le_bytes([dib[8], dib[9], dib[10], dib[11]]);
    let bpp = u16::from_le_bytes([dib[14], dib[15]]);
    let compression = u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]]);
    if width_i32 <= 0 || height_i32 == 0 {
        return Err("Invalid DIB dimensions".into());
    }
    let width = width_i32 as u32;
    let height = height_i32.unsigned_abs();
    let top_down = height_i32 < 0;
    if width > 8192 || height > 8192 {
        return Err("DIB image exceeds 8192px dimension cap".into());
    }
    if !matches!(bpp, 24 | 32) || !matches!(compression, 0 | 3) {
        return Err(format!(
            "Unsupported DIB format: bpp={bpp} comp={compression}"
        ));
    }
    let mut pixel_offset = bi_size;
    if compression == 3 && bi_size == 40 {
        pixel_offset = pixel_offset.saturating_add(12);
    }
    let stride = ((width as usize) * usize::from(bpp)).div_ceil(32) * 4;
    let needed = pixel_offset
        .checked_add(
            stride
                .checked_mul(height as usize)
                .ok_or("DIB stride overflow")?,
        )
        .ok_or("DIB offset overflow")?;
    if needed > dib.len() {
        return Err("DIB pixel data truncated".into());
    }
    let pixels = &dib[pixel_offset..needed];
    let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
    let mut any_nonzero_alpha = false;
    for y in 0..(height as usize) {
        let src_y = if top_down {
            y
        } else {
            (height as usize) - 1 - y
        };
        let src_row = &pixels[src_y * stride..(src_y + 1) * stride];
        let dst_row = &mut rgba[y * (width as usize) * 4..(y + 1) * (width as usize) * 4];
        if bpp == 32 {
            for x in 0..(width as usize) {
                let s = x * 4;
                dst_row[s] = src_row[s + 2]; // R
                dst_row[s + 1] = src_row[s + 1]; // G
                dst_row[s + 2] = src_row[s]; // B
                let a = src_row[s + 3];
                if a != 0 {
                    any_nonzero_alpha = true;
                }
                dst_row[s + 3] = a;
            }
        } else {
            for x in 0..(width as usize) {
                let s = x * 3;
                let d = x * 4;
                dst_row[d] = src_row[s + 2]; // R
                dst_row[d + 1] = src_row[s + 1]; // G
                dst_row[d + 2] = src_row[s]; // B
                dst_row[d + 3] = 255;
            }
        }
    }
    if bpp == 32 && !any_nonzero_alpha {
        for px in rgba.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }
    parchment_core::harness::encode_rgba_to_png_bytes(width, height, &rgba)
}

#[cfg(not(windows))]
pub fn read_clipboard_attachments(_dest_dir: &Path) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_lease_activates_and_releases_cleanly() {
        let lease = WindowsPowerLease::new();
        lease.acquire();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(lease.status(), PowerLeaseStatus::Active);
        lease.release();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(lease.status(), PowerLeaseStatus::Idle);
    }

    #[test]
    fn open_web_link_blocks_local_paths_and_executables() {
        assert!(open_web_link("C:\\Windows\\System32\\calc.exe").is_err());
        assert!(open_web_link("file:///etc/passwd").is_err());
        assert!(open_web_link("javascript:alert(1)").is_err());
    }
}
