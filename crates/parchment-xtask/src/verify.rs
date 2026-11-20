//! Local dual-stack verification harness (`cargo xtask status|review|affected|rust|android-so|test|android|ktlint|final|pre-commit|install-hooks`).
//!
//! Covers both Rust Desktop (`parchment-desktop` + workspace crates) and Kotlin Mobile
//! (`composeApp` + `androidApp` + slim 16 KB-aligned `arm64-v8a` `libparchment_core.so`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::quality;

const APP_VERSION: &str = "3.0.0";
const APP_VERSION_CODE: &str = "300";
const MAX_ANDROID_SO_BYTES: u64 = 2_621_440; // 2.50 MB raw budget (16 KB uncompressed mmap)
const MAX_ANDROID_APK_BYTES: u64 = 7_864_320; // 7.50 MB signed Release APK budget
const MAX_DESKTOP_EXE_BYTES: u64 = 36_700_160; // 35.0 MB standalone GPUI .exe budget
const MAX_TARGET_CACHE_BYTES: u64 = 5 * 1024 * 1024 * 1024; // 5.0 GiB 3-profile warm Cargo budget
const MAX_GRADLE_CACHE_BYTES: u64 = 1024 * 1024 * 1024; // 1.0 GiB Gradle cache budget
const MAX_LOCAL_LOG_BYTES: u64 = 2 * 1024 * 1024; // 2.0 MiB single verification log retention
const SIGNER_VARS: &[&str] = &[
    "ANDROID_KEYSTORE_PATH",
    "ANDROID_KEYSTORE_PASSWORD",
    "ANDROID_KEY_ALIAS",
];

const KOTLIN_CODE_PREFIXES: &[&str] = &["mobile/", "gradle/"];
const KOTLIN_CODE_FILES: &[&str] = &[
    "build.gradle.kts",
    "settings.gradle.kts",
    "gradle.properties",
    ".editorconfig",
    "gradlew",
    "gradlew.bat",
];
const ANDROID_REACHABLE_PREFIXES: &[&str] = &[
    "mobile/compose-app/src/androidMain/",
    "mobile/compose-app/src/jvmShared/",
    "mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/platform/",
    "gradle/",
    "mobile/compose-app/build.gradle.kts",
    "mobile/android-app/",
];
const RUST_PREFIXES: &[&str] = &["crates/", ".cargo/"];
const RUST_FILES: &[&str] = &["Cargo.toml", "Cargo.lock", "rustfmt.toml", "clippy.toml"];

fn state_dir(repo: &Path) -> PathBuf {
    repo.join(".parchment-local/verification")
}

fn ledger_path(repo: &Path) -> PathBuf {
    state_dir(repo).join("ledger.txt")
}

/// OS-enforced advisory lock preventing concurrent Gradle/Cargo verification runs.
struct SingleRunLock(File);

impl SingleRunLock {
    fn acquire(repo: &Path) -> Result<Self, String> {
        let dir = state_dir(repo);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(dir.join("run.lock"))
            .map_err(|e| format!("failed to open lock file: {e}"))?;
        file.try_lock().map_err(|_| {
            "BLOCKED: another verification is already running. Wait for it to finish.".to_string()
        })?;
        Ok(Self(file))
    }
}

impl Drop for SingleRunLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn fingerprint(repo: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    for path in quality::repo_paths(repo)? {
        let data = std::fs::read(repo.join(&path)).map_err(|e| e.to_string())?;
        map.insert(path, format!("{:016x}", fnv1a_64(&data)));
    }
    Ok(map)
}

struct Ledger {
    last: String,
    hashes: BTreeMap<String, String>,
}

fn load_ledger(repo: &Path) -> Ledger {
    let mut last = String::new();
    let mut hashes = BTreeMap::new();
    if let Ok(raw) = std::fs::read_to_string(ledger_path(repo)) {
        for line in raw.lines() {
            if let Some(v) = line.strip_prefix("#last\t") {
                last = v.to_string();
            } else if let Some((path, hash)) = line.split_once('\t') {
                hashes.insert(path.to_string(), hash.to_string());
            }
        }
    }
    Ledger { last, hashes }
}

fn save_ledger(repo: &Path, last: &str, hashes: &BTreeMap<String, String>) -> Result<(), String> {
    let dir = state_dir(repo);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut out = String::with_capacity(hashes.len() * 64 + 128);
    if !last.is_empty() {
        out.push_str("#last\t");
        out.push_str(last);
        out.push('\n');
    }
    for (p, h) in hashes {
        out.push_str(p);
        out.push('\t');
        out.push_str(h);
        out.push('\n');
    }
    let tmp = ledger_path(repo).with_extension("tmp");
    std::fs::write(&tmp, out).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, ledger_path(repo)).map_err(|e| e.to_string())
}

fn changed_paths(cur: &BTreeMap<String, String>, base: &BTreeMap<String, String>) -> Vec<String> {
    let mut keys: BTreeSet<&str> = cur.keys().map(String::as_str).collect();
    keys.extend(base.keys().map(String::as_str));
    keys.into_iter()
        .filter(|&k| cur.get(k) != base.get(k))
        .map(ToString::to_string)
        .collect()
}

