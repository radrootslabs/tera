"""Exact legacy-name exceptions, separate from current app naming."""

from __future__ import annotations

import collections
import json
import re
import subprocess
import tempfile
from pathlib import Path
from typing import Any

POLICY = "test-fixtures/legacy-identifiers.v1.json"
MAX_BYTES = 2 * 1024 * 1024
MAX_FILES = 4096
ROOTS = (
    "Tera/",
    "TeraTests/",
    "TeraUITests/",
    "TeraPublicAPITests/",
    "core/crates/",
    "scripts/",
    "TeraFFI/scripts/",
)
ROOT_FILES = {
    "AGENTS.md",
    "README.md",
    "Cargo.toml",
    "Package.swift",
    "project.yml",
    "Makefile",
    "TeraFFI/Makefile",
    "TeraFFI/producer.toml",
    "radroots.lib.source-lock.v1.toml",
}
GENERATED_OR_TOOL_CACHE = (
    "Tera/Generated/",
    "Tera/Frameworks/",
    "scripts/persona-verifier/.venv/",
)
TEXT_SUFFIXES = {
    ".swift",
    ".rs",
    ".py",
    ".sh",
    ".toml",
    ".plist",
    ".xcconfig",
    ".strings",
    ".json",
    ".yml",
    ".md",
    ".kt",
    ".kts",
}
IDENTIFIER = re.compile(r"(?i)\b[a-z0-9_]*radroots[a-z0-9_]*(?:[.-][a-z0-9_-]+)*")
DECLARATION = re.compile(
    r"(?m)^\s*(?:(?:public|private|internal|fileprivate|final|open|indirect|"
    r"nonisolated|static|async|unsafe|pub(?:\([^\n)]*\))?)\s+)*"
    r"(?:class|struct|enum|actor|protocol|typealias|trait|type|fn|fun|func|static\s+(?:var|let))\s+"
    r"(?i:radroots)[a-zA-Z0-9_]*\b"
)
CATEGORY_FIELDS = {"owner", "reason", "reader", "removal_condition"}


