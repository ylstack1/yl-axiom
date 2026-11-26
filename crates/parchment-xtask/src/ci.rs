//! Zero-dependency CI workflow tools (`cargo xtask ci ...`).
//!
//! Covers path-based CI job planning (both Rust Desktop and Kotlin Mobile), single-source release
//! version resolution, download-retry Gradle execution, balanced Kotlin `@Test` sharding,
//! CycloneDX SBOM validation, and private Android keystore staging.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::quality::strip_kotlin_comments;

const ZERO_SHA: &str = "0000000000000000000000000000000000000000";
const EXPOSED_CERT_SHA256: &str =
    "ed96a0a733e9e6d38633f04a6a182c997fe60d480d9e33e5f078faa2536d26eb";

const SHARED_GRADLE_FILES: &[&str] = &[
    "build.gradle.kts",
    "settings.gradle.kts",
    "gradle.properties",
    ".editorconfig",
    "gradlew",
    "gradlew.bat",
    "mobile/compose-app/build.gradle.kts",
    "mobile/compose-app/stability-config.conf",
    ".release-please-manifest.json",
    ".github/workflows/build-installers.yml",
];

const RUST_ANDROID_PREFIXES: &[&str] = &[
    "crates/parchment-model/",
    "crates/parchment-storage/",
    "crates/parchment-prose/",
    "crates/parchment-harness/",
    "crates/parchment-core/",
];

const RUST_CORE_PREFIXES: &[&str] = &[
    "crates/parchment-model/",
    "crates/parchment-storage/",
    "crates/parchment-protocol/",
    "crates/parchment-prose/",
    "crates/parchment-harness/",
    "crates/parchment-core/",
];

const RUST_ALL_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rustfmt.toml",
    "clippy.toml",
    ".release-please-manifest.json",
];
const SIGNING_TRIGGERS: &[&str] = &[
    ".github/scripts/ValidateAndroidSigningKey.java",
    ".github/workflows/build-installers.yml",
    "crates/parchment-xtask/src/ci.rs",
];

const WINDOWS_NATIVE_TEST_FILTERS: &[&str] = &[
    "io.parchment.ai.CdsWarmupWatchdogTest",
    "io.parchment.ai.WindowCaptionTest",
    "io.parchment.ai.platform.*",
    "io.parchment.ai.harness.OwnedShell*",
];

pub fn dispatch(repo: &Path, args: &[String]) -> i32 {
    let Some((sub, rest)) = args.split_first() else {
        eprintln!("Usage: cargo xtask ci <plan|run-gradle|shard-tests|check-sbom|stage-signing>");
        return 2;
    };
    let res = match sub.as_str() {
        "plan" => cmd_plan(repo, rest),
        "run-gradle" => return cmd_run_gradle(repo, rest),
        "shard-tests" => cmd_shard_tests(repo, rest),
        "check-sbom" => cmd_check_sbom(rest),
        "stage-signing" => cmd_stage_signing(repo),
        other => Err(format!("unknown ci subcommand: {other}")),
    };
    match res {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("FAIL ci {sub}: {msg}");
            1
        }
    }
}

// ---- 1. CI Path Planner & Version Resolver ----

pub fn resolve_version(
    git_ref: &str,
    build_number: u64,
    version_line: &str,
    version_base_run: u64,
) -> Result<BTreeMap<&'static str, String>, String> {
    if build_number < 1 {
        return Err(format!("Invalid build number: {build_number}"));
    }
    let (app_version, release_tag) = if let Some(ver) = git_ref.strip_prefix("refs/tags/v") {
        (ver.to_string(), format!("v{ver}"))
    } else {
        let ver = if build_number >= version_base_run {
            format!("{version_line}.{}", build_number - version_base_run)
        } else {
            format!("2.0.{build_number}")
        };
        (ver.clone(), format!("v{ver}"))
    };
    if !is_semver(&app_version) {
        return Err(format!("Invalid release version: {app_version}"));
    }
    let mut map = BTreeMap::new();
    map.insert("build_number", build_number.to_string());
    map.insert("app_version", app_version);
    map.insert("app_version_code", (200 + build_number).to_string());
    map.insert("release_tag", release_tag);
    Ok(map)
}

fn is_semver(v: &str) -> bool {
    let mut parts = v.split('.');
    let Some(a) = parts.next() else { return false };
    let Some(b) = parts.next() else { return false };
    let Some(c) = parts.next() else { return false };
    parts.next().is_none()
        && [a, b, c]
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|ch| ch.is_ascii_digit()))
}

