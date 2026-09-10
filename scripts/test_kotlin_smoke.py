from __future__ import annotations

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

import ffi_source as source  # noqa: E402
import kotlin_smoke as smoke  # noqa: E402


class KotlinSmokeTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.path = self.root / "TEST-smoke.xml"
        self.suite = ET.Element(
            "testsuite", tests="9", failures="0", errors="0", skipped="0"
        )
        for identity in sorted(smoke.EXPECTED_CASES):
            name, case = identity.split("/")
            ET.SubElement(self.suite, "testcase", classname=name, name=case)

    def write(self) -> None:
        self.path.write_bytes(ET.tostring(self.suite))

    def test_only_complete_executed_inventory_is_green(self) -> None:
        self.write()
        result = smoke.test_results(self.root)
        self.assertEqual(result["passed"], 9)
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
        self.suite.set("tests", "8")
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


if __name__ == "__main__":
    unittest.main()
