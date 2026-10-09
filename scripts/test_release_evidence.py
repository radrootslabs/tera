"""Actual release shell branches with synthetic dependency/artifact dispatch."""

from __future__ import annotations

import json
import os
import selectors
import shlex
import signal
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

from scripts.fixture_tool_dispatch import FixtureToolDispatcher
from scripts.fixture_test_policy import limit as fixture_limit, load_policy

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))


class ReleaseCommand:
    """Bounded capture and cleanup while the unreaped leader reserves its PGID."""

    LIMITS = {"stdout": 1024 * 1024, "stderr": 64 * 1024}

    def __init__(self, argv: list[str], cwd: Path, environment: dict, timeout: float):
        if not 0 < timeout <= fixture_limit("release_command"):
            raise ValueError("release fixture deadline must be within 30 seconds")
        self.argv, self.cwd, self.environment, self.timeout = (
            argv,
            cwd,
            environment,
            timeout,
        )
        self.output = {name: bytearray() for name in self.LIMITS}
        self.selector = selectors.DefaultSelector()
        self.process = None
        self.started = time.monotonic()
        self.record = {
            "argv": argv,
            "cwd": str(cwd),
            "timeout": timeout,
            "policy_id": load_policy()["id"],
            "policy_sha256": load_policy()["source_sha256"],
            "started_ns": time.time_ns(),
            "signals": [],
            "wait_reaped": False,
            "group_absent": False,
        }

    def execute(self) -> subprocess.CompletedProcess:
        try:
            self.process = subprocess.Popen(
                self.argv,
                cwd=self.cwd,
                env=self.environment,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                bufsize=0,
                start_new_session=True,
            )
            self.record.update(pid=self.process.pid, pgid=self.process.pid)
            for name in self.LIMITS:
                stream = getattr(self.process, name)
                os.set_blocking(stream.fileno(), False)
                self.selector.register(stream, selectors.EVENT_READ, name)
            self.capture(self.started + self.timeout)
        except BaseException as error:
            self.record["error"] = type(error).__name__
            raise
        finally:
            try:
                if self.process is not None:
                    self.settle()
            finally:
                self.selector.close()
                self.record["elapsed_seconds"] = time.monotonic() - self.started
                self.record["ended_ns"] = time.time_ns()
        return subprocess.CompletedProcess(
            self.argv,
            self.process.returncode,
            bytes(self.output["stdout"]),
            bytes(self.output["stderr"]),
        )

    def exited(self) -> bool:
        # WNOWAIT leaves the leader's PID reserved until the last group signal.
        state = os.waitid(
            os.P_PID, self.process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT
        )
        return state is not None and state.si_pid == self.process.pid

    def capture(self, deadline: float) -> None:
        while self.selector.get_map() or not self.exited():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired(
                    self.argv,
                    self.timeout,
                    output=bytes(self.output["stdout"]),
                    stderr=bytes(self.output["stderr"]),
                )
            self.read_ready(min(remaining, 0.05), reject_excess=True)

    def read_ready(self, timeout: float, *, reject_excess: bool) -> None:
        for key, _ in self.selector.select(timeout):
            name = key.data
            available = self.LIMITS[name] - len(self.output[name])
            size = min(65536, available + 1) if reject_excess else 65536
            data = os.read(key.fd, size)
            if not data:
                self.selector.unregister(key.fileobj)
                key.fileobj.close()
                continue
            self.output[name].extend(data[:available])
            if len(data) > available:
                self.record.setdefault("output_excess", name)
                if reject_excess:
                    raise RuntimeError(f"release fixture {name} exceeds capture limit")

    def group_present(self) -> bool:
        try:
            os.killpg(self.process.pid, 0)
        except ProcessLookupError:
            return False
        except PermissionError:
            # Darwin can report EPERM for an unreaped, exited group leader.
            return True
        return True

    def send(self, number: int) -> None:
        # No wait/poll/reap is permitted before the final signal: PID reuse is fenced.
        try:
            os.killpg(self.process.pid, number)
        except ProcessLookupError:
            return
        except PermissionError:
            self.record.setdefault("signal_errors", []).append(
                {"signal": number, "errno": 1}
            )
            if not self.exited():
                raise
            return
        self.record["signals"].append({"signal": number, "at_ns": time.time_ns()})

    def drain_until(self, deadline: float) -> None:
        while self.selector.get_map() and time.monotonic() < deadline:
            self.read_ready(
                min(0.01, max(0, deadline - time.monotonic())), reject_excess=False
            )

    def finish_cleanup(self, deadline: float) -> None:
        self.send(signal.SIGKILL)
        self.process.wait(timeout=max(0.001, deadline - time.monotonic()))
        self.record.update(wait_reaped=True, exit=self.process.returncode)
        self.drain_until(deadline)
        while self.group_present() and time.monotonic() < deadline:
            time.sleep(0.005)
        self.record["group_absent"] = not self.group_present()
        if not self.record["group_absent"] or self.selector.get_map():
            raise RuntimeError(
                "release fixture cleanup did not settle its group and pipes"
            )

    def settle(self) -> None:
        deadline = time.monotonic() + fixture_limit("leaf_cleanup")
        try:
            self.send(signal.SIGTERM)
            grace = time.monotonic() + 0.2
            self.drain_until(grace)
            # Closed pipes do not prove a descendant exited; keep the leader reserved.
            while self.group_present() and time.monotonic() < grace:
                time.sleep(0.005)
        except BaseException as error:
            self.record["cleanup_error"] = type(error).__name__
            raise
        finally:
            try:
                self.finish_cleanup(deadline)
            except BaseException as error:
                self.record["cleanup_error"] = type(error).__name__
                raise
            finally:
                for name in self.LIMITS:
                    getattr(self.process, name).close()


