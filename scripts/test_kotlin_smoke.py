from __future__ import annotations

import contextlib
import copy
import io
import os
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import ffi_artifacts as artifacts  # noqa: E402
import ffi_build as builder  # noqa: E402
import ffi_provenance as provenance  # noqa: E402
import ffi_source as source  # noqa: E402
import kotlin_smoke as smoke  # noqa: E402
import package_contract as contract  # noqa: E402


class KotlinSmokeTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.path = self.root / "TEST-smoke.xml"
        self.suite = ET.Element(
            "testsuite", tests="18", failures="0", errors="0", skipped="0"
        )
        for identity in sorted(smoke.EXPECTED_CASES):
            name, case = identity.split("/")
            ET.SubElement(self.suite, "testcase", classname=name, name=case)

    def write(self) -> None:
        self.path.write_bytes(ET.tostring(self.suite))

    def test_only_complete_executed_inventory_is_green(self) -> None:
        self.write()
        result = smoke.test_results(self.root)
        self.assertEqual(result["passed"], 18)
        self.assertEqual((result["failed"], result["skipped"]), (0, 0))

    def test_empty_and_missing_results_cannot_pass(self) -> None:
        with self.assertRaises(source.ProvenanceError):
            smoke.test_results(self.root)
        self.suite.clear()
        self.suite.attrib.update(tests="0", failures="0", errors="0", skipped="0")
        self.write()
        with self.assertRaises(source.ProvenanceError):
            smoke.test_results(self.root)

    def test_failed_skipped_or_error_results_cannot_pass(self) -> None:
        for key in ("failures", "errors", "skipped"):
            with self.subTest(key=key):
                self.suite.set(key, "1")
                self.write()
                with self.assertRaises(source.ProvenanceError):
                    smoke.test_results(self.root)
                self.suite.set(key, "0")

    def test_case_failures_cannot_hide_behind_zero_suite_totals(self) -> None:
        ET.SubElement(self.suite.find("testcase"), "failure")
        self.write()
        with self.assertRaises(source.ProvenanceError):
            smoke.test_results(self.root)

    def test_renamed_duplicate_or_missing_cases_cannot_pass(self) -> None:
        cases = self.suite.findall("testcase")
        cases[-1].attrib = cases[0].attrib.copy()
        self.write()
        with self.assertRaises(source.ProvenanceError):
            smoke.test_results(self.root)
        self.suite.remove(cases[-1])
        self.suite.set("tests", "15")
        self.write()
        with self.assertRaises(source.ProvenanceError):
            smoke.test_results(self.root)

    def test_result_symlinks_are_rejected(self) -> None:
        self.write()
        alias = self.root / "TEST-alias.xml"
        alias.symlink_to(self.path)
        with self.assertRaises(source.ProvenanceError):
            smoke.test_results(self.root)

    def test_output_requires_extbuild_and_cannot_enter_source(self) -> None:
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(source.ProvenanceError):
                smoke.output_root(self.root)
        with patch.dict(
            os.environ,
            {"EXT_BUILD_RUN_ACTIVE": "1", "EXT_BUILD_PROJECT_DIR": str(self.root)},
        ):
            with self.assertRaises(source.ProvenanceError):
                smoke.output_root(self.root)


