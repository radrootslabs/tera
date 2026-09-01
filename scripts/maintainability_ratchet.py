#!/usr/bin/env python3
"""Fail-closed Swift/Python size and Python complexity ratchet."""

from __future__ import annotations

import argparse
import ast
import json
import sys
from pathlib import Path
from typing import Any

MAX_BASELINE_BYTES = 256 * 1024
BASELINE_PATH = Path("test-fixtures/maintainability-baseline.v1.json")
SWIFT_ROOTS = (Path("Radroots"), Path("RadrootsTests"), Path("RadrootsUITests"))
PYTHON_ROOT = Path("scripts")
EXCLUDED_SWIFT_ROOT = Path("Radroots/Generated")


class MaintainabilityError(Exception):
    """Stable maintainability-policy rejection."""


class _FunctionComplexity(ast.NodeVisitor):
    def __init__(self) -> None:
        self.value = 1

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        del node

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        del node

    def visit_Lambda(self, node: ast.Lambda) -> None:
        del node

    def visit_If(self, node: ast.If) -> None:
        self.value += 1
        self.generic_visit(node)

    def visit_IfExp(self, node: ast.IfExp) -> None:
        self.value += 1
        self.generic_visit(node)

    def visit_For(self, node: ast.For) -> None:
        self.value += 1
        self.generic_visit(node)

    def visit_AsyncFor(self, node: ast.AsyncFor) -> None:
        self.value += 1
        self.generic_visit(node)

    def visit_While(self, node: ast.While) -> None:
        self.value += 1
        self.generic_visit(node)

    def visit_ExceptHandler(self, node: ast.ExceptHandler) -> None:
        self.value += 1
        self.generic_visit(node)

    def visit_BoolOp(self, node: ast.BoolOp) -> None:
        self.value += max(0, len(node.values) - 1)
        self.generic_visit(node)

    def visit_Match(self, node: ast.Match) -> None:
        self.value += len(node.cases)
        self.generic_visit(node)

    def visit_comprehension(self, node: ast.comprehension) -> None:
        self.value += 1 + len(node.ifs)
        self.generic_visit(node)


class _DefinitionCollector(ast.NodeVisitor):
    def __init__(self, path: str) -> None:
        self.path = path
        self.scope: list[str] = []
        self.values: dict[str, int] = {}

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        self.scope.append(node.name)
        self.generic_visit(node)
        self.scope.pop()

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        self._record(node)

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        self._record(node)

    def _record(self, node: ast.FunctionDef | ast.AsyncFunctionDef) -> None:
        name = ".".join((*self.scope, node.name))
        key = f"{self.path}:{name}"
        if key in self.values:
            raise MaintainabilityError("Python function identity is duplicated")
        counter = _FunctionComplexity()
        for statement in node.body:
            counter.visit(statement)
        self.values[key] = counter.value
        self.scope.append(node.name)
        for statement in node.body:
            self.visit(statement)
        self.scope.pop()


def _read_regular(path: Path, maximum: int = MAX_BASELINE_BYTES) -> bytes:
    try:
        if path.is_symlink() or not path.is_file():
            raise MaintainabilityError("maintainability input is not a regular file")
        size = path.stat().st_size
        if size < 1 or size > maximum:
            raise MaintainabilityError("maintainability input exceeds its byte bound")
        value = path.read_bytes()
    except OSError as error:
        raise MaintainabilityError("maintainability input cannot be read") from error
    if len(value) != size:
        raise MaintainabilityError("maintainability input changed while reading")
    return value


def _load_baseline(repo_root: Path) -> dict[str, Any]:
    try:
        value = json.loads(_read_regular(repo_root / BASELINE_PATH))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise MaintainabilityError("maintainability baseline is malformed") from error
    expected = {
        "schema",
        "schema_version",
        "source_revision",
        "thresholds",
        "swift_file_exception",
        "python_file_exception",
        "python_complexity_exception",
        "bounded_module",
    }
    if not isinstance(value, dict) or set(value) != expected:
        raise MaintainabilityError("maintainability baseline fields differ")
    if (
        value["schema"] != "radroots.ios.maintainability-baseline.v1"
        or value["schema_version"] != 1
        or value["source_revision"] != "c63002bcc4d3f6656e93aabe4fca6bd771376629"
    ):
        raise MaintainabilityError("maintainability baseline identity differs")
    return value


def _source_files(repo_root: Path) -> tuple[list[Path], list[Path]]:
    swift = sorted(
        path
        for root in SWIFT_ROOTS
        for path in (repo_root / root).rglob("*.swift")
        if not path.is_symlink()
        and not path.relative_to(repo_root).is_relative_to(EXCLUDED_SWIFT_ROOT)
    )
    python = sorted(
        path for path in (repo_root / PYTHON_ROOT).glob("*.py") if not path.is_symlink()
    )
    if not swift or not python:
        raise MaintainabilityError("maintainability source inventory is empty")
    return swift, python


def _line_inventory(repo_root: Path, paths: list[Path]) -> dict[str, int]:
    values: dict[str, int] = {}
    for path in paths:
        raw = _read_regular(path, 4 * 1024 * 1024)
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError as error:
            raise MaintainabilityError("maintainability source is not UTF-8") from error
        relative = path.relative_to(repo_root).as_posix()
        values[relative] = len(text.splitlines())
    return values