pub fn needs_rust(changed: &[String]) -> bool {
    changed.iter().any(|p| {
        RUST_PREFIXES.iter().any(|pre| p.starts_with(pre)) || RUST_FILES.contains(&p.as_str())
    })
}

pub fn needs_kotlin(changed: &[String]) -> bool {
    changed.iter().any(|p| {
        KOTLIN_CODE_PREFIXES.iter().any(|pre| p.starts_with(pre))
            || KOTLIN_CODE_FILES.contains(&p.as_str())
    })
}

pub fn needs_android(changed: &[String]) -> bool {
    changed.iter().any(|p| {
        ANDROID_REACHABLE_PREFIXES
            .iter()
            .any(|pre| p.starts_with(pre))
            || KOTLIN_CODE_FILES.contains(&p.as_str())
    })
}

pub fn next_check_label(changed: &[String]) -> String {
    if changed.is_empty() {
        return "nothing to check".into();
    }
    let mut parts = Vec::new();
    if needs_rust(changed) {
        parts.push("Rust fmt/clippy/test");
    }
    if needs_kotlin(changed) {
        parts.push(if needs_android(changed) {
            "Kotlin lint, desktop tests & Android compile"
        } else {
            "Kotlin lint & desktop tests"
        });
    }
    if parts.is_empty() {
        "quality gates only".into()
    } else {
        parts.join(" + ")
    }
}

pub fn cmd_status(repo: &Path) -> i32 {
    let branch = git_stdout(repo, &["branch", "--show-current"]);
    let head = git_stdout(repo, &["log", "-1", "--format=%h"]);
    let dirty = git_stdout(repo, &["status", "--porcelain"]).lines().count();
    println!("Repository: {branch} at {head}, {dirty} uncommitted path(s)");
    let ledger = load_ledger(repo);
    if !ledger.last.is_empty() {
        println!("Last check: {}", ledger.last);
    }
    match fingerprint(repo) {
        Ok(cur) => {
            let changed = changed_paths(&cur, &ledger.hashes);
            println!(
                "Not yet verified: {} file(s); next check: {}",
                changed.len(),
                next_check_label(&changed)
            );
        }
        Err(e) => eprintln!("Warning: could not fingerprint repo: {e}"),
    }
    if let Some(notice) = storage_budget_notice(repo) {
        println!("{notice}");
    }
    println!();
    if let Ok(todo) = std::fs::read_to_string(repo.join("TODO.md")) {
        println!("{}", quality::status_block(&todo));
    }
    0
}

pub fn run_review(repo: &Path) -> bool {
    let files = match quality::load_files(repo) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("FAIL review: {e}");
            return false;
        }
    };
    let problems = quality::check_files(&files);
    let diff = Command::new("git")
        .args(["-c", "core.safecrlf=false", "diff", "--check"])
        .current_dir(repo)
        .output();
    let ws_ok = diff.as_ref().is_ok_and(|o| o.status.success());

    let fmt = Command::new("cargo")
        .args(["fmt", "--all", "--", "--check"])
        .current_dir(repo)
        .output();
    let fmt_ok = fmt.as_ref().is_ok_and(|o| o.status.success());

    if problems.is_empty() && ws_ok && fmt_ok {
        println!("PASS review");
        return true;
    }
    println!("FAIL review: {} quality problem(s)", problems.len());
    for line in quality::format_problems(&problems) {
        println!("  {line}");
    }
    if !ws_ok && let Ok(o) = diff {
        println!("Whitespace errors:");
        for l in String::from_utf8_lossy(&o.stdout).lines().take(15) {
            println!("  {l}");
        }
    }
    if !fmt_ok {
        println!("Rust formatting check failed; run `cargo fmt --all`.");
    }
    false
}

pub fn cmd_review(repo: &Path) -> i32 {
    if run_review(repo) { 0 } else { 1 }
}

