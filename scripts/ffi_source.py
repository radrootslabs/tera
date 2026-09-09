"""Bounded source capture for the application-owned native producer."""

from __future__ import annotations

import hashlib
import os
import re
import stat
import subprocess
from pathlib import Path
from typing import Any

import package_contract as contract

MAX_BYTES = 2 * 1024 * 1024
MAX_INPUTS = 2048
INPUTS = [
    "test-fixtures/legacy-identifiers.v1.json",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "Makefile",
    "core",
    "scripts",
    "TeraFFI/Makefile",
    "TeraFFI/scripts",
    "TeraFFI/producer.toml",
    "radroots.lib.source-lock.v1.toml",
]
PRODUCER_PATH = "TeraFFI/producer.toml"


class ProvenanceError(Exception):
    """A source-free, fail-closed provenance rejection."""


def command(root: Path, argv: list[str], data: bytes | None = None) -> bytes:
    if argv[0] == "git":
        argv = ["git", "--no-replace-objects", *argv[1:]]
    try:
        result = subprocess.run(
            argv, cwd=root, input=data, capture_output=True, check=False, timeout=120
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ProvenanceError("producer inspection command unavailable") from error
    if result.returncode or len(result.stdout) > MAX_BYTES:
        raise ProvenanceError("producer inspection command failed or exceeded bound")
    return result.stdout


def read_source(root: Path, relative: str) -> bytes:
    path = Path(relative)
    if path.is_absolute() or str(path) != relative or ".." in path.parts:
        raise ProvenanceError("producer input path is invalid")
    current = root
    for part in path.parts:
        current = current / part
        if current.is_symlink():
            raise ProvenanceError("producer input contains a symlink")
    return contract._read_regular(current, maximum=MAX_BYTES)


def producer_contract(root: Path) -> dict[str, Any]:
    value = contract._read_toml(root / PRODUCER_PATH)
    expected = {
        "schema",
        "repository",
        "foundation_lock",
        "source_inputs",
        "ffi",
        "generator",
        "build",
    }
    contract._exact(set(value), expected, "producer contract fields")
    contract._exact(value["schema"], "tera.native-producer.v1", "producer schema")
    contract._exact(
        value["repository"],
        "https://github.com/radrootslabs/tera",
        "producer repository",
    )
    contract._exact(value["source_inputs"], INPUTS, "producer input inventory")
    contract._exact(
        value["foundation_lock"],
        "radroots.lib.source-lock.v1.toml",
        "foundation lock path",
    )
    contract._exact(
        value["ffi"],
        {
            "package": "tera_ffi",
            "default_features": True,
            "features": [],
            "config": "core/crates/tera_ffi/uniffi.toml",
        },
        "FFI producer selection",
    )
    contract._exact(
        value["generator"],
        {
            "package": "tera_bindgen",
            "default_features": True,
            "features": [],
        },
        "generator selection",
    )
    validate_build(value["build"])
    cargo = contract._read_toml(root / "Cargo.toml")
    lock = contract._read_toml(root / value["foundation_lock"])
    validate_foundation(cargo, lock)
    toolchain = contract._read_toml(root / "rust-toolchain.toml")
    contract._exact(
        toolchain,
        {
            "toolchain": {
                "channel": value["build"]["rust_version"],
                "profile": "minimal",
                "components": ["llvm-tools"],
            }
        },
        "producer toolchain",
    )
    return value


def validate_build(value: Any) -> None:
    value = contract._mapping(value, "producer build")
    contract._exact(
        set(value),
        {
            "rust_version",
            "profile",
            "ios_deployment_target",
            "rust_flags",
            "source_date_epoch",
            "host",
            "host_dylib_install_name",
            "host_linker_reproducible",
            "host_oso_prefix",
            "targets",
        },
        "producer build fields",
    )
    contract._exact(value["rust_version"], "1.97.1", "producer Rust version")
    contract._exact(value["profile"], "release", "producer build profile")
    contract._exact(
        value["ios_deployment_target"], "18.0", "producer deployment target"
    )
    contract._exact(value["host"], "aarch64-apple-darwin", "producer host")
    contract._exact(
        value["host_dylib_install_name"],
        "@rpath/libtera_ffi.dylib",
        "producer host dylib install name",
    )
    contract._exact(
        value["host_linker_reproducible"] is True,
        True,
        "producer host linker reproducibility",
    )
    contract._exact(
        value["host_oso_prefix"], "{extbuild_root}", "producer host debug-map prefix"
    )
    contract._exact(
        value["rust_flags"],
        [
            "--remap-path-prefix={producer_root}=/tera",
            "--remap-path-prefix={cargo_home}=/cargo",
            "--remap-path-prefix={extbuild_root}=/build",
        ],
        "producer Rust flags",
    )
    contract._exact(
        value["targets"],
        [
            "aarch64-apple-ios",
            "aarch64-apple-ios-sim",
            "aarch64-apple-darwin",
        ],
        "producer targets",
    )
    if type(value["source_date_epoch"]) is not int or value["source_date_epoch"] <= 0:
        raise ProvenanceError("producer source epoch is invalid")


def library_rust_flags(build: dict[str, Any], target: str) -> list[str]:
    if target == build["host"]:
        return [
            f"-Clink-arg=-Wl,-install_name,{build['host_dylib_install_name']}",
            "-Clink-arg=-Wl,-reproducible",
            f"-Clink-arg=-Wl,-oso_prefix,{build['host_oso_prefix']}",
        ]
    return []


def validate_foundation(cargo: dict[str, Any], lock: dict[str, Any]) -> None:
    contract._exact(
        lock.get("schema"), "radroots.lib.source-lock.v1", "foundation schema"
    )
    contract._exact(lock.get("repository"), contract.LIB_REMOTE, "foundation remote")
    if not contract.GIT_REVISION.fullmatch(str(lock.get("revision", ""))):
        raise ProvenanceError("foundation revision is invalid")
    for field in (
        "workspace_catalog_sha256",
        "source_archive_sha256",
        "lockfile_sha256",
    ):
        if not contract.SHA256.fullmatch(str(lock.get(field, ""))):
            raise ProvenanceError("foundation digest is invalid")
    dependencies = cargo["workspace"]["dependencies"]
    selected = {
        name: value
        for name, value in dependencies.items()
        if name.startswith("radroots_")
    }
    if not selected:
        raise ProvenanceError("foundation dependency inventory is empty")
    for value in selected.values():
        value = contract._mapping(value, "foundation Cargo dependency")
        contract._exact(value.get("git"), lock["repository"], "foundation Cargo remote")
        contract._exact(value.get("rev"), lock["revision"], "foundation Cargo revision")
        contract._exact(
            value.get("version"), "=" + lock["version"], "foundation Cargo version"
        )


def source_snapshot(root: Path, inputs: list[str]) -> dict[str, Any]:
    actual_root = (
        command(root, ["git", "rev-parse", "--show-toplevel"]).decode().strip()
    )
    if actual_root != str(root):
        raise ProvenanceError("producer source must use its own repository root")
    if command(
        root, ["git", "ls-files", "--others", "--exclude-standard", "-z", "--", *inputs]
    ):
        raise ProvenanceError("producer inputs contain untracked source")
    reject_ignored_source(root, inputs)
    raw = command(root, ["git", "ls-files", "--stage", "-z", "--", *inputs])
    rows = raw.rstrip(b"\0").split(b"\0") if raw else []
    if not rows or len(rows) > MAX_INPUTS:
        raise ProvenanceError("producer input inventory exceeds bounds")
    files: dict[str, Any] = {}
    tree: dict[str, Any] = {}
    for row in rows:
        relative, entry = staged_file(root, row)
        files[relative] = entry
        insert_tree(tree, relative, entry["mode"], entry["git_blob"])
    require_input_inventory(inputs, files)
    return {"policy": "staged_inputs", "tree": tree_identity(tree), "files": files}


def require_input_inventory(inputs: list[str], files: dict[str, Any]) -> None:
    for relative in inputs:
        if not any(
            name == relative or name.startswith(relative + "/") for name in files
        ):
            raise ProvenanceError("required producer input is missing from index")


def reject_ignored_source(root: Path, inputs: list[str]) -> None:
    ignored = command(
        root,
        [
            "git",
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
            "--",
            *inputs,
            ":(exclude)scripts/persona-verifier/.venv/**",
            ":(exclude,glob)**/__pycache__/**",
        ],
    )
    if any(is_input(path, inputs) for path in ignored.decode().split("\0") if path):
        raise ProvenanceError(
            "producer inputs contain ignored source outside tool caches"
        )


def is_input(relative: str, inputs: list[str]) -> bool:
    return any(relative == name or relative.startswith(name + "/") for name in inputs)


def staged_file(root: Path, row: bytes) -> tuple[str, dict[str, Any]]:
    header, encoded_path = row.split(b"\t", 1)
    mode, oid, stage = header.decode("ascii").split()
    relative = encoded_path.decode("utf-8")
    if stage != "0" or mode not in ("100644", "100755"):
        raise ProvenanceError("producer input has unresolved or unsupported mode")
    data = read_source(root, relative)
    blob = command(root, ["git", "cat-file", "blob", oid])
    actual_mode = (
        "100755" if (root / relative).stat().st_mode & stat.S_IXUSR else "100644"
    )
    if data != blob or mode != actual_mode:
        raise ProvenanceError("producer worktree differs from staged source")
    return relative, {
        "mode": mode,
        "git_blob": oid,
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def reject_cargo_configuration(root: Path) -> None:
    locations = [path / ".cargo" for path in (root, *root.parents)]
    locations.append(Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))))
    for location in locations:
        for name in ("config", "config.toml"):
            path = location / name
            if path.exists():
                value = contract._read_toml(path)
                if {"build", "target", "env", "profile", "patch", "unstable"} & set(
                    value
                ):
                    raise ProvenanceError(
                        "ungoverned Cargo configuration affects producer build"
                    )


