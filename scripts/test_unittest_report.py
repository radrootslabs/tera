"""Exercise the standalone producer with actual stdlib tests and disposable roots."""

from __future__ import annotations

import importlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SECRET = "private exception, skip reason, and subtest parameter"
SCRIPTS = Path(__file__).resolve().parent


class StandaloneReportTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="tera-unittest-report-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        (self.root / "scripts").mkdir()
        subprocess.run(
            ["git", "init", "--quiet", str(self.root)], check=True, timeout=10
        )
        shutil.copyfile(
            SCRIPTS / "unittest_report.py", self.root / "scripts/unittest_report.py"
        )
        self.output = self.root / "report.json"

    def module(self, source, name="test_observed"):
        path = self.root / "scripts" / (name + ".py")
        path.write_text("import unittest\nSECRET = " + repr(SECRET) + "\n" + source)
        return "scripts." + name

    def run_process(self, modules, output=None):
        arguments = [
            sys.executable,
            "-m",
            "scripts.unittest_report",
            "--report",
            str(output or self.output),
            *modules,
        ]
        record = self.probe_record(arguments, modules)
        result = subprocess.run(
            arguments,
            cwd=self.root,
            capture_output=True,
            text=True,
            timeout=30,
            env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
        )
        if record is not None:
            (record / "exit.txt").write_text(str(result.returncode))
            (record / "stdout.txt").write_text(result.stdout)
            (record / "stderr.txt").write_text(result.stderr)
            raw = (
                self.output.read_bytes()
                if self.output.is_file() and not self.output.is_symlink()
                else b""
            )
            (record / "report.json").write_bytes(raw)
        return result

    def probe_record(self, arguments, modules):
        evidence = os.environ.get("TERA_REPORT_TEST_EVIDENCE")
        if not evidence:
            return None
        record = Path(tempfile.mkdtemp(prefix="probe-", dir=evidence))
        (record / "argv.json").write_text(json.dumps(arguments))
        (record / "cwd.txt").write_text(str(self.root))
        (record / "producer.py").write_bytes(
            (self.root / "scripts/unittest_report.py").read_bytes()
        )
        for index, module in enumerate(modules[:4096]):
            if len(module) <= 255 and module.startswith("scripts.test_"):
                path = self.root / "scripts" / (module[8:] + ".py")
                if path.is_file() and not path.is_symlink():
                    (record / ("module-" + str(index) + ".py")).write_bytes(
                        path.read_bytes()
                    )
        return record

    def run_report(self, modules, output=None):
        result = self.run_process(modules, output)
        raw = self.output.read_bytes() if self.output.is_file() else b""
        document = json.loads(raw) if raw else None
        if document is not None:
            self.assertNotIn(SECRET, json.dumps(document))
            self.assertEqual(
                set(document), {"schema", "test_ids", "tests_run", "callbacks"}
            )
        return result, document

    def test_genuine_pass_and_successful_subtests_have_one_identity(self):
        module = self.module("""
class ObservedTests(unittest.TestCase):
    def test_one(self):
        for index in range(3):
            with self.subTest(private=SECRET, index=index):
                self.assertTrue(True)
""")
        result, document = self.run_report([module])
        self.assertEqual(result.returncode, 0, result.stderr)
        identity = module + ".ObservedTests.test_one"
        self.assertEqual(document["test_ids"], [identity])
        self.assertEqual(document["tests_run"], 1)
        self.assertEqual(
            [row["callback"] for row in document["callbacks"]],
            [
                "startTestRun",
                "startTest",
                "addSubTest",
                "addSubTest",
                "addSubTest",
                "addSuccess",
                "stopTest",
                "stopTestRun",
            ],
        )
        self.assertEqual(
            [
                row["ordinal"]
                for row in document["callbacks"]
                if row["callback"] == "addSubTest"
            ],
            [1, 2, 3],
        )

    def test_actual_nonpassing_test_outcomes_are_observed(self):
        modes = {
            "failure": ("self.fail(SECRET)", "addFailure"),
            "error": ("raise RuntimeError(SECRET)", "addError"),
            "skip": ("self.skipTest(SECRET)", "addSkip"),
            "cleanup": ("self.addCleanup(lambda: self.fail(SECRET))", "addFailure"),
            "cleanup_error": ("self.addCleanup(lambda: int(SECRET))", "addError"),
            "expected": ("self.fail(SECRET)", "addExpectedFailure"),
            "unexpected": ("pass", "addUnexpectedSuccess"),
        }
        for mode, (body, callback) in modes.items():
            with self.subTest(mode=mode):
                decorator = (
                    "    @unittest.expectedFailure\n"
                    if mode in {"expected", "unexpected"}
                    else ""
                )
                module = self.module(
                    "class ObservedTests(unittest.TestCase):\n"
                    + decorator
                    + "    def test_one(self):\n        "
                    + body
                    + "\n"
                )
                self.output.unlink(missing_ok=True)
                result, document = self.run_report([module])
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    callback, [row["callback"] for row in document["callbacks"]]
                )
                self.assertEqual(document["callbacks"][-1], {"callback": "stopTestRun"})

    def test_actual_subtest_failure_error_and_skip_redact_parameters(self):
        for body, callback in [
            ("self.fail(SECRET)", "addSubTest"),
            ("raise RuntimeError(SECRET)", "addSubTest"),
            ("self.skipTest(SECRET)", "addSkip"),
        ]:
            with self.subTest(callback=callback, body=body):
                module = self.module(
                    "class ObservedTests(unittest.TestCase):\n    def test_one(self):\n        with self.subTest(private=SECRET):\n            "
                    + body
                    + "\n"
                )
                self.output.unlink(missing_ok=True)
                result, document = self.run_report([module])
                self.assertNotEqual(result.returncode, 0)
                event = next(
                    row for row in document["callbacks"] if row["callback"] == callback
                )
                self.assertEqual(event["test_id"], module + ".ObservedTests.test_one")
                if callback == "addSubTest":
                    self.assertFalse(event["successful"])

    def fixture_source(self, scope, phase, skip=False):
        action = (
            "raise unittest.SkipTest(SECRET)" if skip else "raise RuntimeError(SECRET)"
        )
        if scope == "module":
            fixture = "def " + phase + "():\n    " + action + "\n"
            return (
                fixture
                + "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n"
            )
        return (
            "class ObservedTests(unittest.TestCase):\n    @classmethod\n    def "
            + phase
            + "(cls):\n        "
            + action
            + "\n    def test_one(self): pass\n"
        )

    def test_actual_module_and_class_fixture_errors_and_skips(self):
        for scope, phases in [
            ("module", ["setUpModule", "tearDownModule"]),
            ("class", ["setUpClass", "tearDownClass"]),
        ]:
            for phase in phases:
                for skip in [False, True]:
                    with self.subTest(scope=scope, phase=phase, skip=skip):
                        module = self.module(self.fixture_source(scope, phase, skip))
                        self.output.unlink(missing_ok=True)
                        result, document = self.run_report([module])
                        self.assertNotEqual(result.returncode, 0)
                        self.assertIn(
                            "addSkip" if skip else "addError",
                            [row["callback"] for row in document["callbacks"]],
                        )
                        event = next(
                            row
                            for row in document["callbacks"]
                            if row["callback"] == ("addSkip" if skip else "addError")
                        )
                        owner = module + (".ObservedTests" if scope == "class" else "")
                        self.assertEqual(event["test_id"], phase + " (" + owner + ")")
                        self.assertEqual(
                            document["callbacks"][-1], {"callback": "stopTestRun"}
                        )

    def test_actual_class_and_module_cleanup_errors(self):
        for source in [
            "def setUpModule():\n    unittest.addModuleCleanup(lambda: int(SECRET))\nclass ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n",
            "class ObservedTests(unittest.TestCase):\n    @classmethod\n    def setUpClass(cls): cls.addClassCleanup(lambda: int(SECRET))\n    def test_one(self): pass\n",
        ]:
            with self.subTest(source=source):
                module = self.module(source)
                self.output.unlink(missing_ok=True)
                result, document = self.run_report([module])
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    "addError", [row["callback"] for row in document["callbacks"]]
                )

    def test_actual_early_stop_and_interruption_preserve_partial_inventory(self):
        for body in ["self._outcome.result.stop()", "raise KeyboardInterrupt(SECRET)"]:
            with self.subTest(body=body):
                module = self.module(
                    "class ObservedTests(unittest.TestCase):\n    def test_one(self):\n        "
                    + body
                    + "\n    def test_two(self): pass\n"
                )
                self.output.unlink(missing_ok=True)
                result, document = self.run_report([module])
                self.assertNotEqual(result.returncode, 0)
                self.assertIsNotNone(document, result.stderr)
                self.assertEqual(len(document["test_ids"]), 2)
                self.assertEqual(document["tests_run"], 1)
                self.assertEqual(document["callbacks"][-1], {"callback": "stopTestRun"})

    def test_multiple_modules_have_real_completed_tests(self):
        source = (
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n"
        )
        modules = [self.module(source), self.module(source, "test_secondary")]
        result, document = self.run_report(modules)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(document["tests_run"], 2)
        self.assertEqual(len(document["test_ids"]), 2)

    def test_empty_module_and_duplicate_test_inventory_are_rejected(self):
        sources = [
            "pass\n",
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\ndef load_tests(loader, tests, pattern):\n    return unittest.TestSuite([ObservedTests('test_one'), ObservedTests('test_one')])\n",
        ]
        for source in sources:
            with self.subTest(source=source):
                result, _ = self.run_report([self.module(source)])
                self.assertNotEqual(result.returncode, 0)

    def test_selectors_are_bounded_canonical_unique_modules(self):
        module = self.module(
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n"
        )
        for selectors in [
            [module, module],
            [],
            ["scripts/test_observed.py"],
            ["scripts.test_"],
            ["foreign.test_observed"],
            ["scripts.test_" + "x" * 300],
            [module] * 4097,
        ]:
            with self.subTest(selectors=selectors[:2]):
                result, _ = self.run_report(selectors)
                self.assertNotEqual(result.returncode, 0)

    def test_selected_origins_cannot_be_symlinks_directories_or_foreign_ids(self):
        module = self.module(
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n"
        )
        path = self.root / "scripts/test_observed.py"
        foreign = self.root / "foreign.py"
        path.rename(foreign)
        path.symlink_to(foreign)
        result, _ = self.run_report([module])
        self.assertNotEqual(result.returncode, 0)
        path.unlink()
        path.mkdir()
        result, _ = self.run_report([module])
        self.assertNotEqual(result.returncode, 0)
        path.rmdir()
        self.module(
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\nObservedTests.__module__ = 'foreign.test_observed'\n"
        )
        result, _ = self.run_report([module])
        self.assertNotEqual(result.returncode, 0)

    def test_producer_origin_cannot_be_symlinked(self):
        path = self.root / "scripts/unittest_report.py"
        foreign = self.root / "foreign.py"
        path.rename(foreign)
        path.symlink_to(foreign)
        result, _ = self.run_report([self.module("pass\n")])
        self.assertNotEqual(result.returncode, 0)

    def test_safe_output_is_exclusive_and_rejects_symlink_parents(self):
        module = self.module(
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n"
        )
        self.output.write_bytes(b"preserved")
        result = self.run_process([module])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.output.read_bytes(), b"preserved")
        self.output.unlink()
        self.output.symlink_to(self.root / "foreign.json")
        result, _ = self.run_report([module])
        self.assertNotEqual(result.returncode, 0)
        self.output.unlink()
        (self.root / "alias").symlink_to(
            self.root / "scripts", target_is_directory=True
        )
        result, _ = self.run_report([module], self.root / "alias/report.json")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "scripts/report.json").exists())

    def test_source_and_output_replacement_during_execution_fail_closed(self):
        sources = [
            "from pathlib import Path\nclass ObservedTests(unittest.TestCase):\n    def test_one(self):\n        Path(__file__).write_text('pass\\n')\n",
            "from pathlib import Path\nclass ObservedTests(unittest.TestCase):\n    def test_one(self):\n        Path('report.json').unlink()\n        Path('report.json').write_text('replacement')\n",
        ]
        for source in sources:
            with self.subTest(source=source):
                self.output.unlink(missing_ok=True)
                result = self.run_process([self.module(source)])
                self.assertNotEqual(result.returncode, 0)

    def test_real_same_inode_output_tampering_is_rejected(self):
        for action in [
            "path.write_bytes(b'PRIVATE_OUTPUT_SENTINEL')",
            "path.write_bytes(b'PRIVATE_OUTPUT_SENTINEL' * 120000)",
            "path.write_bytes(b'PRIVATE_OUTPUT_SENTINEL'); path.write_bytes(b'')",
        ]:
            with self.subTest(action=action):
                self.output.unlink(missing_ok=True)
                module = self.module(
                    "from pathlib import Path\nclass ObservedTests(unittest.TestCase):\n"
                    "    def test_one(self):\n        path = Path('report.json')\n"
                    "        before = path.stat().st_ino\n        " + action + "\n"
                    "        self.assertEqual(path.stat().st_ino, before)\n"
                    "        Path('testcase-passed').write_text('passed')\n"
                )
                result = self.run_process([module])
                self.assertEqual((self.root / "testcase-passed").read_text(), "passed")
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("PRIVATE_OUTPUT_SENTINEL", result.stderr)

    def test_output_settlement_rechecks_content_and_position(self):
        # Inject a mutation after the real writer returns, before actual close.
        for action in [
            "os.write(output.descriptor, b'PRIVATE_OUTPUT_SENTINEL')",
            "os.ftruncate(output.descriptor, 0)",
            "os.pwrite(output.descriptor, b'X', 0)",
            "os.lseek(output.descriptor, 0, os.SEEK_SET)",
        ]:
            with self.subTest(action=action):
                self.output.unlink(missing_ok=True)
                module = self.module(
                    "import os, sys\nclass ObservedTests(unittest.TestCase):\n"
                    "    def test_one(self):\n"
                    "        owner = sys.modules['__main__']\n"
                    "        original = owner.ReportOutput.write\n"
                    "        def changed(output, document):\n"
                    "            original(output, document)\n            "
                    + action
                    + "\n        owner.ReportOutput.write = changed\n"
                    "        from pathlib import Path\n"
                    "        Path('testcase-passed').write_text('passed')\n"
                )
                result = self.run_process([module])
                self.assertEqual((self.root / "testcase-passed").read_text(), "passed")
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("PRIVATE_OUTPUT_SENTINEL", result.stderr)

    def test_real_callback_limit_is_exact_and_does_not_append_over_bound(self):
        for extra in [0, 1]:
            with self.subTest(extra=extra):
                module = self.module(
                    "class ObservedTests(unittest.TestCase):\n    def test_one(self):\n        for _ in range("
                    + str(4091 + extra)
                    + "):\n            with self.subTest(private=SECRET): pass\n"
                )
                self.output.unlink(missing_ok=True)
                result, document = self.run_report([module])
                self.assertEqual(result.returncode == 0, extra == 0, result.stderr)
                self.assertLessEqual(len(document["callbacks"]), 4096)
                if extra == 0:
                    self.assertEqual(len(document["callbacks"]), 4096)

    def test_real_inventory_limit_is_separate_from_callback_limit(self):
        for count in [4096, 4097]:
            with self.subTest(count=count):
                module = self.module(
                    "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\ndef load_tests(loader, tests, pattern):\n    case = type('ManyTests', (unittest.TestCase,), {'__module__': __name__, **{f'test_{index}': lambda self: None for index in range("
                    + str(count)
                    + ")}})\n    return loader.loadTestsFromTestCase(case)\n"
                )
                self.output.unlink(missing_ok=True)
                result, document = self.run_report([module])
                self.assertNotEqual(result.returncode, 0)
                if count == 4096:
                    self.assertEqual(len(document["test_ids"]), 4096)
                    self.assertLessEqual(len(document["callbacks"]), 4096)

    def report_shape(self, identity, subtests):
        callbacks = [
            {"callback": "startTestRun"},
            {"callback": "startTest", "test_id": identity},
        ]
        callbacks.extend(
            {
                "callback": "addSubTest",
                "test_id": identity,
                "ordinal": index + 1,
                "successful": True,
            }
            for index in range(subtests)
        )
        callbacks.extend(
            [
                {"callback": "addSuccess", "test_id": identity},
                {"callback": "stopTest", "test_id": identity},
                {"callback": "stopTestRun"},
            ]
        )
        return {
            "schema": "tera.unittest-callback-report.v1",
            "test_ids": [identity],
            "tests_run": 1,
            "callbacks": callbacks,
        }

    def test_actual_report_accepts_two_million_bytes_and_rejects_overflow(self):
        prefix = "scripts.test_observed.ManyTests.test_"
        for others in range(4):
            document = self.report_shape(prefix, 0)
            for index in range(others):
                other = self.report_shape(prefix + "a_" + str(index), 0)
                document["test_ids"].insert(index, other["test_ids"][0])
                document["callbacks"][1:1] = other["callbacks"][1:-1]
                document["tests_run"] += 1
            baseline = len(json.dumps(document, sort_keys=True, separators=(",", ":")))
            padding, remainder = divmod(2_000_000 - baseline, 4)
            if remainder == 0:
                break
        self.assertEqual(remainder, 0)
        for extra in [0, 1]:
            with self.subTest(extra=extra):
                source = (
                    "class ManyTests(unittest.TestCase): pass\nfor index in range("
                    + str(others)
                    + "):\n    setattr(ManyTests, 'test_a_' + str(index), lambda self: None)\nsetattr(ManyTests, 'test_' + 'x' * "
                    + str(padding + extra)
                    + ", lambda self: None)\n"
                )
                module = self.module(source)
                self.output.unlink(missing_ok=True)
                result, document = self.run_report([module])
                self.assertEqual(result.returncode == 0, extra == 0, result.stderr)
                self.assertLessEqual(len(self.output.read_bytes()), 2_000_000)
                if extra == 0:
                    self.assertEqual(len(self.output.read_bytes()), 2_000_000)
                    self.assertEqual(document["tests_run"], others + 1)

    def test_partial_testcase_without_success_is_not_accepted(self):
        module = self.module(
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n    def run(self, result):\n        result.startTest(self)\n        result.stopTest(self)\n"
        )
        result, document = self.run_report([module])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(document["tests_run"], 1)
        self.assertNotIn(
            "addSuccess", [row["callback"] for row in document["callbacks"]]
        )

    def test_selected_code_cannot_exit_successfully_during_import(self):
        result, _ = self.run_report([self.module("raise SystemExit(0)\n")])
        self.assertNotEqual(result.returncode, 0)

    def test_selected_module_does_not_execute_forged_cached_bytecode(self):
        module = self.module(
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n"
        )
        source = self.root / "scripts/test_observed.py"
        code = compile("raise SystemExit(0)", str(source), "exec")
        import importlib._bootstrap_external

        cached = Path(importlib.util.cache_from_source(str(source)))
        cached.parent.mkdir()
        cached.write_bytes(
            importlib._bootstrap_external._code_to_timestamp_pyc(
                code, int(source.stat().st_mtime), source.stat().st_size
            )
        )
        result, document = self.run_report([module])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(document["tests_run"], 1)

    def test_foreign_resolution_is_rejected_before_module_execution(self):
        (self.root / "foreign").mkdir()
        (self.root / "foreign/test_secondary.py").write_text(
            "from pathlib import Path\nPath('foreign-executed').write_text('executed')\n"
        )
        first = self.module(
            "import scripts\nfrom pathlib import Path\nscripts.__path__ = [str(Path.cwd() / 'foreign')]\nclass ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n"
        )
        second = self.module(
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n",
            "test_secondary",
        )
        result, _ = self.run_report([first, second])
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "foreign-executed").exists())

    def test_output_directory_rename_cannot_redirect_descriptor_writes(self):
        module = self.module(
            "from pathlib import Path\nclass ObservedTests(unittest.TestCase):\n    def test_one(self):\n        Path('output').rename('moved')\n        Path('output').mkdir()\n        Path('output/report.json').write_text('replacement')\n"
        )
        (self.root / "output").mkdir()
        result = self.run_process([module], self.root / "output/report.json")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.root / "output/report.json").read_text(), "replacement")
        self.assertEqual((self.root / "moved/report.json").read_bytes(), b"")

    def test_duplicate_run_boundary_and_invalid_test_id_are_nonpassing(self):
        for source in [
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): self._outcome.result.startTestRun()\n",
            "class ObservedTests(unittest.TestCase):\n    def test_one(self): pass\n    def id(self): return SECRET\n",
        ]:
            with self.subTest(source=source):
                self.output.unlink(missing_ok=True)
                result, _ = self.run_report([self.module(source)])
                self.assertNotEqual(result.returncode, 0)