class KotlinNativeInputTests(unittest.TestCase):
    """Real cache/record bytes with only installed admission substituted."""

    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name).resolve()
        self.root = self.directory / "capsule"
        self.project = self.directory / "external"
        self.output = self.project / "target/kotlin_smoke"
        self.records = {
            target: {
                "repository": "https://example.invalid/tera",
                "source": {"tree": "a" * 40, "files": []},
                "foundation": {"fixture": "synthetic admitted source tuple"},
                "build": {"target": target, "native": {"sdk_sha256": "b" * 64}},
                "generator": {"native": {"sdk_sha256": "c" * 64}},
            }
            for target in artifacts.TARGETS
        }
        self.write_records(self.records)
        self.base, self.manifest = self.write_candidate(
            self.records, b"current synthetic library; not executable\n"
        )

    def write_records(self, records: dict) -> None:
        directory = self.root / "TeraFFI/source"
        directory.mkdir(parents=True, exist_ok=True)
        for target, record in records.items():
            (directory / f"{target}.json").write_bytes(provenance.encoded(record))

    def write_candidate(self, records: dict, contents: bytes) -> tuple[Path, dict]:
        base = builder.candidate_path(self.project, records)
        library = base / smoke.LIBRARY
        library.parent.mkdir(parents=True, exist_ok=True)
        library.write_bytes(contents)
        files = [artifacts.file_record(base, smoke.LIBRARY)]
        for target, record in records.items():
            relative = f"source/{target}.json"
            path = base / relative
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(provenance.encoded(record))
            files.append(artifacts.file_record(base, relative))
        return base, {
            "candidate": {
                "source": {
                    "repository": records[smoke.HOST]["repository"],
                    "tree": records[smoke.HOST]["source"]["tree"],
                },
                "files": files,
            }
        }

    def native_input(
        self, manifest: dict | None = None, *, after_admission: dict | None = None
    ) -> tuple[dict, Path, dict]:
        def admit(root: Path) -> dict:
            self.assertEqual(root, self.root)
            if after_admission is not None:
                self.write_records(after_admission)
            return manifest or self.manifest

        with (
            patch.object(smoke.installed, "check", side_effect=admit) as admission,
            patch.object(
                provenance, "capture", side_effect=AssertionError("ambient recapture")
            ),
            patch.object(
                builder,
                "capture_sources",
                side_effect=AssertionError("ambient recapture"),
            ),
        ):
            result = smoke.native_input(self.root, self.output)
        admission.assert_called_once_with(self.root)
        return result

    def test_current_tuple_wins_over_preserved_legacy_and_old_tuple(self) -> None:
        legacy = self.project / "target/tera_ffi/candidates" / ("a" * 40)
        library = legacy / smoke.LIBRARY
        library.parent.mkdir(parents=True)
        library.write_bytes(b"preserved legacy library")
        previous = copy.deepcopy(self.records)
        previous[smoke.HOST]["build"]["native"]["sdk_sha256"] = "d" * 64
        old_base, _ = self.write_candidate(previous, b"preserved old tuple library")
        candidate, native, record = self.native_input()
        self.assertEqual(candidate, self.manifest["candidate"])
        self.assertEqual(native, self.base / smoke.LIBRARY)
        self.assertIn(record, candidate["files"])
        self.assertEqual(library.read_bytes(), b"preserved legacy library")
        self.assertEqual(
            (old_base / smoke.LIBRARY).read_bytes(), b"preserved old tuple library"
        )

    def test_every_target_build_and_generator_tuple_changes_cache_identity(
        self,
    ) -> None:
        original = (self.base / smoke.LIBRARY).read_bytes()
        for target in artifacts.TARGETS:
            for section in ("build", "generator"):
                with self.subTest(target=target, section=section):
                    changed = copy.deepcopy(self.records)
                    changed[target][section]["native"]["sdk_sha256"] = "e" * 64
                    self.write_records(changed)
                    base, manifest = self.write_candidate(
                        changed, f"synthetic {target} {section} tuple".encode()
                    )
                    self.assertNotEqual(base, self.base)
                    self.assertEqual(
                        changed[target]["source"], self.records[target]["source"]
                    )
                    _, native, _ = self.native_input(manifest)
                    self.assertEqual(native, base / smoke.LIBRARY)
                    self.assertEqual((self.base / smoke.LIBRARY).read_bytes(), original)

    def test_record_change_after_installed_check_cannot_select_another_native_tuple(
        self,
    ) -> None:
        contents = (self.base / smoke.LIBRARY).read_bytes()
        for target in artifacts.TARGETS:
            for section in ("build", "generator"):
                with self.subTest(target=target, section=section):
                    changed = copy.deepcopy(self.records)
                    changed[target][section]["native"]["sdk_sha256"] = "f" * 64
                    alternate, manifest = self.write_candidate(changed, contents)
                    self.assertNotEqual(alternate, self.base)
                    self.assertEqual(
                        manifest["candidate"]["source"],
                        self.manifest["candidate"]["source"],
                    )
                    self.assertEqual(
                        artifacts.file_record(alternate, smoke.LIBRARY),
                        artifacts.file_record(self.base, smoke.LIBRARY),
                    )
                    with self.assertRaisesRegex(
                        source.ProvenanceError, "source record does not match"
                    ):
                        self.native_input(after_admission=changed)

    def test_each_missing_malformed_or_symlink_record_rejects_without_fallback(
        self,
    ) -> None:
        legacy = self.project / "target/tera_ffi/candidates" / ("a" * 40)
        library = legacy / smoke.LIBRARY
        library.parent.mkdir(parents=True)
        library.write_bytes((self.base / smoke.LIBRARY).read_bytes())
        for target in artifacts.TARGETS:
            path = self.root / "TeraFFI/source" / f"{target}.json"
            original = path.read_bytes()
            for kind in ("missing", "malformed", "symlink"):
                with self.subTest(target=target, kind=kind):
                    path.unlink()
                    if kind == "malformed":
                        path.write_bytes(b"{malformed synthetic record")
                    elif kind == "symlink":
                        actual = self.directory / "synthetic-record.json"
                        actual.write_bytes(original)
                        path.symlink_to(actual)
                    try:
                        with self.assertRaises(contract.PackageContractError):
                            self.native_input()
                    finally:
                        if path.exists() or path.is_symlink():
                            path.unlink()
                        path.write_bytes(original)

    def test_wrong_native_bytes_are_rejected(self) -> None:
        (self.base / smoke.LIBRARY).write_bytes(b"different native tuple bytes")
        with self.assertRaisesRegex(source.ProvenanceError, "does not match"):
            self.native_input()

    def test_candidate_symlink_is_rejected(self) -> None:
        destination = self.base.with_name("preserved-synthetic-candidate")
        self.base.rename(destination)
        self.base.symlink_to(destination, target_is_directory=True)
        with self.assertRaisesRegex(
            source.ProvenanceError, "candidate contains a symlink"
        ):
            self.native_input()

    def test_library_symlink_is_rejected(self) -> None:
        library = self.base / smoke.LIBRARY
        actual = self.directory / "synthetic-library"
        library.rename(actual)
        library.symlink_to(actual)
        with self.assertRaisesRegex(source.ProvenanceError, "path contains a symlink"):
            self.native_input()

    def test_contract_rejection_returns_safe_cli_failure(self) -> None:
        output = io.StringIO()
        with (
            patch.object(sys, "argv", ["kotlin_smoke.py", "verify"]),
            patch.object(
                smoke,
                "run",
                side_effect=contract.PackageContractError(
                    "required JSON contract is malformed"
                ),
            ),
            contextlib.redirect_stderr(output),
        ):
            self.assertEqual(smoke.main(), 1)
        self.assertEqual(
            output.getvalue(),
            "Kotlin binding smoke failed: required JSON contract is malformed\n",
        )


if __name__ == "__main__":
    unittest.main()
