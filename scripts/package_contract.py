#!/usr/bin/env python3
"""Structured standalone package-contract verification for the iOS capsule."""

from __future__ import annotations

import argparse
import hashlib
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
    packages_line, packages_end = _project_package_bounds(lines)
    start = _project_package_start(lines, packages_line, packages_end, package_name)
    return _project_package_fields(lines[start + 1 :])


def _project_package_bounds(lines: list[str]) -> tuple[int, int]:
    start = next(
        (index for index, line in enumerate(lines) if line == "packages:"), None
    )
    if start is None:
        raise PackageContractError("project package inventory is absent")
    end = next(
        (
            index
            for index in range(start + 1, len(lines))
            if lines[index] and not lines[index].startswith((" ", "#"))
        ),
        len(lines),
    )
    return start, end


def _project_package_start(
    lines: list[str], start: int, end: int, package_name: str
) -> int:
    expected = f"  {package_name}:"
    result = next(
        (index for index in range(start + 1, end) if lines[index] == expected), None
    )
    if result is None:
        raise PackageContractError("project package entry is absent")
    return result


def _project_package_fields(lines: list[str]) -> dict[str, str]:
    values: dict[str, str] = {}
    for line in lines:
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
        raise PackageContractError(
            "Swift package manifest output is malformed"
        ) from error
    if not isinstance(value, dict):
        raise PackageContractError("Swift package manifest output is not an object")
    return value


def _apple_revision(package: dict[str, Any]) -> str:
    dependencies = package.get("dependencies")
    if not isinstance(dependencies, list):
        raise PackageContractError("Swift package dependencies are absent")
    matches: list[str] = []
    for dependency in dependencies:
        candidate = _apple_dependency_revision(dependency)
        if candidate is not None:
            matches.append(candidate)
    if (
        len(matches) != 1
        or not isinstance(matches[0], str)
        or GIT_REVISION.fullmatch(matches[0]) is None
    ):
        raise PackageContractError("AppleKit dependency is not one exact revision")
    return matches[0]


def _apple_dependency_revision(dependency: object) -> object | None:
    item = _mapping(dependency, "Swift package dependency")
    source = item.get("sourceControl")
    if not isinstance(source, list) or len(source) != 1:
        return None
    identity = _mapping(source[0], "Swift package source")
    remote = identity.get("location")
    requirement = identity.get("requirement")
    remote_values = remote.get("remote") if isinstance(remote, dict) else None
    if remote_values != [{"urlString": APPLE_KIT_REMOTE}]:
        return None
    if not isinstance(requirement, dict):
        return None
    revisions = requirement.get("revision")
    if not isinstance(revisions, list) or len(revisions) != 1:
        return None
    return revisions[0]


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
        "TERA_IOS_UI_TEST_FIXTURE_CONTROL",
        "TERA_IOS_UI_TEST_FIXTURE_EVIDENCE",
        "TERA_IOS_UI_TEST_NETWORK_PROFILE",
        "TERA_IOS_UI_TEST_SOURCE_COMMIT",
        "TERA_IOS_UI_TEST_SOURCE_TREE",
        "TERA_IOS_UI_TEST_APP_BUILD_SHA256",
        "TERA_IOS_UI_TEST_SIMULATOR_ID",
    }
    if required.difference(document):
        raise PackageContractError("UI test plist inventory is incomplete")


def _verify_repository_layout(root: Path) -> None:
    for forbidden in ("docs", ".github", ".act"):
        path = root / forbidden
        if path.exists() or path.is_symlink():
            raise PackageContractError("forbidden public repository root exists")


