from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import traceback
import unittest
from pathlib import Path

from scripts.fixture_tool_dispatch import CompilerCommand, FixtureToolDispatcher
from scripts.fixture_test_policy import limit, load_policy

ROOT = Path(__file__).resolve().parent.parent


class OwnedFixtureProcess:
    """Reserve a fixture group leader until its descendants have settled."""

    def __init__(self, argv, *, cwd, env, stdout, stderr, record_path):
        self.argv = argv
        self.record_path = record_path
        self.started = time.monotonic()
        self.deadline = self.started + limit("xcode_wrapper")
        self.process = subprocess.Popen(
            argv, cwd=cwd, env=env, stdout=stdout, stderr=stderr, start_new_session=True
        )
        self.pid = self.process.pid
        self.returncode = None
        self.attempted_cleanup = False
        self.settled = False
        self.record = {
            "argv": argv,
            "cwd": str(cwd),
            "pid": self.pid,
            "pgid": self.pid,
            "started_ns": time.time_ns(),
            "signals": [],
            "observations": [],
            "policy_id": load_policy()["id"],
            "policy_sha256": load_policy()["source_sha256"],
        }

    def poll(self):
        if self.settled:
            return self.returncode
        result = os.waitid(os.P_PID, self.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        if result is not None:
            self.returncode = (
                result.si_status
                if result.si_code == os.CLD_EXITED
                else -result.si_status
            )
            self.record.setdefault("exit_observed_ns", time.time_ns())
        return self.returncode

    def wait(self, timeout):
        deadline = min(self.deadline, time.monotonic() + timeout)
        while self.poll() is None:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired(self.argv, timeout)
            time.sleep(min(0.01, remaining))
        return self.returncode

    def send_signal(self, number):
        if self.poll() is None:
            os.kill(self.pid, number)
            self.record["signals"].append(
                {"pid": self.pid, "signal": number, "at_ns": time.time_ns()}
            )

    def group_signal(self, number):
        try:
            os.killpg(self.pid, number)
            self.record["signals"].append(
                {"pgid": self.pid, "signal": number, "at_ns": time.time_ns()}
            )
        except ProcessLookupError:
            pass

    def signal_live_group(self, number, deadline):
        if self.poll() is not None and not self.members(deadline):
            return False
        try:
            self.group_signal(number)
        except PermissionError:
            self.record.setdefault("signal_errors", []).append(
                {
                    "signal": number,
                    "at_ns": time.time_ns(),
                    "error": traceback.format_exc(),
                }
            )
            if self.poll() is None or self.members(deadline):
                raise
            return False
        return True

    def capture_members(self, deadline):
        remaining = deadline - time.monotonic()
        if remaining <= 0.05:
            raise AssertionError("owned observer cleanup budget is exhausted")
        argv = ["ps", "-axo", "pid=,ppid=,pgid=,uid=,lstart="]
        observer = subprocess.Popen(
            argv,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        command = {"argv": argv, "pid": observer.pid, "pgid": observer.pid}
        self.record["observations"].append(command)
        try:
            try:
                stdout, stderr = observer.communicate(
                    timeout=min(0.25, remaining - 0.05)
                )
            except subprocess.TimeoutExpired:
                observer.kill()
                command["timeout"] = True
                stdout, stderr = observer.communicate(
                    timeout=max(0.001, deadline - time.monotonic())
                )
        except BaseException:
            command["cleanup_exception"] = traceback.format_exc()
            raise
        finally:
            command.update(
                exit=observer.returncode,
                wait_reaped=observer.returncode is not None,
                at_ns=time.time_ns(),
            )
        try:
            os.killpg(observer.pid, 0)
            command["group_absent"] = False
        except ProcessLookupError:
            command["group_absent"] = True
        if not command["group_absent"]:
            raise AssertionError("owned process observer group remains")
        return stdout, stderr, command

    def members(self, deadline):
        stdout, stderr, command = self.capture_members(deadline)
        prefix = str(self.record_path) + f".ps-{len(self.record['observations']):02}"
        Path(prefix + ".stdout").write_bytes(stdout)
        Path(prefix + ".stderr").write_bytes(stderr)
        if len(stdout) > 2_000_000 or len(stderr) > 65_536 or command["exit"]:
            raise AssertionError("owned fixture group observation failed")
        rows = []
        for line in stdout.decode().splitlines():
            fields = line.split(None, 4)
            if len(fields) == 5 and int(fields[2]) == self.pid:
                rows.append(dict(zip(("pid", "ppid", "pgid", "uid", "birth"), fields)))
        command["members"] = rows
        return [row for row in rows if int(row["pid"]) != self.pid]

    def require_group_absent(self):
        try:
            os.killpg(self.pid, 0)
        except ProcessLookupError:
            return
        raise AssertionError("owned fixture group remains after leader reaping")

    def close(self):
        if self.attempted_cleanup:
            if not self.settled:
                raise AssertionError("previous owned fixture cleanup failed")
            return
        self.attempted_cleanup = True
        started = time.monotonic()
        deadline = started + limit("leaf_cleanup")
        self.record["cleanup_started_ns"] = time.time_ns()
        try:
            if self.signal_live_group(signal.SIGTERM, deadline):
                time.sleep(0.1)
            self.signal_live_group(signal.SIGKILL, deadline)
            while self.members(deadline):
                if time.monotonic() >= deadline:
                    raise AssertionError("owned fixture descendants did not settle")
                time.sleep(min(0.01, max(0, deadline - time.monotonic())))
            self.record["descendants_absent_ns"] = time.time_ns()
            self.returncode = self.process.wait(
                timeout=max(0.001, deadline - time.monotonic())
            )
            self.record.update(exit=self.returncode, wait_reaped=True)
            self.require_group_absent()
            self.record["group_absent"] = True
            if time.monotonic() > deadline:
                raise AssertionError("owned fixture cleanup exceeded two seconds")
            self.settled = True
        except BaseException:
            self.record["cleanup_exception"] = traceback.format_exc()
            raise
        finally:
            self.record.update(
                settled=self.settled,
                cleanup_seconds=time.monotonic() - started,
                cleanup_ended_ns=time.time_ns(),
            )
            self.record_path.write_text(json.dumps(self.record, indent=2) + "\n")


class XcodeSelectionTests(unittest.TestCase):
    def setUp(self) -> None:
        evidence = os.environ.get("TERA_C138_TEST_EVIDENCE")
        if evidence:
            self.root = Path(tempfile.mkdtemp(prefix="xcode-selector-", dir=evidence))
        else:
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
        self.selector_sequence = 0
        self.dispatcher = FixtureToolDispatcher(self.root, ("xcodebuild",))
        self.dispatcher.executable(
            "xcodebuild",
            "import json, os, sys\n"
            "from pathlib import Path\n"
            "Path(os.environ['TEST_XCODE_ARGUMENTS']).write_text(json.dumps(sys.argv[1:]))\n",
        )
        self.environment = {
            **os.environ,
            "PATH": f"{self.root}:{os.environ['PATH']}",
            "TEST_XCODE_ARGUMENTS": str(self.arguments),
            "XCODE_DERIVED_DATA": str(self.root / "derived"),
            "XCODE_SOURCE_PACKAGES": str(self.root / "sources"),
            "XCODE_PACKAGE_CACHE": str(self.root / "cache"),
        }

    def run_selector(self, target: str, *selector: str) -> subprocess.CompletedProcess:
        self.selector_sequence += 1
        command = CompilerCommand(
            [
                "/bin/bash",
                str(self.root / "scripts/xcode.sh"),
                "project-test",
                "platform=iOS Simulator,id=078A0172-207D-4EA6-8767-74DF7C42A73A",
                target,
                *selector,
            ],
            self.root,
            env=self.environment,
            budget="selector_command",
        )
        try:
            result = command.execute()
        finally:
            (self.root / f"selector-{self.selector_sequence:02}.json").write_text(
                json.dumps(command.record, indent=2)
            )
        return subprocess.CompletedProcess(
            result.args,
            result.returncode,
            result.stdout.decode(),
            result.stderr.decode(),
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
        self.dispatcher = FixtureToolDispatcher(
            self.tools, ("xcrun", "uv", "xcodebuild")
        )
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
        self.active_process = None

    def executable(self, name: str, body: str) -> None:
        self.dispatcher.executable(name, body)

    def await_file(
        self, path: Path, process: subprocess.Popen, *, timeout=None
    ) -> dict:
        deadline = min(
            process.deadline,
            process.started + limit("xcode_readiness")
            if timeout is None
            else time.monotonic() + timeout,
        )
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

    def settle_on_failure(self, process) -> None:
        if process is not None:
            process.close()

    def check_previous_cleanup(self) -> None:
        if self.active_process is not None and not self.active_process.settled:
            raise AssertionError("prior physical fixture cleanup is incomplete")

    def restore_role(self, process, mode, xcode, previous_mode, prefix) -> None:
        self.settle_on_failure(process)
        started = time.time_ns()
        if mode == "missing":
            (self.tools / "saved-xcode").rename(xcode)
        else:
            xcode.chmod(previous_mode)
        Path(str(prefix) + ".role-restoration.json").write_text(
            json.dumps({"started_ns": started, "ended_ns": time.time_ns()}) + "\n"
        )

    def observe_child(self, process, paths, mode, forwarded):
        ready, event, release, settled = paths
        data = self.await_file(ready, process)
        self.assertTrue(data["lock_exists"])
        self.assertFalse(data["ignored_int"])
        self.assertTrue(Path(data["lock"]).exists())
        if forwarded is not None:
            process.send_signal(forwarded)
            signal_data = self.await_file(event, process, timeout=5)
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
        self.check_previous_cleanup()
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
        process = None
        try:
            with (
                Path(str(prefix) + ".stdout").open("wb") as stdout,
                Path(str(prefix) + ".stderr").open("wb") as stderr,
            ):
                process = OwnedFixtureProcess(
                    argv,
                    cwd=self.root,
                    env=environment,
                    stdout=stdout,
                    stderr=stderr,
                    record_path=Path(str(prefix) + ".ownership.json"),
                )
                self.active_process = process
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
            self.restore_role(process, mode, xcode, previous_mode, prefix)

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


class XcodeFixtureOwnershipTests(unittest.TestCase):
    def setUp(self):
        evidence = os.environ.get("TERA_C138_TEST_EVIDENCE")
        if evidence:
            self.root = Path(tempfile.mkdtemp(prefix="owned-xcode-", dir=evidence))
        else:
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            self.root = Path(temporary.name).resolve()
        self.ready = self.root / "ready.json"
        self.late = self.root / "late"
        self.sentinel = self.root / "unrelated"
        self.sentinel.write_bytes(b"unrelated invocation\n")

    def start_owned(self, leader_finish):
        child = self.root / "child.py"
        child.write_text(
            "import json, os, signal, time\nfrom pathlib import Path\n"
            "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
            "signal.signal(signal.SIGINT, signal.SIG_IGN)\n"
            f"Path({str(self.ready)!r}).write_text(json.dumps({{'pid': os.getpid(), 'parent': os.getppid(), 'pgid': os.getpgrp()}}))\n"
            "os.close(1); os.close(2)\n"
            "time.sleep(8)\n"
            f"Path({str(self.late)!r}).write_text('actual late writer ran\\n')\n"
        )
        leader = self.root / "leader.py"
        leader.write_text(
            "import os, subprocess, sys, time\nfrom pathlib import Path\n"
            f"subprocess.Popen([sys.executable, {str(child)!r}], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\n"
            f"deadline=time.monotonic()+5\nwhile not Path({str(self.ready)!r}).exists() and time.monotonic()<deadline: time.sleep(.01)\n"
            + leader_finish
        )
        with (
            (self.root / "stdout").open("wb") as stdout,
            (self.root / "stderr").open("wb") as stderr,
        ):
            process = OwnedFixtureProcess(
                [sys.executable, str(leader)],
                cwd=self.root,
                env=os.environ,
                stdout=stdout,
                stderr=stderr,
                record_path=self.root / "ownership.json",
            )
        self.addCleanup(process.close)
        deadline = time.monotonic() + 5
        while not self.ready.exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        data = json.loads(self.ready.read_text())
        self.assertEqual(data["pgid"], process.pid)
        return process, data

    def assert_settlement(self, process, data):
        with self.assertRaises(ProcessLookupError):
            os.kill(data["pid"], 0)
        with self.assertRaises(ProcessLookupError):
            os.killpg(process.pid, 0)
        self.assertFalse(self.late.exists())
        self.assertEqual(self.sentinel.read_bytes(), b"unrelated invocation\n")
        record = json.loads((self.root / "ownership.json").read_text())
        self.assertTrue(record["group_absent"])
        self.assertTrue(record["wait_reaped"])
        self.assertLess(record["descendants_absent_ns"], record["cleanup_ended_ns"])
        self.assertLessEqual(record["cleanup_seconds"], 2)
        self.assertIn(signal.SIGKILL, [row["signal"] for row in record["signals"]])
        for observer in record["observations"]:
            self.assertTrue(observer["wait_reaped"])
            self.assertTrue(observer["group_absent"])
        process.close()

    def test_exited_leader_keeps_identity_until_ignored_term_child_settles(self):
        process, data = self.start_owned("sys.exit(7)\n")
        self.assertEqual(process.wait(timeout=5), 7)
        os.kill(process.pid, 0)
        os.kill(data["pid"], 0)
        process.close()
        self.assertEqual(process.returncode, 7)
        self.assert_settlement(process, data)

    def test_closed_stdio_live_leader_and_ignored_term_child_are_owned(self):
        process, data = self.start_owned("os.close(1); os.close(2)\ntime.sleep(8)\n")
        self.assertIsNone(process.poll())
        process.close()
        self.assertEqual(process.returncode, -signal.SIGTERM)
        self.assert_settlement(process, data)

    def test_exited_leader_without_descendants_reaps_actual_status(self):
        body = self.root / "exit.py"
        body.write_text("import sys\nsys.exit(7)\n")
        with (
            (self.root / "stdout").open("wb") as stdout,
            (self.root / "stderr").open("wb") as stderr,
        ):
            process = OwnedFixtureProcess(
                [sys.executable, str(body)],
                cwd=self.root,
                env=os.environ,
                stdout=stdout,
                stderr=stderr,
                record_path=self.root / "ownership.json",
            )
        self.addCleanup(process.close)
        self.assertEqual(process.wait(timeout=5), 7)
        held = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        self.assertEqual((held.si_pid, held.si_status), (process.pid, 7))
        process.close()
        self.assertEqual(process.returncode, 7)
        with self.assertRaises(ProcessLookupError):
            os.killpg(process.pid, 0)
        record = json.loads((self.root / "ownership.json").read_text())
        self.assertTrue(record["wait_reaped"])
        self.assertTrue(record["group_absent"])
        self.assertEqual(record["signals"], [])
        self.assertLessEqual(record["cleanup_seconds"], 2)

    def test_timeout_settles_before_missing_role_restore_and_next_subcase(self):
        fixture = PhysicalXcodeLifecycleTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        original = (fixture.tools / "xcrun.body.py").read_text()
        fixture.executable(
            "xcrun",
            "import json, os, signal, time\nfrom pathlib import Path\n"
            "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
            "signal.signal(signal.SIGINT, signal.SIG_IGN)\n"
            "Path(os.environ['MOCK_READY']).write_text(json.dumps({'pid': os.getpid(), 'parent': os.getppid(), 'pgid': os.getpgrp()}))\n"
            "time.sleep(8)\n"
            "Path(os.environ['MOCK_READY']+'.late').write_text('actual late writer ran\\n')\n"
            + original,
        )
        with self.assertRaises(subprocess.TimeoutExpired):
            fixture.lifecycle("physical-app-build", "missing")
        ready = json.loads((fixture.root / "run-01.ready").read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(ready["pid"], 0)
        ownership = json.loads((fixture.root / "run-01.ownership.json").read_text())
        restored = json.loads(
            (fixture.root / "run-01.role-restoration.json").read_text()
        )
        self.assertTrue(ownership["group_absent"])
        self.assertLessEqual(ownership["cleanup_seconds"], 2)
        self.assertLessEqual(ownership["cleanup_ended_ns"], restored["started_ns"])
        self.assertTrue(os.access(fixture.tools / "xcodebuild", os.X_OK))
        self.assertFalse((fixture.root / "run-01.ready.late").exists())
        fixture.executable("xcrun", original)
        fixture.lifecycle("physical-app-build", "not-executable")
        self.assertEqual(fixture.sentinel.read_bytes(), b"unrelated invocation lock\n")


if __name__ == "__main__":
    unittest.main()