class OutputIntegrityTests(unittest.TestCase):
    """Injected timing controls around actual writes of genuine observations."""

    def setUp(self):
        self.producer = importlib.import_module("scripts.unittest_report")
        temporary = tempfile.TemporaryDirectory(prefix="tera-output-integrity-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        case = type(
            "ObservedTests",
            (unittest.TestCase,),
            {"__module__": "scripts.test_observed", "test_one": lambda self: None},
        )("test_one")
        suite, budget = unittest.TestSuite([case]), self.producer.ReportBudget()
        inventory = self.producer.inventory(suite, ["scripts.test_observed"], budget)
        result, passing = self.producer.observe(suite, inventory, budget, io.StringIO())
        self.assertTrue(passing)
        self.document = {
            "schema": self.producer.SCHEMA,
            "test_ids": inventory,
            "tests_run": result.testsRun,
            "callbacks": result.callbacks,
        }

    def output(self, name):
        path = self.root / (name + ".json")
        output = self.producer.ReportOutput(str(path))
        self.addCleanup(os.close, output.directory)
        self.addCleanup(os.close, output.descriptor)
        evidence = os.environ.get("TERA_REPORT_TEST_EVIDENCE")
        if evidence:
            record = Path(
                tempfile.mkdtemp(prefix="injected-" + name + "-", dir=evidence)
            )
            self.addCleanup(shutil.copyfile, path, record / "report.json")
        return output

    def mutate(self, output, mode):
        if mode == "append":
            os.pwrite(
                output.descriptor, SECRET.encode(), os.fstat(output.descriptor).st_size
            )
        elif mode == "truncate":
            os.ftruncate(output.descriptor, 0)
        elif mode == "same_size":
            os.pwrite(output.descriptor, b"X", 0)
        else:
            os.lseek(output.descriptor, 0, os.SEEK_SET)

    def test_actual_write_races_reject_append_truncate_content_and_offset(self):
        original = os.write
        for mode in ["append", "truncate", "same_size", "offset"]:
            with self.subTest(mode=mode):
                output, calls = self.output("write-" + mode), []

                def changed(descriptor, raw):
                    count = original(descriptor, raw)
                    if not calls:
                        calls.append(count)
                        self.mutate(output, mode)
                    return count

                with patch.object(self.producer.os, "write", changed):
                    with self.assertRaises(self.producer.ReportError) as failure:
                        output.write(self.document)
                self.assertNotIn(SECRET, str(failure.exception))
                self.assertTrue(calls)

    def test_prewrite_descriptor_offset_is_rejected(self):
        output = self.output("prewrite-offset")
        os.lseek(output.descriptor, 3, os.SEEK_SET)
        with self.assertRaises(self.producer.ReportError):
            output.write(self.document)
        self.assertEqual(os.fstat(output.descriptor).st_size, 0)

    def test_actual_short_writes_preserve_exact_emitted_bytes(self):
        output, original = self.output("short-write"), os.write

        def partial(descriptor, raw):
            return original(descriptor, raw[:7])

        with patch.object(self.producer.os, "write", partial):
            output.write(self.document)
        output.verify()
        raw = (self.root / "short-write.json").read_bytes()
        self.assertEqual(
            raw, self.producer.ENCODER.encode(self.document).encode("ascii")
        )
        self.assertEqual(os.lseek(output.descriptor, 0, os.SEEK_CUR), len(raw))

    def test_descriptor_read_races_are_bounded_and_fenced(self):
        original = os.pread
        for mode in ["same_size", "offset"]:
            with self.subTest(mode=mode):
                output, reads = self.output("read-" + mode), []

                def changed(descriptor, count, offset):
                    raw = original(descriptor, count, offset)
                    reads.append((count, offset))
                    if len(reads) == 1:
                        self.mutate(output, mode)
                    return raw

                with patch.object(self.producer.os, "pread", changed):
                    with self.assertRaises(self.producer.ReportError):
                        output.write(self.document)
                self.assertTrue(reads)
                self.assertTrue(all(0 < count <= 65536 for count, _ in reads))


