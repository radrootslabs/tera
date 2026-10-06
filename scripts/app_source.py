"""Bounded staged authored-app identity, separate from the native graph."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import selectors
import stat
import subprocess
import sys
import time
from contextlib import ExitStack
from pathlib import Path
from typing import Any

import ffi_source as source
import package_contract as contract

MAX_BYTES = source.MAX_BYTES
MAX_INPUTS = source.MAX_INPUTS
MAX_SECONDS = 120
INPUTS = ["Package.swift", "project.yml", "Tera"]
EXCLUDED = ["Tera/Generated", "Tera/Frameworks"]
GIT_OVERRIDES = {
    "GIT_INDEX_FILE",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_CONFIG",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_SHALLOW_FILE",
    "GIT_REPLACE_REF_BASE",
}


def remaining(deadline: float) -> float:
    value = deadline - time.monotonic()
    if not 0 < value <= MAX_SECONDS:
        raise source.ProvenanceError("authored app inspection deadline expired")
    return value


def read_git_output(child: subprocess.Popen, deadline: float) -> bytes:
    with selectors.DefaultSelector() as selection:
        output = bytearray()
        sizes = {"stdout": 0, "stderr": 0}
        for name in sizes:
            stream = getattr(child, name)
            os.set_blocking(stream.fileno(), False)
            selection.register(stream, selectors.EVENT_READ, name)
        while selection.get_map():
            for key, _ in selection.select(remaining(deadline)):
                chunk = os.read(key.fd, min(65536, MAX_BYTES + 1 - sizes[key.data]))
                if not chunk:
                    selection.unregister(key.fileobj)
                    continue
                sizes[key.data] += len(chunk)
                if sizes[key.data] > MAX_BYTES:
                    raise source.ProvenanceError(
                        "authored app Git output exceeds byte bound"
                    )
                if key.data == "stdout":
                    output.extend(chunk)
        return bytes(output)


def git(root: Path, arguments: list[str], deadline: float) -> bytes:
    remaining(deadline)
    try:
        with subprocess.Popen(
            ["git", "--no-replace-objects", *arguments],
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        ) as child:
            try:
                output = read_git_output(child, deadline)
                if child.wait(timeout=remaining(deadline)):
                    raise source.ProvenanceError("authored app Git inspection failed")
                return output
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()
    except (OSError, subprocess.TimeoutExpired) as error:
        raise source.ProvenanceError(
            "authored app Git inspection unavailable"
        ) from error


def reject_git_overrides() -> None:
    if any(
        name in GIT_OVERRIDES
        or name.startswith(("GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_"))
        for name in os.environ
    ):
        raise source.ProvenanceError("authored app Git selection override is active")


def reject_intent_to_add(root: Path, paths: list[str], deadline: float) -> None:
    visible, invisible = source.staged_diff_views(paths)
    if git(root, visible, deadline) != git(root, invisible, deadline):
        raise source.ProvenanceError(
            "authored app input is intent-to-add or index changed"
        )


def scoped_inventory(root: Path, deadline: float) -> bytes:
    paths = ["--", *INPUTS, *(f":(exclude){path}" for path in EXCLUDED)]
    reject_intent_to_add(root, paths, deadline)
    for selection in (
        ["--others", "--exclude-standard"],
        ["--others", "--ignored", "--exclude-standard"],
    ):
        if git(root, ["ls-files", *selection, "-z", *paths], deadline):
            raise source.ProvenanceError(
                "authored app contains untracked or ignored input"
            )
    raw = git(root, ["ls-files", "--stage", "-z", *paths], deadline)
    if not raw or raw.count(b"\0") > MAX_INPUTS:
        raise source.ProvenanceError("authored app inventory exceeds bounds")
    return raw


def identity(info: os.stat_result) -> tuple[int, ...]:
    return (
        info.st_dev,
        info.st_ino,
        info.st_mode,
        info.st_size,
        info.st_mtime_ns,
        info.st_ctime_ns,
    )


def admit_directory(stack: ExitStack, name: str | Path, parent: int | None) -> int:
    descriptor = os.open(
        name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent
    )
    stack.callback(os.close, descriptor)
    info = os.fstat(descriptor)
    if not info.st_mode & 0o444 or not info.st_mode & 0o111:
        raise source.ProvenanceError("authored app directory is unreadable")
    return descriptor


def check_directories(
    ancestors: list[tuple[int | None, str | Path, int, tuple]],
) -> None:
    for parent, name, descriptor, before in ancestors:
        if (
            identity(os.fstat(descriptor)) != before
            or identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) != before
        ):
            raise source.ProvenanceError(
                "authored app directory changed during capture"
            )


def read_bounded(member: int, expected_size: int, deadline: float) -> bytes:
    chunks, size = [], 0
    while True:
        remaining(deadline)
        chunk = os.read(member, min(65536, MAX_BYTES + 1 - size))
        if not chunk:
            break
        size += len(chunk)
        if size > MAX_BYTES:
            raise source.ProvenanceError("authored app input exceeds byte bound")
        chunks.append(chunk)
    if size != expected_size:
        raise source.ProvenanceError("authored app input changed during capture")
    return b"".join(chunks)


def read_member(
    stack: ExitStack, parent: int, name: str, deadline: float
) -> tuple[bytes, str]:
    member = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    stack.callback(os.close, member)
    info = os.fstat(member)
    if not stat.S_ISREG(info.st_mode) or not info.st_mode & 0o444:
        raise source.ProvenanceError(
            "authored app input is not readable regular source"
        )
    if info.st_size > MAX_BYTES:
        raise source.ProvenanceError("authored app input exceeds byte bound")
    data = read_bounded(member, info.st_size, deadline)
    if identity(os.fstat(member)) != identity(info) or identity(
        os.stat(name, dir_fd=parent, follow_symlinks=False)
    ) != identity(info):
        raise source.ProvenanceError("authored app input changed during capture")
    mode = "100755" if info.st_mode & stat.S_IXUSR else "100644"
    return data, mode


def read_authored_source(
    root: Path, relative: str, deadline: float
) -> tuple[bytes, str]:
    path = Path(relative)
    if path.is_absolute() or str(path) != relative or ".." in path.parts:
        raise source.ProvenanceError("authored app input path is invalid")
    try:
        with ExitStack() as stack:
            descriptor = admit_directory(stack, root, None)
            ancestors = [(None, root, descriptor, identity(os.fstat(descriptor)))]
            for name in path.parts[:-1]:
                parent = descriptor
                descriptor = admit_directory(stack, name, parent)
                ancestors.append(
                    (parent, name, descriptor, identity(os.fstat(descriptor)))
                )
            check_directories(ancestors)
            result = read_member(stack, descriptor, path.name, deadline)
            check_directories(ancestors)
            return result
    except OSError as error:
        raise source.ProvenanceError(
            "authored app input cannot be safely read"
        ) from error


def staged_member(
    root: Path, row: bytes, deadline: float
) -> tuple[str, dict[str, Any]]:
    header, encoded_path = row.split(b"\t", 1)
    mode, oid, stage = header.decode("ascii").split()
    relative = encoded_path.decode("utf-8")
    if not source.is_input(relative, INPUTS) or source.is_input(relative, EXCLUDED):
        raise source.ProvenanceError(
            "authored app Git inventory selects an unowned input"
        )
    if stage != "0" or mode not in ("100644", "100755"):
        raise source.ProvenanceError(
            "authored app input has unresolved or unsupported mode"
        )
    data, actual_mode = read_authored_source(root, relative, deadline)
    blob = git(root, ["cat-file", "blob", oid], deadline)
    if data != blob or mode != actual_mode:
        raise source.ProvenanceError("authored app worktree differs from staged input")
    return relative, {
        "mode": mode,
        "git_blob": oid,
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def encoded(value: dict[str, Any]) -> bytes:
    raw = (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()
    if len(raw) > MAX_BYTES:
        raise source.ProvenanceError("authored app record exceeds byte bound")
    return raw


def capture(root: Path) -> dict[str, Any]:
    reject_git_overrides()
    deadline = time.monotonic() + MAX_SECONDS
    if root.is_symlink() or git(
        root, ["rev-parse", "--show-toplevel"], deadline
    ).decode().strip() != str(root):
        raise source.ProvenanceError(
            "authored app capture requires its own repository root"
        )
    raw = scoped_inventory(root, deadline)
    files, tree = {}, {}
    for row in raw.rstrip(b"\0").split(b"\0"):
        relative, entry = staged_member(root, row, deadline)
        if relative in files:
            raise source.ProvenanceError("authored app input inventory is duplicated")
        files[relative] = entry
        source.insert_tree(tree, relative, entry["mode"], entry["git_blob"])
    source.require_input_inventory(INPUTS, files)
    if scoped_inventory(root, deadline) != raw:
        raise source.ProvenanceError(
            "authored app index or inventory changed during capture"
        )
    for relative, entry in files.items():
        data, mode = read_authored_source(root, relative, deadline)
        if mode != entry["mode"] or hashlib.sha256(data).hexdigest() != entry["sha256"]:
            raise source.ProvenanceError("authored app worktree changed during capture")
    if scoped_inventory(root, deadline) != raw:
        raise source.ProvenanceError(
            "authored app index or inventory changed during capture"
        )
    remaining(deadline)
    result = {
        "policy": "staged_inputs",
        "tree": source.tree_identity(tree),
        "files": dict(sorted(files.items())),
    }
    encoded(result)
    remaining(deadline)
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, required=True)
    args = parser.parse_args()
    try:
        value = {
            "source": capture(args.repo_root),
            "source_date_epoch": contract.producer_source_epoch(args.repo_root),
        }
        sys.stdout.buffer.write(encoded(value))
        return 0
    except (
        source.ProvenanceError,
        contract.PackageContractError,
        ValueError,
        UnicodeError,
    ):
        print("error: authored app source capture rejected", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
