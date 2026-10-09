"""Actual native mock tools, argument fidelity and bounded compiler ownership."""

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
from unittest.mock import patch

from scripts.fixture_tool_dispatch import (
    CompilerCommand,
    FixtureDispatchError,
    FixtureToolDispatcher,
)
from scripts.fixture_test_policy import limit


class FixtureToolDispatchTests(unittest.TestCase):
    def setUp(self) -> None:
        evidence = os.environ.get("TERA_C138_TEST_EVIDENCE")
        if evidence:
            self.root = Path(tempfile.mkdtemp(prefix="tool-dispatch-", dir=evidence))
        else:
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            self.root = Path(temporary.name).resolve()
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.dispatcher = FixtureToolDispatcher(
            self.tools, ("uv", "cargo", "install", "xcrun", "xcodebuild", "rg")
        )
        self.sequence = 0

    def command(
        self, argv: list[str], *, cwd: Path | None = None, timeout=None
    ) -> subprocess.CompletedProcess:
        self.sequence += 1
        command = CompilerCommand(
            argv,
            cwd or self.root,
            limit("native_helper") if timeout is None else timeout,
            budget="native_helper",
        )
        try:
            return command.execute()
        finally:
            prefix = self.root / f"command-{self.sequence:02}"
            prefix.with_suffix(".json").write_text(json.dumps(command.record, indent=2))
            for name, raw in command.output.items():
                Path(str(prefix) + f".{name}").write_bytes(raw)
            self.assert_settled(command.record)

    def assert_settled(self, record: dict) -> None:
        self.assertTrue(record["wait_reaped"], record)
        self.assertTrue(record["group_absent"], record)
        self.assertLessEqual(record["elapsed_seconds"], record["timeout"] + 2.5)
        with self.assertRaises(ProcessLookupError):
            os.kill(record["pid"], 0)
        with self.assertRaises(ProcessLookupError):
            os.killpg(record["pgid"], 0)

    def body(self, role: str, status: int = 0) -> str:
        return (
            "import json, os, sys\nfrom pathlib import Path\n"
            f"Path({str(self.root / (role + '.json'))!r}).write_text(json.dumps({{"
            "'argv': sys.argv, 'file': __file__, 'interpreter': sys.executable, "
            "'cwd': os.getcwd(), 'path': os.environ['PATH'], "
            "'literal': os.environ.get('FIXTURE_LITERAL'), "
            "'stdin_closed': os.read(0, 1) == b'', 'pid': os.getpid(), "
            "'pgid': os.getpgrp(), 'sid': os.getsid(0)}))\n"
            f"sys.exit({status})\n"
        )

    def environment(self) -> dict:
        return {"PATH": str(self.tools), "FIXTURE_LITERAL": "space $literal value"}

    def test_first_copies_preserve_actual_body_arguments_and_process_identity(
        self,
    ) -> None:
        arguments = ["", "space argument", "$literal", "-m", "0644", "--quiet", "é"]
        bodies = {}
        for role, status in (("uv", 0), ("cargo", 7), ("install", 0)):
            body = self.body(role, status)
            bodies[role] = body
            self.dispatcher.executable(role, body)
            with patch.dict(os.environ, self.environment(), clear=True):
                result = self.command([str(self.tools / role), *arguments])
            self.assertEqual(result.returncode, status, result.stderr)
            observed = json.loads((self.root / f"{role}.json").read_text())
            record = json.loads(
                (self.root / f"command-{self.sequence:02}.json").read_text()
            )
            self.assertEqual(
                observed["argv"], [str(self.tools / f"{role}.body.py"), *arguments]
            )
            self.assertEqual(observed["file"], str(self.tools / f"{role}.body.py"))
            self.assertEqual(observed["interpreter"], sys.executable)
            self.assertEqual(observed["cwd"], str(self.root))
            self.assertEqual(observed["path"], str(self.tools))
            self.assertEqual(observed["literal"], self.environment()["FIXTURE_LITERAL"])
            self.assertTrue(observed["stdin_closed"])
            self.assertEqual(
                (observed["pid"], observed["pgid"], observed["sid"]),
                (record["pid"],) * 3,
            )
        for role, body in bodies.items():
            self.assertEqual(
                (self.tools / f"{role}.body.py").read_bytes(), body.encode()
            )
        compiler = json.loads(
            (self.dispatcher.directory / "compilation.json").read_text()
        )
        self.assertEqual(compiler["command"]["timeout"], limit("compiler"))
        self.assertEqual(compiler["command"]["exit"], 0)
        self.assert_settled(compiler["command"])
        independent_tools = self.root / "independent-tools"
        independent_tools.mkdir()
        independent = FixtureToolDispatcher(independent_tools, self.dispatcher.roles)
        independent.executable("uv", self.body("independent"))
        for field in ("source", "binary"):
            self.assertEqual(
                independent.record[field]["sha256"], compiler[field]["sha256"]
            )
        self.assertNotEqual(
            independent.record["command"]["pid"], compiler["command"]["pid"]
        )
        self.assertNotEqual(
            independent.binary.stat().st_ino, self.dispatcher.binary.stat().st_ino
        )
        self.assert_settled(independent.record["command"])
        (self.root / "stable-compilations.json").write_text(
            json.dumps([compiler, independent.record], indent=2) + "\n"
        )
        with patch.dict(os.environ, {"PATH": str(independent_tools)}, clear=True):
            result = self.command([str(independent_tools / "uv"), *arguments])
        self.assertEqual(result.returncode, 0, result.stderr)
        observed = json.loads((self.root / "independent.json").read_text())
        self.assertEqual(
            observed["argv"], [str(independent_tools / "uv.body.py"), *arguments]
        )
        record = json.loads(
            (self.root / f"command-{self.sequence:02}.json").read_text()
        )
        self.assertEqual(
            (observed["pid"], observed["pgid"], observed["sid"]),
            (record["pid"],) * 3,
        )
        self.assertEqual(observed["interpreter"], sys.executable)
        self.assertEqual(observed["cwd"], str(self.root))
        self.assertEqual(observed["path"], str(independent_tools))
        self.assertTrue(observed["stdin_closed"])

    def test_original_shell_forms_and_copy_permission_remain_independent(self) -> None:
        for role in ("xcrun", "uv", "xcodebuild", "rg"):
            self.dispatcher.executable(role, self.body(role))
        identities = [
            (self.tools / role).stat().st_ino
            for role in ("xcrun", "uv", "xcodebuild", "rg")
        ]
        self.assertEqual(len(set(identities)), 4)
        self.assertTrue(
            all(
                not (self.tools / role).is_symlink()
                for role in ("xcrun", "uv", "xcodebuild", "rg")
            )
        )
        (self.tools / "xcodebuild").chmod(0o600)
        (self.tools / "rg").unlink()
        for shell, role in (("/bin/sh", "uv"), ("/bin/bash", "xcrun")):
            script = self.root / f"{role}.sh"
            script.write_text(f"{role} \"space argument\" '$literal'\n")
            with patch.dict(os.environ, self.environment(), clear=True):
                result = self.command([shell, str(script)])
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                json.loads((self.root / f"{role}.json").read_text())["argv"][1:],
                ["space argument", "$literal"],
            )
        with patch.dict(os.environ, self.environment(), clear=True):
            for role, expected in (("xcodebuild", 126), ("rg", 127)):
                with self.subTest(role=role):
                    result = self.command(["/bin/sh", "-c", role])
                    self.assertEqual(result.returncode, expected)

    def test_missing_body_unknown_role_and_foreign_path_do_not_execute(self) -> None:
        self.dispatcher.executable("uv", self.body("uv"))
        (self.tools / "uv.body.py").unlink()
        unknown = self.tools / "unknown"
        shutil.copyfile(self.dispatcher.binary, unknown)
        unknown.chmod(0o700)
        with patch.dict(os.environ, self.environment(), clear=True):
            self.assertEqual(self.command([str(self.tools / "uv")]).returncode, 66)
            self.assertEqual(self.command([str(unknown)]).returncode, 64)
        foreign = self.root / "foreign"
        foreign.mkdir()
        (foreign / "uv.body.py").write_text(self.body("uv"))
        with patch.dict(os.environ, {"PATH": str(foreign)}, clear=True):
            self.assertEqual(self.command([str(self.tools / "uv")]).returncode, 64)
        self.dispatcher.executable("uv", self.body("uv"))
        self.dispatcher.executable("cargo", self.body("cargo"))
        foreign_image = foreign / "uv"
        shutil.copyfile(self.dispatcher.binary, foreign_image)
        foreign_image.chmod(0o700)
        for image, argv0 in (
            (foreign_image, self.tools / "uv"),
            (self.tools / "uv", self.tools / "cargo"),
        ):
            with self.subTest(image=str(image), argv0=str(argv0)):
                script = self.root / f"exec-image-{self.sequence:02}.py"
                intent = script.with_suffix(".json")
                native_argv = [str(argv0), "space $literal"]
                script.write_text(
                    "import json, os\nfrom pathlib import Path\n"
                    f"Path({str(intent)!r}).write_text(json.dumps({{'image': {str(image)!r}, 'argv': {native_argv!r}, 'pid': os.getpid(), 'pgid': os.getpgrp()}}))\n"
                    f"os.execv({str(image)!r}, {native_argv!r})\n"
                )
                with patch.dict(os.environ, self.environment(), clear=True):
                    result = self.command([sys.executable, str(script)])
                self.assertEqual(result.returncode, 64, result.stderr)
                observed = json.loads(intent.read_text())
                record = json.loads(
                    (self.root / f"command-{self.sequence:02}.json").read_text()
                )
                self.assertEqual(observed["pid"], record["pid"])
                self.assertEqual(observed["pgid"], record["pgid"])
        alias = self.root / "tools-alias"
        alias.symlink_to(self.tools, target_is_directory=True)
        with patch.dict(os.environ, {"PATH": str(alias)}, clear=True):
            self.assertEqual(self.command([str(self.tools / "uv")]).returncode, 64)
        self.assertFalse((self.root / "uv.json").exists())
        self.assertFalse((self.root / "cargo.json").exists())
        with self.assertRaises(FixtureDispatchError):
            self.dispatcher.executable("unknown", "raise AssertionError\n")

    def test_real_handler_signal_is_the_actual_process_status(self) -> None:
        self.dispatcher.executable(
            "cargo",
            "import json, os, signal\nfrom pathlib import Path\n"
            f"Path({str(self.root / 'signal.json')!r}).write_text(json.dumps({{'pid': os.getpid(), 'pgid': os.getpgrp()}}))\n"
            "os.kill(os.getpid(), signal.SIGTERM)\n",
        )
        with patch.dict(os.environ, self.environment(), clear=True):
            result = self.command([str(self.tools / "cargo")])
        self.assertEqual(result.returncode, -signal.SIGTERM)
        observed = json.loads((self.root / "signal.json").read_text())
        record = json.loads((self.root / "command-01.json").read_text())
        self.assertEqual(observed["pid"], record["pid"])
        self.assertEqual(observed["pgid"], record["pgid"])

    def test_compiler_absence_and_nonzero_fail_closed_with_exact_raw_record(
        self,
    ) -> None:
        for case, compiler in (("absent", None), ("nonzero", "/usr/bin/false")):
            tools = self.root / case
            tools.mkdir()
            with (
                self.subTest(case=case),
                patch(
                    "scripts.fixture_tool_dispatch.shutil.which", return_value=compiler
                ),
            ):
                with self.assertRaises(FixtureDispatchError):
                    FixtureToolDispatcher(tools, ("uv",))
            directory = tools / ".fixture-dispatch"
            record = json.loads((directory / "compilation.json").read_text())
            self.assertFalse((directory / "dispatch").exists())
            if compiler:
                self.assertEqual(record["command"]["exit"], 1)
                self.assert_settled(record["command"])
                self.assertEqual((directory / "compiler.stdout").read_bytes(), b"")
                self.assertEqual((directory / "compiler.stderr").read_bytes(), b"")

    def test_compiler_timeout_kills_ignoring_descendant_and_prevents_late_write(
        self,
    ) -> None:
        ready, late = self.root / "compiler-child.json", self.root / "late.txt"
        script = self.root / "compiler-control.py"
        script.write_text(
            "import json, os, signal, subprocess, sys, time\nfrom pathlib import Path\n"
            "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
            f"child = subprocess.Popen([sys.executable, '-c', {('import os, signal, time; from pathlib import Path; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(1); Path(' + repr(str(late)) + ').write_text("late")')!r}])\n"
            f"Path({str(ready)!r}).write_text(json.dumps({{'pid': os.getpid(), 'child': child.pid, 'pgid': os.getpgrp(), 'stdin_closed': os.read(0, 1) == b''}}))\n"
            "time.sleep(10)\n"
        )
        with self.assertRaises(subprocess.TimeoutExpired):
            self.command([sys.executable, str(script)], timeout=0.5)
        observed = json.loads(ready.read_text())
        self.assertTrue(observed["stdin_closed"])
        with self.assertRaises(ProcessLookupError):
            os.kill(observed["child"], 0)
        time.sleep(0.5)
        self.assertFalse(late.exists())
        record = json.loads((self.root / "command-01.json").read_text())
        self.assertIn(signal.SIGKILL, [row["signal"] for row in record["signals"]])

    def test_compiler_overflow_caps_retained_bytes_and_settles(self) -> None:
        for name, descriptor in (("stdout", 1), ("stderr", 2)):
            with self.subTest(stream=name):
                script = self.root / f"overflow-{name}.py"
                script.write_text(
                    f"import os\nos.write({descriptor}, b'x' * (2 * 1024 * 1024))\n"
                )
                with self.assertRaises(FixtureDispatchError):
                    self.command([sys.executable, str(script)])
                prefix = self.root / f"command-{self.sequence:02}"
                self.assertEqual(
                    len(Path(str(prefix) + f".{name}").read_bytes()),
                    CompilerCommand.LIMITS[name],
                )
                self.assertEqual(
                    json.loads(prefix.with_suffix(".json").read_text())[
                        "output_excess"
                    ],
                    name,
                )
