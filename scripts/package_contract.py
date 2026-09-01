#!/usr/bin/env python3
"""Structured standalone package-contract verification for the iOS capsule."""

from __future__ import annotations

import argparse
import json
import os
import plistlib
import re
import subprocess
import sys
import tempfile
import tomllib
from collections.abc import Mapping
from pathlib import Path
from typing import Any

MAX_CONTRACT_BYTES = 2 * 1024 * 1024
GIT_REVISION = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
APPLE_KIT_REMOTE = "https://github.com/radrootslabs/apple_kit.git"
LIB_REMOTE = "https://github.com/radrootslabs/lib"
SECP256K1_REMOTE = "https://github.com/21-DOT-DEV/swift-secp256k1.git"
SECP256K1_REVISION = "e70a10e036a55fffea31568f0af92d69b6d449cd"


class PackageContractError(Exception):
    """A stable, source-free package-contract rejection."""


def _read_regular(path: Path, *, maximum: int = MAX_CONTRACT_BYTES) -> bytes:
    try:
        if path.is_symlink() or not path.is_file():
            raise PackageContractError("required contract input is not a regular file")
        size = path.stat().st_size
        if size < 0 or size > maximum:
            raise PackageContractError("required contract input exceeds its byte limit")
        value = path.read_bytes()
    except OSError as error:
        raise PackageContractError("required contract input cannot be read") from error
    if len(value) != size:
        raise PackageContractError("required contract input changed while reading")
    return value


def _read_text(path: Path) -> str:
    try:
        return _read_regular(path).decode("utf-8")
    except UnicodeDecodeError as error:
        raise PackageContractError("required contract input is not UTF-8") from error


def _read_toml(path: Path) -> dict[str, Any]:
    try:
        value = tomllib.loads(_read_text(path))
    except tomllib.TOMLDecodeError as error:
        raise PackageContractError("required TOML contract is malformed") from error
    if not isinstance(value, dict):
        raise PackageContractError("required TOML contract is not an object")
    return value


def _read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(_read_text(path))
    except json.JSONDecodeError as error:
        raise PackageContractError("required JSON contract is malformed") from error
    if not isinstance(value, dict):
        raise PackageContractError("required JSON contract is not an object")
    return value


def _read_plist(path: Path) -> dict[str, Any]:
    try:
        value = plistlib.loads(_read_regular(path))
    except (plistlib.InvalidFileException, ValueError, TypeError) as error:
        raise PackageContractError("required plist contract is malformed") from error
    if not isinstance(value, dict):
        raise PackageContractError("required plist contract is not a dictionary")
    return value