def _cargo_workspace(root: Path) -> dict[str, Any]:
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        try:
            result = subprocess.run(
                [
                    "cargo",
                    "metadata",
                    "--manifest-path",
                    str(root / "Cargo.toml"),
                    "--locked",
                    "--no-deps",
                    "--format-version",
                    "1",
                ],
                check=False,
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=stderr,
                timeout=60,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise PackageContractError("Cargo workspace cannot be evaluated") from error
        stdout.seek(0)
        output = stdout.read(MAX_CONTRACT_BYTES + 1)
    if result.returncode != 0 or len(output) > MAX_CONTRACT_BYTES:
        raise PackageContractError("Cargo workspace evaluation failed")
    try:
        value = json.loads(output)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PackageContractError("Cargo workspace output is malformed") from error
    if not isinstance(value, dict):
        raise PackageContractError("Cargo workspace output is not an object")
    return value


def _local_cargo_path(value: object, root: Path) -> Path:
    if not isinstance(value, str) or not value:
        raise PackageContractError("local Cargo path is invalid")
    path = Path(value).resolve()
    if not path.is_relative_to(root.resolve()):
        raise PackageContractError("local Cargo path escapes the standalone repository")
    return path.relative_to(root.resolve())


def _validate_app_workspace(document: dict[str, Any], root: Path) -> None:
    _exact(document.get("workspace_root"), str(root), "Cargo workspace root")
    packages = document.get("packages")
    members = document.get("workspace_members")
    if not isinstance(packages, list) or not packages or not isinstance(members, list):
        raise PackageContractError("Cargo workspace members are absent")
    identifiers = [
        package.get("id") for package in packages if isinstance(package, dict)
    ]
    if not all(isinstance(item, str) for item in identifiers + members):
        raise PackageContractError("Cargo workspace member identity is invalid")
    _exact(sorted(identifiers), sorted(members), "Cargo workspace member inventory")
    for package in packages:
        _validate_app_package(_mapping(package, "Cargo package"), root)
    _validate_mobile_defaults(document, packages, root)


def _validate_mobile_defaults(
    document: Mapping[str, Any], packages: list[Any], root: Path
) -> None:
    owned = {
        _local_cargo_path(package.get("manifest_path"), root).parent.name: package["id"]
        for package in packages
    }
    if "tera_wasm" not in owned:
        return
    defaults = document.get("workspace_default_members")
    if not isinstance(defaults, list) or owned["tera_wasm"] in defaults:
        raise PackageContractError("non-default WASM package entered mobile defaults")
    for name in ("tera_core", "tera_ffi"):
        if name in owned and owned[name] not in defaults:
            raise PackageContractError("owned runtime is missing from mobile defaults")


def _validate_app_package(package: Mapping[str, Any], root: Path) -> None:
    manifest = _local_cargo_path(package.get("manifest_path"), root)
    directory = manifest.parent
    if directory != Path("crates/source_lock") and not directory.is_relative_to(
        "core/crates"
    ):
        raise PackageContractError("application Rust package is outside its owned root")
    dependencies = package.get("dependencies")
    if not isinstance(dependencies, list):
        raise PackageContractError("Cargo dependency inventory is absent")
    for dependency in dependencies:
        item = _mapping(dependency, "Cargo dependency")
        if "path" in item:
            _local_cargo_path(item["path"], root)


def _verify_cargo_and_source(root: Path) -> tuple[str, str]:
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

    consumer = _read_toml(root / "radroots.lib.source-lock.v1.toml")
    _exact(consumer.get("repository"), LIB_REMOTE, "consumer Lib remote")
    lib_revision = consumer.get("revision")
    if (
        not isinstance(lib_revision, str)
        or GIT_REVISION.fullmatch(lib_revision) is None
    ):
        raise PackageContractError("consumer Lib revision is invalid")
    release_version = consumer.get("version")
    _exact(release_version, "0.1.0-alpha", "consumer Lib version")
    _exact(
        ffi_dependency,
        {"git": LIB_REMOTE, "rev": lib_revision, "version": "=0.1.0-alpha"},
        "transition shim FFI dependency",
    )
    _verify_owned_source_lock(root, consumer)
    return release_version, lib_revision


def _verify_owned_source_lock(root: Path, foundation: dict[str, Any]) -> None:
    lock = _read_toml(root / "TeraFFI/source.lock")
    _exact(
        set(lock),
        {
            "schema",
            "repository",
            "source_tree",
            "manifest_sha256",
            "source_date_epoch",
            "foundation",
        },
        "installed source fields",
    )
    _exact(lock["schema"], "tera.installed-source.v1", "installed source schema")
    _exact(
        lock["repository"], "https://github.com/radrootslabs/tera", "installed producer"
    )
    _exact(
        lock["foundation"],
        {key: foundation[key] for key in ("repository", "revision", "version")},
        "installed foundation",
    )
    if (
        not isinstance(lock["source_tree"], str)
        or GIT_REVISION.fullmatch(lock["source_tree"]) is None
    ):
        raise PackageContractError("installed source tree is invalid")
    _exact(lock["source_date_epoch"], 1787871027, "installed source epoch")
    _exact(
        lock["manifest_sha256"],
        hashlib.sha256(_read_regular(root / "TeraFFI/provenance.json")).hexdigest(),
        "installed manifest digest",
    )


def _verify_apple_dependencies(root: Path) -> str:
    package = _swift_package(root)
    _exact(package.get("name"), "tera", "Swift package name")
    _exact(package.get("defaultLocalization"), "en", "Swift localization")
    apple_revision = _apple_revision(package)
    project = parse_project_package(_read_text(root / "project.yml"), "RadrootsKit")
    if set(project) != {"url", "revision"}:
        raise PackageContractError("project AppleKit field inventory differs")
    _exact(project.get("url"), APPLE_KIT_REMOTE, "project AppleKit remote")
    _exact(project.get("revision"), apple_revision, "project AppleKit revision")
    return apple_revision


def _verify_apple_configuration(root: Path) -> None:
    _validate_privacy(_read_plist(root / "Tera/Resources/PrivacyInfo.xcprivacy"))
    _validate_app_plist(_read_plist(root / "Tera/Info.plist"))
    _validate_ui_test_plist(_read_plist(root / "TeraUITests/Info.plist"))

    base = parse_xcconfig_assignments(_read_text(root / "Tera/Config/Base.xcconfig"))
    debug = parse_xcconfig_assignments(_read_text(root / "Tera/Config/Debug.xcconfig"))
    if set(base) != {
        "TERA_IOS_RUNTIME_MODE",
        "TERA_IOS_NOSTR_RELAY_URLS",
        "TERA_IOS_BLOSSOM_ORIGINS",
        "TERA_IOS_KEYCHAIN_SERVICE_PREFIX",
    }:
        raise PackageContractError("base xcconfig field inventory differs")
    if set(debug) != {
        "TERA_IOS_RUNTIME_MODE",
        "PRODUCT_BUNDLE_IDENTIFIER",
        "TERA_IOS_NOSTR_RELAY_URLS",
        "TERA_IOS_BLOSSOM_ORIGINS",
        "TERA_IOS_KEYCHAIN_SERVICE_PREFIX",
    }:
        raise PackageContractError("debug xcconfig field inventory differs")
    _exact(
        base.get("TERA_IOS_NOSTR_RELAY_URLS"),
        "wss:$(SLASH)$(SLASH)radroots.org$(SLASH)",
        "base relay",
    )
    _exact(
        base.get("TERA_IOS_BLOSSOM_ORIGINS"),
        "https:$(SLASH)$(SLASH)blossom.radroots.org",
        "base Blossom origin",
    )
    _exact(
        debug.get("TERA_IOS_NOSTR_RELAY_URLS"),
        "ws:$(SLASH)$(SLASH)127.0.0.1:21000",
        "debug relay",
    )
    _exact(
        debug.get("TERA_IOS_BLOSSOM_ORIGINS"),
        "http:$(SLASH)$(SLASH)127.0.0.1:21100",
        "debug Blossom origin",
    )
    _verify_installation_compatibility(root, base, debug)


def _verify_installation_compatibility(
    root: Path, base: dict[str, str], debug: dict[str, str]
) -> None:
    baseline = _read_json(root / "test-fixtures/tera-compatibility.v1.json")
    _exact(
        baseline.get("schema"), "tera.compatibility-baseline.v1", "compatibility schema"
    )
    production = parse_xcconfig_assignments(_read_text(root / "Tera/tera.xcconfig"))
    actual = {
        "production_bundle_identifier": production.get("PRODUCT_BUNDLE_IDENTIFIER"),
        "debug_bundle_identifier": debug.get("PRODUCT_BUNDLE_IDENTIFIER"),
        "production_keychain_service_prefix": base.get(
            "TERA_IOS_KEYCHAIN_SERVICE_PREFIX"
        ),
        "debug_keychain_service_prefix": debug.get("TERA_IOS_KEYCHAIN_SERVICE_PREFIX"),
    }
    _exact(actual, baseline.get("installation"), "installed identity compatibility")


def _verify_package_locks(root: Path, apple_revision: str) -> None:
    resolved_paths = (
        root / "Package.resolved",
        root
        / "Tera.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved",
    )
    resolved = [_read_json(path) for path in resolved_paths]
    for document in resolved:
        validate_resolved(document, apple_revision)
    if resolved[0].get("pins") != resolved[1].get("pins"):
        raise PackageContractError("Swift and Xcode package locks disagree")


def _verify_persona_toolchain(root: Path) -> None:
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
    dependency_groups = _mapping(
        verifier_project.get("dependency-groups"), "verifier dependency groups"
    )
    _exact(dependency_groups, {"dev": ["ruff==0.12.12"]}, "verifier dev tools")
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
    _exact(locked_packages.get("ruff"), "0.12.12", "verifier ruff lock")


def _verify_required_files(root: Path) -> None:
    required_files = (
        ".swiftformat",
        ".swiftlint.yml",
        "scripts/maintainability_ratchet.py",
        "scripts/local-social-fixture.py",
        "scripts/swift-quality.sh",
        "scripts/linux-shared-rust.sh",
        "test-fixtures/maintainability-baseline.v1.json",
        "test-fixtures/swiftlint-maintainability-baseline.v1.json",
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


def verify(repo_root: Path) -> tuple[str, str]:
    root = repo_root.resolve()
    _verify_repository_layout(root)
    _validate_app_workspace(_cargo_workspace(root), root)
    release_version, _ = _verify_cargo_and_source(root)
    apple_revision = _verify_apple_dependencies(root)
    _verify_apple_configuration(root)
    _verify_package_locks(root, apple_revision)
    _verify_persona_toolchain(root)
    _verify_required_files(root)
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
