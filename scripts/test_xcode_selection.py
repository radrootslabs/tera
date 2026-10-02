from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class XcodeSelectionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "scripts").mkdir()
        shutil.copyfile(ROOT / "scripts/xcode.sh", self.root / "scripts/xcode.sh")
        (self.root / "TeraUITests").mkdir()
        shutil.copyfile(
            ROOT / "TeraUITests/TeraAccessibilityUITests.swift",
            self.root / "TeraUITests/TeraAccessibilityUITests.swift",
        )
        self.arguments = self.root / "arguments.json"
        executable = self.root / "xcodebuild"
        executable.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\n"
            "from pathlib import Path\n"
            "Path(os.environ['TEST_XCODE_ARGUMENTS']).write_text(json.dumps(sys.argv[1:]))\n"
        )
        executable.chmod(0o700)
        self.environment = {
            **os.environ,
            "PATH": f"{self.root}:{os.environ['PATH']}",
            "TEST_XCODE_ARGUMENTS": str(self.arguments),
            "XCODE_DERIVED_DATA": str(self.root / "derived"),
            "XCODE_SOURCE_PACKAGES": str(self.root / "sources"),
            "XCODE_PACKAGE_CACHE": str(self.root / "cache"),
        }

    def run_selector(self, target: str, *selector: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [
                "/bin/bash",
                str(self.root / "scripts/xcode.sh"),
                "project-test",
                "platform=iOS Simulator,id=078A0172-207D-4EA6-8767-74DF7C42A73A",
                target,
                *selector,
            ],
            env=self.environment,
            capture_output=True,
            text=True,
            check=False,
        )

    def assert_selection(self, target: str, *selector: str) -> None:
        result = self.run_selector(target, *selector)
        self.assertEqual(result.returncode, 0, result.stderr)
        arguments = json.loads(self.arguments.read_text())
        expected = target + (f"/{selector[0]}" if selector and selector[0] else "")
        self.assertIn(f"-only-testing:{expected}", arguments)
        self.assertEqual(arguments[-1], "test")
        self.assertIn("-disableAutomaticPackageResolution", arguments)
        self.assertIn("-onlyUsePackageVersionsFromResolvedFile", arguments)
        for flag, variable in [
            ("-derivedDataPath", "XCODE_DERIVED_DATA"),
            ("-clonedSourcePackagesDirPath", "XCODE_SOURCE_PACKAGES"),
            ("-packageCachePath", "XCODE_PACKAGE_CACHE"),
        ]:
            self.assertEqual(
                arguments[arguments.index(flag) + 1], self.environment[variable]
            )

    def test_default_full_targets_and_empty_make_argument_are_preserved(self) -> None:
        self.assert_selection("TeraTests")
        self.assert_selection("TeraUITests")
        self.assert_selection("TeraUITests", "")

    def test_owned_class_and_method_select_exact_native_case(self) -> None:
        self.assert_selection("TeraUITests", "TeraAccessibilityUITests")
        self.assert_selection(
            "TeraUITests",
            "TeraAccessibilityUITests/testActualPhotoDescriptionRemainsEditableAcrossSaveFailureAndRecovery",
        )

    def test_unknown_or_injected_selector_never_launches_xcode(self) -> None:
        for target, selector in [
            ("TeraTests", "TeraAccessibilityUITests"),
            ("Unknown", "TeraAccessibilityUITests"),
            ("TeraUITests", "MissingUITests"),
            ("TeraUITests", "TeraAccessibilityUITests/testMissing"),
            ("TeraUITests", "../TeraAccessibilityUITests"),
            ("TeraUITests", "TeraAccessibilityUITests/"),
            (
                "TeraUITests",
                "TeraAccessibilityUITests/testMissing/-skip-testing:TeraUITests",
            ),
            ("TeraUITests", "TeraAccessibilityUITests;touch marker"),
        ]:
            with self.subTest(target=target, selector=selector):
                result = self.run_selector(target, selector)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.arguments.exists())

    def test_extra_arguments_never_launch_xcode(self) -> None:
        result = self.run_selector("TeraUITests", "", "-skip-testing:TeraUITests")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.arguments.exists())


if __name__ == "__main__":
    unittest.main()