pub fn classify_installers(
    paths: Option<&HashSet<String>>,
    reuse_android_run: bool,
) -> BTreeMap<&'static str, String> {
    let mut out = BTreeMap::new();
    let Some(paths) = paths else {
        out.insert("rust_core", "true".into());
        out.insert("rust_windows", "true".into());
        out.insert("desktop_tests", "true".into());
        out.insert("windows_tests", "true".into());
        out.insert(
            "android",
            if reuse_android_run { "false" } else { "true" }.into(),
        );
        out.insert(
            "signing_scripts",
            if reuse_android_run { "false" } else { "true" }.into(),
        );
        return out;
    };

    let any_p = |pred: &dyn Fn(&str) -> bool| paths.iter().any(|p| pred(p.as_str()));

    let shared_gradle = any_p(&|p| {
        p.starts_with("gradle/")
            || SHARED_GRADLE_FILES.iter().any(|s| p == s.as_str())
            || p.starts_with("mobile/compose-app/src/commonMain/")
            || p.starts_with("mobile/compose-app/src/jvmShared/")
    });
    let rust_desktop_or_core = any_p(&|p| {
        RUST_CORE_PREFIXES.iter().any(|pre| p.starts_with(pre))
            || p.starts_with("crates/parchment-desktop/")
            || p.starts_with(".cargo/")
            || RUST_ALL_FILES.contains(&p)
            || p == ".github/workflows/build-installers.yml"
    });
    let rust_core_for_android = any_p(&|p| {
        RUST_ANDROID_PREFIXES.iter().any(|pre| p.starts_with(pre))
            || p.starts_with(".cargo/")
            || p == "Cargo.toml"
            || p == "Cargo.lock"
    });
    let rust_core = any_p(&|p| {
        (p.starts_with("crates/") && !p.starts_with("crates/parchment-desktop/"))
            || p.starts_with(".cargo/")
            || RUST_ALL_FILES.contains(&p)
            || p == ".github/workflows/build-installers.yml"
    });
    let rust_windows = rust_desktop_or_core || any_p(&|p| p.starts_with("installer/"));

    let desktop_tests = shared_gradle
        || any_p(&|p| {
            p.starts_with("mobile/compose-app/src/desktopMain/")
                || p.starts_with("mobile/compose-app/src/commonTest/")
                || p.starts_with("mobile/compose-app/src/desktopTest/")
        });
    let windows_tests = shared_gradle
        || any_p(&|p| {
            p.starts_with("mobile/compose-app/src/desktopMain/")
                || p.starts_with(
                    "mobile/compose-app/src/desktopTest/kotlin/io/parchment/ai/platform/",
                )
                || p.starts_with(
                    "mobile/compose-app/src/desktopTest/kotlin/io/parchment/ai/harness/OwnedShell",
                )
                || p.starts_with(
                    "mobile/compose-app/src/commonTest/kotlin/io/parchment/ai/harness/OwnedShell",
                )
                || p == "mobile/compose-app/src/desktopTest/kotlin/io/parchment/ai/CdsWarmupWatchdogTest.kt"
                || p == "mobile/compose-app/src/desktopTest/kotlin/io/parchment/ai/WindowCaptionTest.kt"
        });
    let android = !reuse_android_run
        && (shared_gradle
            || rust_core_for_android
            || any_p(&|p| {
                p.starts_with("mobile/android-app/")
                    || p.starts_with("mobile/compose-app/src/androidMain/")
                    || p == ".github/scripts/ValidateAndroidSigningKey.java"
                    || p == ".github/scripts/sbom.init.gradle.kts"
            }));
    let signing_scripts = !reuse_android_run && any_p(&|p| SIGNING_TRIGGERS.contains(&p));

    let b = |v: bool| if v { "true" } else { "false" }.to_string();
    out.insert("rust_core", b(rust_core));
    out.insert("rust_windows", b(rust_windows));
    out.insert("desktop_tests", b(desktop_tests));
    out.insert("windows_tests", b(windows_tests));
    out.insert("android", b(android));
    out.insert("signing_scripts", b(signing_scripts));
    out
}

pub fn classify_quality_signals(paths: Option<&HashSet<String>>) -> BTreeMap<&'static str, String> {
    let mut out = BTreeMap::new();
    let Some(paths) = paths else {
        out.insert("coverage", "true".into());
        out.insert("gallery", "true".into());
        out.insert("sbom", "true".into());
        return out;
    };
    let any_p = |pred: &dyn Fn(&str) -> bool| paths.iter().any(|p| pred(p.as_str()));
    let shared_gradle = any_p(&|p| {
        p.starts_with("gradle/")
            || matches!(
                p,
                "build.gradle.kts"
                    | "settings.gradle.kts"
                    | "gradle.properties"
                    | "gradlew"
                    | "gradlew.bat"
                    | "mobile/compose-app/build.gradle.kts"
            )
    });
    let coverage = shared_gradle
        || any_p(&|p| {
            p.starts_with("mobile/compose-app/src/commonMain/")
                || p.starts_with("mobile/compose-app/src/jvmShared/")
                || p.starts_with("mobile/compose-app/src/desktopMain/")
                || p.starts_with("mobile/compose-app/src/commonTest/")
                || p.starts_with("mobile/compose-app/src/desktopTest/")
                || p == "mobile/compose-app/stability-config.conf"
                || p == ".github/scripts/kover.init.gradle.kts"
                || p == ".github/workflows/quality-signals.yml"
        });
    let gallery = shared_gradle
        || any_p(&|p| {
            p.starts_with("mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/ui/")
                || p.starts_with("mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/model/")
                || p.starts_with(
                    "mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/strings/",
                )
                || p.starts_with("mobile/compose-app/src/commonMain/resources/")
                || p.starts_with("mobile/compose-app/src/jvmShared/")
                || p.starts_with("mobile/compose-app/src/desktopMain/")
                || p == "mobile/compose-app/stability-config.conf"
                || p == "mobile/compose-app/src/desktopTest/kotlin/io/parchment/ai/ui/UiGalleryTest.kt"
                || p == ".github/workflows/quality-signals.yml"
        });
    let sbom = shared_gradle
        || any_p(&|p| {
            p == "mobile/android-app/build.gradle.kts"
                || p == ".github/scripts/sbom.init.gradle.kts"
                || p == ".github/workflows/quality-signals.yml"
        });
    let b = |v: bool| if v { "true" } else { "false" }.to_string();
    out.insert("coverage", b(coverage));
    out.insert("gallery", b(gallery));
    out.insert("sbom", b(sbom));
    out
}

