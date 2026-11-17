#!/usr/bin/env python3
"""Deterministic desktop test sharding and Windows-native filter selection.

Standard library only. Discovers every @Test class in commonTest and desktopTest so new suites
and packages are automatically assigned to a shard without hardcoded package lists.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

REPO = Path(__file__).resolve().parents[2]
TEST_ROOTS = (
    Path("composeApp/src/commonTest/kotlin"),
    Path("composeApp/src/desktopTest/kotlin"),
)
WINDOWS_NATIVE_FILTERS = (
    "io.parchment.ai.CdsWarmupWatchdogTest",
    "io.parchment.ai.WindowCaptionTest",
    "io.parchment.ai.platform.*",
    "io.parchment.ai.harness.OwnedShell*",
)

_PACKAGE = re.compile(r"^package\s+([\w.]+)", re.M)
_TOP_LEVEL_CLASS = re.compile(
    r"^(?:@[A-Za-z_][\w.]*(?:\([^)\n]*\))?\s+)*(?:internal\s+|public\s+|final\s+)*class\s+([A-Za-z_]\w*)\b",
    re.M,
)
_HTTP_FIXTURE = re.compile(r"\b(?:HttpServer\.create|ServerSocket|EmbeddedServer|SunHttpServer)\b")
_COMPOSE_SCENE = re.compile(r"\bImageComposeScene\b")
_SLEEP_OR_DELAY = re.compile(r"\b(?:delay|Thread\.sleep)\s*\(")
_DISK_CHURN = re.compile(r"\b(?:createTempDirectory|AtomicFileStorage|SnapshotFileStorage)\b")
_TEST_ANNOT = re.compile(r"@Test\b")


def _estimate_runtime_weight(raw_len: int, code: str, cls_name: str) -> int:
    # UiGalleryTest skips rendering in regular desktopTest shards unless -Pparchment.gallery=true is passed.
    if cls_name == "UiGalleryTest":
        return max(1, raw_len // 4)
    http_bonus = 60_000 if cls_name.endswith(("HttpTest", "EndToEndTest")) else 0
    http_hits = len(_HTTP_FIXTURE.findall(code))
    scene_hits = len(_COMPOSE_SCENE.findall(code))
    delay_hits = len(_SLEEP_OR_DELAY.findall(code))
    disk_hits = len(_DISK_CHURN.findall(code))
    test_count = len(_TEST_ANNOT.findall(code))
    return (
        raw_len
        + http_bonus
        + http_hits * 22_000
        + scene_hits * 8_000
        + delay_hits * 3_500
        + disk_hits * 4_000
        + test_count * 1_200
    )


def _strip_comments(text: str) -> str:
    out: list[str] = []
    i, n = 0, len(text)
    while i < n:
        if text[i:i + 3] == '"""':
            end = text.find('"""', i + 3)
            end = n if end < 0 else end + 3
            out.append(text[i:end])
            i = end
        elif text[i] == '"':
            j = i + 1
            while j < n and text[j] not in '"\n':
                j += 2 if text[j] == "\\" else 1
            out.append(text[i:j + 1])
            i = j + 1
        elif text[i:i + 2] == "//":
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text[i:i + 2] == "/*":
            j = text.find("*/", i + 2)
            j = n if j < 0 else j + 2
            out.append("\n" * text.count("\n", i, j))
            i = j
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def discover_test_classes(repo: Path = REPO) -> list[tuple[int, str]]:
    """Returns deterministic (byte_weight, fqcn) pairs sorted heaviest-first."""
    by_name: dict[str, int] = {}
    for rel_root in TEST_ROOTS:
        root = repo / rel_root
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.kt")):
            raw = path.read_bytes()
            code = _strip_comments(raw.decode("utf-8", errors="replace"))
            if "@Test" not in code:
                continue
            pkg_match = _PACKAGE.search(code)
            package = pkg_match.group(1) if pkg_match else ""
            classes = [m.group(1) for m in _TOP_LEVEL_CLASS.finditer(code)]
            if not classes:
                continue
            for cls in classes:
                fqcn = f"{package}.{cls}" if package else cls
                weight = max(1, _estimate_runtime_weight(len(raw), code, cls) // len(classes))
                by_name[fqcn] = by_name.get(fqcn, 0) + weight
    if not by_name:
        raise ValueError(f"No desktop test classes discovered under {repo}")
    return sorted(((weight, fqcn) for fqcn, weight in by_name.items()), key=lambda item: (-item[0], item[1]))


def assign_shards(weighted_classes: list[tuple[int, str]], total: int) -> list[list[str]]:
    if total < 1:
        raise ValueError("total shards must be at least 1")
    if len(weighted_classes) < total:
        raise ValueError(f"cannot split {len(weighted_classes)} test classes into {total} non-empty shards")
    shards: list[list[str]] = [[] for _ in range(total)]
    loads = [0] * total
    for weight, fqcn in weighted_classes:
        target = min(range(total), key=lambda idx: (loads[idx], idx))
        shards[target].append(fqcn)
        loads[target] += weight
    return [sorted(shard) for shard in shards]


def matches_filter(fqcn: str, pattern: str) -> bool:
    regex = "^" + ".*".join(re.escape(part) for part in pattern.split("*")) + "$"
    return bool(re.match(regex, fqcn))


def gradle_test_args(patterns: list[str] | tuple[str, ...]) -> list[str]:
    args: list[str] = []
    for pattern in patterns:
        args.extend(("--tests", pattern))
    return args


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--windows-native", action="store_true", help="print --tests arguments for Windows/platform suites")
    mode.add_argument("--index", type=int, help="0-based shard index")
    parser.add_argument("--total", type=int, default=2, help="total number of shards (default: 2)")
    parser.add_argument("--repo", type=Path, default=REPO)
    options = parser.parse_args(argv)

    if options.windows_native:
        selected: list[str] | tuple[str, ...] = WINDOWS_NATIVE_FILTERS
    else:
        if options.index < 0 or options.index >= options.total:
            parser.error(f"--index must be in 0..{options.total - 1}")
        shards = assign_shards(discover_test_classes(options.repo), options.total)
        selected = shards[options.index]

    print(" ".join(gradle_test_args(selected)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
