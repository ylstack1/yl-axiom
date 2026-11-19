#!/usr/bin/env python3
"""Repository rules enforced by code, so they hold for any person, model or tool.

Standard library only. Run directly or through `python scripts/verify.py review`.
"""

from __future__ import annotations

import collections
import json
import posixpath
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BASELINE_PATH = REPO / "scripts" / "quality-baseline.json"

MAIN_SETS = ("commonMain", "desktopMain", "androidMain", "jvmShared")
TEST_SETS = ("commonTest", "desktopTest")
CRLF_SUFFIXES = (".bat", ".iss")
BINARY_SUFFIXES = (
    ".ttf", ".otf", ".woff", ".woff2", ".png", ".jpg", ".jpeg", ".webp", ".gif", ".ico", ".bmp",
    ".mp3", ".wav", ".ogg", ".mp4", ".webm", ".pdf", ".zip", ".jar", ".apk", ".exe", ".dll",
    ".so", ".dylib", ".jks", ".keystore", ".p12", ".pfx",
)
SECRET_SUFFIXES = (".jks", ".keystore", ".p12", ".pfx")
SECRET_NAMES = ("local.properties", ".env")
MAX_KOTLIN_LINES = 3000
MAX_REPORTED_PER_RULE = 15
DOC_BUDGETS = {"AGENTS.md": 20000, "TODO.md": 20000}  # characters; every session reads these
STATUS_FIELDS = ("**Source:**", "**Last verified", "**Next action:**")
STATUS_HEADING = "## Current status"
MAX_STATUS_CHARS = 1400
# Resolved by the compiler through delegates, operators and destructuring, so a name search cannot judge them.
IMPLICIT_IMPORTS = {
    "getValue", "setValue", "provideDelegate", "plus", "minus", "times", "div", "rem", "get", "set", "invoke",
    "contains", "iterator", "plusAssign", "minusAssign", "unaryMinus", "unaryPlus", "not", "compareTo", "inc", "dec",
}
BANNED_PATTERNS = (
    (re.compile(r"androidx\.compose\.material"), "Compose Foundation only: Material is not allowed."),
    (re.compile(r"\bGlobalScope\b"), "Use an owned coroutine scope, not GlobalScope."),
    (re.compile(r"(?<![\w.])println\("), "Do not leave console output in app code."),
    (re.compile(r"\bFIXME\b|(?<![\w.])TODO\("), "Track unfinished work in TODO.md."),
    (re.compile(r"\.clickable\(enabled = false\)"), "Use blockTapThrough(); a disabled clickable merges reader semantics."),
    (re.compile(r"\bproviderProtocol = \w+\.protocol\b"), "Stamp resolveWireProtocol(profile); a Custom profile's stored protocol hides its real wire."),
)

_MODIFIERS = (
    r"(?:(?:private|internal|public|protected|actual|expect|inline|suspend|operator|infix|tailrec|external|open|"
    r"abstract|final|const|lateinit|data|sealed|enum|value|annotation|override|@[A-Za-z_.]+(?:\([^)]*\))?)\s+)*"
)
_DECLARATION = re.compile(
    r"^[ \t]*(" + _MODIFIERS + r")(fun|val|var|class|object|interface|typealias)\s+"
    r"(?:<[^>\n]+>\s+)?(?:[A-Za-z_][\w.<>?]*\.)?([A-Za-z_]\w*)",
    re.M,
)
_WORD = re.compile(r"[A-Za-z_]\w*")
_IMPORT = re.compile(r"^import ([\w.]+?)(?: as (\w+))?$", re.M)
_IMPORT_LINE = re.compile(r"^import .*$", re.M)
_TASK_ID = re.compile(r"^- \[[ x]\] \*\*([0-9]+\.[0-9]+[a-z]?)\b", re.M)
_COMMENT_OR_STRING = re.compile(
    r'"""[\s\S]*?(?:"""|$)|"(?:\\.|[^"\\\n])*"|\'(?:\\.|[^\'\\\n]){0,9}\'|//[^\n]*|/\*[\s\S]*?(?:\*/|$)'
)


@dataclass(frozen=True)
class Problem:
    rule: str
    path: str
    line: int
    message: str

    def __str__(self) -> str:
        location = f"{self.path}:{self.line}" if self.line else self.path
        return f"{location} [{self.rule}] {self.message}"


def _strip_match(match: re.Match[str]) -> str:
    token = match.group(0)
    if token[0] in "\"'":
        return token
    return "\n" * token.count("\n") if token.startswith("/*") else ""


def strip_comments(text: str) -> str:
    """Removes comments only, so `"*/*"` in a string cannot hide code from the scanners."""
    return _COMMENT_OR_STRING.sub(_strip_match, text)


def source_set(path: str) -> str:
    match = re.match(r"composeApp/src/([^/]+)/", path)
    return match.group(1) if match else ""


def is_kotlin(path: str) -> bool:
    return path.endswith(".kt") and bool(source_set(path))