pub fn classify_codeql(paths: Option<&HashSet<String>>) -> BTreeMap<&'static str, String> {
    let all = r#"[{"language":"java-kotlin","build-mode":"manual"},{"language":"rust","build-mode":"none"},{"language":"actions","build-mode":"none"}]"#;
    let mut out = BTreeMap::new();
    let Some(paths) = paths else {
        out.insert("has_targets", "true".into());
        out.insert("matrix", all.into());
        return out;
    };
    if paths.contains(".github/workflows/codeql.yml") {
        out.insert("has_targets", "true".into());
        out.insert("matrix", all.into());
        return out;
    }
    let has_jvm = paths.iter().any(|p| {
        !p.starts_with("mobile/compose-app/src/commonTest/")
            && !p.starts_with("mobile/compose-app/src/desktopTest/")
            && (p.ends_with(".kt")
                || p.ends_with(".java")
                || p.ends_with(".kts")
                || p.starts_with("gradle/")
                || matches!(p.as_str(), "gradle.properties" | "gradlew" | "gradlew.bat")
                || p.ends_with("AndroidManifest.xml"))
    });
    let has_rust = paths.iter().any(|p| {
        p.ends_with(".rs") || p == "Cargo.toml" || p == "Cargo.lock" || p.starts_with(".cargo/")
    });
    let has_actions = paths
        .iter()
        .any(|p| p.starts_with(".github/workflows/") || p == ".github/dependabot.yml");

    let mut entries = Vec::new();
    if has_jvm {
        entries.push(r#"{"language":"java-kotlin","build-mode":"manual"}"#);
    }
    if has_rust {
        entries.push(r#"{"language":"rust","build-mode":"none"}"#);
    }
    if has_actions {
        entries.push(r#"{"language":"actions","build-mode":"none"}"#);
    }
    if entries.is_empty() {
        out.insert("has_targets", "false".into());
        out.insert(
            "matrix",
            r#"[{"language":"actions","build-mode":"none"}]"#.into(),
        );
    } else {
        out.insert("has_targets", "true".into());
        out.insert("matrix", format!("[{}]", entries.join(",")));
    }
    out
}

pub fn classify_android(paths: Option<&HashSet<String>>) -> BTreeMap<&'static str, String> {
    let mut out = BTreeMap::new();
    let Some(paths) = paths else {
        out.insert("rust_core", "true".into());
        out.insert("android", "true".into());
        out.insert("ktlint", "true".into());
        return out;
    };

    let shared_gradle = paths.iter().any(|p| {
        p.starts_with("gradle/")
            || SHARED_GRADLE_FILES.iter().any(|s| p == s.as_str())
            || p.starts_with("mobile/compose-app/src/commonMain/")
            || p.starts_with("mobile/compose-app/src/jvmShared/")
    });
    let rust_core_for_android = paths.iter().any(|p| {
        RUST_ANDROID_PREFIXES.iter().any(|pre| p.starts_with(pre))
            || p.starts_with(".cargo/")
            || p == "Cargo.toml"
            || p == "Cargo.lock"
    });
    let android = shared_gradle
        || rust_core_for_android
        || paths.iter().any(|p| {
            p.starts_with("mobile/android-app/")
                || p.starts_with("mobile/compose-app/src/androidMain/")
                || p == ".github/scripts/ValidateAndroidSigningKey.java"
                || p == ".github/scripts/sbom.init.gradle.kts"
        });
    let ktlint = shared_gradle
        || paths.iter().any(|p| {
            p.ends_with(".kt")
                || p.ends_with(".kts")
                || p == "build.gradle.kts"
                || p == "settings.gradle.kts"
                || p == "gradle.properties"
                || p == ".editorconfig"
        });

    let b = |v: bool| if v { "true" } else { "false" }.to_string();
    out.insert("rust_core", b(rust_core_for_android));
    out.insert("android", b(android));
    out.insert("ktlint", b(ktlint));
    out
}

fn changed_files_from_git(
    repo: &Path,
    base_sha: &str,
    github_repo: &str,
) -> Option<HashSet<String>> {
    let base = base_sha.trim();
    if base.len() != 40 || base == ZERO_SHA || !base.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let has_commit = Command::new("git")
        .args(["cat-file", "-e", &format!("{base}^{{commit}}")])
        .current_dir(repo)
        .status()
        .is_ok_and(|s| s.success())
        || Command::new("git")
            .args(["fetch", "--no-tags", "--depth=1", "origin", base])
            .current_dir(repo)
            .status()
            .is_ok_and(|s| s.success());
    if has_commit
        && let Ok(diff) = Command::new("git")
            .args(["diff", "--name-only", "-z", base, "HEAD"])
            .current_dir(repo)
            .output()
        && diff.status.success()
    {
        let set = diff
            .stdout
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        return Some(set);
    }
    if !github_repo.is_empty()
        && std::env::var("GH_TOKEN").is_ok_and(|t| !t.is_empty())
        && let Ok(api) = Command::new("gh")
            .args([
                "api",
                &format!("repos/{github_repo}/compare/{base}...HEAD"),
                "--jq",
                ".files[].filename",
            ])
            .current_dir(repo)
            .output()
        && api.status.success()
    {
        let set = String::from_utf8_lossy_owned(api.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(ToString::to_string)
            .collect();
        return Some(set);
    }
    None
}

fn resolve_build_number(
    run_number: u64,
    git_ref: &str,
    github_repo: &str,
    reuse_android_run: &str,
) -> Result<u64, String> {
    let reuse = reuse_android_run.trim();
    if reuse.is_empty() {
        return Ok(run_number);
    }
    if !reuse.bytes().all(|b| b.is_ascii_digit()) {
        return Err("reuse_android_run must be a numeric run ID".into());
    }
    if git_ref.starts_with("refs/tags/") {
        return Err("Reusing an APK is for branch runs only".into());
    }
    let out = Command::new("gh")
        .args([
            "api",
            &format!("repos/{github_repo}/actions/runs/{reuse}"),
            "--jq",
            r#"\(.path) \(.run_number)"#,
        ])
        .output()
        .map_err(|e| format!("failed to query gh api for run {reuse}: {e}"))?;
    if !out.status.success() {
        return Err(format!("Could not read run {reuse}"));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.split_whitespace();
    let path = parts.next().unwrap_or("");
    let num = parts.next().unwrap_or("");
    if path != ".github/workflows/build-installers.yml" {
        return Err(format!("Run {reuse} is not an installer build"));
    }
    num.parse::<u64>()
        .map_err(|_| format!("Invalid run_number for run {reuse}"))
}

fn cmd_plan(repo: &Path, args: &[String]) -> Result<(), String> {
    let mode = args.first().map(String::as_str).unwrap_or("");
    if !matches!(
        mode,
        "installers" | "quality-signals" | "codeql" | "android"
    ) {
        return Err(
            "Usage: cargo xtask ci plan <installers|quality-signals|codeql|android>".into(),
        );
    }
    let event = std::env::var("GITHUB_EVENT_NAME").unwrap_or_default();
    let git_ref = std::env::var("GITHUB_REF").unwrap_or_default();
    let base_sha = std::env::var("BASE_SHA").unwrap_or_default();
    let github_repo = std::env::var("GITHUB_REPOSITORY").unwrap_or_default();
    let run_number: u64 = std::env::var("GITHUB_RUN_NUMBER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let version_line = std::env::var("VERSION_LINE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            crate::verify::read_manifest_version(repo)
                .ok()
                .and_then(|v| {
                    let core = v.split('-').next().unwrap_or(&v);
                    let mut parts = core.split('.');
                    let maj = parts.next()?;
                    let min = parts.next()?;
                    Some(format!("{maj}.{min}"))
                })
                .unwrap_or_else(|| "3.1".into())
        });
    let version_base_run: u64 = std::env::var("VERSION_BASE_RUN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(73);
    let reuse_android = std::env::var("REUSE_ANDROID_RUN").unwrap_or_default();

    let use_diff =
        matches!(event.as_str(), "push" | "pull_request") && !git_ref.starts_with("refs/tags/");
    let paths = if use_diff {
        changed_files_from_git(repo, &base_sha, &github_repo)
    } else {
        None
    };

    let mut outputs = BTreeMap::new();
    match mode {
        "installers" => {
            let build_num =
                resolve_build_number(run_number, &git_ref, &github_repo, &reuse_android)?;
            outputs.extend(resolve_version(
                &git_ref,
                build_num,
                &version_line,
                version_base_run,
            )?);
            outputs.extend(classify_installers(
                paths.as_ref(),
                !reuse_android.trim().is_empty(),
            ));
        }
        "quality-signals" => outputs.extend(classify_quality_signals(paths.as_ref())),
        "codeql" => outputs.extend(classify_codeql(paths.as_ref())),
        "android" => outputs.extend(classify_android(paths.as_ref())),
        _ => unreachable!(),
    }

    let mut lines = String::new();
    for (k, v) in &outputs {
        let line = format!("{k}={v}\n");
        print!("{line}");
        lines.push_str(&line);
    }
    if let Ok(gh_out) = std::env::var("GITHUB_OUTPUT")
        && !gh_out.is_empty()
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&gh_out)
            .map_err(|e| format!("failed to write GITHUB_OUTPUT: {e}"))?;
        f.write_all(lines.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---- 2. Download-Retry Gradle Runner ----

pub fn is_retryable_download_failure(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    if lower.contains("compilation error")
        || lower.contains("script compilation errors")
        || lower.contains("there were failing tests")
        || lower.contains("verification failed")
        || (lower.contains("checksum") && (lower.contains("mismatch") || lower.contains("failed")))
    {
        return false;
    }
    let transport = lower.contains("sockettimeoutexception")
        || lower.contains("connecttimeoutexception")
        || lower.contains("connection reset")
        || lower.contains("read timed out")
        || lower.contains("connect timed out")
        || contains_http_5xx(&lower);
    let download_ctx = lower.contains("could not get")
        || lower.contains("could not head")
        || lower.contains("could not download")
        || lower.contains("could not install gradle")
        || lower.contains("could not resolve")
        || lower.contains("downloading http://")
        || lower.contains("downloading https://")
        || lower.contains("gradle distribution");
    transport && download_ctx
}

fn contains_http_5xx(lower: &str) -> bool {
    for marker in [
        "server returned http response code: 5",
        "received status code 5",
        "http status code: 5",
        "http status 5",
        "http response code: 5",
        "http 5",
    ] {
        let mut from = 0;
        while let Some(rel) = lower[from..].find(marker) {
            let idx = from + rel + marker.len();
            let rest = lower.as_bytes().get(idx..idx + 2);
            if let Some([a, b]) = rest
                && a.is_ascii_digit()
                && b.is_ascii_digit()
            {
                return true;
            }
            from = idx;
        }
    }
    false
}

fn cmd_run_gradle(repo: &Path, args: &[String]) -> i32 {
    let mut attempts = 3usize;
    let mut idx = 0usize;
    while idx < args.len() {
        if args[idx] == "--attempts" && idx + 1 < args.len() {
            attempts = args[idx + 1].parse().unwrap_or(3).max(1);
            idx += 2;
        } else if args[idx] == "--" {
            idx += 1;
            break;
        } else {
            break;
        }
    }
    let gradle_args = &args[idx..];
    if gradle_args.is_empty() {
        eprintln!("Usage: cargo xtask ci run-gradle [--attempts N] -- <gradle args...>");
        return 2;
    }
    let wrapper = repo.join(if cfg!(windows) {
        "gradlew.bat"
    } else {
        "gradlew"
    });
    for attempt in 1..=attempts {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new(&wrapper);
            c.args(gradle_args);
            c
        } else {
            let mut c = Command::new("bash");
            c.arg(&wrapper).args(gradle_args);
            c
        };
        let Ok(mut child) = cmd
            .current_dir(repo)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        else {
            eprintln!("Failed to spawn Gradle wrapper at {}", wrapper.display());
            return 1;
        };
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            eprintln!("Failed to capture Gradle stdout/stderr pipes");
            return 1;
        };
        let err_handle = std::thread::spawn(move || {
            let mut tail = VecDeque::new();
            let mut len = 0usize;
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("{line}");
                push_tail(&mut tail, &mut len, line);
            }
            tail
        });
        let mut tail = VecDeque::new();
        let mut len = 0usize;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            println!("{line}");
            push_tail(&mut tail, &mut len, line);
        }
        let code = child.wait().map_or(1, |s| s.code().unwrap_or(1));
        if let Ok(err_tail) = err_handle.join() {
            for l in err_tail {
                push_tail(&mut tail, &mut len, l);
            }
        }
        let combined: String = tail.into_iter().collect::<Vec<_>>().join("\n");
        if code == 0 || !is_retryable_download_failure(&combined) || attempt == attempts {
            return code;
        }
        let delay = (10 * attempt as u64).min(30);
        println!(
            "Transient artifact download failed; retry {}/{attempts} in {delay}s. Cached tasks are reused.",
            attempt + 1
        );
        std::thread::sleep(Duration::from_secs(delay));
    }
    1
}