class ReleaseFixture(unittest.TestCase):
    def setUp(self) -> None:
        evidence = os.environ.get("TERA_C138_TEST_EVIDENCE")
        if evidence:
            self.root = Path(tempfile.mkdtemp(prefix="release-", dir=evidence))
        else:
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            self.root = Path(temporary.name).resolve()
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.dispatcher = FixtureToolDispatcher(
            self.tools, ("cargo", "uv", "install", "rg")
        )
        (self.root / "scripts").mkdir()
        for name in (
            "release-evidence.sh",
            "app_source.py",
            "ffi_source.py",
            "package_contract.py",
            "app_dependency_graph.py",
            "legacy_identifiers.py",
            "package_privacy.py",
        ):
            path = SCRIPTS / name
            if path.exists():
                shutil.copyfile(path, self.root / "scripts" / name)
        for tool in (
            "jq",
            "shasum",
            "awk",
            "mktemp",
            "rm",
            "mkdir",
            "install",
            "cmp",
            "dirname",
            "git",
            "rg",
        ):
            resolved = shutil.which(tool)
            self.assertIsNotNone(resolved, f"fixture requires {tool}")
            (self.tools / tool).symlink_to(resolved)
        metadata = {
            "packages": [
                {
                    "id": "ffi",
                    "name": "tera_ffi",
                    "version": "0.1.0-alpha",
                    "source": None,
                    "license": "GPL-3.0-or-later",
                }
            ],
            "resolve": {"nodes": [{"id": "ffi", "dependencies": []}]},
            "workspace_members": ["ffi"],
        }
        self.write("metadata.json", json.dumps(metadata))
        self.executable(
            "cargo",
            "import os\nfrom pathlib import Path\n"
            "print(Path(os.environ['FIXTURE_METADATA']).read_text())\n",
        )
        self.executable(
            "uv",
            "import json, os, sys\nfrom pathlib import Path\n"
            "args = sys.argv[1:]\n"
            "expected = ['run', '--project', os.environ['FIXTURE_PROJECT'], "
            "'--offline', '--frozen', 'python']\n"
            "if args[:6] != expected: sys.exit(64)\n"
            "with Path(os.environ['FIXTURE_UV_LOG']).open('a') as out:\n"
            "    out.write(json.dumps(args) + '\\n')\n"
            "os.execv(sys.executable, [sys.executable, *args[6:]])\n",
        )
        installer = shutil.which("install")
        self.executable(
            "install",
            "import os, sys\nfrom pathlib import Path\n"
            "with Path(os.environ['FIXTURE_INSTALL_LOG']).open('a') as out:\n"
            "    out.write('install\\n')\n"
            f"os.execv({installer!r}, [{installer!r}, *sys.argv[1:]])\n",
        )
        self.write("Package.swift", "// fixture package\n")
        self.write("project.yml", "name: Tera\n")
        self.write(
            "Tera/Runtime/TeraGeneratedFixture.swift",
            "private func value() -> Int { 1 }\n",
        )
        self.write("Tera/Resources/message.txt", "fixture resource\n")
        self.write("Tera/Config/Debug.xcconfig", "FIXTURE = one\n")
        self.write("Tera/Info.plist", "fixture plist\n")
        self.write("Tera/Resources/PrivacyInfo.xcprivacy", "fixture privacy\n")
        self.write("Tera/Generated/Fixture.swift", "// synthetic generated binding\n")
        self.write("Tera/Frameworks/Fixture.a", "synthetic native bytes\n")
        self.write("Cargo.lock", "synthetic locked graph\n")
        shutil.copyfile(
            SCRIPTS.parent / "Package.resolved", self.root / "Package.resolved"
        )
        self.write(
            "Tera.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved",
            (self.root / "Package.resolved").read_text(),
        )
        self.write("Tera.xcodeproj/project.pbxproj", "synthetic generated project\n")
        self.write("TeraFFI/api/TeraKitBindings.symbols.json", "{}\n")
        self.write("api/TeraApp.symbols.json", "{}\n")
        self.write(
            "TeraFFI/provenance.json",
            json.dumps({"candidate": {"source": {"tree": "b" * 40}}}),
        )
        # Existing public producer paths are data, not newly invented identities.
        import ffi_source

        shutil.copyfile(
            SCRIPTS.parent / ffi_source.INPUTS[-1], self.root / ffi_source.INPUTS[-1]
        )
        self.epoch(1787871027)
        self.git("init", "--quiet")
        self.git("add", ".")
        self.environment = {
            **os.environ,
            "PATH": str(self.tools),
            "TMPDIR": str(self.root),
            "FIXTURE_METADATA": str(self.root / "metadata.json"),
            "FIXTURE_PROJECT": str(self.root / "scripts/persona-verifier"),
            "FIXTURE_UV_LOG": str(self.root / "uv.jsonl"),
            "FIXTURE_INSTALL_LOG": str(self.root / "install.log"),
        }
        self.commands = 0

    def write(self, relative: str, text: str) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def executable(self, name: str, body: str) -> None:
        self.dispatcher.executable(name, body)

    def git(self, *arguments: str) -> str:
        return subprocess.check_output(
            ["git", *arguments], cwd=self.root, text=True, stderr=subprocess.DEVNULL
        ).strip()

    def epoch(self, value: int) -> None:
        self.write("TeraFFI/producer.toml", f"[build]\nsource_date_epoch = {value}\n")
        self.write(
            "TeraFFI/source/aarch64-apple-ios.json",
            json.dumps(
                {
                    "foundation": {"revision": "a" * 40},
                    "build": {"source_date_epoch": value},
                }
            ),
        )

    def run_release(self, mode: str, *, timeout=None) -> subprocess.CompletedProcess:
        argv = ["/bin/sh", str(self.root / "scripts/release-evidence.sh"), mode]
        command = ReleaseCommand(
            argv,
            self.root,
            self.environment,
            fixture_limit("release_command") if timeout is None else timeout,
        )
        self.commands += 1
        try:
            return command.execute()
        finally:
            record = command.record
            for name, output in command.output.items():
                (self.root / f"command-{self.commands:02}.{name}").write_bytes(output)
                record[name] = output.decode(errors="replace")
            self.write(f"command-{self.commands:02}.json", json.dumps(record))

    def prior(self) -> dict[str, bytes]:
        return {
            name: (self.root / "release" / name).read_bytes()
            for name in ("sbom.cdx.json", "provenance.json")
        }

    def baseline(self) -> dict[str, bytes]:
        result = self.run_release("write")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.installs = (self.root / "install.log").read_bytes()
        return self.prior()

    def assert_rejected_unchanged(self, mode: str, prior: dict[str, bytes]) -> None:
        result = self.run_release(mode)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(self.prior(), prior)
        self.assertEqual((self.root / "install.log").read_bytes(), self.installs)