class LegacyIdentifierError(ValueError):
    """A source-free naming contract rejection."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise LegacyIdentifierError(message)


def safe_path(value: Any) -> str:
    require(isinstance(value, str) and bool(value), "legacy path is absent")
    path = Path(value)
    require(
        not path.is_absolute() and ".." not in path.parts and str(path) == value,
        "legacy path is unsafe",
    )
    require(
        not any(character in value for character in "*?[]\0\n"),
        "legacy path must be exact",
    )
    return value


def selected(path: str) -> bool:
    if path.startswith(GENERATED_OR_TOOL_CACHE) or "__pycache__" in Path(path).parts:
        return False
    return path in ROOT_FILES or (
        path.startswith(ROOTS) and Path(path).suffix in TEXT_SUFFIXES
    )


def read_regular(root: Path, relative: str) -> bytes:
    path = root
    for part in Path(safe_path(relative)).parts:
        path /= part
        require(not path.is_symlink(), "legacy source contains a symlink")
    require(path.is_file(), "legacy source is not a regular file")
    size = path.stat().st_size
    require(size <= MAX_BYTES, "legacy source exceeds its byte bound")
    value = path.read_bytes()
    require(len(value) == size, "legacy source changed during inspection")
    return value


def git_paths(root: Path, *, ignored: bool = False) -> list[str]:
    flags = ["--others", "--ignored"] if ignored else ["--cached", "--others"]
    paths = [*ROOTS, *sorted(ROOT_FILES)]
    paths.extend(f":(exclude,glob){path}**" for path in GENERATED_OR_TOOL_CACHE)
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
        try:
            result = subprocess.run(
                ["git", "ls-files", "-z", *flags, "--exclude-standard", "--", *paths],
                cwd=root,
                stdin=subprocess.DEVNULL,
                stdout=output,
                stderr=errors,
                timeout=30,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise LegacyIdentifierError(
                "legacy source inventory is unavailable"
            ) from error
        output.seek(0)
        value = output.read(MAX_BYTES + 1)
    require(
        result.returncode == 0 and len(value) <= MAX_BYTES,
        "legacy source inventory failed or exceeded its bound",
    )
    return sorted(set(value.decode("utf-8").split("\0")) - {""})


def source_texts(root: Path) -> dict[str, str]:
    require(
        not any(selected(path) for path in git_paths(root, ignored=True)),
        "ignored app source cannot bypass the legacy guard",
    )
    paths = [path for path in git_paths(root) if selected(path)]
    require(
        0 < len(paths) <= MAX_FILES, "legacy source file inventory exceeds its bound"
    )
    sources = {}
    total = 0
    for path in paths:
        value = read_regular(root, path)
        total += len(value)
        require(total <= 32 * MAX_BYTES, "legacy source aggregate exceeds its bound")
        sources[path] = value.decode("utf-8")
    return sources


def categories(value: Any) -> set[str]:
    require(isinstance(value, dict) and bool(value), "legacy categories are absent")
    for item in value.values():
        require(
            isinstance(item, dict) and set(item) == CATEGORY_FIELDS,
            "legacy category metadata differs",
        )
        require(
            all(isinstance(text, str) and bool(text.strip()) for text in item.values()),
            "legacy category owner, reader or removal condition is absent",
        )
    return set(value)


def occurrences(value: Any, identifier: str) -> dict[tuple[str, str], int]:
    require(isinstance(value, list) and bool(value), "legacy occurrences are absent")
    result = {}
    for item in value:
        require(
            isinstance(item, dict) and set(item) == {"path", "count"},
            "legacy occurrence fields differ",
        )
        path = safe_path(item["path"])
        require(
            selected(path), "legacy exception is outside the inspected source scope"
        )
        count = item["count"]
        require(
            type(count) is int and 0 < count <= 10000,
            "legacy occurrence count is invalid",
        )
        key = (path, identifier)
        require(key not in result, "legacy occurrence is duplicated")
        result[key] = count
    return result


def policy_index(policy: Any) -> dict[tuple[str, str], int]:
    require(
        isinstance(policy, dict)
        and set(policy) == {"schema", "baseline_commit", "categories", "entries"},
        "legacy policy fields differ",
    )
    require(
        policy["schema"] == "tera.legacy-identifiers.v1", "legacy policy schema differs"
    )
    require(
        isinstance(policy["baseline_commit"], str)
        and re.fullmatch(r"[0-9a-f]{40}", policy["baseline_commit"]) is not None,
        "legacy baseline revision is invalid",
    )
    kinds = categories(policy["categories"])
    entries = policy["entries"]
    require(
        isinstance(entries, list) and 0 < len(entries) <= 1024,
        "legacy entry inventory exceeds its bound",
    )
    result = {}
    seen = set()
    for item in entries:
        require(
            isinstance(item, dict)
            and set(item) == {"identifier", "category", "occurrences"},
            "legacy entry fields differ",
        )
        identifier = item["identifier"]
        require(
            isinstance(identifier, str)
            and IDENTIFIER.fullmatch(identifier) is not None,
            "legacy identifier must be exact",
        )
        require(identifier not in seen, "legacy identifier is duplicated")
        require(
            isinstance(item["category"], str) and item["category"] in kinds,
            "legacy category is unknown",
        )
        seen.add(identifier)
        result.update(occurrences(item["occurrences"], identifier))
    return result


def inspect_sources(sources: dict[str, str]) -> collections.Counter:
    observed = collections.Counter()
    for path, text in sources.items():
        if Path(path).suffix in {".swift", ".rs", ".kt", ".kts"}:
            require(
                not Path(path).name.lower().startswith("radroots"),
                "app-owned source filename uses the legacy brand",
            )
            require(
                DECLARATION.search(text) is None,
                "app-owned declaration uses the legacy brand",
            )
        observed.update((path, match[0]) for match in IDENTIFIER.finditer(text))
    return observed


def validate(policy: Any, sources: dict[str, str]) -> dict[str, int]:
    expected = policy_index(policy)
    observed = inspect_sources(sources)
    require(
        set(observed) <= set(expected), "unapproved legacy identifier or source path"
    )
    require(
        observed == expected,
        "legacy exception is stale or its occurrence count differs",
    )
    return {
        "identifiers": len(policy["entries"]),
        "source_identifier_pairs": len(expected),
        "occurrences": sum(observed.values()),
    }


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result = {}
    for key, value in pairs:
        require(key not in result, "legacy JSON field is duplicated")
        result[key] = value
    return result


def verify(root: Path) -> dict[str, int]:
    try:
        policy = json.loads(read_regular(root, POLICY), object_pairs_hook=unique_object)
        return validate(policy, source_texts(root))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise LegacyIdentifierError(
            "legacy source or policy cannot be decoded"
        ) from error