fn push_tail(tail: &mut VecDeque<String>, len: &mut usize, line: String) {
    const MAX_TAIL: usize = 2_000_000;
    *len += line.len() + 1;
    tail.push_back(line);
    while *len > MAX_TAIL {
        if let Some(front) = tail.pop_front() {
            *len = len.saturating_sub(front.len() + 1);
        }
    }
}

// ---- 3. Deterministic Kotlin Test Sharding ----

pub fn discover_test_classes(repo: &Path) -> Result<Vec<(usize, String)>, String> {
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    for rel in [
        "mobile/compose-app/src/commonTest/kotlin",
        "mobile/compose-app/src/desktopTest/kotlin",
    ] {
        let root = repo.join(rel);
        if !root.is_dir() {
            continue;
        }
        let mut kt_files = Vec::new();
        collect_kt_files(&root, &mut kt_files);
        kt_files.sort();
        for path in kt_files {
            let Ok(raw) = std::fs::read(&path) else {
                continue;
            };
            let code = strip_kotlin_comments(&String::from_utf8_lossy(&raw));
            if !code.contains("@Test") {
                continue;
            }
            let pkg = code
                .lines()
                .find_map(|l| {
                    l.strip_prefix("package ")
                        .map(|r| r.split_whitespace().next().unwrap_or(""))
                })
                .unwrap_or("");
            let classes: Vec<&str> = code.lines().filter_map(extract_top_level_class).collect();
            if classes.is_empty() {
                continue;
            }
            for cls in &classes {
                let fqcn = if pkg.is_empty() {
                    (*cls).to_string()
                } else {
                    format!("{pkg}.{cls}")
                };
                let weight = (estimate_test_weight(raw.len(), &code, cls) / classes.len()).max(1);
                *by_name.entry(fqcn).or_insert(0) += weight;
            }
        }
    }
    if by_name.is_empty() {
        return Err(format!(
            "No desktop test classes discovered under {}",
            repo.display()
        ));
    }
    let mut items: Vec<(usize, String)> = by_name.into_iter().map(|(k, v)| (v, k)).collect();
    items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(items)
}