class ReleaseEvidenceTests(ReleaseFixture):
    def assert_staged_change_stales(self, relative: str, text: str) -> None:
        prior = self.baseline()
        self.write(relative, text)
        self.git("add", relative)
        self.assert_rejected_unchanged("check", prior)

    def test_private_body_only_staged_change_invalidates_release_evidence(self) -> None:
        self.assert_staged_change_stales(
            "Tera/Runtime/TeraGeneratedFixture.swift",
            "private func value() -> Int { 2 }\n",
        )

    def test_resource_change_invalidates_release_evidence(self) -> None:
        self.assert_staged_change_stales("Tera/Resources/message.txt", "new resource\n")

    def test_configuration_change_invalidates_release_evidence(self) -> None:
        self.assert_staged_change_stales(
            "Tera/Config/Debug.xcconfig", "FIXTURE = two\n"
        )

    def test_staged_add_remove_invalidates_release_evidence(self) -> None:
        prior = self.baseline()
        self.write("Tera/Runtime/Added.swift", "private func added() {}\n")
        self.git("add", "Tera/Runtime/Added.swift")
        self.assert_rejected_unchanged("check", prior)
        prior = self.baseline()
        self.git("rm", "-f", "Tera/Runtime/Added.swift")
        self.assert_rejected_unchanged("check", prior)

    def test_package_and_project_manifest_edits_invalidate_release_evidence(
        self,
    ) -> None:
        for relative in ("Package.swift", "project.yml"):
            with self.subTest(relative=relative):
                self.assert_staged_change_stales(
                    relative, "changed authored manifest\n"
                )

    def test_app_or_epoch_drift_during_rendering_rejects_before_output_change(
        self,
    ) -> None:
        prior = self.baseline()
        control = self.root / "drift.json"
        self.environment["FIXTURE_DRIFT"] = str(control)
        self.executable(
            "cargo",
            "import json, os, subprocess\nfrom pathlib import Path\n"
            "control = Path(os.environ['FIXTURE_DRIFT'])\n"
            "if control.exists():\n"
            "    value = json.loads(control.read_text())\n"
            "    Path(value['path']).write_text(value['text'])\n"
            "    subprocess.run(['git', 'add', value['path']], check=True)\n"
            "print(Path(os.environ['FIXTURE_METADATA']).read_text())\n",
        )
        for relative, text in (
            (
                "Tera/Runtime/TeraGeneratedFixture.swift",
                "private func value() -> Int { 2 }\n",
            ),
            ("TeraFFI/producer.toml", "[build]\nsource_date_epoch = 12345\n"),
        ):
            original = (self.root / relative).read_bytes()
            for mode in ("write", "check"):
                with self.subTest(relative=relative, mode=mode):
                    self.write(
                        "drift.json",
                        json.dumps({"path": str(self.root / relative), "text": text}),
                    )
                    self.assert_rejected_unchanged(mode, prior)
                    (self.root / relative).write_bytes(original)
                    self.git("add", relative)
            control.unlink()