def _mapping(value: object, key: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise PackageContractError(f"structured contract field is invalid: {key}")
    return value


def _exact(value: object, expected: object, key: str) -> None:
    if value != expected:
        raise PackageContractError(f"structured contract field differs: {key}")


def parse_make_assignments(text: str) -> dict[str, str]:
    assignments: dict[str, str] = {}
    expression = re.compile(r"^override ([A-Z0-9_]+) := ([^\r\n]+)$")
    for line in text.splitlines():
        match = expression.fullmatch(line)
        if match is None:
            if line.strip() and not line.lstrip().startswith("#"):
                raise PackageContractError(
                    "source-lock contains an unsupported statement"
                )
            continue
        key, value = match.groups()
        if key in assignments:
            raise PackageContractError("source-lock assignment is duplicated")
        assignments[key] = value
    return assignments


def parse_xcconfig_assignments(text: str) -> dict[str, str]:
    assignments: dict[str, str] = {}
    expression = re.compile(r"^([A-Z][A-Z0-9_]*)\s*=\s*(\S(?:.*\S)?)$")
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("//") or line.startswith("#"):
            continue
        match = expression.fullmatch(line)
        if match is None:
            raise PackageContractError("xcconfig contains an unsupported statement")
        key, value = match.groups()
        if key in assignments:
            raise PackageContractError("xcconfig assignment is duplicated")
        assignments[key] = value
    return assignments


def parse_project_package(text: str, package_name: str) -> dict[str, str]:
    lines = text.splitlines()
    packages_line = next(
        (index for index, line in enumerate(lines) if line == "packages:"),
        None,
    )
    if packages_line is None:
        raise PackageContractError("project package inventory is absent")
    expected_header = f"  {package_name}:"
    packages_end = next(
        (
            index
            for index in range(packages_line + 1, len(lines))
            if lines[index] and not lines[index].startswith((" ", "#"))
        ),
        len(lines),
    )
    start = next(
        (
            index
            for index in range(packages_line + 1, packages_end)
            if lines[index] == expected_header
        ),
        None,
    )
    if start is None:
        raise PackageContractError("project package entry is absent")
    values: dict[str, str] = {}
    for line in lines[start + 1 :]:
        if line and not line.startswith("    "):
            break
        match = re.fullmatch(r"    ([a-z_]+): (\S+)", line)
        if match is None:
            if line.strip():
                raise PackageContractError("project package entry is malformed")
            continue
        key, value = match.groups()
        if key in values:
            raise PackageContractError("project package field is duplicated")
        values[key] = value
    return values


def validate_resolved(document: dict[str, Any], apple_revision: str) -> None:
    _exact(document.get("version"), 3, "package lock version")
    pins = document.get("pins")
    if not isinstance(pins, list) or len(pins) != 2:
        raise PackageContractError("package lock pin inventory differs")
    selected: dict[str, str] = {}
    for pin in pins:
        item = _mapping(pin, "package lock pin")
        _exact(item.get("kind"), "remoteSourceControl", "package lock pin kind")
        location = item.get("location")
        state = _mapping(item.get("state"), "package lock pin state")
        revision = state.get("revision")
        if not isinstance(location, str) or not location.startswith("https://"):
            raise PackageContractError("package lock location is invalid")
        if not isinstance(revision, str) or GIT_REVISION.fullmatch(revision) is None:
            raise PackageContractError("package lock revision is invalid")
        if location in selected:
            raise PackageContractError("package lock location is duplicated")
        selected[location] = revision
    _exact(selected.get(APPLE_KIT_REMOTE), apple_revision, "AppleKit package pin")
    _exact(
        selected.get(SECP256K1_REMOTE),
        SECP256K1_REVISION,
        "secp256k1 package pin",
    )


def _swift_package(repo_root: Path) -> dict[str, Any]:
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        try:
            result = subprocess.run(
                [
                    "swift",
                    "package",
                    "--package-path",
                    str(repo_root),
                    "dump-package",
                ],
                check=False,
                stdout=stdout,
                stderr=stderr,
                timeout=60,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise PackageContractError(
                "Swift package manifest cannot be evaluated"
            ) from error
        stdout.seek(0)
        output = stdout.read(MAX_CONTRACT_BYTES + 1)
    if result.returncode != 0 or len(output) > MAX_CONTRACT_BYTES:
        raise PackageContractError("Swift package manifest evaluation failed")
    try:
        value = json.loads(output)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PackageContractError("Swift package manifest output is malformed") from error
    if not isinstance(value, dict):
        raise PackageContractError("Swift package manifest output is not an object")
    return value


def _apple_revision(package: dict[str, Any]) -> str:
    dependencies = package.get("dependencies")
    if not isinstance(dependencies, list):
        raise PackageContractError("Swift package dependencies are absent")
    matches: list[str] = []
    for dependency in dependencies:
        item = _mapping(dependency, "Swift package dependency")
        source = item.get("sourceControl")
        if not isinstance(source, list) or len(source) != 1:
            continue
        identity = _mapping(source[0], "Swift package source")
        remote = identity.get("location")
        revision = identity.get("requirement")
        remote_values = remote.get("remote") if isinstance(remote, dict) else None
        if (
            isinstance(remote_values, list)
            and remote_values == [{"urlString": APPLE_KIT_REMOTE}]
            and isinstance(revision, dict)
            and isinstance(revision.get("revision"), list)
            and len(revision["revision"]) == 1
        ):
            matches.append(revision["revision"][0])
    if (
        len(matches) != 1
        or not isinstance(matches[0], str)
        or GIT_REVISION.fullmatch(matches[0]) is None
    ):
        raise PackageContractError("AppleKit dependency is not one exact revision")
    return matches[0]


def _validate_privacy(document: dict[str, Any]) -> None:
    _exact(document.get("NSPrivacyTracking"), False, "privacy tracking")
    _exact(document.get("NSPrivacyTrackingDomains"), [], "privacy tracking domains")
    _exact(document.get("NSPrivacyCollectedDataTypes"), [], "privacy collected data")
    _exact(
        document.get("NSPrivacyAccessedAPITypes"),
        [
            {
                "NSPrivacyAccessedAPIType": "NSPrivacyAccessedAPICategoryUserDefaults",
                "NSPrivacyAccessedAPITypeReasons": ["CA92.1"],
            }
        ],
        "privacy accessed APIs",
    )


def _validate_app_plist(document: dict[str, Any]) -> None:
    for key in (
        "NSCameraUsageDescription",
        "NSFaceIDUsageDescription",
        "NSLocalNetworkUsageDescription",
    ):
        value = document.get(key)
        if not isinstance(value, str) or not value.strip():
            raise PackageContractError(f"required plist purpose is absent: {key}")
    _exact(
        document.get("NSAppTransportSecurity"),
        {"NSAllowsLocalNetworking": True},
        "app transport security",
    )
    for forbidden in ("NSBonjourServices", "NSPhotoLibraryUsageDescription"):
        if forbidden in document:
            raise PackageContractError(f"forbidden plist field is present: {forbidden}")


def _validate_ui_test_plist(document: dict[str, Any]) -> None:
    required = {
        "RADROOTS_IOS_UI_TEST_FIXTURE_CONTROL",
        "RADROOTS_IOS_UI_TEST_FIXTURE_EVIDENCE",
        "RADROOTS_IOS_UI_TEST_NETWORK_PROFILE",
        "RADROOTS_IOS_UI_TEST_SOURCE_COMMIT",
        "RADROOTS_IOS_UI_TEST_SOURCE_TREE",
        "RADROOTS_IOS_UI_TEST_APP_BUILD_SHA256",
        "RADROOTS_IOS_UI_TEST_SIMULATOR_ID",
    }
    if required.difference(document):
        raise PackageContractError("UI test plist inventory is incomplete")


def verify(repo_root: Path) -> tuple[str, str]:
    root = repo_root.resolve()
    for forbidden in ("docs", ".github", ".act"):
        path = root / forbidden
        if path.exists() or path.is_symlink():
            raise PackageContractError("forbidden public repository root exists")

    cargo = _read_toml(root / "Cargo.toml")
    workspace = _mapping(cargo.get("workspace"), "Cargo workspace")
    workspace_package = _mapping(workspace.get("package"), "Cargo workspace package")
    _exact(
        workspace_package.get("repository"),
        "https://github.com/radrootslabs/tera",
        "Cargo repository",
    )
    ffi_dependency = _mapping(
        _mapping(workspace.get("dependencies"), "Cargo workspace dependencies").get(
            "radroots_mobile_ffi"
        ),
        "Cargo FFI dependency",
    )

    source = parse_make_assignments(_read_text(root / "RadrootsFFI/source.lock"))
    required_source = {
        "RADROOTS_FIELD_LIB_GIT_URL",
        "RADROOTS_FIELD_LIB_GIT_REV",
        "RADROOTS_FIELD_FFI_CRATE_VERSION",
        "RADROOTS_FIELD_SOURCE_DATE_EPOCH",
        "RADROOTS_FIELD_FFI_DEVICE_SHA256",
        "RADROOTS_FIELD_FFI_SIMULATOR_SHA256",
        "RADROOTS_FIELD_FFI_SWIFT_SHA256",
        "RADROOTS_FIELD_FFI_HEADER_SHA256",
        "RADROOTS_FIELD_FFI_MODULEMAP_SHA256",
        "RADROOTS_FIELD_FFI_API_SHA256",
        "RADROOTS_FIELD_FFI_XCFRAMEWORK_SHA256",
    }
    if set(source) != required_source:
        raise PackageContractError("FFI source-lock field inventory differs")
    _exact(source["RADROOTS_FIELD_LIB_GIT_URL"], LIB_REMOTE, "FFI Lib remote")
    lib_revision = source["RADROOTS_FIELD_LIB_GIT_REV"]
    if GIT_REVISION.fullmatch(lib_revision) is None:
        raise PackageContractError("FFI Lib revision is invalid")
    for key in required_source:
        if key.endswith("SHA256") and SHA256.fullmatch(source[key]) is None:
            raise PackageContractError("FFI source-lock digest is invalid")
    release_version = source["RADROOTS_FIELD_FFI_CRATE_VERSION"]
    _exact(release_version, "0.1.0-alpha", "FFI release version")
    if set(ffi_dependency) != {"git", "rev", "version"}:
        raise PackageContractError("Cargo FFI dependency field inventory differs")
    _exact(ffi_dependency.get("git"), LIB_REMOTE, "Cargo FFI remote")
    _exact(ffi_dependency.get("rev"), lib_revision, "Cargo FFI revision")
    _exact(ffi_dependency.get("version"), "=0.1.0-alpha", "Cargo FFI version")
    epoch = source["RADROOTS_FIELD_SOURCE_DATE_EPOCH"]
    if not epoch.isascii() or not epoch.isdecimal() or int(epoch) <= 0:
        raise PackageContractError("FFI source date epoch is invalid")

    consumer = _read_toml(root / "radroots.lib.source-lock.v1.toml")
    _exact(consumer.get("repository"), LIB_REMOTE, "consumer Lib remote")
    _exact(consumer.get("revision"), lib_revision, "consumer Lib revision")
    _exact(consumer.get("version"), release_version, "consumer Lib version")

    package = _swift_package(root)
    _exact(package.get("name"), "radroots_ios_app", "Swift package name")
    _exact(package.get("defaultLocalization"), "en", "Swift localization")
    apple_revision = _apple_revision(package)
    project = parse_project_package(_read_text(root / "project.yml"), "RadrootsKit")
    if set(project) != {"url", "revision"}:
        raise PackageContractError("project AppleKit field inventory differs")
    _exact(project.get("url"), APPLE_KIT_REMOTE, "project AppleKit remote")
    _exact(project.get("revision"), apple_revision, "project AppleKit revision")

    _validate_privacy(_read_plist(root / "Radroots/Resources/PrivacyInfo.xcprivacy"))
    _validate_app_plist(_read_plist(root / "Radroots/Info.plist"))
    _validate_ui_test_plist(_read_plist(root / "RadrootsUITests/Info.plist"))

    base = parse_xcconfig_assignments(_read_text(root / "Radroots/Config/Base.xcconfig"))
    debug = parse_xcconfig_assignments(_read_text(root / "Radroots/Config/Debug.xcconfig"))
    if set(base) != {
        "RADROOTS_FIELD_IOS_RUNTIME_MODE",
        "RADROOTS_FIELD_IOS_NOSTR_RELAY_URLS",
        "RADROOTS_FIELD_IOS_BLOSSOM_ORIGINS",
        "RADROOTS_FIELD_IOS_KEYCHAIN_SERVICE_PREFIX",
    }:
        raise PackageContractError("base xcconfig field inventory differs")
    if set(debug) != {
        "RADROOTS_FIELD_IOS_RUNTIME_MODE",
        "PRODUCT_BUNDLE_IDENTIFIER",
        "RADROOTS_FIELD_IOS_NOSTR_RELAY_URLS",
        "RADROOTS_FIELD_IOS_BLOSSOM_ORIGINS",
        "RADROOTS_FIELD_IOS_KEYCHAIN_SERVICE_PREFIX",
    }:
        raise PackageContractError("debug xcconfig field inventory differs")
    _exact(
        base.get("RADROOTS_FIELD_IOS_NOSTR_RELAY_URLS"),
        "wss:$(SLASH)$(SLASH)radroots.org$(SLASH)",
        "base relay",
    )
    _exact(
        base.get("RADROOTS_FIELD_IOS_BLOSSOM_ORIGINS"),
        "https:$(SLASH)$(SLASH)blossom.radroots.org",
        "base Blossom origin",
    )
    _exact(
        debug.get("RADROOTS_FIELD_IOS_NOSTR_RELAY_URLS"),
        "ws:$(SLASH)$(SLASH)127.0.0.1:21000",
        "debug relay",
    )
    _exact(
        debug.get("RADROOTS_FIELD_IOS_BLOSSOM_ORIGINS"),
        "http:$(SLASH)$(SLASH)127.0.0.1:21100",
        "debug Blossom origin",
    )

    resolved_paths = (
        root / "Package.resolved",
        root / "Radroots.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved",
    )
    resolved = [_read_json(path) for path in resolved_paths]
    for document in resolved:
        validate_resolved(document, apple_revision)
    if resolved[0].get("pins") != resolved[1].get("pins"):
        raise PackageContractError("Swift and Xcode package locks disagree")

    verifier_project = _read_toml(root / "scripts/persona-verifier/pyproject.toml")
    verifier_lock = _read_toml(root / "scripts/persona-verifier/uv.lock")
    verifier_metadata = _mapping(verifier_project.get("project"), "verifier project")
    _exact(
        verifier_metadata.get("requires-python"),
        "==3.14.7",
        "verifier Python",
    )
    _exact(
        verifier_metadata.get("dependencies"),
        ["jsonschema==4.26.0"],
        "verifier dependencies",
    )
    _exact(verifier_lock.get("requires-python"), "==3.14.7", "verifier lock Python")
    package_rows = verifier_lock.get("package")
    if not isinstance(package_rows, list):
        raise PackageContractError("verifier lock package inventory is invalid")
    locked_packages = {
        item.get("name"): item.get("version")
        for item in package_rows
        if isinstance(item, dict)
    }
    _exact(locked_packages.get("jsonschema"), "4.26.0", "verifier jsonschema lock")

    required_files = (
        ".swiftformat",
        ".swiftlint.yml",
        "scripts/local-social-fixture.py",
        "scripts/swift-quality.sh",
        "scripts/linux-shared-rust.sh",
        "test-fixtures/bud11-upload-authorization-mutations.v1.json",
        "test-fixtures/bud11-upload-authorization-mutations.v1.schema.json",
        "test-fixtures/local-social-personas.v1.json",
        "test-fixtures/local-social-personas.v1.schema.json",
        "test-fixtures/local-social-persona-results.v1.schema.json",
        "test-fixtures/local-social-persona-attempt-evidence.v1.schema.json",
        "test-fixtures/local-social-persona-results.v2.schema.json",
    )
    for relative in required_files:
        _read_regular(root / relative)
    for relative in ("scripts/swift-quality.sh", "scripts/linux-shared-rust.sh"):
        if not os.access(root / relative, os.X_OK):
            raise PackageContractError("required package command is not executable")
    return release_version, apple_revision


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, required=True)
    arguments = parser.parse_args(argv)
    try:
        version, apple_revision = verify(arguments.repo_root)
    except PackageContractError as error:
        print(f"package_contract: {error}", file=sys.stderr)
        return 1
    print(f"package contracts agree at {version}; apple_kit@{apple_revision}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
