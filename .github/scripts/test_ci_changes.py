import json
from pathlib import Path
import re
import unittest

from ci_changes import (
    REPO,
    classify_codeql,
    classify_installers,
    classify_quality_signals,
    resolve_build_number,
    resolve_version,
)

PINNED_USES = re.compile(r"^[ \t]*-?[ \t]*uses:\s*([^\s@#]+)@([0-9a-f]{40})\s+#\s*v?\d", re.M)
ANY_USES = re.compile(r"^[ \t]*-?[ \t]*uses:\s*(.+)$", re.M)


class VersionAndReuseResolutionTest(unittest.TestCase):
    def test_branch_and_tag_versions_follow_release_contract(self):
        self.assertEqual(
            {"build_number": "73", "app_version": "2.1.0", "app_version_code": "273", "release_tag": "v2.1.0"},
            resolve_version(ref="refs/heads/main", build_number=73),
        )
        self.assertEqual(
            {"build_number": "65", "app_version": "2.0.65", "app_version_code": "265", "release_tag": "v2.0.65"},
            resolve_version(ref="refs/heads/main", build_number=65),
        )
        self.assertEqual(
            {"build_number": "90", "app_version": "2.4.0", "app_version_code": "290", "release_tag": "v2.4.0"},
            resolve_version(ref="refs/tags/v2.4.0", build_number=90),
        )
        with self.assertRaises(ValueError):
            resolve_version(ref="refs/tags/v2.4-beta", build_number=90)

    def test_reuse_android_run_validates_id_ref_and_workflow(self):
        self.assertEqual(80, resolve_build_number(run_number=80, ref="refs/heads/main", repo="o/r"))
        self.assertEqual(
            74,
            resolve_build_number(
                run_number=80,
                ref="refs/heads/main",
                repo="o/r",
                reuse_android_run="12345",
                fetch_run=lambda repo, rid: {"path": ".github/workflows/build-installers.yml", "run_number": 74},
            ),
        )
        with self.assertRaises(ValueError):
            resolve_build_number(run_number=80, ref="refs/heads/main", repo="o/r", reuse_android_run="abc")
        with self.assertRaises(ValueError):
            resolve_build_number(run_number=80, ref="refs/tags/v2.4.0", repo="o/r", reuse_android_run="123")
        with self.assertRaises(ValueError):
            resolve_build_number(
                run_number=80,
                ref="refs/heads/main",
                repo="o/r",
                reuse_android_run="123",
                fetch_run=lambda repo, rid: {"path": ".github/workflows/codeql.yml", "run_number": 74},
            )


