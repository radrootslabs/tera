"""Produce exact sanitized historical owner bytes from explicit public Git roots."""

from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess
import tarfile
from legacy_upgrade_fixture_admission import admit_output, derive_refusal_inputs

MAX_FROZEN_FIXTURE_BYTES = 64 * 1024 * 1024

SOURCES = {
    "tera": ("e25819267b51f659e5dfdf7b318239a8969bf45c", "radrootslabs/tera.git"),
    "lib": ("ad17b7d3455a7147cfa303d976fc5c70c3a4c0cb", "radrootslabs/lib.git"),
    "apple_kit": (
        "35aedb6b54ff645b663fecff26082b3e91fcb232",
        "radrootslabs/apple_kit.git",
    ),
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git(root, *args):
    return subprocess.check_output(
        ["git", "--no-replace-objects", "-C", str(root), *args]
    )


def validate_root(root, owner):
    root = root.resolve(strict=True)
    if (
        Path(git(root, "rev-parse", "--show-toplevel").decode().strip()).resolve()
        != root
    ):
        raise ValueError("Explicit source root is not its Git root")
    remote = git(root, "remote", "get-url", "origin").decode().strip()
    suffix = SOURCES[owner][1]
    if remote not in [
        "ssh://git@github.com/" + suffix,
        "git@github.com:" + suffix,
        "https://github.com/" + suffix,
    ]:
        raise ValueError("Explicit source identity differs")
    revision = SOURCES[owner][0]
    if (
        git(root, "rev-parse", "--verify", revision + "^{commit}").decode().strip()
        != revision
    ):
        raise ValueError("Historical exact source is unavailable")
    subprocess.run(
        [
            "git",
            "--no-replace-objects",
            "-C",
            str(root),
            "merge-base",
            "--is-ancestor",
            revision,
            "origin/master",
        ],
        check=True,
    )
    return root


def archive(root, owner, work):
    revision = SOURCES[owner][0]
    path = work / (owner + ".tar")
    directory = work / owner
    raw = git(root, "archive", "--format=tar", revision)
    path.write_bytes(raw)
    directory.mkdir()
    with tarfile.open(path) as tar:
        members = []
        for member in tar.getmembers():
            name = Path(member.name)
            if (
                name.is_absolute()
                or ".." in name.parts
                or not (member.isdir() or member.isfile())
            ):
                raise ValueError("Historical archive contains unsupported paths")
            if name.parts[0] not in [".github", ".act"]:
                members.append(member)
        tar.extractall(directory, members=members, filter="data")
    return directory, {
        "owner": owner,
        "repository": "https://github.com/" + SOURCES[owner][1],
        "commit": revision,
        "tree": git(root, "rev-parse", revision + "^{tree}").decode().strip(),
        "archive_sha256": digest(raw),
    }


def copied_source(root, revision, path, destination, marker=None):
    raw = git(root, "show", revision + ":" + path)
    selected = raw if marker is None else raw[: raw.index(marker)]
    destination.write_bytes(selected)
    return {
        "path": path,
        "whole_blob_sha256": digest(raw),
        "compiled_sha256": digest(selected),
        "compiled_byte_count": len(selected),
        "selection": "whole_file"
        if marker is None
        else "exact_prefix_before:" + marker.decode().strip(),
    }


def run(argv, work, name, environment):
    log = work / (name + ".txt")
    with log.open("xb") as stream:
        result = subprocess.run(
            argv, stdout=stream, stderr=subprocess.STDOUT, env=environment
        )
    if result.returncode:
        raise RuntimeError(
            "Historical producer lane failed; inspect its retained external log: "
            + name
        )
    return {
        "name": name,
        "exit_code": result.returncode,
        "log_sha256": digest(log.read_bytes()),
    }


def prepare_host(root, apple, work, swift_writer):
    target = apple / "Tests/TeraHistoricalFixtureWriterTests"
    target.mkdir()
    manifest = apple / "Package.swift"
    original = manifest.read_bytes()
    anchor = b"    targets: [\n"
    if original.count(anchor) != 1:
        raise ValueError("Historical test target insertion differs")
    compiled = original.replace(
        anchor,
        anchor
        + b'        .testTarget(name: "TeraHistoricalFixtureWriterTests", dependencies: ["RadrootsKit"]),\n',
    )
    manifest.write_bytes(compiled)
    revision = SOURCES["tera"][0]
    test_target = {
        "path": "apple_kit/Package.swift",
        "whole_blob_sha256": digest(original),
        "compiled_sha256": digest(compiled),
        "compiled_byte_count": len(compiled),
        "selection": "explicit_isolated_historical_writer_test_target_only",
    }
    selections = []
    for path, name, marker in [
        (
            "Radroots/State/RadrootsConfigurationStore.swift",
            "TeraLegacyConfigurationSource.swift",
            None,
        ),
        (
            "Radroots/Runtime/RadrootsRuntimeModels.swift",
            "TeraLegacyRuntimeModelsSource.swift",
            b"enum RadrootsRuntimeSignerAvailability",
        ),
        (
            "Radroots/Runtime/RadrootsCheckedTime.swift",
            "TeraLegacyCheckedTimeSource.swift",
            None,
        ),
    ]:
        selections.append(copied_source(root, revision, path, target / name, marker))
    (target / "TeraHistoricalFixtureWriterTests.swift").write_bytes(
        swift_writer.read_bytes()
    )
    return selections, test_target


def produce(roots, work, swift_writer, rust_writer):
    # The caller must run through extbuild; never override Cargo routing.
    managed = Path(os.environ["CARGO_TARGET_DIR"]).resolve().parent
    work = work.resolve()
    if work == managed or not work.is_relative_to(managed):
        raise ValueError("Work must be a fresh child of the managed output root")
    work.mkdir(parents=True, exist_ok=False)
    archives = {}
    provenance = []
    for owner, root in roots.items():
        archives[owner], record = archive(validate_root(root, owner), owner, work)
        provenance.append(record)
    selections, test_target = prepare_host(
        roots["tera"], archives["apple_kit"], work, swift_writer
    )
    test = archives["lib"] / "crates/mobile_ffi/tests/tera_historical_fixture_writer.rs"
    test.parent.mkdir(exist_ok=True)
    test.write_bytes(rust_writer.read_bytes())
    output = work / "output"
    output.mkdir()
    environment = os.environ.copy()
    environment["TERA_LEGACY_FIXTURE_OUTPUT"] = str(output)
    environment["RADROOTS_LIB_REVISION"] = SOURCES["lib"][0]
    environment["RADROOTS_CONSUMER_REVISION"] = SOURCES["tera"][0]
    environment["SOURCE_DATE_EPOCH"] = (
        git(roots["lib"], "show", "-s", "--format=%ct", SOURCES["lib"][0])
        .decode()
        .strip()
    )
    swift = [
        "swift",
        "test",
        "--package-path",
        str(archives["apple_kit"]),
        "--scratch-path",
        str(work / "swift_scratch"),
        "--cache-path",
        os.environ["SWIFTPM_CACHE"],
        "--disable-automatic-resolution",
        "--filter",
        "TeraHistoricalFixtureWriterTests/testProduceHistoricalOwnerBytes",
    ]
    receipts = []
    environment["TERA_LEGACY_FIXTURE_PHASE"] = "host"
    receipts.append(run(swift, work, "historical-host", environment))
    receipts.append(
        run(
            [
                "cargo",
                "test",
                "--manifest-path",
                str(archives["lib"] / "Cargo.toml"),
                "--locked",
                "-p",
                "radroots_mobile_ffi",
                "--test",
                "tera_historical_fixture_writer",
                "--",
                "--test-threads=1",
            ],
            work,
            "historical-rust",
            environment,
        )
    )
    environment["TERA_LEGACY_FIXTURE_PHASE"] = "transfer"
    receipts.append(run(swift, work, "historical-transfer", environment))
    admission = admit_output(output, work)
    admission["negative_derivatives"] = derive_refusal_inputs(
        output, json.loads((output / "rust-metadata.json").read_text())["public_key"]
    )
    (output / "owner-expectations.json").write_text(
        json.dumps(admission, indent=2) + "\n"
    )
    files = {
        str(path.relative_to(output)): digest(path.read_bytes())
        for path in sorted(output.rglob("*"))
        if path.is_file()
    }
    record = {
        "state": "HISTORICAL_WRITER_ADMITTED_CURRENT_READERS_PENDING",
        "sources": provenance,
        "selected_unchanged_old_host_sources": selections,
        "historical_writer_test_target": test_target,
        "writer_source_sha256": {
            "swift": digest(swift_writer.read_bytes()),
            "rust": digest(rust_writer.read_bytes()),
        },
        "actual_command_receipts": receipts,
        "actual_case_admission": admission,
        "files": files,
        "limits": "Synthetic old actual owner writes, media preparation and typed native persistence. No old complete app binary, OS transfer, physical device, Keychain secret, network publication or byte-identical regeneration claim.",
    }
    (work / "producer.json").write_text(json.dumps(record, indent=2) + "\n")
    return record


def frozen_source(work, relative):
    path = Path(relative)
    if path.is_absolute() or not path.parts or ".." in path.parts:
        raise ValueError("Frozen fixture path escaped its owner")
    source = work / "output"
    if source.is_symlink():
        raise ValueError("Admitted owner bytes changed before freeze")
    for part in path.parts:
        source /= part
        if source.is_symlink():
            raise ValueError("Admitted owner bytes changed before freeze")
    if not source.is_file():
        raise ValueError("Admitted owner bytes changed before freeze")
    return source


def frozen_inputs(work, record):
    if record["state"] != "HISTORICAL_WRITER_ADMITTED_CURRENT_READERS_PENDING":
        raise ValueError("Historical writer admission is missing")
    contents = {}
    total = 0
    for relative, expected_hash in record["files"].items():
        path = Path(relative)
        source = frozen_source(work, relative)
        total += source.stat().st_size
        if total > MAX_FROZEN_FIXTURE_BYTES:
            raise ValueError("Historical fixture tooling input exceeds its byte bound")
        content = source.read_bytes()
        if digest(content) != expected_hash:
            raise ValueError("Admitted owner bytes changed before freeze")
        contents[path] = content
    return contents


def freeze_output(work, record, destination):
    """Freeze admitted owner bytes once; never rewrite an existing fixture."""
    expected = (
        Path(__file__).resolve().parent.parent / "test-fixtures/legacy_upgrade_v1"
    )
    if (
        destination.resolve() != expected.resolve()
        or expected.exists()
        or expected.is_symlink()
    ):
        raise ValueError("Frozen fixture must be a fresh canonical owner path")
    contents = frozen_inputs(work, record)
    expected.mkdir()
    for path, content in contents.items():
        output = expected / path
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(content)
    (expected / "manifest.json").write_text(json.dumps(record, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser()
    for owner in SOURCES:
        parser.add_argument(
            "--" + owner.replace("_", "-") + "-git-root", type=Path, required=True
        )
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--swift-writer", type=Path, required=True)
    parser.add_argument("--rust-writer", type=Path, required=True)
    parser.add_argument("--fixture-destination", type=Path)
    args = parser.parse_args()
    roots = {
        owner: validate_root(getattr(args, owner + "_git_root"), owner)
        for owner in SOURCES
    }
    result = produce(roots, args.work, args.swift_writer, args.rust_writer)
    if args.fixture_destination is not None:
        freeze_output(args.work, result, args.fixture_destination)
    print(json.dumps({"state": result["state"], "file_count": len(result["files"])}))


if __name__ == "__main__":
    main()