class ReleaseScannerTests(ReleaseFixture):
    def test_clean_deterministic_write_and_check_bind_canonical_epoch(self) -> None:
        self.epoch(12345)
        self.git("add", "TeraFFI")
        prior = self.baseline()
        self.assertEqual(self.run_release("check").returncode, 0)
        self.assertEqual(self.run_release("write").returncode, 0)
        self.assertEqual(self.prior(), prior)
        output = json.loads(prior["provenance.json"])
        self.assertEqual(output["source"]["source_date_epoch"], 12345)
        self.assertEqual(output["schema"], "tera.release-provenance.v1")
        self.assertEqual(output["source"]["authored_app"]["policy"], "staged_inputs")
        self.assertIn(
            "Tera/Runtime/TeraGeneratedFixture.swift",
            output["source"]["authored_app"]["files"],
        )

    def test_missing_scanner_rejects_write_and_check_preserving_prior_bytes(
        self,
    ) -> None:
        prior = self.baseline()
        (self.tools / "rg").unlink()
        for mode in ("write", "check"):
            with self.subTest(mode=mode):
                self.assert_rejected_unchanged(mode, prior)

    def test_scanner_status2_rejects_write_and_check_preserving_prior_bytes(
        self,
    ) -> None:
        prior = self.baseline()
        self.executable("rg", "import sys\nsys.exit(2)\n")
        for mode in ("write", "check"):
            with self.subTest(mode=mode):
                self.assert_rejected_unchanged(mode, prior)

    def test_forbidden_material_rejects_without_printing_it_or_changing_outputs(
        self,
    ) -> None:
        prior = self.baseline()
        metadata = json.loads((self.root / "metadata.json").read_text())
        sentinel = "BEGIN PRIVATE KEY C138_PROTECTED_SENTINEL"
        metadata["packages"][0]["source"] = sentinel
        self.write("metadata.json", json.dumps(metadata))
        for mode in ("write", "check"):
            with self.subTest(mode=mode):
                result = self.run_release(mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.prior(), prior)
                self.assertNotIn(sentinel.encode(), result.stdout + result.stderr)

    def test_unreadable_rendered_input_rejects_write_and_check_preserving_prior_bytes(
        self,
    ) -> None:
        prior = self.baseline()
        scanner = shutil.which("rg")
        self.executable(
            "rg",
            "import os, sys\n"
            "import subprocess\n"
            "os.chmod(sys.argv[-1], 0)\n"
            f"status = subprocess.run([{scanner!r}, *sys.argv[1:]], check=False).returncode\n"
            "os.chmod(sys.argv[-1], 0o644)\n"
            "sys.exit(status)\n",
        )
        for mode in ("write", "check"):
            with self.subTest(mode=mode):
                self.assert_rejected_unchanged(mode, prior)