class PathFilteringTest(unittest.TestCase):
    def test_docs_and_release_please_metadata_skip_heavy_builds(self):
        paths = {"README.md", "docs/DESIGN.md", "CHANGELOG.md", ".release-please-manifest.json"}
        self.assertEqual(
            {"desktop_tests": "false", "windows_tests": "false", "android": "false", "windows": "false", "signing_scripts": "false"},
            classify_installers(paths),
        )
        self.assertEqual({"coverage": "false", "gallery": "false", "sbom": "false"}, classify_quality_signals(paths))
        codeql = classify_codeql(paths)
        self.assertEqual("false", codeql["has_targets"])
        self.assertTrue(json.loads(codeql["matrix"]), "placeholder matrix must stay non-empty JSON")

    def test_android_only_changes_skip_desktop_tests_and_windows_installer(self):
        paths = {"composeApp/src/androidMain/kotlin/io/parchment/ai/MainActivity.kt", "androidApp/proguard-rules.pro"}
        self.assertEqual(
            {"desktop_tests": "false", "windows_tests": "false", "android": "true", "windows": "false", "signing_scripts": "false"},
            classify_installers(paths),
        )
        self.assertEqual({"coverage": "false", "gallery": "false", "sbom": "false"}, classify_quality_signals(paths))
        codeql = classify_codeql(paths)
        self.assertEqual("true", codeql["has_targets"])
        self.assertEqual([{"language": "java-kotlin", "build-mode": "manual"}], json.loads(codeql["matrix"]))

    def test_desktop_only_changes_skip_android_build(self):
        paths = {"composeApp/src/desktopMain/kotlin/io/parchment/ai/Main.kt"}
        self.assertEqual(
            {"desktop_tests": "true", "windows_tests": "true", "android": "false", "windows": "true", "signing_scripts": "false"},
            classify_installers(paths),
        )
        self.assertEqual({"coverage": "true", "gallery": "true", "sbom": "false"}, classify_quality_signals(paths))

    def test_pure_non_windows_unit_test_runs_linux_shards_only(self):
        paths = {"composeApp/src/desktopTest/kotlin/io/parchment/ai/network/StreamRecoveryHttpTest.kt"}
        self.assertEqual(
            {"desktop_tests": "true", "windows_tests": "false", "android": "false", "windows": "false", "signing_scripts": "false"},
            classify_installers(paths),
        )
        self.assertEqual({"coverage": "true", "gallery": "false", "sbom": "false"}, classify_quality_signals(paths))
        self.assertEqual("false", classify_codeql(paths)["has_targets"])

    def test_windows_platform_test_runs_windows_tests_without_repackaging_installer(self):
        paths = {"composeApp/src/desktopTest/kotlin/io/parchment/ai/platform/WindowsChromeTest.kt"}
        self.assertEqual(
            {"desktop_tests": "true", "windows_tests": "true", "android": "false", "windows": "false", "signing_scripts": "false"},
            classify_installers(paths),
        )

    def test_installer_script_change_builds_windows_without_android_or_desktop_tests(self):
        paths = {"installer/windows-installer.iss"}
        self.assertEqual(
            {"desktop_tests": "false", "windows_tests": "false", "android": "false", "windows": "true", "signing_scripts": "false"},
            classify_installers(paths),
        )

    def test_fallback_none_runs_everything_and_respects_reuse_android_run(self):
        self.assertEqual(
            {"desktop_tests": "true", "windows_tests": "true", "android": "true", "windows": "true", "signing_scripts": "true"},
            classify_installers(None, reuse_android_run=False),
        )
        self.assertEqual(
            {"desktop_tests": "true", "windows_tests": "true", "android": "false", "windows": "true", "signing_scripts": "false"},
            classify_installers(None, reuse_android_run=True),
        )

    def test_dependency_changes_enable_sbom(self):
        self.assertEqual(
            {"coverage": "true", "gallery": "true", "sbom": "true"},
            classify_quality_signals({"gradle/libs.versions.toml"}),
        )

    def test_codeql_selects_only_affected_languages(self):
        py_only = classify_codeql({"scripts/verify.py"})
        self.assertEqual([{"language": "python", "build-mode": "none"}], json.loads(py_only["matrix"]))
        wf_only = classify_codeql({".github/workflows/stale.yml"})
        self.assertEqual([{"language": "actions", "build-mode": "none"}], json.loads(wf_only["matrix"]))


