"""Run the test-only generated Kotlin/JNA boundary on the governed native host."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any

import ffi_artifacts as artifacts
import ffi_installed as installed
import ffi_provenance as provenance
import ffi_source as source

HARNESS = "scripts/kotlin_smoke"
HOST = "aarch64-apple-darwin"
LIBRARY = f"native/{HOST}/libtera_ffi.dylib"
WRAPPER_HASHES = {
    "gradlew": "ab5c0cad16305af2e619c159c1f58dd68d07fab9c11e36701e109c0277407f7a",
    "gradle/wrapper/gradle-wrapper.jar": "497c8c2a7e5031f6aa847f88104aa80a93532ec32ee17bdb8d1d2f67a194a9c7",
}
EXPECTED_CASES = {
    "tera.smoke.ScopeLifecycleTests/scopeAndUnsignedContextsCrossRustWithoutSignedNarrowing",
    "tera.smoke.ScopeLifecycleTests/cancelledCloseWaitRetainsNativeCallbackDrainAndClosedAdmission",
    "tera.smoke.ScopeLifecycleTests/independentSubscriptionDisposalStopsOnlyItsObserver",
    "tera.smoke.ScopeLifecycleTests/errorRecoveryAndProtectedDataFailureRemainTyped",
    "tera.smoke.ScopeLifecycleTests/cancellationHandleKeepsIdentityAndDisposesIdempotently",
    "tera.smoke.MediaOwnershipTests/callerCloseBeforeGeneratedConversionPreservesTheAdmittedBytes",
    "tera.smoke.MediaOwnershipTests/recycledCallerSlotAndDisposedSourceWrapperCannotSubstituteBytes",
    "tera.smoke.MediaOwnershipTests/admissionRejectsInvalidDescriptorsAndWrongTypesSynchronously",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise source.ProvenanceError(message)


def output_root(root: Path) -> Path:
    require(
        bool(os.environ.get("EXT_BUILD_RUN_ACTIVE")), "Kotlin smoke requires extbuild"
    )
    project = os.environ.get("EXT_BUILD_PROJECT_DIR")
    require(bool(project), "Kotlin smoke output routing is absent")
    return provenance.output_path(
        root, HOST, str(Path(project) / "target/kotlin_smoke")
    )


def verify_host(root: Path, *, offline: bool) -> str:
    require(
        platform.system() == "Darwin" and platform.machine() == "arm64",
        "Kotlin smoke host is unsupported",
    )
    java = subprocess.check_output(["java", "--version"], text=True, timeout=30).strip()
    require(
        bool(re.match(r"(?:openjdk|java) 21[. ]", java)),
        "Kotlin smoke requires the discovered JDK 21",
    )
    cache = os.environ.get("GRADLE_USER_HOME")
    require(bool(cache) and Path(cache).is_absolute(), "Gradle cache routing is absent")
    require(
        not Path(cache).resolve().is_relative_to(root),
        "Gradle cache is inside the source repository",
    )
    for name, digest in WRAPPER_HASHES.items():
        require(
            artifacts.file_record(root / HARNESS, name)["sha256"] == digest,
            "Gradle wrapper bytes differ",
        )
    properties = (
        root / HARNESS / "gradle/wrapper/gradle-wrapper.properties"
    ).read_text()
    require(
        "distributionUrl=https\\://services.gradle.org/distributions/gradle-9.5.0-bin.zip\n"
        in properties
        and "distributionSha256Sum=553c78f50dafcd54d65b9a444649057857469edf836431389695608536d6b746\n"
        in properties,
        "Gradle distribution pin differs",
    )
    if offline:
        # --offline governs Gradle dependency resolution; the wrapper can still
        # download its distribution on first use. Require its completed install.
        distributions = Path(cache) / "wrapper/dists/gradle-9.5.0-bin"
        require(
            any(
                p.is_file() and (p.parent / "gradle-9.5.0/bin/gradle").is_file()
                for p in distributions.glob("*/gradle-9.5.0-bin.zip.ok")
            ),
            "Gradle 9.5.0 is not bootstrapped; run make kotlin-smoke-bootstrap",
        )
    return java


def native_input(
    root: Path, output: Path
) -> tuple[dict[str, Any], Path, dict[str, Any]]:
    manifest = installed.check(root)
    candidate = manifest["candidate"]
    tree = candidate["source"]["tree"]
    base = output.parent / "tera_ffi/candidates" / tree
    require(base.resolve() == base, "Kotlin native candidate contains a symlink")
    record = artifacts.file_record(base, LIBRARY)
    require(
        record in candidate["files"],
        "Kotlin smoke native library does not match the installed producer",
    )
    return candidate, base / LIBRARY, record


def generate(root: Path, output: Path, library: Path) -> Path:
    destination = provenance.output_path(root, HOST, str(output / "generated"))
    argv = [
        "cargo",
        "run",
        "-p",
        "tera_bindgen",
        "--locked",
        "--",
        "generate",
        str(library),
        "--library",
        "--language",
        "kotlin",
        "--metadata-no-deps",
        "--no-format",
        "--out-dir",
        str(destination),
        "--config",
        "core/crates/tera_ffi/uniffi.toml",
    ]
    subprocess.run(argv, cwd=root, check=True, timeout=600)
    generated = destination / "uniffi/tera_core/tera_core.kt"
    require(
        generated.is_file() and not generated.is_symlink(),
        "Generated Kotlin source is absent",
    )
    require(
        list(destination.rglob("*.kt")) == [generated],
        "Generated Kotlin source inventory differs",
    )
    return generated


def test_results(directory: Path) -> dict[str, Any]:
    files = sorted(directory.glob("TEST-*.xml"))
    require(bool(files), "Kotlin smoke produced no test results")
    cases = []
    for path in files:
        require(
            path.resolve() == path and not path.is_symlink(),
            "Kotlin test result contains a symlink",
        )
        require(
            path.stat().st_size <= 2_000_000,
            "Kotlin test result exceeds its byte bound",
        )
        suite = ET.fromstring(path.read_bytes())
        require(suite.tag == "testsuite", "Kotlin test result has an invalid root")
        require(
            all(
                suite.attrib.get(key) == "0"
                for key in ("failures", "errors", "skipped")
            ),
            "Kotlin smoke has failed or skipped tests",
        )
        observed = suite.findall("testcase")
        require(
            len(observed) == int(suite.attrib["tests"]) and bool(observed),
            "Kotlin test result count differs",
        )
        require(
            all(not list(case) for case in observed),
            "Kotlin test case contains a failure or skip",
        )
        cases.extend(
            f"{case.attrib['classname']}/{case.attrib['name']}" for case in observed
        )
    require(
        len(cases) == len(set(cases)) and set(cases) == EXPECTED_CASES,
        "Kotlin smoke inventory is incomplete",
    )
    return {"passed": len(cases), "failed": 0, "skipped": 0, "cases": sorted(cases)}


def run(root: Path, *, offline: bool) -> None:
    output = output_root(root)
    java = verify_host(root, offline=offline)
    candidate, library, native = native_input(root, output)
    generated = generate(root, output, library)
    result_directory = output / "build/test-results/test"
    require(
        result_directory.resolve() == result_directory,
        "Kotlin result directory contains a symlink",
    )
    # Remove only prior XML for this exact owned task so stale results cannot pass.
    for path in result_directory.glob("TEST-*.xml"):
        require(not path.is_symlink(), "Kotlin result is a symlink")
        path.unlink()
    argv = [
        str(root / HARNESS / "gradlew"),
        "--no-daemon",
        "--console=plain",
        "--dependency-verification=strict",
        "--project-cache-dir",
        str(output / "project_cache"),
        f"-Pkotlin.project.persistent.dir={output / 'kotlin'}",
        "-p",
        str(root / HARNESS),
        "test",
    ]
    if offline:
        argv.append("--offline")
    environment = dict(
        os.environ,
        TERA_KOTLIN_SMOKE_ROOT=str(output),
        TERA_KOTLIN_NATIVE_DIR=str(library.parent),
    )
    subprocess.run(argv, cwd=root, env=environment, check=True, timeout=600)
    results = test_results(result_directory)
    require(
        installed.check(root)["candidate"] == candidate,
        "Producer changed during Kotlin smoke",
    )
    require(
        artifacts.file_record(library.parent.parent.parent, LIBRARY) == native,
        "Native library changed during Kotlin smoke",
    )
    report = {
        "schema": "tera.kotlin-binding-smoke.v1",
        "source": candidate["source"],
        "native": native,
        "generated_sha256": hashlib.sha256(generated.read_bytes()).hexdigest(),
        "jdk": java,
        "host": HOST,
        "dependency_resolution": "offline" if offline else "bootstrap",
        "gradle_argv": argv,
        "results": results,
        "disposition": "local_native_binding_smoke_only",
    }
    provenance.write_atomic(output / "result.json", provenance.encoded(report))
    print(json.dumps(report, indent=2))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("verify", "bootstrap"))
    args = parser.parse_args()
    try:
        run(Path(__file__).resolve().parent.parent, offline=args.mode == "verify")
    except (
        OSError,
        ValueError,
        ET.ParseError,
        subprocess.SubprocessError,
        source.ProvenanceError,
    ) as error:
        print(f"Kotlin binding smoke failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
