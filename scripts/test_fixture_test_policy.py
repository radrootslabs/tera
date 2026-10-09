"""Approved fixed policy, genuine short controls and one invocation clock."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts import fixture_test_policy as policy
from scripts.fixture_tool_dispatch import CompilerCommand


class FixtureTestPolicyTests(unittest.TestCase):
    def test_fixed_successor_budgets_preserve_functional_and_cleanup_limits(self):
        limits = policy.load_policy()["limits"]
        self.assertEqual(limits["compiler"], 30)
        self.assertEqual(limits["native_helper"], 30)
        self.assertEqual(limits["release_command"], 120)
        self.assertEqual(limits["reporter_child"], 120)
        self.assertEqual(limits["selector_command"], 120)
        self.assertEqual(limits["aggregate"], 600)
        self.assertEqual(limits["partition"], 900)
        self.assertEqual(limits["hostile_parser"], 3)
        self.assertEqual(limits["leaf_cleanup"], 2)
        self.assertEqual(limits["root_cleanup"], 4)
        self.assertEqual(limits["xcode_readiness"], 30)
        self.assertEqual(limits["xcode_wrapper"], 60)

    def test_unknown_policy_invalid_budgets_and_quiet_gate_are_rejected(self):
        for change in ("id", "negative", "boolean", "infinite", "quiet", "short"):
            with (
                self.subTest(change=change),
                tempfile.TemporaryDirectory() as directory,
            ):
                value = policy.load_policy()
                value.pop("source_sha256")
                if change == "id":
                    value["id"] = "unapproved"
                elif change in ("negative", "boolean", "infinite"):
                    value["limits"]["compiler"] = {
                        "negative": -1,
                        "boolean": True,
                        "infinite": float("inf"),
                    }[change]
                elif change == "quiet":
                    value["quiet_workstation_required"] = True
                else:
                    value["functional_short_intervals_unchanged"] = False
                path = Path(directory) / "policy.json"
                path.write_text(json.dumps(value))
                with (
                    patch.object(policy, "POLICY_PATH", path),
                    self.assertRaises(ValueError),
                ):
                    policy.load_policy()

    def test_duplicate_policy_keys_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "policy.json"
            path.write_text('{"id":"first","id":"second"}')
            with (
                patch.object(policy, "POLICY_PATH", path),
                self.assertRaises(ValueError),
            ):
                policy.load_policy()

    def test_explicit_short_timeout_counts_spawn_delay_and_settles(self):
        with tempfile.TemporaryDirectory() as directory:
            command = CompilerCommand(
                [sys.executable, "-S", "-c", "import time; time.sleep(30)"],
                Path(directory),
                0.1,
                budget="native_helper",
            )
            actual_spawn = subprocess.Popen

            def delayed_spawn(*args, **kwargs):
                time.sleep(0.2)
                return actual_spawn(*args, **kwargs)

            with (
                patch.object(subprocess, "Popen", delayed_spawn),
                self.assertRaises(subprocess.TimeoutExpired),
            ):
                command.execute()
            self.assertEqual(command.record["timeout"], 0.1)
            self.assertTrue(command.record["wait_reaped"])
            self.assertTrue(command.record["group_absent"])
            self.assertLessEqual(command.record["elapsed_seconds"], 2.3)


if __name__ == "__main__":
    unittest.main()