pub fn cmd_rust(repo: &Path, dry_run: bool) -> i32 {
    let steps: &[(&str, &[&str])] = &[
        ("rustfmt", &["fmt", "--all", "--", "--check"]),
        (
            "clippy (workspace)",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        (
            "clippy (slim android-jni)",
            &[
                "clippy",
                "-p",
                "parchment-core",
                "--no-default-features",
                "--features",
                "android-jni",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("test (workspace)", &["test", "--workspace"]),
        (
            "test (slim android-jni)",
            &[
                "test",
                "-p",
                "parchment-core",
                "--no-default-features",
                "--features",
                "android-jni",
            ],
        ),
    ];
    if dry_run {
        for (name, args) in steps {
            println!("Rust [{name}]: cargo {}", args.join(" "));
        }
        return 0;
    }
    let started = Instant::now();
    for (name, args) in steps {
        println!("==> Rust {name}");
        let ok = Command::new("cargo")
            .args(*args)
            .current_dir(repo)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("FAIL rust ({name})");
            return 1;
        }
    }
    println!(
        "PASS rust: {}",
        format_duration(started.elapsed().as_secs())
    );
    auto_enforce_storage_budget(repo);
    0
}

/// Verifies ELF64 `PT_LOAD` segment alignment >= `0x4000` (16 KB Android 15+ page size).
pub fn verify_elf64_16kb_alignment(bytes: &[u8]) -> Result<usize, String> {
    if bytes.len() < 64 || &bytes[0..4] != b"\x7fELF" || bytes[4] != 2 || bytes[5] != 1 {
        return Err("not a valid 64-bit little-endian ELF binary".into());
    }
    let u16_le = |off: usize| -> Result<u16, String> {
        let arr: [u8; 2] = bytes
            .get(off..off + 2)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| "truncated ELF header".to_string())?;
        Ok(u16::from_le_bytes(arr))
    };
    let u32_le = |off: usize| -> Result<u32, String> {
        let arr: [u8; 4] = bytes
            .get(off..off + 4)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| "truncated ELF header".to_string())?;
        Ok(u32::from_le_bytes(arr))
    };
    let u64_le = |off: usize| -> Result<u64, String> {
        let arr: [u8; 8] = bytes
            .get(off..off + 8)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| "truncated ELF header".to_string())?;
        Ok(u64::from_le_bytes(arr))
    };

    let e_phoff = u64_le(32)? as usize;
    let e_phentsize = u16_le(54)? as usize;
    let e_phnum = u16_le(56)? as usize;
    if e_phentsize < 56 || e_phoff + e_phentsize * e_phnum > bytes.len() {
        return Err("truncated ELF program header table".into());
    }
    let mut load_segments = 0usize;
    for i in 0..e_phnum {
        let base = e_phoff + i * e_phentsize;
        let p_type = u32_le(base)?;
        if p_type == 1 {
            // PT_LOAD
            let p_align = u64_le(base + 48)?;
            if p_align < 0x4000 {
                return Err(format!(
                    "PT_LOAD segment #{load_segments} has p_align=0x{p_align:x} (< 0x4000 16 KB)"
                ));
            }
            load_segments += 1;
        }
    }
    if load_segments == 0 {
        return Err("no PT_LOAD segments found in ELF".into());
    }
    Ok(load_segments)
}

pub fn cmd_android_so(repo: &Path, dry_run: bool) -> i32 {
    if dry_run {
        println!(
            "Android SO: cargo ndk -t arm64-v8a -P 26 -o mobile/android-app/src/main/jniLibs build -p parchment-core --no-default-features --features android-jni --profile release-android"
        );
        return 0;
    }
    let started = Instant::now();
    let ok = Command::new("cargo")
        .args([
            "ndk",
            "-t",
            "arm64-v8a",
            "-P",
            "26",
            "-o",
            "mobile/android-app/src/main/jniLibs",
            "build",
            "-p",
            "parchment-core",
            "--no-default-features",
            "--features",
            "android-jni",
            "--profile",
            "release-android",
        ])
        .env(
            "CFLAGS_aarch64_linux_android",
            "-Oz -fno-unwind-tables -fno-asynchronous-unwind-tables -DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_OMIT_DEPRECATED",
        )
        .current_dir(repo)
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("FAIL android-so: cargo ndk build failed");
        return 1;
    }
    let so_path = repo.join("mobile/android-app/src/main/jniLibs/arm64-v8a/libparchment_core.so");
    let Ok(bytes) = std::fs::read(&so_path) else {
        eprintln!("FAIL android-so: missing {}", so_path.display());
        return 1;
    };
    let size = bytes.len() as u64;
    if size > MAX_ANDROID_SO_BYTES {
        eprintln!(
            "FAIL android-so: {:.2} MB exceeds {:.2} MB raw budget",
            size as f64 / 1_048_576.0,
            MAX_ANDROID_SO_BYTES as f64 / 1_048_576.0
        );
        return 1;
    }
    match verify_elf64_16kb_alignment(&bytes) {
        Ok(segs) => {
            println!(
                "PASS android-so: {:.2} MB ({size} B), {segs} PT_LOAD segments 16 KB aligned, {}",
                size as f64 / 1_048_576.0,
                format_duration(started.elapsed().as_secs())
            );
            auto_enforce_storage_budget(repo);
            0
        }
        Err(e) => {
            eprintln!("FAIL android-so: {e}");
            1
        }
    }
}

pub fn cmd_affected(repo: &Path, dry_run: bool) -> i32 {
    let ledger = load_ledger(repo);
    let cur = match fingerprint(repo) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("FAIL affected: {e}");
            return 1;
        }
    };
    let changed = changed_paths(&cur, &ledger.hashes);
    println!(
        "Changed since last verified state: {} file(s); next check: {}",
        changed.len(),
        next_check_label(&changed)
    );
    if dry_run {
        return 0;
    }
    if changed.is_empty() {
        println!("PASS affected: nothing changed since the last verified state");
        return 0;
    }
    if !run_review(repo) {
        return 1;
    }
    let run_rust = needs_rust(&changed);
    let run_kt = needs_kotlin(&changed);
    if !run_rust && !run_kt {
        let _ = save_ledger(repo, "PASS affected (review only)", &cur);
        println!("PASS affected: no build needed");
        return 0;
    }
    let _lock = match SingleRunLock::acquire(repo) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    if run_rust && cmd_rust(repo, false) != 0 {
        let _ = save_ledger(repo, "FAIL affected (rust)", &ledger.hashes);
        return 1;
    }
    if run_kt {
        let mut tasks = vec![
            "ktlintCheck".to_string(),
            ":composeApp:desktopTest".to_string(),
        ];
        if needs_android(&changed) {
            tasks.push(":composeApp:compileAndroidMain".to_string());
        }
        if run_gradle(repo, "affected", &tasks, true, false) != 0 {
            let _ = save_ledger(repo, "FAIL affected (kotlin)", &ledger.hashes);
            return 1;
        }
    }
    let _ = save_ledger(repo, "PASS affected", &cur);
    auto_enforce_storage_budget(repo);
    println!("PASS affected");
    0
}