class WorkflowSecurityAndOptimizationInvariantsTest(unittest.TestCase):
    def check_workflow_bytes(self, name: str, raw: bytes) -> None:
        self.assertNotIn(b"\r\n", raw, f"{name} must use LF line endings")
        text = raw.decode("utf-8")

        self.assertRegex(text, re.compile(r"^concurrency:\s*$", re.M), f"{name} must declare concurrency")
        self.assertRegex(
            text,
            re.compile(r"^permissions:\s*(read-all|\n  contents:\s*read)\b", re.M),
            f"{name} must keep read-only top-level permissions",
        )

        all_uses = ANY_USES.findall(text)
        pinned_uses = PINNED_USES.findall(text)
        self.assertEqual(len(all_uses), len(pinned_uses), f"{name} has unpinned actions: {all_uses}")

        self.assertNotRegex(
            text,
            r'cache:\s*["\']?gradle["\']?',
            f"{name} must use gradle/actions/setup-gradle instead of setup-java cache",
        )
        self.assertNotRegex(
            text,
            r"-m\s+unittest\s+\.github/",
            f"{name} must not pass dot-prefixed .github/ file paths to python -m unittest",
        )
        self.assertNotRegex(
            text,
            r"/android/sdk/build-tools/3[56]",
            f"{name} must not cache preinstalled 1.2 GB Android build-tools 35/36 directories",
        )
        self.assertNotIn(
            "sdkmanager --licenses",
            text,
            f"{name} must write static Android SDK license hashes instead of booting sdkmanager JVM",
        )
        self.assertNotIn(
            "cache-read-only: false",
            text,
            f"{name} must not force cache-read-only: false on pull_request workflows",
        )
        if "cache-encryption-key:" not in text:
            self.assertNotRegex(
                text,
                r"(?<!no-)--configuration-cache\b",
                f"{name} must not serialize --configuration-cache on one-shot --no-daemon CI runs without cache-encryption-key",
            )
        if "shell: pwsh" in text:
            self.assertNotRegex(
                text,
                r"""['"]-Dorg\.gradle\.jvmargs=[^'"]+\s+[^'"]+['"]""",
                f"{name} must not pass space-containing -Dorg.gradle.jvmargs CLI arg through gradlew.bat",
            )
        if name == "build-installers.yml":
            self.assertIn(
                "Using AOT-linked classes: true",
                text,
                "build-installers.yml must verify installed Parchment.exe maps AOT-linked classes across timezones",
            )
        for expr_match in re.finditer(r"\$\{\{(.*?)\}\}", text, re.S):
            expr = expr_match.group(1)
            self.assertNotRegex(
                expr,
                r"\s[+*%]\s",
                f"{name} uses unsupported arithmetic operator inside GitHub Actions expression: {expr.strip()}",
            )

        for match in re.finditer(r"uses:\s*actions/checkout@[0-9a-f]{40}.*?(?=\n[ \t]*-\s|\Z)", text, re.S):
            block = match.group(0)
            self.assertIn("persist-credentials: false", block, f"{name} checkout missing persist-credentials: false")
            self.assertIn("fetch-depth: 1", block, f"{name} checkout missing fetch-depth: 1")
            self.assertIn("fetch-tags: false", block, f"{name} checkout missing fetch-tags: false")

        jobs_section = text.split("\njobs:\n", 1)[1]
        job_headers = list(re.finditer(r"^  ([a-zA-Z0-9_-]+):\s*$", jobs_section, re.M))
        self.assertTrue(job_headers, f"{name} has no jobs")
        for idx, header in enumerate(job_headers):
            start = header.start()
            end = job_headers[idx + 1].start() if idx + 1 < len(job_headers) else len(jobs_section)
            job_block = jobs_section[start:end]
            self.assertRegex(
                job_block,
                re.compile(r"^    timeout-minutes:\s*\d+\s*$", re.M),
                f"{name} job '{header.group(1)}' missing timeout-minutes",
            )

    def test_all_workflows_enforce_pinning_concurrency_timeouts_and_shallow_checkouts(self):
        workflows_dir = REPO / ".github" / "workflows"
        workflows = sorted(workflows_dir.glob("*.yml"))
        self.assertEqual(8, len(workflows))
        for path in workflows:
            self.check_workflow_bytes(path.name, path.read_bytes())

        installers_text = (workflows_dir / "build-installers.yml").read_text(encoding="utf-8")
        automerge_text = (workflows_dir / "dependabot-automerge.yml").read_text(encoding="utf-8")
        self.assertIn("name: Build Android APK & Windows EXE Installer", installers_text)
        self.assertIn('workflows: ["Build Android APK & Windows EXE Installer"]', automerge_text)

    def test_guard_rejects_unpinned_actions_legacy_cache_missing_timeouts_and_crlf(self):
        good = (REPO / ".github" / "workflows" / "scorecard.yml").read_text(encoding="utf-8")
        for broken in (
            good.replace("@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1", "@v7"),
            good.replace("fetch-depth: 1\n", ""),
            good.replace("    timeout-minutes: 15\n", ""),
            good.replace("concurrency:\n", "x:\n"),
            good + '          cache: "gradle"\n',
            good.replace("${{ github.ref }}", "${{ matrix.shard + 1 }}"),
        ):
            with self.assertRaises(AssertionError):
                self.check_workflow_bytes("broken.yml", broken.encode("utf-8"))
        with self.assertRaises(AssertionError):
            self.check_workflow_bytes("crlf.yml", good.replace("\n", "\r\n").encode("utf-8"))


if __name__ == "__main__":
    unittest.main()