fn collect_kt_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_kt_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("kt") {
            out.push(p);
        }
    }
}

fn extract_top_level_class(line: &str) -> Option<&str> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let mut s = line;
    loop {
        if let Some(rest) = s.strip_prefix('@') {
            let mut i = 0;
            let b = rest.as_bytes();
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                i += 1;
            }
            if i < b.len() && b[i] == b'(' {
                let mut d = 1usize;
                i += 1;
                while i < b.len() && d > 0 {
                    if b[i] == b'(' {
                        d += 1;
                    } else if b[i] == b')' {
                        d -= 1;
                    }
                    i += 1;
                }
            }
            s = rest[i..].trim_start();
            continue;
        }
        if let Some(rest) = s
            .strip_prefix("internal ")
            .or_else(|| s.strip_prefix("public "))
            .or_else(|| s.strip_prefix("final "))
        {
            s = rest.trim_start();
            continue;
        }
        let rest = s.strip_prefix("class ")?;
        let end = rest
            .bytes()
            .position(|c| !(c.is_ascii_alphanumeric() || c == b'_'))
            .unwrap_or(rest.len());
        return (end > 0).then_some(&rest[..end]);
    }
}

fn estimate_test_weight(raw_len: usize, code: &str, cls: &str) -> usize {
    if cls == "UiGalleryTest" {
        return (raw_len / 4).max(1);
    }
    let http_bonus = if cls.ends_with("HttpTest") || cls.ends_with("EndToEndTest") {
        60_000
    } else {
        0
    };
    let probe_bonus = if cls.ends_with("ProbeTest") || cls.ends_with("BaselineTest") {
        25_000
    } else {
        0
    };
    let count = |needle: &str| code.matches(needle).count();
    let http_hits = count("HttpServer.create")
        + count("ServerSocket")
        + count("EmbeddedServer")
        + count("SunHttpServer");
    let scene_hits = count("ImageComposeScene");
    let delay_hits = count("delay(") + count("Thread.sleep(");
    let disk_hits =
        count("createTempDirectory") + count("AtomicFileStorage") + count("SnapshotFileStorage");
    let test_hits = count("@Test");
    raw_len
        + http_bonus
        + probe_bonus
        + http_hits * 22_000
        + scene_hits * 14_000
        + delay_hits * 3_500
        + disk_hits * 4_000
        + test_hits * 1_200
}

