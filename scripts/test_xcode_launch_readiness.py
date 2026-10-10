"""Cold launch-failure startup and genuine short cleanup remain distinct."""

from __future__ import annotations

import unittest

from scripts import test_xcode_selection as xcode_fixture


class XcodeLaunchReadinessTests(unittest.TestCase):
    def test_delayed_cold_missing_and_nonexecutable_launch_preserves_refusal(self):
        fixture = xcode_fixture.PhysicalXcodeLifecycleTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        fixture.OPERATIONS = ("physical-app-build",)
        original = (fixture.tools / "uv.body.py").read_text()
        fixture.executable("uv", "import time\ntime.sleep(6)\n" + original)
        for mode in ("missing", "not-executable"):
            with self.subTest(mode=mode):
                fixture.every_branch(mode)
                uv_log = fixture.root / f"run-{fixture.sequence:02}.uv"
                self.assertTrue(uv_log.is_file())


if __name__ == "__main__":
    unittest.main()
