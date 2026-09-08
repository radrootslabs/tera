"""Generate or check staged producer evidence, separate from installed artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tempfile
from pathlib import Path
from typing import Any

import ffi_source as source
import package_contract as contract


def capture(root: Path, target: str) -> dict[str, Any]:
    source.reject_build_overrides()
    source.reject_cargo_configuration(root)
    config = source.producer_contract(root)
    build = config["build"]
    if target not in build["targets"]:
        raise source.ProvenanceError("producer target is not governed")
    snapshot = source.source_snapshot(root, config["source_inputs"])
    rustc = source.command(root, ["rustc", "-Vv"]).decode().strip()
    if f"release: {build['rust_version']}\n" not in rustc + "\n":
        raise source.ProvenanceError(
            "active Rust compiler differs from producer contract"
        )
    if f"host: {build['host']}\n" not in rustc + "\n":
        raise source.ProvenanceError("active Rust host differs from producer contract")
    result = {
        "schema": "tera.producer-source.v1",
        "repository": config["repository"],
        "source": snapshot,
        "foundation": contract._read_toml(root / config["foundation_lock"]),
        "cargo_lock_sha256": hashlib.sha256(
            source.read_source(root, "Cargo.lock")
        ).hexdigest(),
        "build": {
            **config["ffi"],
            "target": target,
            "profile": build["profile"],
            "ios_deployment_target": build["ios_deployment_target"],
            "rust_flags": build["rust_flags"],
            "source_date_epoch": build["source_date_epoch"],
            "rustc": rustc,
            "symbol_reader": source.command(
                root, [str(symbol_reader(root, build["host"])), "--version"]
            )
            .decode()
            .strip(),
            "apple_toolchain": apple_toolchain(root),
            "feature_graph": source.feature_graph(
                root, config["ffi"]["package"], target
            ),
        },
        "generator": {
            **config["generator"],
            "target": build["host"],
            "profile": "dev",
            "profile_overrides": source.allowed_profile_overrides(),
            "feature_graph": source.feature_graph(
                root, config["generator"]["package"], build["host"]
            ),
        },
        "disposition": "local_staged_source_only",
    }
    if source.source_snapshot(root, config["source_inputs"]) != snapshot:
        raise source.ProvenanceError("producer source changed during capture")
    return result


def symbol_reader(root: Path, host: str) -> Path:
    sysroot = Path(
        source.command(root, ["rustc", "--print", "sysroot"]).decode().strip()
    )
    reader = sysroot / "lib/rustlib" / host / "bin/llvm-nm"
    if not reader.is_file() or not os.access(reader, os.X_OK):
        raise source.ProvenanceError("Rust toolchain llvm-tools component is required")
    return reader


def apple_toolchain(root: Path) -> dict[str, str]:
    commands = {
        "xcode": ["xcodebuild", "-version"],
        "swift": ["xcrun", "swiftc", "--version"],
        "swiftformat": ["swiftformat", "--version"],
        "iphoneos_sdk": ["xcrun", "--sdk", "iphoneos", "--show-sdk-build-version"],
        "iphonesimulator_sdk": [
            "xcrun",
            "--sdk",
            "iphonesimulator",
            "--show-sdk-build-version",
        ],
    }
    return {
        name: source.command(root, argv).decode().strip()
        for name, argv in commands.items()
    }


def encoded(value: dict[str, Any]) -> bytes:
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()


def verify_record(actual: bytes, expected: dict[str, Any]) -> None:
    if actual != encoded(expected):
        raise source.ProvenanceError(
            "producer provenance differs from the exact source/build tuple"
        )


def output_path(root: Path, target: str, requested: str | None) -> Path:
    external = os.environ.get("EXT_BUILD_PROJECT_DIR")
    if not external or not os.environ.get("EXT_BUILD_RUN_ACTIVE"):
        raise source.ProvenanceError("producer provenance requires extbuild")
    base = Path(external).resolve()
    path = (
        Path(requested)
        if requested
        else base / "target/tera_ffi/source" / f"{target}.json"
    )
    path = path.absolute()
    if not path.is_relative_to(base) or path.is_relative_to(root):
        raise source.ProvenanceError("producer evidence must use external build output")
    if path.resolve() != path or path.is_symlink():
        raise source.ProvenanceError("producer evidence output contains a symlink")
    return path


def write_atomic(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: str | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as handle:
            temporary = handle.name
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        temporary = None
    finally:
        if temporary:
            Path(temporary).unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("contract-check", "write", "check"))
    parser.add_argument("--target")
    parser.add_argument(
        "--repo-root", type=Path, default=Path(__file__).resolve().parent.parent
    )
    parser.add_argument("--output")
    args = parser.parse_args()
    try:
        root = args.repo_root.resolve()
        if args.mode == "contract-check":
            source.producer_contract(root)
            print(
                "FFI producer contract verified; installed artifacts remain separately governed"
            )
            return 0
        if not args.target:
            raise source.ProvenanceError("producer target is required")
        path = output_path(root, args.target, args.output)
        record = capture(root, args.target)
        if args.mode == "write":
            write_atomic(path, encoded(record))
        verify_record(contract._read_regular(path), record)
    except (
        source.ProvenanceError,
        contract.PackageContractError,
        OSError,
        ValueError,
        KeyError,
    ) as error:
        print(f"FFI producer provenance: {error}", file=sys.stderr)
        return 1
    print(
        f"FFI producer source {args.mode}: {args.target}; tree={record['source']['tree']}; local staged source only"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