pub fn assign_shards(
    weighted: &[(usize, String)],
    total: usize,
    lead_overhead: usize,
) -> Result<Vec<Vec<String>>, String> {
    if total < 1 {
        return Err("total shards must be >= 1".into());
    }
    if weighted.len() < total {
        return Err(format!(
            "cannot split {} classes into {total} shards",
            weighted.len()
        ));
    }
    let mut shards = vec![Vec::new(); total];
    let mut loads = vec![0usize; total];
    loads[0] = lead_overhead;
    for (w, fqcn) in weighted {
        let Some(target) = (0..total).min_by_key(|&i| (loads[i], i)) else {
            return Err("shard count must be at least 1".into());
        };
        shards[target].push(fqcn.clone());
        loads[target] += *w;
    }
    for s in &mut shards {
        s.sort();
    }
    Ok(shards)
}

fn cmd_shard_tests(repo: &Path, args: &[String]) -> Result<(), String> {
    let mut windows_native = false;
    let mut index: Option<usize> = None;
    let mut total = 2usize;
    let mut lead_overhead = 0usize;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--windows-native" => {
                windows_native = true;
                i += 1;
            }
            "--index" if i + 1 < args.len() => {
                index = Some(args[i + 1].parse().map_err(|_| "invalid --index")?);
                i += 2;
            }
            "--total" if i + 1 < args.len() => {
                total = args[i + 1].parse().map_err(|_| "invalid --total")?;
                i += 2;
            }
            "--lead-overhead" if i + 1 < args.len() => {
                lead_overhead = args[i + 1].parse().map_err(|_| "invalid --lead-overhead")?;
                i += 2;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let selected: Vec<String> = if windows_native {
        WINDOWS_NATIVE_TEST_FILTERS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    } else {
        let idx = index.ok_or("pass --windows-native or --index <I>")?;
        let shards = assign_shards(&discover_test_classes(repo)?, total, lead_overhead)?;
        shards
            .get(idx)
            .cloned()
            .ok_or_else(|| format!("--index {idx} out of range 0..{total}"))?
    };
    let mut parts = Vec::with_capacity(selected.len() * 2);
    for pat in selected {
        parts.push("--tests".to_string());
        parts.push(pat);
    }
    println!("{}", parts.join(" "));
    Ok(())
}

// ---- 4. CycloneDX SBOM Validator ----

pub fn check_sbom_json(raw: &str) -> (usize, Vec<String>) {
    // Count `"type": "library"` components and check required group names in CycloneDX JSON.
    let mut lib_count = 0usize;
    let mut pos = 0usize;
    while let Some(rel) = raw[pos..].find("\"type\"") {
        let after = raw[pos + rel + 6..].trim_start();
        if let Some(val) = after.strip_prefix(':')
            && val.trim_start().starts_with("\"library\"")
        {
            lib_count += 1;
        }
        pos += rel + 6;
    }
    let mut issues = Vec::new();
    if lib_count < 50 {
        issues.push(format!(
            "only {lib_count} libraries listed, expected at least 50"
        ));
    }
    for group in [
        "io.ktor",
        "org.jetbrains.compose.runtime",
        "org.jetbrains.kotlinx",
    ] {
        let quoted = format!("\"{group}\"");
        if !raw.contains(&quoted) {
            issues.push(format!("no {group} library listed"));
        }
    }
    (lib_count, issues)
}

