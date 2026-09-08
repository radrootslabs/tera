"""Exact staged native artifact inventory and source-tuple verification."""

from __future__ import annotations

import hashlib
import plistlib
import re
from pathlib import Path
from typing import Any

import ffi_provenance as provenance
import ffi_source as source
import package_contract as contract

TARGETS = ("aarch64-apple-ios", "aarch64-apple-ios-sim", "aarch64-apple-darwin")
MAX_ARTIFACT_BYTES = 256 * 1024 * 1024
FRAMEWORK = "TeraFFI.xcframework"
MODULE = "TeraKitBindings"
MANIFEST = "provenance.json"


def expected_paths() -> set[str]:
    paths = {
        "generated/TeraKitBindings.swift",
        "generated/TeraFFI.h",
        "generated/TeraFFI.modulemap",
        "headers/TeraFFI.h",
        "headers/module.modulemap",
        f"{FRAMEWORK}/Info.plist",
        "api/TeraKitBindings.symbols.json",
        "abi_symbols.json",
    }
    for target in TARGETS:
        extension = "dylib" if target == TARGETS[-1] else "a"
        paths.add(f"native/{target}/libtera_ffi.{extension}")
        paths.add(f"source/{target}.json")
    for platform in ("ios-arm64", "ios-arm64-simulator"):
        for relative in (
            "libtera_ffi.a",
            "Headers/TeraFFI.h",
            "Headers/module.modulemap",
        ):
            paths.add(f"{FRAMEWORK}/{platform}/{relative}")
    return paths


def regular_path(root: Path, relative: str) -> Path:
    path = Path(relative)
    if path.is_absolute() or str(path) != relative or ".." in path.parts:
        raise source.ProvenanceError("native artifact path is invalid")
    current = root
    for part in path.parts:
        current = current / part
        if current.is_symlink():
            raise source.ProvenanceError("native artifact path contains a symlink")
    if not current.is_file():
        raise source.ProvenanceError("native artifact file is missing")
    return current


def file_record(root: Path, relative: str) -> dict[str, Any]:
    path = regular_path(root, relative)
    size = path.stat().st_size
    if size <= 0 or size > MAX_ARTIFACT_BYTES:
        raise source.ProvenanceError("native artifact exceeds its byte bound")
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while data := handle.read(1024 * 1024):
            digest.update(data)
    if path.stat().st_size != size:
        raise source.ProvenanceError("native artifact changed during verification")
    return {"path": relative, "bytes": size, "sha256": digest.hexdigest()}


def inventory(root: Path) -> list[dict[str, Any]]:
    paths = set()
    for path in root.rglob("*"):
        if path.is_symlink():
            raise source.ProvenanceError("native artifact inventory contains a symlink")
        if path.is_file() and path.relative_to(root).as_posix() != MANIFEST:
            paths.add(path.relative_to(root).as_posix())
    if paths != expected_paths():
        raise source.ProvenanceError("native artifact inventory differs")
    return [file_record(root, relative) for relative in sorted(paths)]


def validate_module_files(root: Path) -> None:
    generated = root / "generated"
    header = contract._read_regular(generated / "TeraFFI.h")
    modulemap = contract._read_regular(generated / "TeraFFI.modulemap")
    if b"module TeraFFI {" not in modulemap or b'header "TeraFFI.h"' not in modulemap:
        raise source.ProvenanceError("generated native module identity differs")
    swift = contract._read_regular(generated / "TeraKitBindings.swift")
    if b"import TeraFFI" not in swift:
        raise source.ProvenanceError("generated Swift module does not import its FFI")
    for prefix in (
        "headers",
        f"{FRAMEWORK}/ios-arm64/Headers",
        f"{FRAMEWORK}/ios-arm64-simulator/Headers",
    ):
        if contract._read_regular(root / prefix / "TeraFFI.h") != header:
            raise source.ProvenanceError(
                "packaged FFI header differs from generated header"
            )
        if contract._read_regular(root / prefix / "module.modulemap") != modulemap:
            raise source.ProvenanceError(
                "packaged FFI module map differs from generated module map"
            )


