#!/usr/bin/env python3
"""Path-based CI change classification and single-source release version resolution.

Standard library only. Keeps git fetches shallow (--depth=1) and falls back conservatively
to running all targets whenever history is unavailable (tags, workflow_dispatch, force-push).
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

REPO = Path(__file__).resolve().parents[2]
ZERO_SHA = "0" * 40
SEMVER = re.compile(r"^\d+\.\d+\.\d+$")

SHARED_BUILD_PREFIXES = (
    "gradle/",
)
SHARED_BUILD_FILES = {
    "build.gradle.kts",
    "settings.gradle.kts",
    "gradle.properties",
    "gradlew",
    "gradlew.bat",
    "composeApp/build.gradle.kts",
    ".github/workflows/build-installers.yml",
    ".github/scripts/run_gradle.py",
    ".github/scripts/ci_changes.py",
}
SHARED_APP_PREFIXES = (
    "composeApp/src/commonMain/",
    "composeApp/src/jvmShared/",
)
ANDROID_ONLY_PREFIXES = (
    "androidApp/",
    "composeApp/src/androidMain/",
)
ANDROID_SCRIPT_FILES = {
    ".github/scripts/stage_android_signing.py",
    ".github/scripts/ValidateAndroidSigningKey.java",
    ".github/scripts/check_sbom.py",
    ".github/scripts/sbom.init.gradle.kts",
}
SIGNING_TEST_TRIGGERS = {
    ".github/scripts/stage_android_signing.py",
    ".github/scripts/test_stage_android_signing.py",
    ".github/scripts/ValidateAndroidSigningKey.java",
    ".github/workflows/build-installers.yml",
}
DESKTOP_ONLY_PREFIXES = (
    "composeApp/src/desktopMain/",
    "installer/",
)
DESKTOP_TEST_PREFIXES = (
    "composeApp/src/commonTest/",
    "composeApp/src/desktopTest/",
)
WINDOWS_TEST_PREFIXES = (
    "composeApp/src/desktopTest/kotlin/io/parchment/ai/platform/",
    "composeApp/src/desktopTest/kotlin/io/parchment/ai/harness/OwnedShell",
    "composeApp/src/commonTest/kotlin/io/parchment/ai/harness/OwnedShell",
)
WINDOWS_TEST_FILES = {
    "composeApp/src/desktopTest/kotlin/io/parchment/ai/CdsWarmupWatchdogTest.kt",
    "composeApp/src/desktopTest/kotlin/io/parchment/ai/WindowCaptionTest.kt",
}
GALLERY_UI_PREFIXES = (
    "composeApp/src/commonMain/kotlin/io/parchment/ai/ui/",
    "composeApp/src/commonMain/kotlin/io/parchment/ai/model/",
    "composeApp/src/commonMain/resources/",
    "composeApp/src/jvmShared/",
    "composeApp/src/desktopMain/",
)
GALLERY_UI_FILES = {
    "composeApp/src/desktopTest/kotlin/io/parchment/ai/ui/UiGalleryTest.kt",
    ".github/workflows/quality-signals.yml",
}
SBOM_FILES = {
    "build.gradle.kts",
    "settings.gradle.kts",
    "gradle.properties",
    "gradlew",
    "gradlew.bat",
    "composeApp/build.gradle.kts",
    "androidApp/build.gradle.kts",
    ".github/scripts/check_sbom.py",
    ".github/scripts/test_check_sbom.py",
    ".github/scripts/sbom.init.gradle.kts",
    ".github/workflows/quality-signals.yml",
}
ALL_CODEQL_LANGUAGES = (
    {"language": "java-kotlin", "build-mode": "manual"},
    {"language": "python", "build-mode": "none"},
    {"language": "actions", "build-mode": "none"},
)


def _matches(path: str, prefixes: tuple[str, ...], exact: set[str] | frozenset[str] = frozenset()) -> bool:
    return path in exact or path.startswith(prefixes)


def resolve_version(
    *,
    ref: str,
    build_number: int,
    version_line: str = "2.1",
    version_base_run: int = 73,
) -> dict[str, str]:
    if build_number < 1:
        raise ValueError(f"Invalid build number: {build_number}")
    if ref.startswith("refs/tags/v"):
        app_version = ref.removeprefix("refs/tags/v")
        release_tag = ref.removeprefix("refs/tags/")
    else:
        if build_number >= version_base_run:
            app_version = f"{version_line}.{build_number - version_base_run}"
        else:
            app_version = f"2.0.{build_number}"
        release_tag = f"v{app_version}"
    if not SEMVER.match(app_version):
        raise ValueError(f"Invalid release version: {app_version}")
    return {
        "build_number": str(build_number),
        "app_version": app_version,
        "app_version_code": str(200 + build_number),
        "release_tag": release_tag,
    }


def resolve_build_number(
    *,
    run_number: int,
    ref: str,
    repo: str,
    reuse_android_run: str = "",
    fetch_run=None,
) -> int:
    reuse = reuse_android_run.strip()
    if not reuse:
        return run_number
    if not reuse.isdigit():
        raise ValueError("reuse_android_run must be a run ID")
    if ref.startswith("refs/tags/"):
        raise ValueError("Reusing an APK is for branch runs only")
    if fetch_run is None:
        result = subprocess.run(
            ["gh", "api", f"repos/{repo}/actions/runs/{reuse}"],
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        if result.returncode != 0:
            raise ValueError(f"Could not read run {reuse}")
        payload = json.loads(result.stdout)
    else:
        payload = fetch_run(repo, reuse)
    if payload.get("path") != ".github/workflows/build-installers.yml":
        raise ValueError(f"Run {reuse} is not an installer build")
    return int(payload["run_number"])


def changed_files_from_git(repo: Path, base_sha: str, github_repo: str = "") -> set[str] | None:
    """Fetches a single shallow base commit if needed and returns changed paths, or None on fallback."""
    base = base_sha.strip()
    if not base or base == ZERO_SHA or not re.fullmatch(r"[0-9a-fA-F]{40}", base):
        return None
    has_commit = subprocess.run(
        ["git", "cat-file", "-e", f"{base}^{{commit}}"],
        cwd=repo,
        capture_output=True,
    ).returncode == 0
    if not has_commit:
        has_commit = subprocess.run(
            ["git", "fetch", "--no-tags", "--depth=1", "origin", base],
            cwd=repo,
            capture_output=True,
        ).returncode == 0
    if has_commit:
        diff = subprocess.run(
            ["git", "diff", "--name-only", "-z", base, "HEAD"],
            cwd=repo,
            capture_output=True,
        )
        if diff.returncode == 0:
            return {entry for entry in diff.stdout.decode("utf-8", errors="replace").split("\0") if entry}
    if github_repo and os.environ.get("GH_TOKEN"):
        api = subprocess.run(
            ["gh", "api", f"repos/{github_repo}/compare/{base}...HEAD", "--jq", ".files[].filename"],
            cwd=repo,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        if api.returncode == 0:
            return {line.strip() for line in api.stdout.splitlines() if line.strip()}
    return None


def classify_installers(paths: set[str] | None, *, reuse_android_run: bool = False) -> dict[str, str]:
    if paths is None:
        return {
            "desktop_tests": "true",
            "android": "false" if reuse_android_run else "true",
            "windows": "true",
            "signing_scripts": "false" if reuse_android_run else "true",
        }
    shared = any(_matches(p, SHARED_BUILD_PREFIXES + SHARED_APP_PREFIXES, SHARED_BUILD_FILES) for p in paths)
    desktop_tests = shared or any(
        _matches(p, DESKTOP_ONLY_PREFIXES + DESKTOP_TEST_PREFIXES, {".github/scripts/shard_tests.py"})
        and not p.startswith("installer/")
        for p in paths
    )
    android = (not reuse_android_run) and (
        shared or any(_matches(p, ANDROID_ONLY_PREFIXES, ANDROID_SCRIPT_FILES) for p in paths)
    )
    windows = shared or any(
        _matches(
            p,
            DESKTOP_ONLY_PREFIXES + WINDOWS_TEST_PREFIXES,
            WINDOWS_TEST_FILES | {"composeApp/compose-desktop.pro"},
        )
        for p in paths
    )
    signing_scripts = (not reuse_android_run) and any(p in SIGNING_TEST_TRIGGERS for p in paths)
    return {
        "desktop_tests": "true" if desktop_tests else "false",
        "android": "true" if android else "false",
        "windows": "true" if windows else "false",
        "signing_scripts": "true" if signing_scripts else "false",
    }


def classify_quality_signals(paths: set[str] | None) -> dict[str, str]:
    if paths is None:
        return {"coverage": "true", "gallery": "true", "sbom": "true"}
    shared_gradle = any(
        _matches(
            p,
            SHARED_BUILD_PREFIXES,
            {"build.gradle.kts", "settings.gradle.kts", "gradle.properties", "gradlew", "gradlew.bat", "composeApp/build.gradle.kts"},
        )
        for p in paths
    )
    coverage = shared_gradle or any(
        _matches(
            p,
            SHARED_APP_PREFIXES + ("composeApp/src/desktopMain/",) + DESKTOP_TEST_PREFIXES,
            {".github/scripts/kover.init.gradle.kts", ".github/workflows/quality-signals.yml"},
        )
        for p in paths
    )
    gallery = shared_gradle or any(_matches(p, GALLERY_UI_PREFIXES, GALLERY_UI_FILES) for p in paths)
    sbom = any(_matches(p, SHARED_BUILD_PREFIXES, SBOM_FILES) for p in paths)
    return {
        "coverage": "true" if coverage else "false",
        "gallery": "true" if gallery else "false",
        "sbom": "true" if sbom else "false",
    }


def classify_codeql(paths: set[str] | None) -> dict[str, str]:
    if paths is None or ".github/workflows/codeql.yml" in paths:
        return {"has_targets": "true", "matrix": json.dumps(list(ALL_CODEQL_LANGUAGES), separators=(",", ":"))}
    selected: list[dict[str, str]] = []
    # CodeQL compiles only production desktop and Android source sets (:composeApp:compileKotlinDesktop and
    # :composeApp:compileAndroidMain), so pure unit-test edits under commonTest/desktopTest never enter the DB.
    has_jvm = any(
        not p.startswith(DESKTOP_TEST_PREFIXES)
        and (
            p.endswith((".kt", ".java", ".kts"))
            or p.startswith("gradle/")
            or p in {"gradle.properties", "gradlew", "gradlew.bat"}
            or p.endswith("AndroidManifest.xml")
        )
        for p in paths
    )
    has_python = any(p.endswith(".py") for p in paths)
    has_actions = any(p.startswith(".github/workflows/") or p == ".github/dependabot.yml" for p in paths)
    if has_jvm:
        selected.append({"language": "java-kotlin", "build-mode": "manual"})
    if has_python:
        selected.append({"language": "python", "build-mode": "none"})
    if has_actions:
        selected.append({"language": "actions", "build-mode": "none"})
    if not selected:
        # Keep non-empty placeholder JSON so GitHub Actions matrix parsing stays valid even before `if:` skips the job.
        return {
            "has_targets": "false",
            "matrix": json.dumps([{"language": "actions", "build-mode": "none"}], separators=(",", ":")),
        }
    return {"has_targets": "true", "matrix": json.dumps(selected, separators=(",", ":"))}


def _write_outputs(outputs: dict[str, str], github_output: str | None) -> None:
    lines = [f"{key}={value}" for key, value in outputs.items()]
    for line in lines:
        print(line)
    if github_output:
        with open(github_output, "a", encoding="utf-8") as handle:
            handle.write("\n".join(lines) + "\n")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("installers", "quality-signals", "codeql"))
    parser.add_argument("--event", default=os.environ.get("GITHUB_EVENT_NAME", ""))
    parser.add_argument("--ref", default=os.environ.get("GITHUB_REF", ""))
    parser.add_argument("--base", default=os.environ.get("BASE_SHA", ""))
    parser.add_argument("--repo-dir", type=Path, default=REPO)
    parser.add_argument("--github-repo", default=os.environ.get("GITHUB_REPOSITORY", ""))
    parser.add_argument("--run-number", type=int, default=int(os.environ.get("GITHUB_RUN_NUMBER", "1") or "1"))
    parser.add_argument("--version-line", default=os.environ.get("VERSION_LINE", "2.1"))
    parser.add_argument("--version-base-run", type=int, default=int(os.environ.get("VERSION_BASE_RUN", "73") or "73"))
    parser.add_argument("--reuse-android-run", default=os.environ.get("REUSE_ANDROID_RUN", ""))
    parser.add_argument("--github-output", default=os.environ.get("GITHUB_OUTPUT"))
    options = parser.parse_args(argv)

    use_git_diff = options.event in ("push", "pull_request") and not options.ref.startswith("refs/tags/")
    paths = changed_files_from_git(options.repo_dir, options.base, options.github_repo) if use_git_diff else None

    if options.mode == "installers":
        reuse = bool(options.reuse_android_run.strip())
        build_number = resolve_build_number(
            run_number=options.run_number,
            ref=options.ref,
            repo=options.github_repo,
            reuse_android_run=options.reuse_android_run,
        )
        outputs = {
            **resolve_version(
                ref=options.ref,
                build_number=build_number,
                version_line=options.version_line,
                version_base_run=options.version_base_run,
            ),
            **classify_installers(paths, reuse_android_run=reuse),
        }
    elif options.mode == "quality-signals":
        outputs = classify_quality_signals(paths)
    else:
        outputs = classify_codeql(paths)

    _write_outputs(outputs, options.github_output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
