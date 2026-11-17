import pathlib
import re
import tempfile
import unittest

from shard_tests import (
    REPO,
    TEST_ROOTS,
    WINDOWS_NATIVE_FILTERS,
    assign_shards,
    discover_test_classes,
    gradle_test_args,
    matches_filter,
)

WINDOWS_MARKERS = re.compile(
    r'\bos\.name\b|\bWindowsNative\b|\bWindowsChrome\b|\bWindowsFolderDialog\b|\bWindowsMemory\b|'
    r'\bWindowsPowerLease\b|\bkernel32\b|\buser32\b|\bpowershell\.exe\b'
)


class ShardTestsTest(unittest.TestCase):
    def test_live_repository_partition_is_complete_and_disjoint(self):
        weighted = discover_test_classes(REPO)
        all_classes = {fqcn for _, fqcn in weighted}
        self.assertGreaterEqual(len(all_classes), 70)
        for total in (2, 3, 4):
            shards = assign_shards(weighted, total)
            self.assertEqual(total, len(shards))
            self.assertTrue(all(shards), "every shard must be non-empty")
            combined = [fqcn for shard in shards for fqcn in shard]
            self.assertEqual(len(all_classes), len(combined), "shards must be pairwise disjoint")
            self.assertEqual(all_classes, set(combined), "union of shards must equal all discovered test classes")

    def test_sharding_is_deterministic_and_ignores_commented_classes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            common = root / "composeApp/src/commonTest/kotlin/io/parchment/ai"
            desktop = root / "composeApp/src/desktopTest/kotlin/io/parchment/ai/newpkg"
            common.mkdir(parents=True)
            desktop.mkdir(parents=True)
            (common / "AlphaTest.kt").write_text(
                "package io.parchment.ai\nimport kotlin.test.Test\n// class CommentedOutTest\nclass AlphaTest { @Test fun a() {} }\n",
                encoding="utf-8",
            )
            (desktop / "BetaSpec.kt").write_text(
                "package io.parchment.ai.newpkg\nimport kotlin.test.Test\n@OptIn(ExperimentalStdlibApi::class)\ninternal class BetaSpec { @Test fun b() {} }\n",
                encoding="utf-8",
            )
            (desktop / "HelperOnly.kt").write_text(
                "package io.parchment.ai.newpkg\nclass HelperOnly\n",
                encoding="utf-8",
            )
            discovered = discover_test_classes(root)
            self.assertEqual(
                {"io.parchment.ai.AlphaTest", "io.parchment.ai.newpkg.BetaSpec"},
                {fqcn for _, fqcn in discovered},
            )
            shards_first = assign_shards(discovered, 2)
            shards_second = assign_shards(discovered, 2)
            self.assertEqual(shards_first, shards_second)

    def test_every_windows_specific_test_is_covered_by_windows_native_filters(self):
        weighted = discover_test_classes(REPO)
        known = {fqcn for _, fqcn in weighted}
        uncovered: list[str] = []
        for rel_root in TEST_ROOTS:
            for path in sorted((REPO / rel_root).rglob("*.kt")):
                text = path.read_text(encoding="utf-8")
                if "@Test" not in text or not WINDOWS_MARKERS.search(text):
                    continue
                pkg = re.search(r"^package\s+([\w.]+)", text, re.M).group(1)
                for match in re.finditer(r"class\s+([A-Za-z_]\w*)\b", text):
                    fqcn = f"{pkg}.{match.group(1)}"
                    if fqcn in known and not any(matches_filter(fqcn, pat) for pat in WINDOWS_NATIVE_FILTERS):
                        uncovered.append(fqcn)
        self.assertEqual(
            [],
            uncovered,
            f"Windows-specific test suites missing from WINDOWS_NATIVE_FILTERS: {uncovered}",
        )

    def test_gradle_test_args_formats_flags(self):
        self.assertEqual(["--tests", "a.B", "--tests", "c.D"], gradle_test_args(["a.B", "c.D"]))

    def test_heaviest_http_suites_spread_across_two_shards(self):
        shards = assign_shards(discover_test_classes(REPO), 2)
        first, second = set(shards[0]), set(shards[1])
        self.assertNotEqual(
            "io.parchment.ai.network.AgentPermissionHttpTest" in first,
            "io.parchment.ai.network.StreamRecoveryHttpTest" in first,
            "heaviest HTTP suites should land in opposite shards",
        )
        self.assertEqual(0, len(first & second))


if __name__ == "__main__":
    unittest.main()
