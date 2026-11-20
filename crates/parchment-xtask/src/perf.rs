//! Unified performance benchmarks for Rust GPUI desktop (`parchment-desktop.exe`) and Android APK/phone.

use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
#[cfg(windows)]
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
const IDLE_SECONDS: u64 = 10;
#[cfg(windows)]
const TIMEOUT_SECONDS: u64 = 30;

pub fn run_perf(repo: &Path, android_install: bool) -> Result<(), String> {
    let mut lines = Vec::new();

    let so = repo.join("target/aarch64-linux-android/release-android/libparchment_core.so");
    if so.is_file() {
        let bytes = std::fs::metadata(&so).map_err(|e| e.to_string())?.len();
        lines.push(format!(
            "android_arm64_so_raw_mb={:.2}",
            bytes as f64 / 1_048_576.0
        ));
    }

    let apk = repo.join("mobile/android-app/build/outputs/apk/release/androidApp-release.apk");
    if apk.is_file() {
        let bytes = std::fs::metadata(&apk).map_err(|e| e.to_string())?.len();
        lines.push(format!("android_apk_mb={:.2}", bytes as f64 / 1_048_576.0));
    }

    let exe = repo.join("target/release/parchment-desktop.exe");
    if cfg!(windows) && exe.is_file() {
        let bytes = std::fs::metadata(&exe).map_err(|e| e.to_string())?.len();
        lines.push(format!("desktop_exe_mb={:.2}", bytes as f64 / 1_048_576.0));
        measure_windows_desktop(&exe, &mut lines)?;
    } else {
        lines.push("desktop=not built (run `cargo build --release -p parchment-desktop`)".into());
    }

    if android_install && apk.is_file() {
        let _ = Command::new("adb")
            .args(["install", "-r"])
            .arg(&apk)
            .status();
    }
    measure_android_phone(&mut lines);

    let out_dir = repo.join(".parchment-local/verification");
    let _ = std::fs::create_dir_all(&out_dir);
    let report = lines.join("\n") + "\n";
    let _ = std::fs::write(out_dir.join("perf.txt"), &report);
    print!("{report}");
    println!("Saved: .parchment-local/verification/perf.txt");
    Ok(())
}

#[cfg(windows)]
fn measure_windows_desktop(exe: &Path, lines: &mut Vec<String>) -> Result<(), String> {
    let work = std::mem::DropGuard::new(make_temp_dir("parchment-perf")?, |dir| {
        let _ = std::fs::remove_dir_all(dir);
    });
    let seed_run = measure_desktop_run(exe, &work, true)?;
    lines.push(format!(
        "desktop_seed_first_launch_ms={}",
        seed_run.title_ms.round() as i64
    ));

    let mut win_ms = Vec::with_capacity(3);
    let mut title_ms = Vec::with_capacity(3);
    let mut idle_mb = Vec::with_capacity(3);
    for _ in 0..3 {
        let r = measure_desktop_run(exe, &work, false)?;
        win_ms.push(r.window_ms);
        title_ms.push(r.title_ms);
        idle_mb.push(r.idle_private_mb);
    }
    lines.push(format!(
        "desktop_window_ms={}",
        median(&mut win_ms).round() as i64
    ));
    lines.push(format!(
        "desktop_title_ms={}",
        median(&mut title_ms).round() as i64
    ));
    lines.push(format!(
        "desktop_idle_private_mb={:.1}",
        median(&mut idle_mb)
    ));

    let bench_out = work.join("stream_bench.txt");
    let _ = Command::new(exe)
        .args(["--data-dir"])
        .arg(&*work)
        .args(["--bench-stream", "--bench-out"])
        .arg(&bench_out)
        .status();
    let bench = std::fs::read_to_string(&bench_out)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "no output produced".into());
    lines.push(format!("desktop_stream_bench={bench}"));
    Ok(())
}

#[cfg(not(windows))]
fn measure_windows_desktop(_exe: &Path, _lines: &mut Vec<String>) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
struct DesktopRunMetrics {
    window_ms: f64,
    title_ms: f64,
    idle_private_mb: f64,
}