pub fn cmd_gradle_sub(
    repo: &Path,
    name: &str,
    tasks: &[String],
    dry_run: bool,
    gallery: bool,
) -> i32 {
    if dry_run {
        println!("Gradle: {}", tasks.join(" "));
        return 0;
    }
    let _lock = match SingleRunLock::acquire(repo) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    run_gradle(repo, name, tasks, true, gallery)
}

pub fn cmd_final(repo: &Path, dry_run: bool) -> i32 {
    let missing: Vec<&str> = SIGNER_VARS
        .iter()
        .copied()
        .filter(|v| std::env::var(v).unwrap_or_default().trim().is_empty())
        .collect();
    if !missing.is_empty() && !dry_run {
        eprintln!(
            "BLOCKED: final needs the private signer ({}). In PowerShell run: . .\\scripts\\load-private-signer.ps1",
            missing.join(", ")
        );
        return 2;
    }
    if dry_run {
        println!(
            "Final: review -> rust -> android-so -> release desktop exe -> Gradle ktlintCheck :composeApp:desktopTest (gallery) :androidApp:assembleRelease"
        );
        return 0;
    }
    if !run_review(repo) {
        return 1;
    }
    let cur = match fingerprint(repo) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("FAIL final: {e}");
            return 1;
        }
    };
    let _lock = match SingleRunLock::acquire(repo) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    if cmd_rust(repo, false) != 0 {
        return 1;
    }
    if cmd_android_so(repo, false) != 0 {
        return 1;
    }
    if cfg!(windows) {
        println!("==> Building release GPUI desktop (parchment-desktop)");
        let ok = Command::new("cargo")
            .args(["build", "--release", "-p", "parchment-desktop"])
            .current_dir(repo)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("FAIL final: cargo build --release -p parchment-desktop failed");
            return 1;
        }
        let exe = repo.join("target/release/parchment-desktop.exe");
        let size = std::fs::metadata(&exe).map_or(0, |m| m.len());
        if size == 0 || size > MAX_DESKTOP_EXE_BYTES {
            eprintln!(
                "FAIL final: parchment-desktop.exe {:.2} MB outside budget (<= 35.0 MB)",
                size as f64 / 1_048_576.0
            );
            return 1;
        }
        println!("PASS desktop-exe: {:.2} MB", size as f64 / 1_048_576.0);
    }

    let tasks = vec![
        "ktlintCheck".to_string(),
        ":composeApp:desktopTest".to_string(),
        ":androidApp:assembleRelease".to_string(),
    ];
    if run_gradle(repo, "final", &tasks, false, true) != 0 {
        return 1;
    }
    let apk = repo.join("mobile/android-app/build/outputs/apk/release/androidApp-release.apk");
    let apk_bytes = std::fs::read(&apk).unwrap_or_default();
    if apk_bytes.is_empty() || apk_bytes.len() as u64 > MAX_ANDROID_APK_BYTES {
        eprintln!(
            "FAIL final: signed APK {:.2} MB exceeds 7.50 MB budget",
            apk_bytes.len() as f64 / 1_048_576.0
        );
        return 1;
    }
    if !apk_contains_entry(&apk_bytes, b"lib/arm64-v8a/libparchment_core.so") {
        eprintln!("FAIL final: signed APK missing lib/arm64-v8a/libparchment_core.so");
        return 1;
    }
    println!(
        "PASS final: signed APK {:.2} MB (bundles 16 KB-aligned libparchment_core.so)",
        apk_bytes.len() as f64 / 1_048_576.0
    );
    let _ = save_ledger(repo, "PASS final", &cur);
    auto_enforce_storage_budget(repo);
    0
}

fn apk_contains_entry(zip_bytes: &[u8], needle: &[u8]) -> bool {
    zip_bytes.windows(needle.len()).any(|w| w == needle)
}

pub fn cmd_pre_commit(repo: &Path) -> i32 {
    if !run_review(repo) {
        return 1;
    }
    let staged = git_stdout(repo, &["diff", "--cached", "--name-only"]);
    let paths: Vec<String> = staged.lines().map(ToString::to_string).collect();
    if needs_rust(&paths) {
        println!("==> pre-commit: cargo clippy --workspace --all-targets -- -D warnings");
        let ok = Command::new("cargo")
            .args([
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ])
            .current_dir(repo)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return 1;
        }
    }
    if needs_kotlin(&paths) {
        println!("==> pre-commit: ktlintCheck");
        if run_gradle(
            repo,
            "pre-commit-ktlint",
            &["ktlintCheck".into()],
            true,
            false,
        ) != 0
        {
            return 1;
        }
    }
    println!("PASS pre-commit");
    0
}

