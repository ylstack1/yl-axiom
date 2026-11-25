#![allow(clippy::print_stdout)]
//! YL Axiom unified developer & CI automation (`cargo xtask <command>`).
//!
//! Pure Rust `std` only — covers both Rust Desktop (GPUI + core crates) and Kotlin Mobile
//! (Compose Android + slim 16 KB-aligned `arm64-v8a` JNI `.so`).

mod ci;
mod perf;
mod quality;
mod verify;
mod version_sync;

use std::path::PathBuf;
use std::process::ExitCode;

fn repo_root() -> PathBuf {
    if let Ok(ws) = std::env::var("GITHUB_WORKSPACE") {
        let p = PathBuf::from(ws);
        if p.join("Cargo.toml").is_file() && p.join("crates/parchment-xtask/Cargo.toml").is_file() {
            return p;
        }
    }
    if let Ok(cwd) = std::env::current_dir()
        && cwd.join("Cargo.toml").is_file()
        && cwd.join("crates/parchment-xtask/Cargo.toml").is_file()
    {
        return cwd;
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn print_usage() {
    eprintln!(
        "YL Axiom xtask (Rust Desktop + Kotlin Mobile)

Usage: cargo xtask <command> [options]

Commands:
  status                  Git status, verified baseline, pending checks, and TODO status
  review                  Static quality gates + git diff --check + rustfmt --check
  affected [--dry-run]    Review + affected Rust and/or Kotlin checks (ledger-backed)
  rust [--dry-run]        rustfmt --check + clippy -D warnings + cargo test (full & slim JNI)
  android-so [--dry-run]  Build slim arm64-v8a libparchment_core.so + verify 16 KB ELF align
  ktlint [--dry-run]      Run Kotlin formatting/lint check (ktlintCheck)
  fmt [--dry-run]         Auto-format both Rust (rustfmt) and Kotlin (ktlintFormat)
  test <filters...>       Run targeted Kotlin tests via Gradle (:composeApp:desktopTest)
  android [--dry-run]     Compile Android Kotlin sources (:composeApp:compileAndroidMain)
  final [--dry-run]       Full release gate (review, rust, android-so, desktop .exe, signed APK)
  perf [--android-install] Measure desktop .exe startup/RAM/streaming + Android APK/phone
  disk                    Report workspace build/cache disk usage against storage budgets
  prune [--dry-run]       Safely reclaim stale incremental/deps/log bloat without cold rebuilds
  pre-commit              Fast git pre-commit hook (review + rustfmt + staged clippy/ktlint)
  install-hooks           Configure git core.hooksPath = .githooks
  ci <subcommand>         CI helpers (plan, run-gradle, shard-tests, check-sbom, stage-signing)"
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        print_usage();
        return ExitCode::from(2);
    };
    let repo = repo_root();
    let dry_run = rest.iter().any(|a| a == "--dry-run");
    let code = match cmd.as_str() {
        "status" => verify::cmd_status(&repo),
        "review" => verify::cmd_review(&repo),
        "affected" | "check" => verify::cmd_affected(&repo, dry_run),
        "rust" => verify::cmd_rust(&repo, dry_run),
        "android-so" => verify::cmd_android_so(&repo, dry_run),
        "ktlint" => {
            verify::cmd_gradle_sub(&repo, "ktlint", &["ktlintCheck".into()], dry_run, false)
        }
        "fmt" => {
            if dry_run {
                println!("Format: cargo fmt --all && Gradle ktlintFormat");
                0
            } else {
                let rust_ok = std::process::Command::new("cargo")
                    .args(["fmt", "--all"])
                    .current_dir(&repo)
                    .status()
                    .is_ok_and(|s| s.success());
                if !rust_ok {
                    1
                } else {
                    verify::cmd_gradle_sub(
                        &repo,
                        "ktlint-format",
                        &["ktlintFormat".into()],
                        false,
                        false,
                    )
                }
            }
        }
        "android" => verify::cmd_gradle_sub(
            &repo,
            "android",
            &[":composeApp:compileAndroidMain".into()],
            dry_run,
            false,
        ),
        "test" => {
            let filters: Vec<String> = rest.iter().filter(|a| *a != "--dry-run").cloned().collect();
            if filters.is_empty() {
                eprintln!("Usage: cargo xtask test <filter...>");
                2
            } else {
                let gallery = filters.iter().any(|f| f.contains("Gallery"));
                let mut tasks = vec![":composeApp:desktopTest".to_string()];
                for f in filters {
                    tasks.push("--tests".into());
                    tasks.push(f);
                }
                verify::cmd_gradle_sub(&repo, "test", &tasks, dry_run, gallery)
            }
        }
        "final" => verify::cmd_final(&repo, dry_run),
        "perf" => {
            let android_install = rest.iter().any(|a| a == "--android-install");
            match perf::run_perf(&repo, android_install) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("FAIL perf: {e}");
                    1
                }
            }
        }
        "disk" => verify::cmd_disk(&repo),
        "prune" => verify::cmd_prune(&repo, dry_run),
        "pre-commit" => verify::cmd_pre_commit(&repo),
        "install-hooks" => verify::cmd_install_hooks(&repo),
        "ci" => ci::dispatch(&repo, rest),
        "-h" | "--help" | "help" => {
            print_usage();
            0
        }
        other => {
            eprintln!("Unknown xtask command: {other}");
            print_usage();
            2
        }
    };
    ExitCode::from(code as u8)
}