def insert_tree(tree: dict[str, Any], relative: str, mode: str, oid: str) -> None:
    parts = relative.split("/")
    for part in parts[:-1]:
        tree = tree.setdefault(part, {})
    tree[parts[-1]] = (mode, oid)


def tree_identity(tree: dict[str, Any]) -> str:
    rows = []
    for name, value in tree.items():
        if isinstance(value, dict):
            mode, oid, key = "40000", tree_identity(value), name.encode() + b"/"
        else:
            mode, oid = value
            key = name.encode()
        rows.append((key, f"{mode} {name}".encode() + b"\0" + bytes.fromhex(oid)))
    body = b"".join(row for _, row in sorted(rows))
    return hashlib.sha1(
        b"tree " + str(len(body)).encode() + b"\0" + body, usedforsecurity=False
    ).hexdigest()


def reject_build_overrides() -> None:
    forbidden = (
        "RUSTFLAGS",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_ENCODED_RUSTFLAGS",
        "GIT_INDEX_FILE",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    )
    if any(os.environ.get(name) for name in forbidden):
        raise ProvenanceError("ungoverned Rust build override is active")
    if any(
        re.fullmatch(
            r"CARGO_(BUILD_.*|PROFILE_.*|TARGET_.*_(RUSTFLAGS|LINKER|RUNNER))", name
        )
        and name not in allowed_profile_overrides()
        for name in os.environ
    ):
        raise ProvenanceError("ungoverned Cargo build override is active")


def allowed_profile_overrides() -> dict[str, str]:
    # Extbuild's development debug policy affects the generator, not release libraries.
    name = "CARGO_PROFILE_DEV_DEBUG"
    if (
        os.environ.get("EXT_BUILD_RUN_ACTIVE")
        and os.environ.get(name) == "line-tables-only"
    ):
        return {name: "line-tables-only"}
    return {}


def feature_graph(root: Path, package: str, target: str) -> list[str]:
    raw = command(
        root,
        [
            "cargo",
            "tree",
            "--locked",
            "--offline",
            "-p",
            package,
            "--target",
            target,
            "--edges",
            "normal,build",
            "--prefix",
            "none",
            "--format",
            "{p}|{f}",
        ],
    ).decode()
    return sorted(set(raw.replace(str(root), "<producer-root>").splitlines()))