pub fn cmd_install_hooks(repo: &Path) -> i32 {
    let ok = Command::new("git")
        .args(["config", "core.hooksPath", ".githooks"])
        .current_dir(repo)
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        println!("Installed git hooks path: .githooks (pre-commit runs `cargo xtask pre-commit`)");
        0
    } else {
        eprintln!("FAIL install-hooks: git config failed");
        1
    }
}

fn run_gradle(repo: &Path, name: &str, tasks: &[String], config_cache: bool, gallery: bool) -> i32 {
    let dir = state_dir(repo);
    let _ = std::fs::create_dir_all(&dir);
    let log_path = dir.join(format!("{name}.log"));
    let wrapper = repo.join(if cfg!(windows) {
        "gradlew.bat"
    } else {
        "gradlew"
    });

    let mut flags: Vec<String> = vec![
        "--no-daemon".into(),
        "--max-workers=2".into(),
        "-Pkotlin.compiler.execution.strategy=in-process".into(),
        format!("-PappVersion={APP_VERSION}"),
        format!("-PappVersionCode={APP_VERSION_CODE}"),
        "--console=plain".into(),
        "--quiet".into(),
    ];
    if config_cache {
        flags.push("--configuration-cache".into());
    }
    if gallery {
        flags.push("-Pparchment.gallery=true".into());
    }

    let started = Instant::now();
    let Ok(log_file) = File::create(&log_path) else {
        eprintln!("FAIL {name}: cannot create {}", log_path.display());
        return 1;
    };
    let Ok(log_err) = log_file.try_clone() else {
        return 1;
    };
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new(&wrapper);
        c.args(tasks).args(&flags);
        c
    } else {
        let mut c = Command::new("bash");
        c.arg(&wrapper).args(tasks).args(&flags);
        c
    };
    if let Ok(sdk) = std::env::var("ANDROID_HOME")
        && !sdk.is_empty()
    {
        cmd.env("ANDROID_SDK_ROOT", sdk);
    }
    let status = cmd
        .current_dir(repo)
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_err))
        .status();
    let exit_code = status.map_or(1, |s| s.code().unwrap_or(1));
    let elapsed = format_duration(started.elapsed().as_secs());

    if exit_code == 0 {
        println!("PASS {name}: Gradle succeeded, {elapsed}");
        return 0;
    }
    eprintln!("FAIL {name}: Gradle exit {exit_code}, {elapsed}");
    if let Ok(log_text) = std::fs::read_to_string(&log_path) {
        let mut shown = 0usize;
        for line in log_text.lines() {
            if (line.starts_with("e: ") && !line.starts_with("e: Incremental compilation failed"))
                || line.contains("FAILED")
                || line.starts_with("> ")
            {
                eprintln!("  {line}");
                shown += 1;
                if shown >= 25 {
                    break;
                }
            }
        }
        if shown == 0 {
            let tail: Vec<&str> = log_text.lines().rev().take(15).collect();
            for line in tail.into_iter().rev() {
                eprintln!("  {line}");
            }
        }
    }
    eprintln!("Log: .parchment-local/verification/{name}.log");
    exit_code
}

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn format_duration(secs: u64) -> String {
    if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}
fn dir_size_bytes(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    if path.is_file() {
        return std::fs::metadata(path).map_or(0, |m| m.len());
    }
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    stack.push(p);
                } else {
                    total = total.saturating_add(meta.len());
                }
            }
        }
    }
    total
}

fn fmt_gib_or_mib(bytes: u64) -> String {
    let mib = bytes as f64 / 1_048_576.0;
    if mib >= 1024.0 {
        format!("{:.2} GiB", mib / 1024.0)
    } else {
        format!("{mib:.1} MiB")
    }
}

fn cache_totals(repo: &Path) -> (u64, u64) {
    let target_b = dir_size_bytes(&repo.join("target"));
    let gradle_b = dir_size_bytes(&repo.join("mobile/compose-app/build"))
        + dir_size_bytes(&repo.join("mobile/android-app/build"))
        + dir_size_bytes(&repo.join("build"))
        + dir_size_bytes(&repo.join(".gradle"));
    (target_b, gradle_b)
}

pub fn storage_budget_notice(repo: &Path) -> Option<String> {
    let (target_b, gradle_b) = cache_totals(repo);
    if target_b > MAX_TARGET_CACHE_BYTES || gradle_b > MAX_GRADLE_CACHE_BYTES {
        Some(format!(
            "Storage QC: build caches at {} (target {}, gradle {}; budgets 5.0 GiB / 1.0 GiB) — run `cargo xtask prune`",
            fmt_gib_or_mib(target_b + gradle_b),
            fmt_gib_or_mib(target_b),
            fmt_gib_or_mib(gradle_b)
        ))
    } else {
        None
    }
}

