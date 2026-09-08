from __future__ import annotations

import copy
import os
import plistlib
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import ffi_artifacts as artifacts  # noqa: E402
import ffi_build as builder  # noqa: E402
import ffi_provenance as provenance  # noqa: E402
import ffi_source as source  # noqa: E402
import package_contract as contract  # noqa: E402


class NativeArtifactTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        for relative in artifacts.expected_paths():
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic verifier fixture; not an executable library\n")
        self.records = {
            target: {
                "repository": "https://github.com/radrootslabs/tera",
                "source": {"tree": "a" * 40},
                "build": {"target": target},
            }
            for target in artifacts.TARGETS
        }
        for target, record in self.records.items():
            (self.root / "source" / f"{target}.json").write_bytes(
                provenance.encoded(record)
            )
        self.install_headers()
        self.install_framework_info()
        symbols = {target: ["ffi_tera_ffi_fixture"] for target in artifacts.TARGETS}
        (self.root / "abi_symbols.json").write_bytes(provenance.encoded(symbols))
        (self.root / "api/TeraKitBindings.symbols.json").write_bytes(
            provenance.encoded(
                {
                    "schema": "radroots.swift-api-snapshot.v1",
                    "module": {"name": artifacts.MODULE},
                    "symbols": [{"synthetic": True}],
                }
            )
        )
        (self.root / artifacts.MANIFEST).write_bytes(
            provenance.encoded(artifacts.manifest(self.root, self.records))
        )

    def test_framework_metadata_is_independent_of_xcode_library_order(self) -> None:
        path = self.root / artifacts.FRAMEWORK / "Info.plist"
        original = plistlib.loads(path.read_bytes())
        builder.canonicalize_framework_info(path)
        expected = path.read_bytes()
        original["AvailableLibraries"].reverse()
        path.write_bytes(plistlib.dumps(original, sort_keys=False))
        builder.canonicalize_framework_info(path)
        self.assertEqual(path.read_bytes(), expected)

    def install_headers(self) -> None:
        header = b"void ffi_tera_ffi_fixture(void);\n"
        modulemap = b'module TeraFFI { header "TeraFFI.h" export * }\n'
        for prefix in (
            "headers",
            "TeraFFI.xcframework/ios-arm64/Headers",
            "TeraFFI.xcframework/ios-arm64-simulator/Headers",
        ):
            (self.root / prefix / "TeraFFI.h").write_bytes(header)
            (self.root / prefix / "module.modulemap").write_bytes(modulemap)
        (self.root / "generated/TeraFFI.h").write_bytes(header)
        (self.root / "generated/TeraFFI.modulemap").write_bytes(modulemap)
        (self.root / "generated/TeraKitBindings.swift").write_text("import TeraFFI\n")

    def install_framework_info(self) -> None:
        libraries = []
        for identifier, variant in (
            ("ios-arm64", None),
            ("ios-arm64-simulator", "simulator"),
        ):
            record = {
                "LibraryIdentifier": identifier,
                "SupportedArchitectures": ["arm64"],
                "SupportedPlatform": "ios",
                "LibraryPath": "libtera_ffi.a",
                "HeadersPath": "Headers",
            }
            if variant:
                record["SupportedPlatformVariant"] = variant
            libraries.append(record)
        (self.root / "TeraFFI.xcframework/Info.plist").write_bytes(
            plistlib.dumps({"AvailableLibraries": libraries})
        )

    def test_consistent_fixture_is_checked_without_claiming_build_execution(
        self,
    ) -> None:
        result = artifacts.check(self.root, self.records)
        self.assertEqual(result["disposition"], "local_candidate_not_installed")
        self.assertEqual(len(result["files"]), len(artifacts.expected_paths()))

    def test_tampered_or_missing_built_library_is_rejected(self) -> None:
        path = self.root / "native/aarch64-apple-ios/libtera_ffi.a"
        path.write_bytes(b"changed synthetic bytes")
        with self.assertRaisesRegex(source.ProvenanceError, "differs from its built"):
            artifacts.check(self.root, self.records)
        path.unlink()
        with self.assertRaisesRegex(source.ProvenanceError, "missing"):
            artifacts.check(self.root, self.records)

    def test_mismatched_generated_and_packaged_headers_are_rejected(self) -> None:
        (self.root / "headers/TeraFFI.h").write_text("void other(void);\n")
        with self.assertRaisesRegex(source.ProvenanceError, "header differs"):
            artifacts.check(self.root, self.records)

    def test_target_source_tuple_mismatch_is_rejected(self) -> None:
        records = copy.deepcopy(self.records)
        records[artifacts.TARGETS[0]]["build"]["target"] = artifacts.TARGETS[1]
        with self.assertRaisesRegex(source.ProvenanceError, "tuples disagree"):
            artifacts.check(self.root, records)

    def test_cross_target_abi_mismatch_is_rejected(self) -> None:
        symbols = contract._read_json(self.root / "abi_symbols.json")
        symbols[artifacts.TARGETS[0]] = ["ffi_tera_ffi_other"]
        (self.root / "abi_symbols.json").write_bytes(provenance.encoded(symbols))
        with self.assertRaisesRegex(source.ProvenanceError, "ABI symbols differ"):
            artifacts.check(self.root, self.records)

    def test_extra_files_including_nested_provenance_are_rejected(self) -> None:
        (self.root / "headers/provenance.json").write_text("{}\n")
        with self.assertRaisesRegex(source.ProvenanceError, "inventory differs"):
            artifacts.check(self.root, self.records)

    def test_symlink_cannot_supply_artifact_bytes(self) -> None:
        path = self.root / "native/aarch64-apple-ios/libtera_ffi.a"
        path.unlink()
        path.symlink_to(self.root / "native/aarch64-apple-ios-sim/libtera_ffi.a")
        with self.assertRaisesRegex(source.ProvenanceError, "symlink"):
            artifacts.check(self.root, self.records)

    def test_build_environment_applies_recorded_flags_and_foundation_identity(
        self,
    ) -> None:
        config = source.producer_contract(SCRIPTS.parent)
        with patch.dict(os.environ, {"TERA_CONSUMER_REVISION": "f" * 40}):
            environment = builder.build_environment(SCRIPTS.parent, self.root, config)
        self.assertNotIn("TERA_CONSUMER_REVISION", environment)
        self.assertIn("=/tera", environment["CARGO_ENCODED_RUSTFLAGS"])
        self.assertEqual(
            environment["RADROOTS_LIB_REVISION"],
            contract._read_toml(SCRIPTS.parent / "radroots.lib.source-lock.v1.toml")[
                "revision"
            ],
        )
        self.assertEqual(
            environment["SOURCE_DATE_EPOCH"], str(config["build"]["source_date_epoch"])
        )


if __name__ == "__main__":
    unittest.main()