def check_encoding(files: dict[str, bytes]) -> list[Problem]:
    problems = []
    for path, data in files.items():
        if path.lower().endswith(BINARY_SUFFIXES) or b"\0" in data[:8000]:
            continue
        if data.startswith(b"\xef\xbb\xbf"):
            problems.append(Problem("encoding", path, 1, "Remove the UTF-8 byte-order mark."))
        crlf = data.count(b"\r\n")
        if path.endswith(CRLF_SUFFIXES):
            if crlf != data.count(b"\n"):
                problems.append(Problem("encoding", path, 0, "This file type must use CRLF line endings."))
        elif crlf:
            problems.append(Problem("encoding", path, 0, "Use LF line endings."))
        try:
            data.decode("utf-8")
        except UnicodeDecodeError:
            problems.append(Problem("encoding", path, 0, "File is not valid UTF-8."))
        if path.endswith(".ps1") and not data.isascii():
            problems.append(Problem("encoding", path, 0, "PowerShell scripts must be ASCII; Windows PowerShell misreads UTF-8 without a BOM."))
    return problems


def check_unused_imports(texts: dict[str, str]) -> list[Problem]:
    problems = []
    for path, text in texts.items():
        body_words = set(_WORD.findall(_IMPORT_LINE.sub("", text)))
        for match in _IMPORT.finditer(text):
            name = match.group(1)
            symbol = match.group(2) or name.rsplit(".", 1)[-1]
            if symbol in IMPLICIT_IMPORTS or symbol.startswith("component") or symbol == "*":
                continue
            if symbol not in body_words:
                line = text.count("\n", 0, match.start()) + 1
                problems.append(Problem("unused-import", path, line, f"Remove unused import {name}."))
    return problems


def check_dead_code(
    texts: dict[str, str],
    baseline: dict,
    stripped: dict[str, str] | None = None,
) -> list[Problem]:
    code_by_path = stripped if stripped is not None else {p: strip_comments(t) for p, t in texts.items()}
    main = {p: code_by_path[p] for p in texts if source_set(p) in MAIN_SETS}
    tests = {p: code_by_path[p] for p in texts if source_set(p) in TEST_SETS}
    main_words = collections.Counter(_WORD.findall("\n".join(main.values())))
    test_words = collections.Counter(_WORD.findall("\n".join(tests.values())))
    declarations: collections.Counter = collections.Counter()
    found = []
    for path, text in main.items():
        for match in _DECLARATION.finditer(text):
            declarations[match.group(3)] += 1
            found.append((path, match, text))
    allowed = set(baseline.get("testOnly", ()))
    problems, seen = [], set()
    for path, match, text in found:
        modifiers, kind, name = match.groups()
        if name in seen or "override" in modifiers or name.startswith("_") or name == "main":
            continue
        line_text = text[text.rfind("\n", 0, match.start()) + 1:text.find("\n", match.end())]
        if kind in ("val", "var") and line_text.rstrip().endswith(","):
            continue  # Constructor properties may exist only for serialization.
        if main_words[name] - declarations[name] > 0:
            continue
        seen.add(name)
        line = text.count("\n", 0, match.start()) + 1
        if test_words[name]:
            if name not in allowed:
                problems.append(Problem("dead-code", path, line, f"{name} is used only by tests; remove it or list it as a test seam."))
        else:
            problems.append(Problem("dead-code", path, line, f"{name} is never used."))
    return problems


def check_file_sizes(texts: dict[str, str], baseline: dict) -> list[Problem]:
    allowed = baseline.get("longFiles", {})
    problems = []
    for path, text in texts.items():
        if source_set(path) not in MAIN_SETS:
            continue
        lines = text.count("\n")
        limit = allowed.get(path, MAX_KOTLIN_LINES)
        if lines > limit:
            problems.append(Problem("file-size", path, 0, f"{lines} lines exceeds the limit of {limit}. Split by responsibility; do not raise the limit."))
    return problems


def check_banned(texts: dict[str, str], stripped: dict[str, str] | None = None) -> list[Problem]:
    problems = []
    for path, text in texts.items():
        if source_set(path) not in MAIN_SETS:
            continue
        code = stripped[path] if stripped is not None else strip_comments(text)
        for pattern, message in BANNED_PATTERNS:
            for match in pattern.finditer(code):
                problems.append(Problem("banned", path, code.count("\n", 0, match.start()) + 1, message))
    return problems


def status_block(todo: str) -> str:
    if STATUS_HEADING not in todo:
        return ""
    rest = todo.split(STATUS_HEADING, 1)[1]
    return rest.split("\n## ", 1)[0].strip()