/// Automatically prunes superseded build hashes, orphaned pre-1.100 `deps/`, and legacy v2
/// folders whenever workspace build caches exceed the 5 GiB / 1 GiB storage budgets.
fn auto_enforce_storage_budget(repo: &Path) {
    let (target_b, gradle_b) = cache_totals(repo);
    if target_b <= MAX_TARGET_CACHE_BYTES && gradle_b <= MAX_GRADLE_CACHE_BYTES {
        return;
    }
    let (removed, reclaimed) = prune_workspace_caches(repo, false);
    if removed > 0 {
        let (after_target, after_gradle) = cache_totals(repo);
        println!(
            "Storage QC: auto-reclaimed {} across {removed} stale item(s) (now target {}, gradle {}).",
            fmt_gib_or_mib(reclaimed),
            fmt_gib_or_mib(after_target),
            fmt_gib_or_mib(after_gradle)
        );
    }
}
pub fn cmd_disk(repo: &Path) -> i32 {
    let buckets: &[(&str, u64)] = &[
        ("target/", MAX_TARGET_CACHE_BYTES),
        ("mobile/compose-app/build/", MAX_GRADLE_CACHE_BYTES),
        ("mobile/android-app/build/", MAX_GRADLE_CACHE_BYTES),
        ("build/", MAX_GRADLE_CACHE_BYTES),
        (".gradle/", MAX_GRADLE_CACHE_BYTES),
        (".parchment-local/", 128 * 1024 * 1024),
    ];
    let mut total_cache = 0u64;
    let mut over_budget = false;
    println!("Workspace build & cache storage report:");
    for &(rel, budget) in buckets {
        let bytes = dir_size_bytes(&repo.join(rel));
        total_cache = total_cache.saturating_add(bytes);
        let flag = if bytes > budget {
            over_budget = true;
            " [OVER BUDGET]"
        } else {
            ""
        };
        println!("  {:>10}  {rel}{flag}", fmt_gib_or_mib(bytes));
    }
    println!(
        "  {:>10}  TOTAL build/cache artifacts",
        fmt_gib_or_mib(total_cache)
    );
    if over_budget {
        println!("Run `cargo xtask prune` to safely reclaim stale incremental/deps/log bloat.");
    }
    0
}

/// Extracts the crate stem before the trailing `-<16hex>` Cargo hash (e.g. `libfoo-0123456789abcdef.rlib` -> `libfoo`).
fn cargo_hashed_stem(name: &str) -> Option<&str> {
    let base = name.split('.').next()?;
    let (stem, hash) = base.rsplit_once('-')?;
    if stem.is_empty()
        || hash.len() < 12
        || !hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || b.is_ascii_alphanumeric())
    {
        return None;
    }
    // Cargo incremental dirs use `<crate>-<base36>` and deps/build use `<crate>-<16hex>`.
    if hash.bytes().any(|b| b.is_ascii_digit()) {
        Some(stem)
    } else {
        None
    }
}

/// Prunes stale hashed entries in `dir`, retaining the `keep_newest` most recently modified per crate stem.
pub fn prune_stale_hashed_entries(dir: &Path, keep_newest: usize, dry_run: bool) -> (usize, u64) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut groups: BTreeMap<String, Vec<(std::time::SystemTime, PathBuf)>> = BTreeMap::new();
    for entry in rd.flatten() {
        let p = entry.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = cargo_hashed_stem(name) else {
            continue;
        };
        // Group by stem + extension so `.rlib`/`.rmeta`/`.d` for the same hash stay aligned or prune together by hash key.
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or(if p.is_dir() { "<dir>" } else { "" });
        let key = format!("{stem}\0{ext}");
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        groups.entry(key).or_default().push((mtime, p));
    }
    let mut removed = 0usize;
    let mut reclaimed = 0u64;
    for entries in groups.values_mut() {
        if entries.len() <= keep_newest {
            continue;
        }
        entries.sort_by_key(|a| std::cmp::Reverse(a.0));
        for (_, stale) in entries.iter().skip(keep_newest) {
            let sz = dir_size_bytes(stale);
            if !dry_run {
                let ok = if stale.is_dir() {
                    std::fs::remove_dir_all(stale).is_ok()
                } else {
                    std::fs::remove_file(stale).is_ok()
                };
                if !ok {
                    continue;
                }
            }
            removed += 1;
            reclaimed = reclaimed.saturating_add(sz);
        }
    }
    (removed, reclaimed)
}

fn is_cargo_hash_dir_name(name: &str) -> bool {
    name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

fn newest_mtime_in(dir: &Path) -> Option<std::time::SystemTime> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut newest: Option<std::time::SystemTime> = None;
    for entry in rd.flatten() {
        if let Ok(mt) = entry.metadata().and_then(|m| m.modified()) {
            newest = Some(newest.map_or(mt, |cur| cur.max(mt)));
        }
    }
    newest
}

/// Returns the Cargo unit kind key inside `build/<crate>/<hash>/fingerprint/` (e.g.
/// `lib-foo.json`, `run-build-script-*.json`, `build-script-*.json`, `test-*.json`) so
/// distinct unit kinds for the same crate never prune each other.
fn cargo_unit_kind_key(hash_dir: &Path) -> String {
    let mut names: Vec<String> = std::fs::read_dir(hash_dir.join("fingerprint"))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_str()?.to_string();
            n.ends_with(".json").then_some(n)
        })
        .collect();
    names.sort();
    if !names.is_empty() {
        return names.join("+");
    }
    if hash_dir.join("run").is_dir() {
        return "run-build-script".into();
    }
    "unit".into()
}

fn contains_proc_macro_dll(hash_dir: &Path) -> bool {
    std::fs::read_dir(hash_dir.join("out"))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("dll"))
        })
}