def validate_framework(root: Path) -> None:
    info = plistlib.loads(contract._read_regular(root / FRAMEWORK / "Info.plist"))
    libraries = info.get("AvailableLibraries", [])
    expected = {
        "ios-arm64": None,
        "ios-arm64-simulator": "simulator",
    }
    if len(libraries) != 2 or {
        item.get("LibraryIdentifier") for item in libraries
    } != set(expected):
        raise source.ProvenanceError("XCFramework platform inventory differs")
    for item in libraries:
        validate_platform(item, expected[item["LibraryIdentifier"]])
    for target, platform in zip(TARGETS[:2], expected, strict=True):
        first = file_record(root, f"native/{target}/libtera_ffi.a")
        packaged = file_record(root, f"{FRAMEWORK}/{platform}/libtera_ffi.a")
        if (first["bytes"], first["sha256"]) != (packaged["bytes"], packaged["sha256"]):
            raise source.ProvenanceError(
                "XCFramework library differs from its built target"
            )


def validate_platform(item: dict[str, Any], variant: str | None) -> None:
    if (
        item.get("SupportedArchitectures") != ["arm64"]
        or item.get("SupportedPlatform") != "ios"
        or item.get("SupportedPlatformVariant") != variant
        or item.get("LibraryPath") != "libtera_ffi.a"
        or item.get("HeadersPath") != "Headers"
    ):
        raise source.ProvenanceError("XCFramework library contract differs")


def validate_abi(root: Path) -> None:
    symbols = contract._read_json(root / "abi_symbols.json")
    if set(symbols) != set(TARGETS):
        raise source.ProvenanceError("native ABI target inventory differs")
    host = symbols[TARGETS[-1]]
    if not isinstance(host, list) or not host or host != sorted(set(host)):
        raise source.ProvenanceError("native ABI symbols are invalid")
    if any(symbols[target] != host for target in TARGETS):
        raise source.ProvenanceError("native target ABI symbols differ")
    header = contract._read_regular(root / "generated/TeraFFI.h").decode()
    declared = set(
        re.findall(r"\b((?:ffi|uniffi)_tera_ffi_[A-Za-z0-9_]+)\s*\(", header)
    )
    if not declared or not declared.issubset(host):
        raise source.ProvenanceError(
            "generated header declares an unavailable native symbol"
        )


def manifest(root: Path, records: dict[str, dict[str, Any]]) -> dict[str, Any]:
    if set(records) != set(TARGETS):
        raise source.ProvenanceError("producer source target inventory differs")
    source_tree = records[TARGETS[0]]["source"]["tree"]
    for target, record in records.items():
        if (
            record["source"]["tree"] != source_tree
            or record["build"]["target"] != target
        ):
            raise source.ProvenanceError("producer source tuples disagree")
        provenance.verify_record(
            contract._read_regular(root / "source" / f"{target}.json"), record
        )
    validate_module_files(root)
    validate_framework(root)
    validate_abi(root)
    validate_api(root)
    return {
        "schema": "radroots.artifact-manifest.v2",
        "product": "tera",
        "target": "ios",
        "language": "swift",
        "external_names": ["TeraFFI", "TeraKitBindings"],
        "source": {
            "repository": records[TARGETS[0]]["repository"],
            "tree": source_tree,
        },
        "source_records": {target: f"source/{target}.json" for target in TARGETS},
        "files": inventory(root),
        "disposition": "local_candidate_not_installed",
    }


def validate_api(root: Path) -> None:
    value = contract._read_json(root / "api/TeraKitBindings.symbols.json")
    if (
        value.get("schema") != "radroots.swift-api-snapshot.v1"
        or value.get("module", {}).get("name") != MODULE
        or not isinstance(value.get("symbols"), list)
        or not value["symbols"]
    ):
        raise source.ProvenanceError("generated Swift API snapshot identity differs")


def check(root: Path, records: dict[str, dict[str, Any]]) -> dict[str, Any]:
    expected = manifest(root, records)
    if contract._read_regular(root / MANIFEST) != provenance.encoded(expected):
        raise source.ProvenanceError("native artifact provenance is stale")
    return expected