fn cmd_check_sbom(args: &[String]) -> Result<(), String> {
    let path = args
        .first()
        .ok_or("Usage: cargo xtask ci check-sbom <bom.json>")?;
    let raw = std::fs::read_to_string(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let (count, issues) = check_sbom_json(&raw);
    println!("{path}: {count} libraries");
    if issues.is_empty() {
        Ok(())
    } else {
        for issue in &issues {
            println!("::error::SBOM {issue}");
        }
        Err(issues.join("; "))
    }
}

// ---- 5. Private Android Signer Staging (Zero-Dep Base64 + Keytool/Java Verify) ----

pub fn decode_base64_strict(input: &str) -> Result<Vec<u8>, String> {
    if input.is_empty() || input.len() > 2_000_000 {
        return Err("A bounded ANDROID_KEYSTORE_BASE64 secret is required".into());
    }
    let mut clean = Vec::with_capacity(input.len());
    for b in input.bytes() {
        if !b.is_ascii_whitespace() {
            clean.push(b);
        }
    }
    if clean.is_empty() || !clean.len().is_multiple_of(4) {
        return Err("Invalid base64 keystore length".into());
    }
    let val = |c: u8| -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("Invalid base64 character in keystore secret".into()),
        }
    };
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    let (chunks, _) = clean.as_chunks::<4>();
    let last_idx = chunks.len().saturating_sub(1);
    for (idx, chunk) in chunks.iter().enumerate() {
        let a = val(chunk[0])?;
        let b = val(chunk[1])?;
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            let c = val(chunk[2])?;
            out.push((b << 4) | (c >> 2));
            if chunk[3] != b'=' {
                let d = val(chunk[3])?;
                out.push((c << 6) | d);
            } else if idx != last_idx {
                return Err("Unexpected base64 padding before end".into());
            }
        } else if chunk[3] != b'=' || idx != last_idx {
            return Err("Invalid base64 padding".into());
        }
    }
    if out.is_empty() {
        return Err("Keystore secret is empty".into());
    }
    Ok(out)
}

pub fn extract_cert_sha256(keytool_output: &str) -> Result<String, String> {
    let idx = keytool_output
        .find("SHA256:")
        .ok_or("Signer certificate could not be verified")?;
    let rest = keytool_output[idx + 7..].trim_start();
    let hex: String = rest
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == ':')
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if hex.len() != 64 {
        return Err("Signer certificate could not be verified".into());
    }
    if hex == EXPOSED_CERT_SHA256 {
        return Err("The exposed repository signing identity cannot sign a release".into());
    }
    Ok(hex)
}

