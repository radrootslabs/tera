from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
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


class PhysicalXcodeLifecycleTests(unittest.TestCase):
    OPERATIONS = ("physical-app-build", "physical-ui-build", "physical-ui-test")

    def setUp(self) -> None:
        evidence = os.environ.get("TERA_C138_TEST_EVIDENCE")
        if evidence:
            self.root = Path(tempfile.mkdtemp(prefix="physical-wrapper-", dir=evidence))
        else:
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            self.root = Path(temporary.name).resolve()
        (self.root / "scripts").mkdir()
        for name in ("xcode.sh", "xcode_child.py"):
            if (ROOT / "scripts" / name).exists():
                shutil.copyfile(ROOT / "scripts" / name, self.root / "scripts" / name)
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.locks = self.root / "locks"
        self.locks.mkdir()
        self.sentinel = self.locks / "tera-ios-lock-state.unrelated"
        self.sentinel.write_bytes(b"unrelated invocation lock\n")
        for name in ("mktemp", "unlink", "dirname", "mkdir"):
            (self.tools / name).symlink_to(shutil.which(name))
        self.executable(
            "xcrun",
            "import json, os, sys\nfrom pathlib import Path\n"
            "assert sys.argv[1:5] == ['devicectl', 'device', 'info', 'lockState']\n"
            "path = Path(sys.argv[sys.argv.index('--json-output') + 1])\n"
            "path.write_text(json.dumps({'result': {'passcodeRequired': False, 'unlockedSinceBoot': True}}))\n"
            "Path(os.environ['MOCK_PREFLIGHT']).write_text(json.dumps({'lock': str(path), 'pid': os.getpid()}))\n"
            "sys.exit(5 if os.environ['MOCK_MODE'] == 'preflight' else 0)\n",
        )
        self.executable(
            "uv",
            "import json, os, sys, time\nfrom pathlib import Path\n"
            "args = sys.argv[1:]\n"
            "expected = ['run', '--project', os.environ['MOCK_PROJECT'], '--offline', '--frozen', 'python']\n"
            "assert args[:6] == expected\n"
            "Path(os.environ['MOCK_UV']).write_text(json.dumps(args))\n"
            "deadline = time.monotonic() + 5\n"
            "while os.environ['MOCK_MODE'] == 'early' and not Path(os.environ['MOCK_RELEASE']).exists() and time.monotonic() < deadline: time.sleep(0.01)\n"
            "os.execv(sys.executable, [sys.executable, *args[6:]])\n",
        )
        self.executable(
            "xcodebuild",
            "import json, os, signal, sys, time\nfrom pathlib import Path\n"
            "lock = Path(json.loads(Path(os.environ['MOCK_PREFLIGHT']).read_text())['lock'])\n"
            "ready = Path(os.environ['MOCK_READY'])\n"
            "release = Path(os.environ['MOCK_RELEASE'])\n"
            "event = Path(os.environ['MOCK_SIGNAL'])\n"
            "settled = Path(os.environ['MOCK_SETTLED'])\n"
            "observed = None\n"
            "ignored_int = signal.getsignal(signal.SIGINT) == signal.SIG_IGN\n"
            "def handle(number, frame):\n"
            "    global observed\n"
            "    observed = number\n"
            "    event.write_text(json.dumps({'pid': os.getpid(), 'signal': number, 'lock_exists': lock.exists()}))\n"
            "signal.signal(signal.SIGINT, handle)\n"
            "signal.signal(signal.SIGTERM, handle)\n"
            "ready.write_text(json.dumps({'pid': os.getpid(), 'parent': os.getppid(), 'lock': str(lock), 'lock_exists': lock.exists(), 'ignored_int': ignored_int, 'argv': sys.argv[1:]}))\n"
            "deadline = time.monotonic() + 10\n"
            "while not release.exists() and time.monotonic() < deadline: time.sleep(0.01)\n"
            "mode = os.environ['MOCK_MODE']\n"
            "status = 33 if observed is not None else (19 if mode == 'failure' else 0)\n"
            "settled.write_text(json.dumps({'pid': os.getpid(), 'signal': observed, 'exit': status, 'lock_exists': lock.exists()}))\n"
            "if mode == 'self-signal':\n"
            "    signal.signal(signal.SIGTERM, signal.SIG_DFL)\n"
            "    os.kill(os.getpid(), signal.SIGTERM)\n"
            "sys.exit(status)\n",
        )
        self.sequence = 0

    def executable(self, name: str, body: str) -> None:
        path = self.tools / name
        path.write_text(f"#!{sys.executable}\n" + body)
        path.chmod(0o700)

    def await_file(self, path: Path, process: subprocess.Popen) -> dict:
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if path.exists():
                try:
                    return json.loads(path.read_text())
                except json.JSONDecodeError:
                    pass
            if process.poll() is not None:
                self.fail(
                    f"wrapper settled before fixture readiness: {process.returncode}"
                )
            time.sleep(0.01)
        self.fail("wrapper fixture readiness deadline expired")

    def settle_on_failure(self, process: subprocess.Popen) -> None:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=3)

    def observe_child(self, process, paths, mode, forwarded):
        ready, event, release, settled = paths
        data = self.await_file(ready, process)
        self.assertTrue(data["lock_exists"])
        self.assertFalse(data["ignored_int"])
        self.assertTrue(Path(data["lock"]).exists())
        if forwarded is not None:
            process.send_signal(forwarded)
            signal_data = self.await_file(event, process)
            self.assertEqual(signal_data["pid"], data["pid"])
            self.assertEqual(signal_data["signal"], forwarded)
            self.assertTrue(signal_data["lock_exists"])
            self.assertIsNone(process.poll())
        release.write_text("settle\n")
        status = process.wait(timeout=5)
        completion = json.loads(settled.read_text())
        self.assertTrue(completion["lock_exists"])
        expected = (
            33
            if forwarded is not None
            else {"failure": 19, "self-signal": 143}.get(mode, 0)
        )
        self.assertEqual(status, expected)
        return data, status

    def lifecycle(
        self, operation: str, mode: str, forwarded: int | None = None
    ) -> None:
        self.sequence += 1
        prefix = self.root / f"run-{self.sequence:02}"
        preflight, ready, release, event, settled = (
            Path(str(prefix) + suffix)
            for suffix in (".preflight", ".ready", ".release", ".signal", ".settled")
        )
        derived = self.root / "derived"
        derived.mkdir(exist_ok=True)
        xcconfig = derived / "fixture.xcconfig"
        xcconfig.write_text("FIXTURE = true\n")
        environment = {
            **os.environ,
            "PATH": str(self.tools),
            "TMPDIR": str(self.locks),
            "XCODE_DERIVED_DATA": str(derived),
            "XCODE_SOURCE_PACKAGES": str(self.root / "sources"),
            "XCODE_PACKAGE_CACHE": str(self.root / "cache"),
            "XCODE_RESULTS": str(self.root / "results"),
            "TERA_IOS_DEVELOPMENT_TEAM": "A1B2C3D4E5",
            "TERA_IOS_PHYSICAL_AUTOMATION": "1",
            "TERA_IOS_UI_TEST_PHYSICAL_AUTOMATION": "1",
            "TERA_IOS_UI_TEST_RUN_ID": "c138-fixture-01",
            "TERA_IOS_UI_TEST_BLOSSOM_ORIGINS": "https://fixture.invalid",
            "MOCK_PREFLIGHT": str(preflight),
            "MOCK_READY": str(ready),
            "MOCK_RELEASE": str(release),
            "MOCK_SIGNAL": str(event),
            "MOCK_SETTLED": str(settled),
            "MOCK_MODE": mode,
            "MOCK_PROJECT": str(self.root / "scripts/persona-verifier"),
            "MOCK_UV": str(prefix) + ".uv",
        }
        argv = [
            "/bin/bash",
            str(self.root / "scripts/xcode.sh"),
            operation,
            "id=AABB-CCDD",
        ]
        argv += (
            [str(xcconfig)]
            if operation == "physical-app-build"
            else [
                "TeraUITests/FixtureUITests/testFixture",
                f"fixture-{self.sequence:02}",
            ]
        )
        xcode = self.tools / "xcodebuild"
        previous_mode = xcode.stat().st_mode
        if mode == "missing":
            xcode.rename(self.tools / "saved-xcode")
        elif mode == "not-executable":
            xcode.chmod(0o600)
        try:
            with (
                Path(str(prefix) + ".stdout").open("wb") as stdout,
                Path(str(prefix) + ".stderr").open("wb") as stderr,
            ):
                process = subprocess.Popen(
                    argv, cwd=self.root, env=environment, stdout=stdout, stderr=stderr
                )
                self.addCleanup(self.settle_on_failure, process)
                if mode == "early":
                    uv_log = Path(str(prefix) + ".uv")
                    self.await_file(uv_log, process)
                    process.send_signal(forwarded)
                    self.assertFalse(ready.exists())
                    release.write_text("finish interpreter selection\n")
                    status = process.wait(timeout=5)
                    data = json.loads(preflight.read_text())
                    self.assertEqual(status, 128 + forwarded)
                    self.assertFalse(ready.exists())
                elif mode in ("preflight", "missing", "not-executable"):
                    status = process.wait(timeout=5)
                    data = json.loads(preflight.read_text())
                    expected = {"preflight": 1, "missing": 127, "not-executable": 126}[
                        mode
                    ]
                    self.assertEqual(status, expected)
                    self.assertFalse(ready.exists())
                else:
                    data, status = self.observe_child(
                        process, (ready, event, release, settled), mode, forwarded
                    )
                Path(str(prefix) + ".command.json").write_text(
                    json.dumps(
                        {
                            "argv": argv,
                            "cwd": str(self.root),
                            "wrapper_pid": process.pid,
                            "wrapper_exit": status,
                            "owned_lock": data["lock"],
                            "settled": process.poll() is not None,
                            "forwarded_signal": forwarded,
                        }
                    )
                )
                self.assertFalse(Path(data["lock"]).exists())
                if ready.exists():
                    self.assertNotEqual(data["pid"], process.pid)
                    with self.assertRaises(ProcessLookupError):
                        os.kill(data["pid"], 0)
                self.assertEqual(
                    self.sentinel.read_bytes(), b"unrelated invocation lock\n"
                )
        finally:
            if mode == "missing":
                (self.tools / "saved-xcode").rename(xcode)
            else:
                xcode.chmod(previous_mode)

    def every_branch(self, mode: str, forwarded: int | None = None) -> None:
        for operation in self.OPERATIONS:
            with self.subTest(operation=operation, mode=mode):
                self.lifecycle(operation, mode, forwarded)

    def test_each_physical_branch_success_keeps_then_cleans_owned_lock(self) -> None:
        self.every_branch("success")

    def test_each_physical_branch_failure_returns_actual_status_and_cleans(
        self,
    ) -> None:
        self.every_branch("failure")

    def test_each_physical_branch_int_forwards_waits_and_cleans(self) -> None:
        self.every_branch("signal", signal.SIGINT)

    def test_each_physical_branch_term_forwards_waits_and_cleans(self) -> None:
        self.every_branch("signal", signal.SIGTERM)

    def test_each_physical_branch_preflight_failure_cleans_without_xcode(self) -> None:
        self.every_branch("preflight")

    def test_each_physical_branch_launch_failure_preserves_status_and_cleans(
        self,
    ) -> None:
        self.every_branch("missing")
        self.every_branch("not-executable")

    def test_child_signal_termination_is_observed_and_reaped(self) -> None:
        self.every_branch("self-signal")

    def test_early_signal_cannot_orphan_owned_child(self) -> None:
        self.every_branch("early", signal.SIGINT)
        self.every_branch("early", signal.SIGTERM)

    def test_unrelated_lock_file_survives_cleanup(self) -> None:
        self.every_branch("success")


if __name__ == "__main__":
    unittest.main()
