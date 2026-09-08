"""Install an owned candidate and verify native inputs without building them."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Any

import ffi_artifacts as artifacts
import ffi_provenance as provenance
import ffi_source as source
import package_contract as contract

MANIFEST = "TeraFFI/provenance.json"
LOCK = "TeraFFI/source.lock"


def installed_paths() -> dict[str, str]:
    result = {}
    for relative in sorted(artifacts.expected_paths()):
        if relative.startswith(artifacts.FRAMEWORK + "/"):
            result[relative] = "Tera/Frameworks/" + relative
        elif relative == "generated/TeraKitBindings.swift":
            result[relative] = "Tera/Generated/TeraKitBindings.swift"
        elif relative.startswith(("api/", "source/")) or relative == "abi_symbols.json":
            result[relative] = "TeraFFI/" + relative
    return result


def installed_manifest(candidate: dict[str, Any]) -> dict[str, Any]:
    paths = installed_paths()
    files = [
        {**item, "path": paths[item["path"]]}
        for item in candidate["files"]
        if item["path"] in paths
    ]
    return {
        "schema": "tera.installed-native-artifacts.v1",
        "candidate": candidate,
        "files": sorted(files, key=lambda item: item["path"]),
        "disposition": "local_installed_not_release_qualified",
    }


def encoded_lock(
    manifest: dict[str, Any], foundation: dict[str, Any], epoch: int
) -> bytes:
    producer = manifest["candidate"]["source"]
    values = {
        "schema": "tera.installed-source.v1",
        "repository": producer["repository"],
        "source_tree": producer["tree"],
        "manifest_sha256": hashlib.sha256(provenance.encoded(manifest)).hexdigest(),
        "source_date_epoch": epoch,
    }
    text = "".join(f"{key} = {json.dumps(value)}\n" for key, value in values.items())
    text += "\n[foundation]\n"
    text += "".join(
        f"{key} = {json.dumps(foundation[key])}\n"
        for key in ("repository", "revision", "version")
    )
    return text.encode()


def verify_files(root: Path, manifest: dict[str, Any]) -> None:
    expected = installed_manifest(manifest["candidate"])
    if manifest != expected:
        raise source.ProvenanceError("installed artifact manifest differs")
    paths = set(installed_paths().values())
    if {item["path"] for item in manifest["files"]} != paths:
        raise source.ProvenanceError("installed artifact inventory differs")
    for item in manifest["files"]:
        if artifacts.file_record(root, item["path"]) != item:
            raise source.ProvenanceError("installed artifact bytes are stale")
    verify_directory_inventory(root, paths)


def verify_directory_inventory(root: Path, paths: set[str]) -> None:
    for relative in (
        "Tera/Generated",
        "Tera/Frameworks/TeraFFI.xcframework",
        "TeraFFI/api",
        "TeraFFI/source",
    ):
        observed = set()
        for path in (root / relative).rglob("*"):
            if path.is_symlink():
                raise source.ProvenanceError(
                    "installed artifact inventory contains a symlink"
                )
            if path.is_file():
                observed.add(path.relative_to(root).as_posix())
        if observed != {path for path in paths if path.startswith(relative + "/")}:
            raise source.ProvenanceError("installed artifact inventory differs")


def check(root: Path) -> dict[str, Any]:
    manifest = contract._read_json(root / MANIFEST)
    verify_files(root, manifest)
    config = source.producer_contract(root)
    snapshot = source.source_snapshot(root, config["source_inputs"])
    foundation = contract._read_toml(root / config["foundation_lock"])
    candidate = manifest["candidate"]
    if candidate["source"] != {
        "repository": config["repository"],
        "tree": snapshot["tree"],
    }:
        raise source.ProvenanceError("installed producer source is stale")
    for target in artifacts.TARGETS:
        record = contract._read_json(root / "TeraFFI/source" / f"{target}.json")
        if (
            record["source"] != snapshot
            or record["foundation"] != foundation
            or record["build"]["target"] != target
        ):
            raise source.ProvenanceError("installed producer tuple differs")
    if contract._read_regular(root / LOCK) != encoded_lock(
        manifest, foundation, config["build"]["source_date_epoch"]
    ):
        raise source.ProvenanceError("installed producer lock is stale")
    return manifest


def install(
    root: Path, candidate_root: Path, records: dict[str, dict[str, Any]]
) -> None:
    if not os.environ.get("EXT_BUILD_RUN_ACTIVE"):
        raise source.ProvenanceError("native installation requires extbuild")
    local_destination(root, "TeraFFI")
    candidate = artifacts.check(candidate_root, records)
    manifest = installed_manifest(candidate)
    config = source.producer_contract(root)
    foundation = contract._read_toml(root / config["foundation_lock"])
    # Final-directory swaps stay on the repository filesystem. A failed install
    # restores every prior directory/file, and publishes its manifest last.
    with tempfile.TemporaryDirectory(
        prefix=".install-", dir=root / "TeraFFI"
    ) as temporary:
        staging = Path(temporary) / "next"
        for relative, destination in installed_paths().items():
            path = staging / destination
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(candidate_root / relative, path)
        (staging / MANIFEST).write_bytes(provenance.encoded(manifest))
        (staging / LOCK).write_bytes(
            encoded_lock(manifest, foundation, config["build"]["source_date_epoch"])
        )
        verify_files(staging, manifest)
        replace_installation(root, staging, Path(temporary) / "previous")
    print(
        "owned native candidate installed and source-verified; not release qualification"
    )


def local_destination(root: Path, relative: str) -> Path:
    current = root
    for part in Path(relative).parts:
        current = current / part
        if current.is_symlink():
            raise source.ProvenanceError("native installation path contains a symlink")
    return current


def replace_installation(root: Path, staging: Path, previous: Path) -> None:
    paths = (
        "Tera/Generated",
        "Tera/Frameworks/TeraFFI.xcframework",
        "TeraFFI/api",
        "TeraFFI/source",
        "TeraFFI/abi_symbols.json",
        LOCK,
        MANIFEST,
    )
    destinations = {relative: local_destination(root, relative) for relative in paths}
    changed = []
    try:
        for relative in paths:
            destination = destinations[relative]
            backup = previous / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            existed = destination.exists()
            if existed:
                backup.parent.mkdir(parents=True, exist_ok=True)
                destination.rename(backup)
            changed.append((relative, existed))
            (staging / relative).rename(destination)
        check(root)
    except Exception:
        for relative, existed in reversed(changed):
            destination = root / relative
            if destination.is_dir():
                shutil.rmtree(destination)
            else:
                destination.unlink(missing_ok=True)
            if existed:
                (previous / relative).rename(destination)
        raise


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    try:
        check(root)
    except (
        source.ProvenanceError,
        contract.PackageContractError,
        OSError,
        ValueError,
        KeyError,
        TypeError,
    ) as error:
        print(
            f"installed native artifacts: {error}; run make ffi-bootstrap",
            file=sys.stderr,
        )
        return 1
    print(
        "installed native artifacts match the owned source tree and exact foundation lock"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