/// Prunes Cargo 1.100 nested `target/<profile>/build/<crate>/<16hex>/` directories grouped by
/// unit kind (preserving `build-script`, `run-build-script`, and active proc-macro DLLs) as well
/// as legacy flat `<crate>-<hash>` build-script directories.
pub fn prune_cargo_build_dir(build_dir: &Path, keep_newest: usize, dry_run: bool) -> (usize, u64) {
    let (mut removed, mut reclaimed) = prune_stale_hashed_entries(build_dir, 1, dry_run);
    let Ok(rd) = std::fs::read_dir(build_dir) else {
        return (removed, reclaimed);
    };
    for crate_entry in rd.flatten() {
        let crate_dir = crate_entry.path();
        if !crate_dir.is_dir() {
            continue;
        }
        let Ok(sub_rd) = std::fs::read_dir(&crate_dir) else {
            continue;
        };
        let mut by_kind: BTreeMap<String, Vec<(std::time::SystemTime, PathBuf)>> = BTreeMap::new();
        for sub in sub_rd.flatten() {
            let p = sub.path();
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if p.is_dir() && is_cargo_hash_dir_name(name) {
                let kind = cargo_unit_kind_key(&p);
                let mt = newest_mtime_in(&p.join("out"))
                    .or_else(|| sub.metadata().and_then(|m| m.modified()).ok())
                    .unwrap_or(std::time::UNIX_EPOCH);
                by_kind.entry(kind).or_default().push((mt, p));
            }
        }
        for hashes in by_kind.values_mut() {
            if hashes.len() <= keep_newest {
                continue;
            }
            hashes.sort_by_key(|a| std::cmp::Reverse(a.0));
            for (_, stale) in hashes.iter().skip(keep_newest) {
                // Windows IDEs (`rust-analyzer`) can map proc-macro DLLs; skip locked DLLs safely.
                if contains_proc_macro_dll(stale) {
                    continue;
                }
                let sz = dir_size_bytes(stale);
                if !dry_run && std::fs::remove_dir_all(stale).is_err() {
                    continue;
                }
                removed += 1;
                reclaimed = reclaimed.saturating_add(sz);
            }
        }
    }
    (removed, reclaimed)
}

/// Removes an orphaned pre-Cargo-1.100 `deps/` directory when the profile has migrated to Cargo's
/// nested `build/<crate>/<16hex>/out/` layout and every file in `deps/` is strictly older than
/// the active `build/` outputs.
pub fn prune_orphaned_legacy_deps(profile_dir: &Path, dry_run: bool) -> (usize, u64) {
    let deps_dir = profile_dir.join("deps");
    let build_dir = profile_dir.join("build");
    if !deps_dir.is_dir() || !build_dir.is_dir() {
        return (0, 0);
    }
    let Ok(rd) = std::fs::read_dir(&build_dir) else {
        return (0, 0);
    };
    let mut nested_crates = 0usize;
    let mut newest_build: Option<std::time::SystemTime> = None;
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let Ok(sub_rd) = std::fs::read_dir(&p) else {
            continue;
        };
        for sub in sub_rd.flatten() {
            let sp = sub.path();
            if sp.is_dir()
                && sp
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(is_cargo_hash_dir_name)
                && sp.join("out").is_dir()
            {
                nested_crates += 1;
                if let Some(mt) = newest_mtime_in(&sp.join("out")) {
                    newest_build = Some(newest_build.map_or(mt, |cur| cur.max(mt)));
                }
                break;
            }
        }
    }
    if nested_crates < 5 {
        return (0, 0);
    }
    let (Some(build_mt), Some(deps_mt)) = (newest_build, newest_mtime_in(&deps_dir)) else {
        return (0, 0);
    };
    if deps_mt >= build_mt {
        return (0, 0);
    }
    let sz = dir_size_bytes(&deps_dir);
    if !dry_run && std::fs::remove_dir_all(&deps_dir).is_err() {
        return (0, 0);
    }
    (1, sz)
}