fn cmd_stage_signing(repo: &Path) -> Result<(), String> {
    for var in ["ANDROID_KEYSTORE_PASSWORD", "ANDROID_KEY_ALIAS"] {
        if std::env::var(var).unwrap_or_default().is_empty() {
            return Err(format!(
                "Configure the {var} repository secret before a release"
            ));
        }
    }
    let data = decode_base64_strict(&std::env::var("ANDROID_KEYSTORE_BASE64").unwrap_or_default())?;
    let runner_temp =
        PathBuf::from(std::env::var("RUNNER_TEMP").map_err(|_| "RUNNER_TEMP not set")?);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let folder = runner_temp.join(format!("parchment-signing-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let target = folder.join("release.p12");

    let cleanup_guard = std::mem::DropGuard::new((target.clone(), folder), |(file, dir)| {
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_dir(dir);
    });
    std::fs::write(&target, &data).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600));
    }
    let alias = std::env::var("ANDROID_KEY_ALIAS").unwrap_or_default();
    let kt = Command::new("keytool")
        .args([
            "-J-Duser.language=en",
            "-J-Duser.country=US",
            "-list",
            "-v",
            "-keystore",
        ])
        .arg(&target)
        .args([
            "-storepass:env",
            "ANDROID_KEYSTORE_PASSWORD",
            "-alias",
            &alias,
        ])
        .output()
        .map_err(|e| format!("failed to run keytool: {e}"))?;
    let kt_out = String::from_utf8_lossy_owned(kt.stdout);
    if !kt.status.success() || !kt_out.contains("Entry type: PrivateKeyEntry") {
        return Err("Private release signer validation failed".into());
    }
    let fingerprint = extract_cert_sha256(&kt_out)?;

    let mut java_cmd = Command::new("java");
    java_cmd.arg(repo.join(".github/scripts/ValidateAndroidSigningKey.java"));
    java_cmd.env("ANDROID_KEYSTORE_PATH", &target);
    let key_pw = std::env::var("ANDROID_KEY_PASSWORD").unwrap_or_default();
    if key_pw.trim().is_empty() {
        java_cmd.env(
            "ANDROID_KEY_PASSWORD",
            std::env::var("ANDROID_KEYSTORE_PASSWORD").unwrap_or_default(),
        );
    }
    let jv = java_cmd
        .output()
        .map_err(|e| format!("failed to run java key check: {e}"))?;
    if !jv.status.success() {
        return Err(
            "Private release key could not be decrypted; check the key password and alias".into(),
        );
    }
    let gh_env = std::env::var("GITHUB_ENV").map_err(|_| "GITHUB_ENV not set")?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&gh_env)
        .map_err(|e| e.to_string())?;
    writeln!(f, "ANDROID_KEYSTORE_PATH={}", target.display()).map_err(|e| e.to_string())?;
    std::mem::DropGuard::dismiss(cleanup_guard);
    println!("Private release signer verified: SHA-256 {fingerprint}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_branch_and_tag_versions() {
        let tag = resolve_version("refs/tags/v3.0.0", 90, "3.0", 73).unwrap();
        assert_eq!(tag["app_version"], "3.0.0");
        assert_eq!(tag["release_tag"], "v3.0.0");
        assert_eq!(tag["app_version_code"], "290");

        let branch = resolve_version("refs/heads/main", 75, "3.0", 73).unwrap();
        assert_eq!(branch["app_version"], "3.0.2");
    }

    #[test]
    fn classifies_rust_and_kotlin_paths_precisely() {
        let docs_only: HashSet<String> = ["docs/performance.md".into(), "README.md".into()].into();
        let plan_docs = classify_installers(Some(&docs_only), false);
        assert_eq!(plan_docs["rust_core"], "false");
        assert_eq!(plan_docs["rust_windows"], "false");
        assert_eq!(plan_docs["android"], "false");
        assert_eq!(plan_docs["desktop_tests"], "false");
        assert_eq!(plan_docs["windows_tests"], "false");

        let rust_desktop: HashSet<String> =
            ["crates/parchment-desktop/src/app/mod.rs".into()].into();
        let plan = classify_installers(Some(&rust_desktop), false);
        assert_eq!(plan["rust_windows"], "true");
        assert_eq!(plan["rust_core"], "false");
        assert_eq!(plan["android"], "false");
        assert_eq!(plan["desktop_tests"], "false");

        let rust_protocol: HashSet<String> = ["crates/parchment-protocol/src/sse.rs".into()].into();
        let plan_proto = classify_installers(Some(&rust_protocol), false);
        assert_eq!(plan_proto["rust_core"], "true");
        assert_eq!(plan_proto["rust_windows"], "true");
        assert_eq!(plan_proto["android"], "false");
        assert_eq!(plan_proto["desktop_tests"], "false");

        let rust_core: HashSet<String> =
            ["crates/parchment-storage/src/store/mod.rs".into()].into();
        let plan2 = classify_installers(Some(&rust_core), false);
        assert_eq!(plan2["rust_core"], "true");
        assert_eq!(plan2["rust_windows"], "true");
        assert_eq!(plan2["android"], "true");
        assert_eq!(plan2["desktop_tests"], "false");

        let xtask_only: HashSet<String> = ["crates/parchment-xtask/src/perf.rs".into()].into();
        let plan_xtask = classify_installers(Some(&xtask_only), false);
        assert_eq!(plan_xtask["rust_core"], "true");
        assert_eq!(plan_xtask["rust_windows"], "false");
        assert_eq!(plan_xtask["android"], "false");

        let clippy_cfg: HashSet<String> = ["clippy.toml".into()].into();
        let plan_clippy = classify_installers(Some(&clippy_cfg), false);
        assert_eq!(plan_clippy["rust_core"], "true");
        assert_eq!(plan_clippy["rust_windows"], "true");
        assert_eq!(plan_clippy["android"], "false");

        let stability_cfg: HashSet<String> =
            ["mobile/compose-app/stability-config.conf".into()].into();
        let plan_stab = classify_installers(Some(&stability_cfg), false);
        assert_eq!(plan_stab["desktop_tests"], "true");
        assert_eq!(plan_stab["android"], "true");
        assert_eq!(plan_stab["rust_core"], "false");
        let q_stab = classify_quality_signals(Some(&stability_cfg));
        assert_eq!(q_stab["coverage"], "true");
        assert_eq!(q_stab["gallery"], "true");

        let strings_only: HashSet<String> = [
            "mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/strings/Strings.kt".into(),
        ]
        .into();
        let q_strings = classify_quality_signals(Some(&strings_only));
        assert_eq!(q_strings["gallery"], "true");
    }

    #[test]
    fn balances_shards_with_lead_overhead() {
        let items = vec![
            (100_000, "a.HeavyA".into()),
            (90_000, "a.HeavyB".into()),
            (80_000, "a.HeavyC".into()),
            (70_000, "a.HeavyD".into()),
        ];
        let shards = assign_shards(&items, 2, 80_000).unwrap();
        // With 80k overhead on shard 0 (ktlintCheck), shard 1 takes HeavyA first so total loads stay even.
        assert!(shards[1].contains(&"a.HeavyA".to_string()));
        assert_eq!(shards[0].len() + shards[1].len(), 4);
    }

    #[test]
    fn retries_only_transient_download_failures() {
        assert!(is_retryable_download_failure(
            "Could not GET https://repo/x.jar: Received status code 503"
        ));
        assert!(!is_retryable_download_failure(
            "Compilation error: unresolved reference; HTTP 503"
        ));
        assert!(!is_retryable_download_failure(
            "Checksum mismatch downloading https://x"
        ));
    }

    #[test]
    fn base64_and_cert_fingerprint_validation() {
        assert_eq!(decode_base64_strict("aGVsbG8=").unwrap(), b"hello");
        assert!(decode_base64_strict("!!!").is_err());
        let bad = format!("SHA256: {EXPOSED_CERT_SHA256}");
        assert!(extract_cert_sha256(&bad).is_err());
        let good = "SHA256: 95:D9:80:4C:FD:9C:80:31:34:40:ED:97:61:91:00:C9:E9:F7:41:C2:79:43:F6:17:00:FA:80:2D:0D:BE:45:74";
        assert_eq!(
            extract_cert_sha256(good).unwrap(),
            "95d9804cfd9c80313440ed97619100c9e9f741c27943f61700fa802d0dbe4574"
        );
    }

    #[test]
    fn checks_sbom_component_counts_and_groups() {
        let mut fake = String::from(r#"{"components":["#);
        for i in 0..55 {
            if i > 0 {
                fake.push(',');
            }
            fake.push_str(r#"{"type":"library","group":"org.example"}"#);
        }
        fake.push_str(r#",{"type":"library","group":"io.ktor"},{"type":"library","group":"org.jetbrains.compose.runtime"},{"type":"library","group":"org.jetbrains.kotlinx"}]}"#);
        let (cnt, issues) = check_sbom_json(&fake);
        assert_eq!(cnt, 58);
        assert!(issues.is_empty());
    }
}
