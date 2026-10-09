"""One authored inventory for standalone quality, tests and the size ratchet."""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

SWIFT_ROOTS = (
    Path("Tera"),
    Path("TeraPublicAPITests"),
    Path("TeraTests"),
    Path("TeraUITests"),
    Path("scripts/legacy_fixture_writers"),
)
PYTHON_ROOT = Path("scripts")
GENERATED_SWIFT = Path("Tera/Generated")
IGNORED_DIRECTORIES = frozenset(
    ("__pycache__", ".venv", ".pytest_cache", ".ruff_cache", ".mypy_cache")
)


class InventoryError(ValueError):
    """An authored source root cannot be inventoried safely."""


def _walk_error(error: OSError) -> None:
    raise InventoryError("authored source inventory cannot be read") from error


def _regular(path: Path) -> Path:
    if path.is_symlink() or not path.is_file():
        raise InventoryError("authored source is not a regular file")
    return path


def _directories(root: Path, current: Path, names: list[str]) -> list[str]:
    retained = []
    for name in sorted(names):
        path = current / name
        if name in IGNORED_DIRECTORIES or path.relative_to(root) == GENERATED_SWIFT:
            continue
        if path.is_symlink():
            raise InventoryError("authored source directory is a link")
        retained.append(name)
    return retained


def _files(root: Path, relative: Path, suffix: str) -> list[Path]:
    directory = root / relative
    if directory.is_symlink() or not directory.is_dir():
        raise InventoryError("authored source root is unavailable")
    found = []
    for current, directories, filenames in os.walk(directory, onerror=_walk_error):
        current = Path(current)
        directories[:] = _directories(root, current, directories)
        found.extend(
            _regular(current / name)
            for name in sorted(filenames)
            if name.endswith(suffix)
        )
    return found


def source_files(root: Path) -> tuple[list[Path], list[Path]]:
    swift = [_regular(root / "Package.swift")]
    for relative in SWIFT_ROOTS:
        swift.extend(_files(root, relative, ".swift"))
    python = _files(root, PYTHON_ROOT, ".py")
    if not python:
        raise InventoryError("authored Python source inventory is empty")
    return sorted(swift), sorted(python)


def command(root: Path, lane: str) -> list[str]:
    _, python = source_files(root)
    paths = [path.relative_to(root).as_posix() for path in python]
    if lane == "test":
        tests = [path for path in paths if Path(path).name.startswith("test_")]
        if not tests:
            raise InventoryError("authored test inventory is empty")
        return [sys.executable, "-m", "unittest", *tests]
    arguments = ["format", "--check"] if lane == "format" else ["check"]
    return ["ruff", *arguments, *paths]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lane", choices=("format", "lint", "test"))
    arguments = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    try:
        argv = command(root, arguments.lane)
    except (InventoryError, OSError) as error:
        print(f"authored inventory: {error}", file=sys.stderr)
        return 1
    os.chdir(root)
    os.execvp(argv[0], argv)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