class ReleaseProcessTests(ReleaseFixture):
    def control(self, body: str) -> None:
        self.write(
            "control.py",
            "import json, os, signal, sys, time\nfrom pathlib import Path\n"
            "root = Path(__file__).parent\n"
            "(root / 'direct.json').write_text(json.dumps({"
            "'pid': os.getpid(), 'pgid': os.getpgrp(), 'sid': os.getsid(0)}))\n" + body,
        )
        self.write(
            "scripts/release-evidence.sh",
            f"exec {shlex.quote(sys.executable)} {shlex.quote(str(self.root / 'control.py'))}\n",
        )

    def settled(self) -> dict:
        record = json.loads(
            (self.root / f"command-{self.commands:02}.json").read_text()
        )
        direct = json.loads((self.root / "direct.json").read_text())
        self.assertEqual(
            (direct["pid"], direct["pgid"], direct["sid"]), (record["pid"],) * 3
        )
        self.assertTrue(record["wait_reaped"])
        self.assertTrue(record["group_absent"])
        with self.assertRaises(ProcessLookupError):
            os.killpg(record["pgid"], 0)
        self.assertLessEqual(record["elapsed_seconds"], record["timeout"] + 2.5)
        return record

    def test_normal_and_rejected_commands_preserve_bytes_status_and_deadline(
        self,
    ) -> None:
        for status in (0, 7):
            with self.subTest(status=status):
                self.control(
                    f"print('ordinary stdout'); print('ordinary stderr', file=sys.stderr); sys.exit({status})\n"
                )
                result = self.run_release("write")
                self.assertEqual(result.returncode, status)
                self.assertEqual(result.stdout, b"ordinary stdout\n")
                self.assertEqual(result.stderr, b"ordinary stderr\n")
                self.assertEqual(
                    self.settled()["timeout"], fixture_limit("release_command")
                )

    def test_stdin_is_closed_even_when_the_caller_has_input(self) -> None:
        self.control("print(repr(sys.stdin.buffer.read()))\n")
        reader, writer = os.pipe()
        original = os.dup(0)
        try:
            os.write(writer, b"P139R_SYNTHETIC_INHERITED_INPUT")
            os.close(writer)
            os.dup2(reader, 0)
            result = self.run_release("write", timeout=1)
        finally:
            os.dup2(original, 0)
            os.close(original)
            os.close(reader)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"b''\n")
        self.settled()

    def test_exact_output_limits_are_preserved(self) -> None:
        self.control(
            "sys.stdout.buffer.write(b'x' * 1048576); sys.stderr.buffer.write(b'y' * 65536)\n"
        )
        result = self.run_release("write", timeout=2)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"x" * 1048576)
        self.assertEqual(result.stderr, b"y" * 65536)
        self.settled()

    def test_output_excess_rejects_and_settles(self) -> None:
        for stream, limit in ReleaseCommand.LIMITS.items():
            with self.subTest(stream=stream):
                self.control(
                    f"while True: sys.{stream}.buffer.write(b'x' * {limit + 1}); sys.{stream}.flush()\n"
                )
                with self.assertRaisesRegex(RuntimeError, "exceeds capture limit"):
                    self.run_release("write", timeout=2)
                record = self.settled()
                self.assertEqual(record["output_excess"], stream)
                output = self.root / f"command-{self.commands:02}.{stream}"
                self.assertEqual(output.stat().st_size, limit)

    def child_control(self, *, ignore_term: bool, close_pipes: bool) -> None:
        handler = "(root / 'term.txt').write_text('received TERM')"
        if not ignore_term:
            handler += "; sys.exit(0)"
        self.control(
            "child = os.fork()\n"
            "if child == 0:\n"
            f"    signal.signal(signal.SIGTERM, lambda *_: ({handler.replace('; ', ', ')}))\n"
            "    (root / 'ready.json').write_text(json.dumps({'pid': os.getpid(), 'pgid': os.getpgrp(), 'sid': os.getsid(0)}))\n"
            + ("    os.close(1); os.close(2)\n" if close_pipes else "")
            + "    time.sleep(1.2)\n"
            "    (root / 'late.txt').write_text('escaped late write')\n"
            "    while True: time.sleep(1)\n"
            "while not (root / 'ready.json').exists(): time.sleep(0.005)\n"
            "print('ready child', flush=True)\n"
        )

    def assert_child_settled(self, *, ignore_term: bool) -> dict:
        record = self.settled()
        child = json.loads((self.root / "ready.json").read_text())
        self.assertEqual(child["pgid"], record["pgid"])
        self.assertEqual(child["sid"], record["pgid"])
        self.assertEqual((self.root / "term.txt").read_text(), "received TERM")
        with self.assertRaises(ProcessLookupError):
            os.kill(child["pid"], 0)
        if ignore_term:
            self.assertIn(
                signal.SIGKILL, [event["signal"] for event in record["signals"]]
            )
        # Ready, actual TERM and absent PID/group establish what the scheduled writer did.
        time.sleep(1.25)
        self.assertFalse((self.root / "late.txt").exists())
        return record

    def test_timeout_settles_child_retaining_capture_pipes(self) -> None:
        self.child_control(ignore_term=False, close_pipes=False)
        with self.assertRaises(subprocess.TimeoutExpired) as observed:
            self.run_release("write", timeout=0.3)
        self.assertEqual(observed.exception.timeout, 0.3)
        self.assertEqual(observed.exception.stdout, b"ready child\n")
        self.assertEqual(
            self.assert_child_settled(ignore_term=False)["error"], "TimeoutExpired"
        )

    def test_timeout_kills_child_ignoring_term(self) -> None:
        self.child_control(ignore_term=True, close_pipes=False)
        with self.assertRaises(subprocess.TimeoutExpired):
            self.run_release("write", timeout=0.3)
        self.assert_child_settled(ignore_term=True)

    def test_normal_completion_settles_child_with_closed_capture_pipes(self) -> None:
        self.child_control(ignore_term=True, close_pipes=True)
        result = self.run_release("write", timeout=2)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"ready child\n")
        self.assert_child_settled(ignore_term=True)


if __name__ == "__main__":
    unittest.main()