def _python_complexity(repo_root: Path, paths: list[Path]) -> dict[str, int]:
    values: dict[str, int] = {}
    for path in paths:
        relative = path.relative_to(repo_root).as_posix()
        try:
            tree = ast.parse(_read_regular(path, 4 * 1024 * 1024), filename=relative)
        except (SyntaxError, ValueError) as error:
            raise MaintainabilityError("Python source cannot be parsed") from error
        collector = _DefinitionCollector(relative)
        collector.visit(tree)
        overlap = set(values) & set(collector.values)
        if overlap:
            raise MaintainabilityError("Python function identity is duplicated")
        values.update(collector.values)
    return values


def snapshot(repo_root: Path) -> dict[str, dict[str, int]]:
    swift, python = _source_files(repo_root)
    return {
        "swift_lines": _line_inventory(repo_root, swift),
        "python_lines": _line_inventory(repo_root, python),
        "python_complexity": _python_complexity(repo_root, python),
    }


def _closed_exception_map(
    value: object,
    *,
    key_name: str,
    ceiling_name: str,
) -> dict[str, int]:
    if not isinstance(value, list):
        raise MaintainabilityError("maintainability exception inventory is invalid")
    result: dict[str, int] = {}
    for row in value:
        if (
            not isinstance(row, dict)
            or set(row) != {key_name, ceiling_name}
            or not isinstance(row[key_name], str)
            or not isinstance(row[ceiling_name], int)
            or row[ceiling_name] < 1
            or row[key_name] in result
        ):
            raise MaintainabilityError("maintainability exception row is invalid")
        result[row[key_name]] = row[ceiling_name]
    if list(result) != sorted(result):
        raise MaintainabilityError("maintainability exceptions are not ordered")
    return result


def _verify_metric(
    observed: dict[str, int],
    exceptions: dict[str, int],
    threshold: int,
    label: str,
) -> None:
    expected_exceptions = {key for key, value in observed.items() if value > threshold}
    if set(exceptions) != expected_exceptions:
        raise MaintainabilityError(f"{label} exception inventory differs")
    for key, ceiling in exceptions.items():
        if ceiling <= threshold or observed[key] > ceiling:
            raise MaintainabilityError(f"{label} metric regressed")


def _thresholds(baseline: dict[str, Any]) -> dict[str, int]:
    value = baseline["thresholds"]
    if (
        not isinstance(value, dict)
        or set(value)
        != {"swift_file_lines", "python_file_lines", "python_function_complexity"}
        or value["swift_file_lines"] != 600
        or value["python_file_lines"] != 800
        or value["python_function_complexity"] != 10
    ):
        raise MaintainabilityError("maintainability thresholds differ")
    return value


def _verify_observed_metrics(
    baseline: dict[str, Any],
    observed: dict[str, dict[str, int]],
    thresholds: dict[str, int],
) -> None:
    specifications = (
        (
            "swift_file_exception",
            "path",
            "maximum_lines",
            "swift_lines",
            "swift_file_lines",
            "Swift file",
        ),
        (
            "python_file_exception",
            "path",
            "maximum_lines",
            "python_lines",
            "python_file_lines",
            "Python file",
        ),
        (
            "python_complexity_exception",
            "function",
            "maximum_complexity",
            "python_complexity",
            "python_function_complexity",
            "Python complexity",
        ),
    )
    for baseline_key, identity, ceiling, metric, threshold, label in specifications:
        exceptions = _closed_exception_map(
            baseline[baseline_key], key_name=identity, ceiling_name=ceiling
        )
        _verify_metric(observed[metric], exceptions, thresholds[threshold], label)


def _verify_bounded_modules(
    baseline: dict[str, Any],
    observed: dict[str, dict[str, int]],
    thresholds: dict[str, int],
) -> None:
    modules = baseline["bounded_module"]
    if not isinstance(modules, list) or modules != sorted(set(modules)):
        raise MaintainabilityError("bounded module inventory differs")
    all_lines = observed["swift_lines"] | observed["python_lines"]
    for path in modules:
        if path not in all_lines:
            raise MaintainabilityError("bounded module is absent")
        limit = thresholds[
            "swift_file_lines" if path.endswith(".swift") else "python_file_lines"
        ]
        if all_lines[path] > limit:
            raise MaintainabilityError("bounded module exceeds its threshold")


def verify(repo_root: Path) -> None:
    baseline = _load_baseline(repo_root)
    thresholds = _thresholds(baseline)
    observed = snapshot(repo_root)
    _verify_observed_metrics(baseline, observed, thresholds)
    _verify_bounded_modules(baseline, observed, thresholds)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("verify", "snapshot"))
    parser.add_argument(
        "--repo-root", type=Path, default=Path(__file__).resolve().parent.parent
    )
    arguments = parser.parse_args()
    try:
        if arguments.command == "snapshot":
            print(json.dumps(snapshot(arguments.repo_root.resolve()), sort_keys=True))
        else:
            verify(arguments.repo_root.resolve())
            print("maintainability ratchet verified")
    except MaintainabilityError as error:
        print(f"maintainability: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
