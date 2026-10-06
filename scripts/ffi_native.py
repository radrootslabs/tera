"""Closed Apple C input policy and execution bindings for cc 1.4.0/SQLite 0.37.0.

Inherited overrides are rejected by presence, including empty values. We never
flatten cc's priority-ordered tool selection or additive flags into one input.
CXX and CPPFLAGS are inactive in the current C graph. PATH is recorded by digest;
only public tool/SDK locations are exported, never arbitrary environment values.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import stat
import time
from pathlib import Path
from typing import Any

import ffi_source as source

TARGET_SDKS = {
    "aarch64-apple-ios": "iphoneos",
    "aarch64-apple-ios-sim": "iphonesimulator",
    "aarch64-apple-darwin": "macosx",
}
HOST = "aarch64-apple-darwin"
TOOL_INPUTS = ("CC", "CFLAGS", "AR", "ARFLAGS", "RANLIB", "RANLIBFLAGS")
# cc 1.4.0 reads these global cargo_env_var_os controls with trim inheritance on.
TRIM_INPUTS = ("CARGO_TRIM_PATHS_SCOPE", "CARGO_TRIM_PATHS_REMAP")
PKG_INPUTS = (
    "PKG_CONFIG",
    "PKG_CONFIG_PATH",
    "PKG_CONFIG_LIBDIR",
    "PKG_CONFIG_SYSROOT_DIR",
    "PKG_CONFIG_ALLOW_CROSS",
)
DIRECT_INPUTS = (
    *TRIM_INPUTS,
    "CC_KNOWN_WRAPPER_CUSTOM",
    "CC_SHELL_ESCAPED_FLAGS",
    "CRATE_CC_NO_DEFAULTS",
    "CC_FORCE_DISABLE",
    "CC_ENABLE_DEBUG_OUTPUT",
    "CROSS_COMPILE",
    "LIBSQLITE3_FLAGS",
    "RING_PREGENERATE_ASM",
    "SQLITE_MAX_VARIABLE_NUMBER",
    "SQLITE_MAX_EXPR_DEPTH",
    "SQLITE_MAX_COLUMN",
    "LIBSQLITE3_SYS_USE_PKG_CONFIG",
    "SQLITE3_LIB_DIR",
    "SQLITE3_INCLUDE_DIR",
    "SQLITE3_STATIC",
    "SQLITE3_NO_PKG_CONFIG",
    "SQLITE3_DYNAMIC",
    "PKG_CONFIG_ALL_STATIC",
    "PKG_CONFIG_ALL_DYNAMIC",
    "PKG_CONFIG_ALLOW_SYSTEM_CFLAGS",
    "PKG_CONFIG_ALLOW_SYSTEM_LIBS",
    "SDKROOT",
    "DEVELOPER_DIR",
    "TOOLCHAINS",
    "SDK_DIR",
    "IPHONEOS_DEPLOYMENT_TARGET",
    "MACOSX_DEPLOYMENT_TARGET",
    "CPATH",
    "C_INCLUDE_PATH",
    "OBJC_INCLUDE_PATH",
    "CPLUS_INCLUDE_PATH",
    "OBJCPLUS_INCLUDE_PATH",
    "LIBRARY_PATH",
    "COMPILER_PATH",
    "GCC_EXEC_PREFIX",
    "CCC_OVERRIDE_OPTIONS",
    "RC_DEBUG_OPTIONS",
    "RC_CFLAGS",
    "ZERO_AR_DATE",
    "CLANG_CONFIG_FILE_SYSTEM_DIR",
    "CLANG_CONFIG_FILE_USER_DIR",
)
PUBLIC_ROOTS = (
    Path("/Applications"),
    Path("/Library/Developer"),
    Path("/usr"),
    Path("/opt/homebrew"),
)
# Streaming identity caps, independent of the unchanged 2 MiB source-file cap.
MAX_NATIVE_NODES = 100_000
MAX_NATIVE_BYTES = 2_000_000_000
MAX_INSPECTION_SECONDS = 120
INSPECTION_LIMITS = {
    "maximum_sdk_nodes": MAX_NATIVE_NODES,
    "maximum_sdk_aggregate_bytes": MAX_NATIVE_BYTES,
    "maximum_inspection_seconds": MAX_INSPECTION_SECONDS,
    "stream_chunk_bytes": 65536,
}


def inherited_names() -> set[str]:
    names = set(DIRECT_INPUTS)
    for base in (*TOOL_INPUTS, *PKG_INPUTS):
        names.update((base, "HOST_" + base, "TARGET_" + base))
        for target in TARGET_SDKS:
            names.update((base + "_" + target, base + "_" + target.replace("-", "_")))
    return names


def reject_inherited(environment: dict[str, str]) -> None:
    if inherited_names().intersection(environment):
        raise source.ProvenanceError("ungoverned inherited native build input")
    if any(
        name.startswith("DYLD_") and name != "DYLD_FALLBACK_LIBRARY_PATH"
        for name in environment
    ):
        raise source.ProvenanceError("ungoverned inherited Apple loader input")


def inherited_identity(environment: dict[str, str]) -> dict[str, Any]:
    reject_inherited(environment)
    path = environment.get("PATH", "")
    if not path or len(path.encode()) > source.MAX_BYTES:
        raise source.ProvenanceError("native search path is missing or exceeds bound")
    result = {
        "path_sha256": hashlib.sha256(path.encode()).hexdigest(),
        "generator_profile_overrides": source.allowed_profile_overrides(),
    }
    fallback = environment.get("DYLD_FALLBACK_LIBRARY_PATH")
    if fallback is not None:
        result["removed_dyld_fallback_sha256"] = hashlib.sha256(
            fallback.encode()
        ).hexdigest()
    return result


def inspection_environment() -> dict[str, str]:
    environment = dict(os.environ)
    reject_inherited(environment)
    environment.pop("DYLD_FALLBACK_LIBRARY_PATH", None)
    return environment


def public_path(value: str) -> Path:
    path = Path(value)
    if not path.is_absolute():
        raise source.ProvenanceError("native tool or SDK path is not absolute")
    if ".." in path.parts or not any(
        path.is_relative_to(root) for root in PUBLIC_ROOTS
    ):
        raise source.ProvenanceError("native tool or SDK path is not public")
    try:
        resolved = path.resolve(strict=True)
    except (OSError, RuntimeError, ValueError):
        raise source.ProvenanceError(
            "native tool or SDK path cannot be resolved"
        ) from None
    if not any(resolved.is_relative_to(root) for root in PUBLIC_ROOTS):
        raise source.ProvenanceError("native tool or SDK path is not public")
    return resolved


def public_tool(value: str) -> Path:
    public_path(value)
    # Preserve the executable alias: Swift/ranlib dispatch by argv[0].
    return Path(value)


def check_deadline(deadline: float) -> None:
    if time.monotonic() > deadline:
        raise source.ProvenanceError("native identity exceeds inspection bound")


def file_stamp(path: Path) -> tuple[int, ...]:
    return stat_stamp(path.stat(follow_symlinks=False))


def stat_stamp(value: os.stat_result) -> tuple[int, ...]:
    return (
        value.st_dev,
        value.st_ino,
        value.st_mode,
        value.st_size,
        value.st_mtime_ns,
        value.st_ctime_ns,
    )


def file_identity(path: Path, deadline: float | None = None) -> dict[str, Any]:
    if deadline is None:
        deadline = time.monotonic() + MAX_INSPECTION_SECONDS
    check_deadline(deadline)
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as handle:
        before = os.fstat(handle.fileno())
        if not stat.S_ISREG(before.st_mode) or before.st_size > MAX_NATIVE_BYTES:
            raise source.ProvenanceError(
                "native identity input is not bounded regular data"
            )
        digest = hashlib.sha256()
        count = 0
        while chunk := handle.read(65536):
            check_deadline(deadline)
            count += len(chunk)
            if count > MAX_NATIVE_BYTES:
                raise source.ProvenanceError("native identity input exceeds bound")
            digest.update(chunk)
        after = os.fstat(handle.fileno())
        if (after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns) != (
            before.st_ino,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        ) or file_stamp(path)[:4] != (
            before.st_dev,
            before.st_ino,
            before.st_mode,
            before.st_size,
        ):
            raise source.ProvenanceError("native identity input changed while reading")
    check_deadline(deadline)
    return {"sha256": digest.hexdigest(), "bytes": count}


def sdk_directory(
    root: Path, directory: Path, inventory: dict[Path, tuple[int, ...]], deadline: float
) -> int:
    check_deadline(deadline)
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    descriptor = os.open(root, flags)
    current = root
    try:
        if stat_stamp(os.fstat(descriptor)) != inventory[current]:
            raise source.ProvenanceError("native SDK directory changed during capture")
        for name in directory.relative_to(root).parts:
            check_deadline(deadline)
            child = os.open(name, flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
            current = current / name
            if stat_stamp(os.fstat(descriptor)) != inventory[current]:
                raise source.ProvenanceError(
                    "native SDK directory changed during capture"
                )
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def sdk_entries(
    root: Path, directory: Path, inventory: dict[Path, tuple[int, ...]], deadline: float
):
    descriptor = sdk_directory(root, directory, inventory, deadline)
    try:
        with os.scandir(descriptor) as entries:
            while True:
                check_deadline(deadline)
                try:
                    entry = next(entries)
                except StopIteration:
                    return
                check_deadline(deadline)
                yield directory / entry.name
    finally:
        os.close(descriptor)


def sdk_inventory(path: Path, deadline: float) -> dict[Path, tuple[int, ...]]:
    check_deadline(deadline)
    if not stat.S_ISDIR(path.lstat().st_mode):
        raise source.ProvenanceError("native SDK root is not a directory")
    inventory = {path: file_stamp(path)}
    pending = [path]
    try:
        while pending:
            directory = pending.pop()
            for entry in sdk_entries(path, directory, inventory, deadline):
                if len(inventory) > MAX_NATIVE_NODES:
                    raise source.ProvenanceError(
                        "native SDK identity exceeds inspection bound"
                    )
                stamp = file_stamp(entry)
                inventory[entry] = stamp
                if stat.S_ISDIR(stamp[2]):
                    pending.append(entry)
    except OSError:
        raise source.ProvenanceError("native SDK enumeration failed") from None
    return inventory


def sdk_identity(path: Path, deadline: float | None = None) -> dict[str, Any]:
    digest = hashlib.sha256()
    size = 0
    if deadline is None:
        deadline = time.monotonic() + MAX_INSPECTION_SECONDS
    inventory = sdk_inventory(path, deadline)
    for entry in sorted(inventory):
        check_deadline(deadline)
        identity, count = sdk_entry(path, entry, deadline)
        size += count
        if size > MAX_NATIVE_BYTES:
            raise source.ProvenanceError("native SDK identity exceeds byte bound")
        digest.update(entry.relative_to(path).as_posix().encode() + b"\0" + identity)
    if sdk_inventory(path, deadline) != inventory:
        raise source.ProvenanceError("native SDK inventory changed during capture")
    return {
        "path": str(path),
        "sha256": digest.hexdigest(),
        "nodes": len(inventory) - 1,
        "bytes": size,
    }


def sdk_entry(root: Path, path: Path, deadline: float) -> tuple[bytes, int]:
    mode = path.lstat().st_mode
    if stat.S_ISLNK(mode):
        if not path.resolve().is_relative_to(root):
            raise source.ProvenanceError("native SDK link escapes its root")
        return b"link\0" + os.readlink(path).encode(), 0
    if stat.S_ISDIR(mode):
        return ("directory\0" + str(stat.S_IMODE(mode))).encode(), 0
    identity = file_identity(path, deadline)
    return str(identity).encode(), identity["bytes"]


def tool_identity(path: Path, deadline: float | None = None) -> dict[str, Any]:
    resolved = public_path(str(path))
    if not os.access(path, os.X_OK):
        raise source.ProvenanceError("native tool is not executable")
    return {
        "path": str(path),
        "resolved_path": str(resolved),
        **file_identity(resolved, deadline),
    }


def query(
    root: Path, argv: list[str], environment: dict[str, str], deadline: float
) -> str:
    check_deadline(deadline)
    timeout = min(MAX_INSPECTION_SECONDS, deadline - time.monotonic())
    return (
        source.command(root, argv, environment=environment, timeout=timeout)
        .decode()
        .strip()
    )


def capture(root: Path, target: str) -> dict[str, Any]:
    try:
        return capture_apple(root, target)
    except (OSError, UnicodeError, RuntimeError, ValueError):
        raise source.ProvenanceError("native Apple input inspection failed") from None


def capture_apple(root: Path, target: str) -> dict[str, Any]:
    deadline = time.monotonic() + MAX_INSPECTION_SECONDS
    environment = dict(os.environ)
    inherited = inherited_identity(environment)
    environment.pop("DYLD_FALLBACK_LIBRARY_PATH", None)
    if target not in TARGET_SDKS:
        raise source.ProvenanceError("native target is not governed")
    xcrun = public_tool(shutil.which("xcrun", path=environment["PATH"]) or "")
    developer = public_path(
        query(root, ["/usr/bin/xcode-select", "-p"], environment, deadline)
    )
    environment["DEVELOPER_DIR"] = str(developer)
    sdk = TARGET_SDKS[target]
    tools = {"xcrun": tool_identity(xcrun, deadline)}
    for key, name in (
        ("compiler", "clang"),
        ("linker", "ld"),
        ("archiver", "ar"),
        ("ranlib", "ranlib"),
        ("swiftc", "swiftc"),
        ("swift_symbolgraph_extract", "swift-symbolgraph-extract"),
        ("xcodebuild", "xcodebuild"),
        ("swift_format", "swift-format"),
        ("swift", "swift"),
    ):
        selected = query(
            root, [str(xcrun), "--sdk", sdk, "--find", name], environment, deadline
        )
        tools[key] = tool_identity(public_tool(selected), deadline)
    formatter = public_tool(shutil.which("swiftformat", path=environment["PATH"]) or "")
    tools["swiftformat"] = tool_identity(formatter, deadline)
    sdk_path = public_path(
        query(
            root, [str(xcrun), "--sdk", sdk, "--show-sdk-path"], environment, deadline
        )
    )
    resources = public_path(
        query(
            root,
            [tools["compiler"]["path"], "-print-resource-dir"],
            environment,
            deadline,
        )
    )
    result = {
        "policy": "closed_apple_c_v1",
        "target": target,
        "sdk_name": sdk,
        "inspection_limits": INSPECTION_LIMITS,
        "inherited": inherited,
        "developer_dir": str(developer),
        "tools": tools,
        "sdk": sdk_identity(sdk_path, deadline),
        "compiler_resources": sdk_identity(resources, deadline),
        "compiler_arguments": ["--no-default-config"],
        "linker_driver_arguments": [
            "--no-default-config",
            "-fuse-ld=" + tools["linker"]["path"],
        ],
        "sdk_version": query(
            root,
            [str(xcrun), "--sdk", sdk, "--show-sdk-version"],
            environment,
            deadline,
        ),
        "sdk_build": query(
            root,
            [str(xcrun), "--sdk", sdk, "--show-sdk-build-version"],
            environment,
            deadline,
        ),
        "deployment": {"IPHONEOS_DEPLOYMENT_TARGET": "18.0"} if target != HOST else {},
    }
    if inherited_identity(dict(os.environ)) != inherited:
        raise source.ProvenanceError("native environment changed during capture")
    check_deadline(deadline)
    return result


def verify(root: Path, context: dict[str, Any]) -> None:
    if capture(root, context["target"]) != context:
        raise source.ProvenanceError("native environment, tool or SDK drift")


def cache_identity(context: dict[str, Any], host: dict[str, Any]) -> str:
    encoded = json.dumps(
        {"target": context, "host": host}, sort_keys=True, separators=(",", ":")
    ).encode()
    return hashlib.sha256(encoded).hexdigest()


def execution_environment(
    root: Path,
    environment: dict[str, str],
    context: dict[str, Any],
    host: dict[str, Any],
) -> dict[str, str]:
    # The caller already supplies governed Rust flags/deployment; reject only
    # the ungoverned trim controls here, including keys absent from os.environ.
    if set(TRIM_INPUTS).intersection(environment):
        raise source.ProvenanceError("ungoverned inherited native build input")
    verify(root, context)
    verify(root, host)
    result = dict(environment)
    result.pop("DYLD_FALLBACK_LIBRARY_PATH", None)
    # PATH is copied before execution, and its inherited identity must match capture.
    if (
        hashlib.sha256(result["PATH"].encode()).hexdigest()
        != context["inherited"]["path_sha256"]
    ):
        raise source.ProvenanceError(
            "native execution search path differs from capture"
        )
    result.update(
        {"DEVELOPER_DIR": context["developer_dir"], "SDKROOT": context["sdk"]["path"]}
    )
    result.pop("IPHONEOS_DEPLOYMENT_TARGET", None)
    result.update(context["deployment"])
    if context["tools"]["linker"] != host["tools"]["linker"]:
        raise source.ProvenanceError("native split host/target linker is not governed")
    identity = cache_identity(context, host)
    # The captured clang driver must dispatch to the captured ld, not a PATH fallback.
    result["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(
        filter(
            None,
            (
                result.get("CARGO_ENCODED_RUSTFLAGS", ""),
                '--cfg=tera_native_context="' + identity + '"',
                "\x1f".join(
                    "-Clink-arg=" + argument
                    for argument in context["linker_driver_arguments"]
                ),
            ),
        )
    )
    for selected in (host, context):
        target = selected["target"]
        # cc tracks CFLAGS via rerun-if-env-changed. Rust flags also bind Cargo
        # units, including builders such as ring that suppress cc metadata.
        result["CFLAGS_" + target] = " ".join(
            [
                *selected["compiler_arguments"],
                "-DTERA_NATIVE_CONTEXT_" + identity + "=1",
            ]
        )
        for name, key in (("CC", "compiler"), ("AR", "archiver"), ("RANLIB", "ranlib")):
            result[name + "_" + target] = selected["tools"][key]["path"]
        result["CARGO_TARGET_" + target.upper().replace("-", "_") + "_LINKER"] = (
            selected["tools"]["compiler"]["path"]
        )
    return result