#[cfg(windows)]
fn measure_desktop_run(
    exe: &Path,
    data_dir: &Path,
    seed: bool,
) -> Result<DesktopRunMetrics, String> {
    let mut cmd = Command::new(exe);
    cmd.arg("--data-dir").arg(data_dir);
    if seed {
        cmd.arg("--seed");
    }
    let started = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch {}: {e}", exe.display()))?;
    let pid = child.id();
    let mut window_ms = -1.0f64;
    let mut title_ms = -1.0f64;

    while started.elapsed() < Duration::from_secs(TIMEOUT_SECONDS) && title_ms < 0.0 {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        for (_, title) in win32::pid_windows(pid) {
            if window_ms < 0.0 {
                window_ms = started.elapsed().as_secs_f64() * 1000.0;
            }
            if title.ends_with("Parchment") && title != "Parchment" {
                title_ms = started.elapsed().as_secs_f64() * 1000.0;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_secs(IDLE_SECONDS));
    let idle_private_mb = if child.try_wait().ok().flatten().is_none() {
        win32::private_mb(pid)
    } else {
        -1.0
    };
    for (hwnd, _) in win32::pid_windows(pid) {
        win32::post_close(hwnd);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(DesktopRunMetrics {
        window_ms,
        title_ms,
        idle_private_mb,
    })
}

#[cfg(windows)]
fn median(vals: &mut [f64]) -> f64 {
    vals.sort_by(|a, b| a.total_cmp(b));
    vals[vals.len() / 2]
}

fn measure_android_phone(lines: &mut Vec<String>) {
    let Ok(out) = Command::new("adb").arg("devices").output() else {
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let has_device = text
        .lines()
        .skip(1)
        .any(|l| l.trim().ends_with("\tdevice") || l.split_whitespace().nth(1) == Some("device"));
    if !has_device {
        return;
    }
    let _ = Command::new("adb")
        .args(["shell", "am", "force-stop", "io.parchment.ai"])
        .status();
    if let Ok(start) = Command::new("adb")
        .args([
            "shell",
            "am",
            "start",
            "-W",
            "io.parchment.ai/.MainActivity",
        ])
        .output()
    {
        for line in String::from_utf8_lossy(&start.stdout).lines() {
            if let Some(val) = line.strip_prefix("TotalTime:") {
                lines.push(format!("android_cold_start_ms={}", val.trim()));
            }
        }
    }
    std::thread::sleep(Duration::from_secs(5));
    if let Ok(mem) = Command::new("adb")
        .args(["shell", "dumpsys", "meminfo", "io.parchment.ai"])
        .output()
        && let Some(mb) = parse_total_pss_mb(&String::from_utf8_lossy(&mem.stdout))
    {
        lines.push(format!("android_idle_pss_mb={mb:.1}"));
    }
}

pub fn parse_total_pss_mb(dumpsys: &str) -> Option<f64> {
    for line in dumpsys.lines() {
        if let Some(idx) = line.find("TOTAL PSS:") {
            let after = line[idx + "TOTAL PSS:".len()..].trim_start();
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(kb) = digits.parse::<f64>() {
                return Some((kb / 1024.0 * 10.0).round() / 10.0);
            }
        }
    }
    None
}

#[cfg(windows)]
fn make_temp_dir(prefix: &str) -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;

    type Hwnd = *mut c_void;
    type Handle = *mut c_void;
    type Bool = i32;
    type EnumProc = unsafe extern "system" fn(Hwnd, isize) -> Bool;

    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCountersEx {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(cb: EnumProc, lparam: isize) -> Bool;
        fn IsWindowVisible(hwnd: Hwnd) -> Bool;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        fn GetWindowTextLengthW(hwnd: Hwnd) -> i32;
        fn GetWindowTextW(hwnd: Hwnd, buf: *mut u16, max_count: i32) -> i32;
        fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> Bool;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: Bool, pid: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> Bool;
    }

    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            handle: Handle,
            counters: *mut ProcessMemoryCountersEx,
            cb: u32,
        ) -> Bool;
    }

    struct EnumState {
        target_pid: u32,
        found: Vec<(usize, String)>,
    }

    unsafe extern "system" fn visit(hwnd: Hwnd, lparam: isize) -> Bool {
        unsafe {
            let state = &mut *(lparam as *mut EnumState);
            if IsWindowVisible(hwnd) != 0 {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid == state.target_pid {
                    let len = GetWindowTextLengthW(hwnd).max(0) as usize;
                    let mut buf = vec![0u16; len + 1];
                    let got =
                        GetWindowTextW(hwnd, buf.as_mut_ptr(), (len + 1) as i32).max(0) as usize;
                    state
                        .found
                        .push((hwnd as usize, String::from_utf16_lossy(&buf[..got])));
                }
            }
        }
        1
    }

    pub fn pid_windows(target_pid: u32) -> Vec<(usize, String)> {
        let mut state = EnumState {
            target_pid,
            found: Vec::new(),
        };
        unsafe {
            EnumWindows(visit, &mut state as *mut EnumState as isize);
        }
        state.found
    }

    pub fn post_close(hwnd: usize) {
        const WM_CLOSE: u32 = 0x0010;
        unsafe {
            PostMessageW(hwnd as Hwnd, WM_CLOSE, 0, 0);
        }
    }

    pub fn private_mb(pid: u32) -> f64 {
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const PROCESS_VM_READ: u32 = 0x0010;
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
            if handle.is_null() {
                return -1.0;
            }
            let mut counters = ProcessMemoryCountersEx {
                cb: std::mem::size_of::<ProcessMemoryCountersEx>() as u32,
                ..Default::default()
            };
            let ok = GetProcessMemoryInfo(handle, &mut counters, counters.cb) != 0;
            CloseHandle(handle);
            if ok {
                (counters.private_usage as f64 / 1_048_576.0 * 10.0).round() / 10.0
            } else {
                -1.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_android_pss_from_dumpsys() {
        let sample = "App Summary\n           TOTAL PSS:   153600   TOTAL RSS: 200000\n";
        assert_eq!(parse_total_pss_mb(sample), Some(150.0));
    }
}
