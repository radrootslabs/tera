"""Complete, bounded redistribution material from the exact Cargo provider."""

from __future__ import annotations

import hashlib
import os
import re
import stat
from pathlib import Path
from typing import Any

import ffi_source as source
import package_contract as contract

ARTIFACT_PATH = "redistribution/shared_data_NOTICE.txt"
INSTALLED_PATH = "Tera/Resources/SharedDataNOTICE.txt"
MAX_BYTES = 65_536
CONTRACT = {
    "package": "radroots_domain",
    "version": "0.1.0-alpha",
    "source_file": "NOTICE",
    "sha256": "4376b5764456a4fa5f74fd712390adeb897d1ccdd62d5f818b2bcd767003760a",
    "maximum_bytes": MAX_BYTES,
    "artifact_path": ARTIFACT_PATH,
    "installed_path": INSTALLED_PATH,
}


def validate_contract(value: Any) -> None:
    contract._exact(value, CONTRACT, "redistribution material authority")


def read_material(path: Path) -> bytes:
    try:
        descriptor = os.open(
            path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK
        )
        with os.fdopen(descriptor, "rb") as stream:
            before = os.fstat(stream.fileno())
            if not stat.S_ISREG(before.st_mode) or not 0 < before.st_size <= MAX_BYTES:
                raise source.ProvenanceError("redistribution material is not bounded")
            data = stream.read(MAX_BYTES + 1)
            after = os.fstat(stream.fileno())
        current = path.lstat()
    except OSError as error:
        raise source.ProvenanceError(
            "redistribution material is unavailable"
        ) from error
    if (
        len(data) != before.st_size
        or identity(before) != identity(after)
        or identity(current) != identity(after)
    ):
        raise source.ProvenanceError("redistribution material changed while reading")
    try:
        data.decode("utf-8")
    except UnicodeDecodeError as error:
        raise source.ProvenanceError("redistribution material is not UTF-8") from error
    return data


def identity(value: os.stat_result) -> tuple[int, ...]:
    return (
        value.st_dev,
        value.st_ino,
        value.st_mode,
        value.st_size,
        value.st_mtime_ns,
        value.st_ctime_ns,
    )


def provider_path(root: Path, foundation: dict[str, Any]) -> Path:
    document = contract._cargo_workspace(root, resolved=True)
    packages = [
        value
        for value in document.get("packages", [])
        if value.get("name") == CONTRACT["package"]
    ]
    if len(packages) != 1:
        raise source.ProvenanceError("redistribution provider is ambiguous")
    package = packages[0]
    revision = foundation["revision"]
    sources = {
        f"git+{repository}?rev={revision}#{revision}"
        for repository in (foundation["repository"], foundation["repository"] + ".git")
    }
    if (
        package.get("version") != CONTRACT["version"]
        or package.get("source") not in sources
    ):
        raise source.ProvenanceError("redistribution provider identity differs")
    manifest = Path(package.get("manifest_path", ""))
    if (
        not manifest.is_absolute()
        or manifest.name != "Cargo.toml"
        or manifest.resolve() != manifest
    ):
        raise source.ProvenanceError("redistribution provider path is invalid")
    return manifest.parent / CONTRACT["source_file"]


def validate_binding(value: dict[str, Any]) -> None:
    value = contract._mapping(value, "redistribution binding")
    expected = {
        "schema",
        "package",
        "version",
        "repository",
        "revision",
        "bytes",
        "sha256",
        "artifact_path",
        "installed_path",
    }
    contract._exact(set(value), expected, "redistribution binding fields")
    for key, expected_value in {
        "schema": "tera.redistribution.v1",
        "package": CONTRACT["package"],
        "version": CONTRACT["version"],
        "repository": contract.LIB_REMOTE,
        "artifact_path": ARTIFACT_PATH,
        "installed_path": INSTALLED_PATH,
    }.items():
        contract._exact(value[key], expected_value, "redistribution binding identity")
    if not isinstance(value["revision"], str) or not re.fullmatch(
        r"[0-9a-f]{40}", value["revision"]
    ):
        raise source.ProvenanceError("redistribution revision is invalid")
    if not isinstance(value["sha256"], str) or not re.fullmatch(
        r"[0-9a-f]{64}", value["sha256"]
    ):
        raise source.ProvenanceError("redistribution digest is invalid")
    if type(value["bytes"]) is not int or not 0 < value["bytes"] <= MAX_BYTES:
        raise source.ProvenanceError("redistribution size is invalid")


def verify_expected(
    value: dict[str, Any], foundation: dict[str, Any], config: dict[str, Any]
) -> None:
    validate_binding(value)
    for key in ("repository", "revision"):
        contract._exact(
            value[key], foundation[key], "redistribution foundation identity"
        )
    contract._exact(value["sha256"], config["sha256"], "redistribution contract digest")


def verify_material(path: Path, binding: dict[str, Any]) -> bytes:
    validate_binding(binding)
    data = read_material(path)
    if (
        len(data) != binding["bytes"]
        or hashlib.sha256(data).hexdigest() != binding["sha256"]
    ):
        raise source.ProvenanceError("redistribution material differs from its binding")
    return data


def verify_inventory(
    files: list[dict[str, Any]], binding: dict[str, Any], relative: str
) -> None:
    validate_binding(binding)
    selected = [item for item in files if item.get("path") == relative]
    contract._exact(
        selected,
        [{"path": relative, "bytes": binding["bytes"], "sha256": binding["sha256"]}],
        "redistribution inventory binding",
    )


def capture(root: Path, foundation: dict[str, Any]) -> dict[str, Any]:
    data = read_material(provider_path(root, foundation))
    result = {
        "schema": "tera.redistribution.v1",
        "package": CONTRACT["package"],
        "version": CONTRACT["version"],
        "repository": foundation["repository"],
        "revision": foundation["revision"],
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
        "artifact_path": ARTIFACT_PATH,
        "installed_path": INSTALLED_PATH,
    }
    verify_expected(result, foundation, CONTRACT)
    return result


def install(
    root: Path, bundle: Path, binding: dict[str, Any], foundation: dict[str, Any]
) -> None:
    verify_expected(binding, foundation, CONTRACT)
    data = verify_material(provider_path(root, foundation), binding)
    destination = bundle / ARTIFACT_PATH
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(data)
    verify_material(destination, binding)
