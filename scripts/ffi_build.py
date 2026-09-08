"""Build and verify native candidates from the sole owned Rust workspace."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

import ffi_artifacts as artifacts
import ffi_provenance as provenance
import ffi_source as source
import package_contract as contract


def build_roots(root: Path) -> tuple[Path, Path]:
    if not os.environ.get("EXT_BUILD_RUN_ACTIVE"):
        raise source.ProvenanceError("native artifact commands require extbuild")
    project = Path(os.environ["EXT_BUILD_PROJECT_DIR"]).resolve()
    target = Path(os.environ["CARGO_TARGET_DIR"]).resolve()
    if not target.is_relative_to(project) or project.is_relative_to(root):
        raise source.ProvenanceError("native outputs must use the external build root")
    return project, target


def build_environment(
    root: Path, project: Path, config: dict[str, Any]
) -> dict[str, str]:
    source.reject_build_overrides()
    values = {
        "producer_root": str(root),
        "extbuild_root": str(project),
        "cargo_home": os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")),
    }
    environment = dict(os.environ)
    environment.pop("RADROOTS_CONSUMER_REVISION", None)
    environment.update(
        {
            "CARGO_ENCODED_RUSTFLAGS": "\x1f".join(
                flag.format(**values) for flag in config["build"]["rust_flags"]
            ),
            "IPHONEOS_DEPLOYMENT_TARGET": config["build"]["ios_deployment_target"],
            "SOURCE_DATE_EPOCH": str(config["build"]["source_date_epoch"]),
            "RADROOTS_LIB_REVISION": contract._read_toml(
                root / config["foundation_lock"]
            )["revision"],
        }
    )
    return environment


def run(
    root: Path, logs: Path, name: str, argv: list[str], environment: dict[str, str]
) -> Path:
    print(f"native candidate: {name}", flush=True)
    log = logs / f"{name}.txt"
    with log.open("wb") as output:
        try:
            result = subprocess.run(
                argv,
                cwd=root,
                env=environment,
                stdout=output,
                stderr=subprocess.STDOUT,
                timeout=1800,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise source.ProvenanceError(
                f"native command unavailable or timed out: {name}"
            ) from error
    if result.returncode or log.stat().st_size > 64 * 1024 * 1024:
        raise source.ProvenanceError(
            f"native command failed; retained diagnostic: {name}"
        )
    return log


def build_libraries(
    root: Path, bundle: Path, target_root: Path, logs: Path, env: dict[str, str]
) -> None:
    for target in artifacts.TARGETS:
        run(
            root,
            logs,
            "build-" + target,
            [
                "cargo",
                "build",
                "--manifest-path",
                str(root / "Cargo.toml"),
                "-p",
                "tera_ffi",
                "--release",
                "--locked",
                "--target",
                target,
            ],
            env,
        )
        extension = "dylib" if target == artifacts.TARGETS[-1] else "a"
        destination = bundle / "native" / target
        destination.mkdir(parents=True)
        shutil.copyfile(
            target_root / target / "release" / f"libtera_ffi.{extension}",
            destination / f"libtera_ffi.{extension}",
        )


def generate_bindings(
    root: Path, bundle: Path, logs: Path, env: dict[str, str]
) -> None:
    generated = bundle / "generated"
    generated.mkdir()
    run(
        root,
        logs,
        "generate-swift",
        [
            "cargo",
            "run",
            "--manifest-path",
            str(root / "Cargo.toml"),
            "-p",
            "tera_bindgen",
            "--locked",
            "--",
            "generate",
            str(bundle / "native/aarch64-apple-darwin/libtera_ffi.dylib"),
            "--library",
            "--language",
            "swift",
            "--metadata-no-deps",
            "--out-dir",
            str(generated),
            "--config",
            str(root / "core/crates/tera_ffi/uniffi.toml"),
        ],
        env,
    )
    headers = bundle / "headers"
    headers.mkdir()
    shutil.copyfile(generated / "TeraFFI.h", headers / "TeraFFI.h")
    shutil.copyfile(generated / "TeraFFI.modulemap", headers / "module.modulemap")


def package_framework(
    root: Path, bundle: Path, logs: Path, env: dict[str, str]
) -> None:
    argv = ["xcodebuild", "-create-xcframework"]
    for target in artifacts.TARGETS[:2]:
        argv += [
            "-library",
            str(bundle / "native" / target / "libtera_ffi.a"),
            "-headers",
            str(bundle / "headers"),
        ]
    argv += ["-output", str(bundle / artifacts.FRAMEWORK)]
    run(root, logs, "package-xcframework", argv, env)


def generate_api(
    root: Path,
    bundle: Path,
    work: Path,
    logs: Path,
    env: dict[str, str],
    deployment: str,
) -> None:
    sdk = (
        source.command(root, ["xcrun", "--sdk", "iphonesimulator", "--show-sdk-path"])
        .decode()
        .strip()
    )
    module = work / "module"
    symbols = work / "symbols"
    module.mkdir()
    symbols.mkdir()
    common = [
        "-module-name",
        artifacts.MODULE,
        "-target",
        f"arm64-apple-ios{deployment}-simulator",
        "-sdk",
        sdk,
        "-I",
        str(bundle / "headers"),
        "-module-cache-path",
        str(work / "module-cache"),
    ]
    run(
        root,
        logs,
        "compile-swift-module",
        [
            "xcrun",
            "--sdk",
            "iphonesimulator",
            "swiftc",
            "-emit-module",
            "-parse-as-library",
            *common,
            "-emit-module-path",
            str(module / f"{artifacts.MODULE}.swiftmodule"),
            str(bundle / "generated/TeraKitBindings.swift"),
        ],
        env,
    )
    run(
        root,
        logs,
        "extract-swift-api",
        [
            "xcrun",
            "--sdk",
            "iphonesimulator",
            "swift-symbolgraph-extract",
            *common,
            "-I",
            str(module),
            "-minimum-access-level",
            "public",
            "-skip-inherited-docs",
            "-skip-synthesized-members",
            "-output-dir",
            str(symbols),
        ],
        env,
    )
    graph = json.loads((symbols / f"{artifacts.MODULE}.symbols.json").read_text())
    output = normalize_api(graph)
    (bundle / "api").mkdir()
    (bundle / "api/TeraKitBindings.symbols.json").write_bytes(
        (json.dumps(output, sort_keys=True, separators=(",", ":")) + "\n").encode()
    )


def normalize_api(graph: dict[str, Any]) -> dict[str, Any]:
    symbols = [
        {
            "kind": item["kind"]["identifier"],
            "precise": item["identifier"]["precise"],
            "path": item["pathComponents"],
            "access": item["accessLevel"],
            "declaration": item.get("declarationFragments"),
        }
        for item in graph["symbols"]
    ]
    relationships = [
        {key: item[key] for key in ("kind", "source", "target")}
        for item in graph["relationships"]
    ]
    return {
        "schema": "radroots.swift-api-snapshot.v1",
        "generator": graph["metadata"]["generator"],
        "module": graph["module"],
        "symbols": sorted(symbols, key=lambda item: item["precise"]),
        "relationships": sorted(
            relationships,
            key=lambda item: (item["kind"], item["source"], item["target"]),
        ),
    }


def record_abi(root: Path, bundle: Path, logs: Path, env: dict[str, str]) -> None:
    symbols = {}
    reader = provenance.symbol_reader(
        root, source.producer_contract(root)["build"]["host"]
    )
    for target in artifacts.TARGETS:
        extension = "dylib" if target == artifacts.TARGETS[-1] else "a"
        log = run(
            root,
            logs,
            "symbols-" + target,
            [
                str(reader),
                "--extern-only",
                "--just-symbol-name",
                "--defined-only",
                str(bundle / "native" / target / f"libtera_ffi.{extension}"),
            ],
            env,
        )
        symbols[target] = sorted(
            set(
                re.findall(
                    r"^_((?:ffi|uniffi)_tera_ffi_[A-Za-z0-9_]+)$",
                    log.read_text(),
                    re.MULTILINE,
                )
            )
        )
    (bundle / "abi_symbols.json").write_bytes(provenance.encoded(symbols))


def capture_sources(root: Path) -> dict[str, dict[str, Any]]:
    return {target: provenance.capture(root, target) for target in artifacts.TARGETS}


def build(
    root: Path,
    project: Path,
    target_root: Path,
    records: dict[str, dict[str, Any]],
    destination: Path,
) -> dict[str, Any]:
    config = source.producer_contract(root)
    environment = build_environment(root, project, config)
    destination.parent.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="build-", dir=destination.parent))
    # Logs survive failure. Only this invocation's temporary workspace is cleaned.
    with tempfile.TemporaryDirectory(
        prefix="work-", dir=destination.parent
    ) as temporary:
        work = Path(temporary)
        bundle = work / "bundle"
        bundle.mkdir()
        build_libraries(root, bundle, target_root, logs, environment)
        generate_bindings(root, bundle, logs, environment)
        package_framework(root, bundle, logs, environment)
        generate_api(
            root,
            bundle,
            work,
            logs,
            environment,
            config["build"]["ios_deployment_target"],
        )
        record_abi(root, bundle, logs, environment)
        if capture_sources(root) != records:
            raise source.ProvenanceError(
                "producer changed during native artifact build"
            )
        (bundle / "source").mkdir()
        for target, record in records.items():
            (bundle / "source" / f"{target}.json").write_bytes(
                provenance.encoded(record)
            )
        result = artifacts.manifest(bundle, records)
        (bundle / artifacts.MANIFEST).write_bytes(provenance.encoded(result))
        artifacts.check(bundle, records)
        if destination.exists():
            raise source.ProvenanceError("native candidate destination already exists")
        bundle.rename(destination)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("build", "check"))
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    try:
        project, target_root = build_roots(root)
        records = capture_sources(root)
        tree = records[artifacts.TARGETS[0]]["source"]["tree"]
        destination = project / "target/tera_ffi/candidates" / tree
        if args.mode == "build" and not destination.exists():
            build(root, project, target_root, records, destination)
        artifacts.check(destination, records)
    except (
        source.ProvenanceError,
        contract.PackageContractError,
        OSError,
        ValueError,
        KeyError,
    ) as error:
        print(f"native candidate: {error}", file=sys.stderr)
        return 1
    print(f"native candidate {args.mode}: tree={tree}; {destination}; not installed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