class ObserverBoundTests(unittest.TestCase):
    def test_byte_budget_accepts_exact_limit_and_rejects_next_byte(self):
        producer = importlib.import_module("scripts.unittest_report")
        budget = producer.ReportBudget()
        budget.size = producer.MAX_BYTES - 1
        budget.add(1)
        self.assertEqual(budget.size, producer.MAX_BYTES)
        with self.assertRaises(producer.ReportError):
            budget.add(1)
        self.assertEqual(budget.size, producer.MAX_BYTES)

    def test_observer_never_retains_exception_skip_or_subtest_values(self):
        producer = importlib.import_module("scripts.unittest_report")
        module = "scripts.test_observed"

        def method(case):
            with case.subTest(private=SECRET):
                case.skipTest(SECRET)

        case_type = type(
            "ObservedTests",
            (unittest.TestCase,),
            {"__module__": module, "test_one": method},
        )
        case = case_type("test_one")
        budget = producer.ReportBudget()
        inventory = producer.inventory(unittest.TestSuite([case]), [module], budget)
        result, passing = producer.observe(
            unittest.TestSuite([case]), inventory, budget, io.StringIO()
        )
        self.assertFalse(passing)
        self.assertEqual(result.skipped, [])
        self.assertNotIn(SECRET, repr(result.__dict__))

    def test_imported_module_origin_is_checked_after_loading(self):
        producer = importlib.import_module("scripts.unittest_report")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "scripts").mkdir()
            (root / "scripts/test_observed.py").write_text("pass\n")
            with patch.object(producer.importlib.util, "find_spec") as find:
                find.return_value.origin = str(root / "foreign.py")
                with self.assertRaises(producer.ReportError):
                    producer.load_modules(root, ["scripts.test_observed"])


if __name__ == "__main__":
    unittest.main()