def check_docs(files: dict[str, bytes]) -> list[Problem]:
    problems = []
    for path, budget in DOC_BUDGETS.items():
        if path not in files:
            problems.append(Problem("docs", path, 0, "Required workflow file is missing."))
        elif (length := len(files[path].decode("utf-8", errors="replace"))) > budget:
            problems.append(Problem("docs", path, 0, f"{length} characters exceeds the {budget} character budget. Condense it; do not raise the budget."))
    if "AGENTS.md" in files and b"scripts/verify.py" not in files["AGENTS.md"]:
        problems.append(Problem("docs", "AGENTS.md", 0, "Must tell readers to use scripts/verify.py."))
    todo_bytes = files.get("TODO.md")
    if todo_bytes is None:
        return problems
    todo = todo_bytes.decode("utf-8", "replace")
    status = status_block(todo)
    if not status:
        problems.append(Problem("docs", "TODO.md", 0, f"Missing the '{STATUS_HEADING}' section."))
    for field in STATUS_FIELDS:
        if status and field not in status:
            problems.append(Problem("docs", "TODO.md", 0, f"Current status must include {field}"))
    if len(status) > MAX_STATUS_CHARS:
        problems.append(Problem("docs", "TODO.md", 0, f"Current status is {len(status)} characters; keep it under {MAX_STATUS_CHARS}."))
    for task_id, count in collections.Counter(_TASK_ID.findall(todo)).items():
        if count > 1:
            problems.append(Problem("docs", "TODO.md", 0, f"Task {task_id} appears {count} times."))
    return problems


def check_secrets(paths: list[str]) -> list[Problem]:
    return [
        Problem("secrets", path, 0, "Private signing or credential files must never be tracked.")
        for path in paths
        if path.lower().endswith(SECRET_SUFFIXES) or path.rsplit("/", 1)[-1] in SECRET_NAMES
    ]


_MD_LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
_FENCE = re.compile(r"```.*?```", re.S)
_HEADING = re.compile(r"^#{1,6}\s+(.+?)\s*#*\s*$", re.M)


def heading_anchors(markdown: str) -> set[str]:
    """GitHub's anchors: lower case, punctuation dropped, spaces to hyphens."""
    return {re.sub(r"[^\w\- ]", "", h.strip().lower()).replace(" ", "-") for h in _HEADING.findall(_FENCE.sub("", markdown))}


def check_links(files: dict[str, bytes]) -> list[Problem]:
    """Relative links in Markdown must reach a tracked file, and #anchors a real heading."""
    problems = []
    docs = {p: d.decode("utf-8", "replace") for p, d in files.items() if p.endswith(".md")}
    for path, text in docs.items():
        for number, line in enumerate(_FENCE.sub(lambda m: "\n" * m.group(0).count("\n"), text).splitlines(), 1):
            for target in _MD_LINK.findall(line):
                if re.match(r"[a-z][a-z0-9+.-]*:", target, re.I):
                    continue  # http, https, mailto: outside the repository
                file_part, _, anchor = target.partition("#")
                resolved = (Path(path).parent / file_part).as_posix() if file_part else path
                resolved = posixpath.normpath(resolved)
                exists = resolved in files or any(p.startswith(resolved.rstrip("/") + "/") for p in files)
                if not exists:
                    problems.append(Problem("docs", path, number, f"Broken link: {target}"))
                elif anchor and resolved in docs and anchor.lower() not in heading_anchors(docs[resolved]):
                    problems.append(Problem("docs", path, number, f"Missing anchor: {target}"))
    return problems


def check_files(files: dict[str, bytes], baseline: dict) -> list[Problem]:
    texts = {p: d.decode("utf-8", "replace").replace("\r\n", "\n").lstrip("﻿") for p, d in files.items() if is_kotlin(p)}
    stripped = {p: strip_comments(t) for p, t in texts.items()}
    return (
        check_encoding(files)
        + check_unused_imports(texts)
        + check_dead_code(texts, baseline, stripped)
        + check_file_sizes(texts, baseline)
        + check_banned(texts, stripped)
        + check_docs(files)
        + check_links(files)
        + check_secrets(sorted(files))
    )


def repo_paths(repo: Path = REPO) -> list[str]:
    output = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=repo, capture_output=True, check=True
    ).stdout
    return [p for p in output.decode("utf-8").split("\0") if p and (repo / p).is_file()]


def load_files(repo: Path = REPO) -> dict[str, bytes]:
    return {
        path: (b"" if path.lower().endswith(BINARY_SUFFIXES) else (repo / path).read_bytes())
        for path in repo_paths(repo)
    }


def load_baseline() -> dict:
    return json.loads(BASELINE_PATH.read_text(encoding="utf-8"))


def format_problems(problems: list[Problem]) -> list[str]:
    lines, shown = [], collections.Counter()
    for problem in problems:
        shown[problem.rule] += 1
        if shown[problem.rule] <= MAX_REPORTED_PER_RULE:
            lines.append(str(problem))
    for rule, count in shown.items():
        if count > MAX_REPORTED_PER_RULE:
            lines.append(f"[{rule}] {count - MAX_REPORTED_PER_RULE} more not shown")
    return lines


def main() -> int:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    problems = check_files(load_files(), load_baseline())
    if problems:
        print(f"FAIL quality: {len(problems)} problem(s)")
        print("\n".join(format_problems(problems)))
        return 1
    print("PASS quality")
    return 0


if __name__ == "__main__":
    sys.exit(main())