fn prune_workspace_caches(repo: &Path, dry_run: bool) -> (usize, u64) {
    let mut total_removed = 0usize;
    let mut total_reclaimed = 0u64;

    // 1. Obsolete host-compiled release-android & dead legacy v2 JVM desktop packaging caches.
    for rel in [
        "target/release-android",
        "build/wix311",
        "mobile/compose-app/build/compose",
        "mobile/compose-app/build/jdk-library-classes",
    ] {
        let p = repo.join(rel);
        if p.is_dir() {
            let sz = dir_size_bytes(&p);
            if dry_run || std::fs::remove_dir_all(&p).is_ok() {
                total_removed += 1;
                total_reclaimed = total_reclaimed.saturating_add(sz);
            }
        }
    }

    // 2. Keep newest incremental session per crate; prune older sessions.
    for rel in ["target/debug/incremental", "target/release/incremental"] {
        let (n, b) = prune_stale_hashed_entries(&repo.join(rel), 1, dry_run);
        total_removed += n;
        total_reclaimed = total_reclaimed.saturating_add(b);
    }

    // 3. Remove orphaned pre-Cargo-1.100 `deps/` superseded by nested `build/<crate>/<hash>/out/`.
    let profiles = [
        ("target/debug", 2usize),
        ("target/release", 1usize),
        ("target/aarch64-linux-android/release-android", 1usize),
    ];
    for (prof, keep) in profiles {
        let prof_dir = repo.join(prof);
        let (n_orph, b_orph) = prune_orphaned_legacy_deps(&prof_dir, dry_run);
        total_removed += n_orph;
        total_reclaimed = total_reclaimed.saturating_add(b_orph);

        let (n_build, b_build) = prune_cargo_build_dir(&prof_dir.join("build"), keep, dry_run);
        total_removed += n_build;
        total_reclaimed = total_reclaimed.saturating_add(b_build);

        if n_orph == 0 {
            let (n_deps, b_deps) = prune_stale_hashed_entries(&prof_dir.join("deps"), 1, dry_run);
            total_removed += n_deps;
            total_reclaimed = total_reclaimed.saturating_add(b_deps);
        }
    }
    // 5. Prune oversized verification logs (> 2 MiB) while preserving ledger.txt and perf.txt.
    let ver_dir = state_dir(repo);
    if let Ok(rd) = std::fs::read_dir(&ver_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("log") {
                let sz = entry.metadata().map_or(0, |m| m.len());
                if sz > MAX_LOCAL_LOG_BYTES && (dry_run || std::fs::remove_file(&p).is_ok()) {
                    total_removed += 1;
                    total_reclaimed = total_reclaimed.saturating_add(sz);
                }
            }
        }
    }
    (total_removed, total_reclaimed)
}

pub fn cmd_prune(repo: &Path, dry_run: bool) -> i32 {
    let (total_removed, total_reclaimed) = prune_workspace_caches(repo, dry_run);
    let verb = if dry_run {
        "Would reclaim"
    } else {
        "Reclaimed"
    };
    println!(
        "{verb} {} across {total_removed} stale cache item(s).",
        fmt_gib_or_mib(total_reclaimed)
    );
    cmd_disk(repo)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn elf64_alignment_checker_accepts_16kb_and_rejects_4kb() {
        let mut elf = vec![0u8; 128];
        elf[0..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2; // ELFCLASS64
        elf[5] = 1; // ELFDATA2LSB
        elf[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff = 64
        elf[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize = 56
        elf[56..58].copy_from_slice(&1u16.to_le_bytes()); // e_phnum = 1
        elf[64..68].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
        elf[64 + 48..64 + 56].copy_from_slice(&0x4000u64.to_le_bytes());
        assert_eq!(verify_elf64_16kb_alignment(&elf).unwrap(), 1);

        elf[64 + 48..64 + 56].copy_from_slice(&0x1000u64.to_le_bytes());
        assert!(verify_elf64_16kb_alignment(&elf).is_err());
    }

    #[test]
    fn affected_classifier_separates_rust_and_kotlin_targets() {
        let rust_only = vec!["crates/parchment-prose/src/diff.rs".to_string()];
        assert!(needs_rust(&rust_only));
        assert!(!needs_kotlin(&rust_only));

        let kt_android = vec![
            "mobile/compose-app/src/androidMain/kotlin/io/parchment/ai/MainActivity.kt".to_string(),
        ];
        assert!(!needs_rust(&kt_android));
        assert!(needs_kotlin(&kt_android));
        assert!(needs_android(&kt_android));

        let docs_only = vec!["README.md".to_string()];
        assert_eq!(next_check_label(&docs_only), "quality gates only");
    }

    #[test]
    fn prune_stale_hashed_entries_keeps_newest_and_removes_older() {
        let dir = std::env::temp_dir().join(format!("parchment-prune-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (i, hash) in ["0000000000000001", "0000000000000002", "0000000000000003"]
            .iter()
            .enumerate()
        {
            let f = dir.join(format!("libfoo-{hash}.rlib"));
            std::fs::write(&f, vec![0u8; 1024 * (i + 1)]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let (removed, reclaimed) = prune_stale_hashed_entries(&dir, 1, false);
        assert_eq!(removed, 2);
        assert!(reclaimed >= 3072);
        assert!(dir.join("libfoo-0000000000000003.rlib").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_cargo_build_dir_and_orphaned_legacy_deps_reclaim_superseded_outputs() {
        let root = std::env::temp_dir().join(format!("parchment-bdir-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let deps = root.join("deps");
        std::fs::create_dir_all(&deps).unwrap();
        std::fs::write(deps.join("libold-0000000000000001.rlib"), vec![0u8; 4096]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));

        let build = root.join("build");
        for i in 0..6 {
            let out_old = build.join(format!("crate{i}/00000000000000a{i}/out"));
            std::fs::create_dir_all(&out_old).unwrap();
            std::fs::write(out_old.join("lib.rlib"), vec![0u8; 2048]).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
        let out_new = build.join("crate0/00000000000000b0/out");
        std::fs::create_dir_all(&out_new).unwrap();
        std::fs::write(out_new.join("lib.rlib"), vec![0u8; 1024]).unwrap();

        let (n_deps, b_deps) = prune_orphaned_legacy_deps(&root, false);
        assert_eq!(n_deps, 1);
        assert!(b_deps >= 4096);
        assert!(!deps.exists());

        let (n_build, b_build) = prune_cargo_build_dir(&build, 1, false);
        assert_eq!(n_build, 1);
        assert!(b_build >= 2048);
        assert!(out_new.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
