"""Actual nested unittest execution, owned ancestry and adversarial controls."""

from __future__ import annotations

import json
import os
import shutil
import stat
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts import test_unittest_report as support

PASS = """import unittest
from pathlib import Path
class NestedTests(unittest.TestCase):
    def test_authored_nested_method_executes(self):
        Path(__name__ + '.executed').write_text(self.id())
        with self.subTest(index=1):
            self.assertEqual(2 + 2, 4)
"""


class NestedReportTests(unittest.TestCase):
    run_report = support.StandaloneReportTests.run_report
    pid_absent = staticmethod(support.StandaloneReportTests.pid_absent)

    def setUp(self):
        support.StandaloneReportTests.setUp(self)
        self.authored_sources = {}

    def run_process(self, modules, output=None):
        result = support.StandaloneReportTests.run_process(self, modules, output)
        record = getattr(self, "last_probe_record", None)
        if record:
            for marker in self.root.glob("*.executed"):
                shutil.copyfile(marker, record / marker.name)
        return result

    def module(self, source=PASS, name="controls.test_nested", *, initializers=False):
        relative = Path("scripts", *name.split(".")).with_suffix(".py")
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if initializers:
            for directory in [self.root / "scripts", *path.parents]:
                if directory.is_relative_to(self.root / "scripts"):
                    (directory / "__init__.py").write_bytes(b"")
        path.write_text(source)
        self.authored_sources[path] = path.read_bytes()
        return "scripts." + name

    def probe_record(self, arguments, modules):
        del modules
        evidence = os.environ.get("TERA_REPORT_TEST_EVIDENCE")
        if not evidence:
            return None
        record = Path(tempfile.mkdtemp(prefix="nested-probe-", dir=evidence))
        self.last_probe_record = record
        (record / "argv.json").write_text(json.dumps(arguments))
        (record / "cwd.txt").write_text(str(self.root))
        (record / "test_id.txt").write_text(self.id())
        shapes = []
        for path in (self.root / "scripts").rglob("*.py"):
            relative = path.relative_to(self.root)
            row = {"path": str(relative), "mode": path.lstat().st_mode}
            shapes.append(row)
            self.copy_fixture(path, record / relative, row)
        (record / "source-shapes.json").write_text(json.dumps(shapes))
        return record

    def copy_fixture(self, path, output, row):
        output.parent.mkdir(parents=True, exist_ok=True)
        if path.is_symlink():
            row["symlink_target"] = os.readlink(path)
        elif stat.S_ISREG(path.lstat().st_mode):
            try:
                output.write_bytes(path.read_bytes())
            except PermissionError:
                row["unreadable"] = True
                if path in self.authored_sources:
                    output.write_bytes(self.authored_sources[path])

    def assert_execution(self, module, document):
        identity = module + ".NestedTests.test_authored_nested_method_executes"
        self.assertIn(identity, document["test_ids"])
        self.assertEqual((self.root / (module + ".executed")).read_text(), identity)

    def test_namespace_nested_authored_test_really_executes(self):
        module = self.module()
        result, document = self.run_report([module])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(document["schema"], "tera.unittest-callback-report.v2")
        self.assert_execution(module, document)

    def test_empty_initializers_and_mixed_flat_nested_selections_execute(self):
        modules = [
            self.module(initializers=True, name="controls.deep.test_nested"),
            self.module(name="test_flat"),
        ]
        result, document = self.run_report(modules)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(document["schema"], "tera.unittest-callback-report.v2")
        self.assertEqual(document["tests_run"], 2)
        for module in modules:
            self.assert_execution(module, document)

    def rejected(self, modules):
        self.output.unlink(missing_ok=True)
        result, document = self.run_report(modules)
        self.assertNotEqual(result.returncode, 0)
        return document

    def test_flat_only_nested_fixture_retains_v1(self):
        module = self.module(name="test_flat")
        result, document = self.run_report([module])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(document["schema"], "tera.unittest-callback-report.v1")
        self.assert_execution(module, document)

    def test_nested_selector_shape_bounds_and_duplicates_fail_closed(self):
        module = self.module()
        for names in [
            [module, module],
            [module] * 4097,
            ["scripts.controls.test_"],
            ["scripts.controls.9bad.test_nested"],
            ["scripts..controls.test_nested"],
            ["scripts.controls.test_nested.method"],
            ["scripts.controls.test_" + "x" * 256],
            ["scripts/controls/test_nested.py"],
        ]:
            with self.subTest(selector=names[0]):
                self.rejected(names)

    def test_symlinked_package_directory_is_rejected(self):
        module = self.module()
        path = self.root / "scripts/controls"
        path.rename(self.root / "foreign")
        path.symlink_to(self.root / "foreign", target_is_directory=True)
        self.rejected([module])
        self.assertFalse((self.root / (module + ".executed")).exists())

    def test_symlinked_initializer_and_selected_module_are_rejected(self):
        module = self.module(initializers=True)
        for name in ["__init__.py", "test_nested.py"]:
            with self.subTest(name=name):
                path = self.root / "scripts/controls" / name
                before = path.read_bytes()
                foreign = self.root / "foreign.py"
                foreign.write_bytes(before)
                path.unlink()
                path.symlink_to(foreign)
                self.rejected([module])
                path.unlink()
                path.write_bytes(before)

    def test_missing_unreadable_and_fifo_selected_source_is_rejected(self):
        module = self.module()
        path = self.root / "scripts/controls/test_nested.py"
        before = path.read_bytes()
        path.unlink()
        self.rejected([module])
        os.mkfifo(path)
        self.rejected([module])
        path.unlink()
        path.write_bytes(before)
        path.chmod(0)
        try:
            self.rejected([module])
        finally:
            path.chmod(0o600)

    def test_initializer_changes_during_import_never_run_selected_test(self):
        module = self.module(initializers=True)
        path = self.root / "scripts/controls/__init__.py"
        for action in [
            "Path(__file__).write_bytes(b'')",
            "Path('scripts/controls/test_nested.py').write_text('pass\\n')",
            "__path__ = [str(Path.cwd() / 'foreign')]",
            "__spec__.origin = str(Path.cwd() / 'foreign.py')",
        ]:
            with self.subTest(action=action):
                self.module(initializers=True)
                path.write_text("from pathlib import Path\n" + action + "\n")
                self.rejected([module])
                self.assertFalse((self.root / (module + ".executed")).exists())

    def test_ancestry_source_presence_mode_and_identity_drift_is_rejected(self):
        for action in [
            "Path('scripts/controls/__init__.py').write_text('new = True\\n')",
            "Path('scripts/controls/__init__.py').chmod(0o400)",
            "Path('scripts/controls').rename('moved'); Path('scripts/controls').mkdir()",
            "Path(__file__).write_text('pass\\n')",
            "sys.modules['scripts.controls'].__path__ = ['foreign']",
            "sys.modules['scripts.controls'].__spec__.origin = 'foreign.py'",
            "sys.modules[__name__] = None",
            "globals()['__name__'] = 'foreign.test_drift'",
            "__spec__.name = 'foreign.test_drift'",
            "__spec__.loader = None",
        ]:
            with self.subTest(action=action):
                source = "import sys\n" + PASS.replace(
                    "        with self.subTest(index=1):",
                    "        " + action + "\n        with self.subTest(index=1):",
                )
                module = self.module(
                    source, name="controls.test_drift", initializers=True
                )
                self.rejected([module])
                shutil.rmtree(self.root / "scripts/controls")

    def test_empty_initializer_added_during_execution_is_rejected(self):
        module = self.module(
            PASS.replace(
                "        with self.subTest(index=1):",
                "        Path('scripts/controls/__init__.py').touch()\n        with self.subTest(index=1):",
            )
        )
        self.rejected([module])

    def test_owned_initializer_executes_source_instead_of_forged_bytecode(self):
        import importlib._bootstrap_external
        import importlib.util

        module = self.module(initializers=True)
        initializer = self.root / "scripts/controls/__init__.py"
        initializer.write_text(
            "from pathlib import Path\nPath('initializer-executed').write_text('actual source')\n"
        )
        cached = Path(importlib.util.cache_from_source(str(initializer)))
        cached.parent.mkdir()
        forged = compile("raise SystemExit(0)", str(initializer), "exec")
        cached.write_bytes(
            importlib._bootstrap_external._code_to_timestamp_pyc(
                forged,
                int(initializer.stat().st_mtime),
                initializer.stat().st_size,
            )
        )
        result, document = self.run_report([module])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            (self.root / "initializer-executed").read_text(), "actual source"
        )
        self.assert_execution(module, document)

    def test_foreign_package_shadowing_is_rejected_before_foreign_import(self):
        module = self.module()
        foreign = self.root / "foreign"
        foreign.mkdir()
        (foreign / "test_other.py").write_text(
            "from pathlib import Path\nPath('foreign-executed').touch()\n"
        )
        first = self.module(
            "import scripts\nfrom pathlib import Path\nscripts.__path__ = [str(Path.cwd() / 'foreign')]\n"
            + PASS,
            name="test_first",
        )
        self.rejected([first, module])
        self.assertFalse((self.root / "foreign-executed").exists())

    def test_nested_partial_nonpassing_duplicate_and_fabricated_lifecycles_reject(self):
        for source in [
            PASS.replace(
                "self.assertEqual(2 + 2, 4)", "self.skipTest('private reason')"
            ),
            PASS.replace("self.assertEqual(2 + 2, 4)", "self.fail('private reason')"),
            PASS.replace(
                "self.assertEqual(2 + 2, 4)", "self._outcome.result.startTestRun()"
            ),
            PASS
            + "\ndef load_tests(loader, tests, pattern):\n    return unittest.TestSuite([NestedTests('test_authored_nested_method_executes')] * 2)\n",
            PASS
            + "\n    def id(self): return 'scripts.controls.test_nested.NestedTests.forged'\n",
            PASS
            + "\n    def run(self, result):\n        result.startTest(self)\n        result.addSuccess(self)\n        result.stopTest(self)\n",
            PASS.replace("self.assertEqual(2 + 2, 4)", "self._outcome.result.stop()")
            + "\n    def test_second(self): pass\n",
        ]:
            with self.subTest(source=source):
                self.rejected([self.module(source)])

    def test_monkey_patched_stdlib_testcase_identity_is_rejected(self):
        module = self.module(
            PASS
            + "\nunittest.TestCase.id = lambda self: 'scripts.controls.test_nested.NestedTests.forged'\n"
        )
        self.rejected([module])

    def test_root_initializer_cannot_prepatch_a_false_testcase_identity(self):
        module = self.module(initializers=True)
        (self.root / "scripts/__init__.py").write_text(
            "import unittest\nunittest.TestCase.id = lambda self: "
            "'scripts.controls.test_nested.NestedTests.forged'\n"
        )
        self.rejected([module])

    def test_monkey_patched_observer_cannot_forge_successful_execution(self):
        source = "import sys\n" + PASS.replace(
            "self.assertEqual(2 + 2, 4)",
            "sys.modules['__main__'].CallbackResult.passing = lambda result: True\n"
            "            self.fail('private failure')",
        )
        self.rejected([self.module(source)])

    def test_instance_callback_shadows_cannot_convert_real_nonpassing_tests(self):
        controls = [
            (
                "result.addFailure = lambda test, err: result.addSuccess(test)",
                "self.fail('private failure')",
            ),
            (
                "result.__dict__['addFailure'] = lambda test, err: result.addSuccess(test)",
                "self.fail('private failure')",
            ),
            (
                "object.__setattr__(result, 'addFailure', lambda test, err: result.addSuccess(test))",
                "self.fail('private failure')",
            ),
            (
                "result.addError = lambda test, err: result.addSuccess(test)",
                "raise RuntimeError('private error')",
            ),
            (
                "result.addSkip = lambda test, reason: result.addSuccess(test)",
                "self.skipTest('private skip')",
            ),
            (
                "result.addExpectedFailure = lambda test, err: result.addSuccess(test)",
                "self.fail('private expected failure')",
            ),
            (
                "result.addUnexpectedSuccess = lambda test: result.addSuccess(test)",
                "self.assertTrue(True)",
            ),
            ("result.passing = lambda: True", "self.fail('private failure')"),
            (
                "result.addSubTest = lambda test, subtest, err: result.addSuccess(test)",
                "with self.subTest(index=1): self.fail('private subtest failure')",
            ),
        ]
        controls.extend(support.observer_code_mutations("self.fail('private failure')"))
        for name in ["test_observer", "controls.test_observer"]:
            for action, outcome in controls:
                with self.subTest(module=name, action=action):
                    source = PASS.replace(
                        "        with self.subTest(index=1):\n            self.assertEqual(2 + 2, 4)",
                        "        result = self._outcome.result\n        "
                        + action
                        + "\n        "
                        + outcome,
                    )
                    if (
                        "addExpectedFailure" in action
                        or "addUnexpectedSuccess" in action
                    ):
                        source = source.replace(
                            "    def test_authored",
                            "    @unittest.expectedFailure\n    def test_authored",
                        )
                    module = self.module(source, name=name)
                    document = self.rejected([module])
                    self.assert_execution(module, document)
                    if "function = type(result).addFailure" in action:
                        self.assertEqual(
                            (self.root / (module + ".failure.executed")).read_text(),
                            document["test_ids"][0],
                        )
                    self.assertNotIn(
                        "addSuccess",
                        [event["callback"] for event in document["callbacks"]],
                    )

    def test_restored_instance_callback_shadows_remain_rejected(self):
        for name in ["test_observer", "controls.test_observer"]:
            for action in [
                "result.addFailure = forged",
                "result.__dict__['addFailure'] = forged",
                "object.__setattr__(result, 'addFailure', forged)",
            ]:
                with self.subTest(module=name, action=action):
                    source = PASS.replace(
                        "        with self.subTest(index=1):\n            self.assertEqual(2 + 2, 4)",
                        """        result = self._outcome.result
        original = result.addFailure
        def forged(test, err):
            result.__dict__.pop('addFailure', None)
            result.addFailure = original
            result.addSuccess(test)
        """
                        + action
                        + "\n        self.fail('private failure')",
                    )
                    module = self.module(source, name=name)
                    document = self.rejected([module])
                    self.assert_execution(module, document)

    def test_unused_assignment_restored_before_callback_is_still_rejected(self):
        for name in ["test_observer", "controls.test_observer"]:
            with self.subTest(module=name):
                source = PASS.replace(
                    "        with self.subTest(index=1):",
                    "        result = self._outcome.result\n"
                    "        original = result.addFailure\n"
                    "        result.addFailure = lambda test, err: result.addSuccess(test)\n"
                    "        result.addFailure = original\n"
                    "        result.rejected = False\n"
                    "        with self.subTest(index=1):",
                )
                module = self.module(source, name=name)
                document = self.rejected([module])
                self.assert_execution(module, document)

    def test_unused_instance_dictionary_callback_shadow_is_rejected(self):
        for name in ["test_observer", "controls.test_observer"]:
            for method in ["addFailure", "addSkip", "stop", "fixture_id"]:
                with self.subTest(module=name, method=method):
                    source = PASS.replace(
                        "        with self.subTest(index=1):",
                        "        self._outcome.result.__dict__['"
                        + method
                        + "'] = lambda *args: None\n"
                        "        with self.subTest(index=1):",
                    )
                    module = self.module(source, name=name)
                    document = self.rejected([module])
                    self.assert_execution(module, document)

    def test_observed_failure_and_callback_journal_cannot_be_erased(self):
        resets = [
            "result.nonpassing = False",
            "result.__dict__['nonpassing'] = False",
            "object.__setattr__(result, 'nonpassing', False)",
        ]
        edits = [
            "result.callbacks[:] = filtered",
            "result.callbacks = filtered",
            "result.__dict__['callbacks'] = filtered",
            "object.__setattr__(result, 'callbacks', filtered)",
            "result.callbacks.clear()",
            "result.callbacks.pop(2)",
            "del result.callbacks[2]",
            "result.callbacks[2]['successful'] = True",
        ]
        for name in ["test_state", "controls.test_state"]:
            for reset in resets:
                for edit in edits:
                    with self.subTest(module=name, reset=reset, edit=edit):
                        source = PASS.replace(
                            "        with self.subTest(index=1):\n            self.assertEqual(2 + 2, 4)",
                            "        result = self._outcome.result\n"
                            "        with self.subTest(index=1): self.fail('private failed subtest')\n"
                            "        " + reset + "\n"
                            "        filtered = [event for event in result.callbacks if event['callback'] != 'addSubTest']\n"
                            "        try:\n            " + edit + "\n"
                            "        except (AttributeError, TypeError): pass\n"
                            "        self._outcome.success = True",
                        )
                        module = self.module(source, name=name)
                        document = self.rejected([module])
                        self.assert_execution(module, document)
                        self.assertTrue(
                            any(
                                event["callback"] == "addSubTest"
                                and not event["successful"]
                                for event in document["callbacks"]
                            )
                        )

    def test_observer_descriptor_getter_and_setter_code_are_fenced(self):
        for name in ["test_descriptor", "controls.test_descriptor"]:
            for mode in ["getter", "setter", "journal_getters"]:
                with self.subTest(module=name, mode=mode):
                    changed = {
                        "getter": "owner.nonpassing.fget.__code__ = false_getter.__code__",
                        "setter": "owner.nonpassing.fset.__code__ = reset_setter.__code__",
                        "journal_getters": "owner.nonpassing.fget.__code__ = false_getter.__code__; owner.callbacks.fget.__code__ = filtered_getter.__code__",
                    }[mode]
                    reset = "result.nonpassing = False" if mode == "setter" else "pass"
                    body = (
                        """        result = self._outcome.result
        with self.subTest(index=1): self.fail('private failed subtest')
        owner = type(result)
        def replacements(facts):
            def false_getter(result): return False if facts else False
            def reset_setter(result, value): facts[result]['nonpassing'] = bool(value)
            def filtered_getter(result):
                return tuple(event for event in facts[result]['callbacks'] if event['callback'] != 'addSubTest')
            return false_getter, reset_setter, filtered_getter
        false_getter, reset_setter, filtered_getter = replacements(None)
        """
                        + changed
                        + "\n        "
                        + reset
                        + "\n        self._outcome.success = True"
                    )
                    source = PASS.replace(
                        "        with self.subTest(index=1):\n            self.assertEqual(2 + 2, 4)",
                        body,
                    )
                    module = self.module(source, name=name)
                    document = self.rejected([module])
                    self.assert_execution(module, document)

    def test_instance_run_shadow_cannot_fabricate_unexecuted_test_callbacks(self):
        for name in ["test_execution", "controls.test_execution"]:
            for target in ["case", "suite", "method", "dispatch", "suite_call"]:
                with self.subTest(module=name, target=target):
                    source = (
                        PASS
                        + """
def load_tests(loader, tests, pattern):
    case = NestedTests('test_authored_nested_method_executes')
    suite = unittest.TestSuite([case])
    def fabricated(result):
        result.startTest(case)
        result.addSuccess(case)
        result.stopTest(case)
    """
                        + {
                            "case": "case.run = fabricated",
                            "suite": "suite.run = fabricated",
                            "method": "setattr(case, case._testMethodName, lambda: None)",
                            "dispatch": "case._callTestMethod = lambda method: None",
                            "suite_call": "class ForgedSuite(unittest.TestSuite):\n        def __call__(self, result): fabricated(result)\n    suite = ForgedSuite([case])",
                        }[target]
                        + "\n    return suite\n"
                    )
                    module = self.module(source, name=name)
                    self.rejected([module])
                    self.assertFalse((self.root / (module + ".executed")).exists())

    def test_setup_dispatch_shadow_cannot_skip_authored_failure(self):
        for name in ["test_dispatch", "controls.test_dispatch"]:
            for target, assignment in [
                ("self._callTestMethod", "self._callTestMethod = lambda method: None"),
                (
                    "self._callTestMethod",
                    "self.__dict__['_callTestMethod'] = lambda method: None",
                ),
                (
                    "type(self).test_authored_failure.__code__",
                    "type(self).test_authored_failure.__code__ = (lambda self: None).__code__",
                ),
                (
                    "unittest.TestCase._callTestMethod.__code__",
                    "unittest.TestCase._callTestMethod.__code__ = (lambda self, method: None).__code__",
                ),
            ]:
                for restored in [False, True]:
                    with self.subTest(
                        module=name, assignment=assignment, restored=restored
                    ):
                        source = (
                            """import unittest
from pathlib import Path
class DispatchTests(unittest.TestCase):
    def setUp(self):
        Path(__name__ + '.setup.executed').write_text(self.id())
        self.addCleanup(lambda: Path(__name__ + '.cleanup.executed').write_text(self.id()))
        self.original = ORIGINAL
        ASSIGNMENT
    def tearDown(self):
        RESTORE
        Path(__name__ + '.teardown.executed').write_text(self.id())
    def test_authored_failure(self):
        Path(__name__ + '.body.executed').write_text(self.id())
        self.fail('actual authored failure')
""".replace("ORIGINAL", target)
                            .replace("ASSIGNMENT", assignment)
                            .replace(
                                "RESTORE",
                                target + " = self.original" if restored else "pass",
                            )
                        )
                        module = self.module(source, name=name)
                        document = self.rejected([module])
                        identity = module + ".DispatchTests.test_authored_failure"
                        self.assertEqual(
                            (self.root / (module + ".setup.executed")).read_text(),
                            identity,
                        )
                        self.assertEqual(
                            (self.root / (module + ".cleanup.executed")).read_text(),
                            identity,
                        )
                        self.assertFalse(
                            (self.root / (module + ".body.executed")).exists()
                        )
                        self.assertIn(
                            "addError",
                            [event["callback"] for event in document["callbacks"]],
                        )
                        self.assertNotIn(
                            "addSuccess",
                            [event["callback"] for event in document["callbacks"]],
                        )

    def test_normal_setup_body_teardown_and_cleanup_keep_stdlib_outcomes(self):
        for name in ["test_fixtures", "controls.test_fixtures"]:
            for body in [
                "self.assertTrue(True)",
                "self.fail('authored failure')",
                "self.skipTest('authored skip')",
                "with self.subTest(index=1): self.fail('authored subtest')",
            ]:
                with self.subTest(module=name, body=body):
                    self.output.unlink(missing_ok=True)
                    source = """import unittest
from pathlib import Path
class FixtureTests(unittest.TestCase):
    def setUp(self):
        Path(__name__ + '.setup.executed').write_text(self.id())
        self.addCleanup(lambda: Path(__name__ + '.cleanup.executed').write_text(self.id()))
    def tearDown(self):
        Path(__name__ + '.teardown.executed').write_text(self.id())
    def test_authored_body(self):
        Path(__name__ + '.body.executed').write_text(self.id())
        BODY
""".replace("BODY", body)
                    module = self.module(source, name=name)
                    result, document = self.run_report([module])
                    self.assertEqual(
                        result.returncode == 0,
                        body == "self.assertTrue(True)",
                        result.stderr,
                    )
                    identity = module + ".FixtureTests.test_authored_body"
                    self.assertEqual(document["test_ids"], [identity])
                    for part in ["setup", "body", "teardown", "cleanup"]:
                        self.assertEqual(
                            (
                                self.root / (module + "." + part + ".executed")
                            ).read_text(),
                            identity,
                        )

    def test_monkey_patched_writer_cannot_export_a_forged_success_report(self):
        source = (
            "import sys\n"
            + PASS
            + """
        owner = sys.modules['__main__']
        original = owner.ReportOutput.write
        def changed(output, document):
            document['tests_run'] = 0
            original(output, document)
        owner.ReportOutput.write = changed
"""
        )
        document = self.rejected([self.module(source)])
        self.assertEqual(document["tests_run"], 0)

    def test_actual_nested_report_byte_budget_remains_two_million(self):
        prefix = "scripts.controls.test_nested.ManyTests.test_"
        for others in range(4):
            document = support.StandaloneReportTests.report_shape(self, prefix, 0)
            for index in range(others):
                other = support.StandaloneReportTests.report_shape(
                    self, prefix + "a_" + str(index), 0
                )
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
                self.output.unlink(missing_ok=True)
                source = (
                    "import unittest\nclass ManyTests(unittest.TestCase): pass\n"
                    "for index in range(" + str(others) + "):\n"
                    "    setattr(ManyTests, 'test_a_' + str(index), lambda self: None)\n"
                    "setattr(ManyTests, 'test_' + 'x' * "
                    + str(padding + extra)
                    + ", lambda self: None)\n"
                )
                result, document = self.run_report([self.module(source)])
                self.assertEqual(result.returncode == 0, extra == 0, result.stderr)
                self.assertLessEqual(len(self.output.read_bytes()), 2_000_000)
                if extra == 0:
                    self.assertEqual(len(self.output.read_bytes()), 2_000_000)

    def test_selected_code_cannot_change_report_budget_constants(self):
        for action in [
            "sys.modules['__main__'].MAX_ITEMS = 5000",
            "sys.modules['scripts.unittest_report_origin'].MAX_BYTES = 5000000",
            "sys.modules['__main__'].ENCODER.sort_keys = False",
        ]:
            with self.subTest(action=action):
                source = "import sys\n" + PASS.replace(
                    "        with self.subTest(index=1):",
                    "        " + action + "\n        with self.subTest(index=1):",
                )
                self.rejected([self.module(source)])

    def test_selected_inventory_cannot_omit_empty_or_foreign_nested_modules(self):
        first = self.module()
        for source in [
            "pass\n",
            PASS + "\nNestedTests.__module__ = 'foreign.test_other'\n",
        ]:
            with self.subTest(source=source):
                other = self.module(source, name="other.test_selected")
                self.rejected([first, other])

    def test_nested_fixture_failure_has_actual_redacted_module_identity(self):
        module = self.module(
            PASS + "\ndef setUpModule(): raise RuntimeError('private fixture error')\n"
        )
        document = self.rejected([module])
        event = next(
            row for row in document["callbacks"] if row["callback"] == "addError"
        )
        self.assertEqual(event["test_id"], "setUpModule (" + module + ")")
        self.assertNotIn("private fixture error", json.dumps(document))

    def test_nested_callback_budget_remains_exact(self):
        for extra in [0, 1]:
            with self.subTest(extra=extra):
                self.output.unlink(missing_ok=True)
                module = self.module(
                    "import unittest\nclass NestedTests(unittest.TestCase):\n"
                    "    def test_many(self):\n        for _ in range("
                    + str(4091 + extra)
                    + "):\n            with self.subTest(index=1): pass\n"
                )
                result, document = self.run_report([module])
                self.assertEqual(result.returncode == 0, extra == 0, result.stderr)
                self.assertLessEqual(len(document["callbacks"]), 4096)

    def test_initializer_drift_after_report_write_is_rejected(self):
        source = (
            "import sys\n"
            + PASS
            + """
        owner = sys.modules['__main__']
        original = owner.ReportOutput.write
        def changed(output, document):
            original(output, document)
            Path('scripts/controls/__init__.py').write_bytes(b'changed = True\n')
        owner.ReportOutput.write = changed
""".replace("b'changed = True\n'", "b'changed = True\\n'")
        )
        self.rejected([self.module(source, initializers=True)])

    def test_actual_descriptor_read_race_fences_same_inode_source_bytes(self):
        from scripts import unittest_report_origin as origin

        module = self.module()
        path = self.root.joinpath(*module.split(".")).with_suffix(".py")
        original = os.read
        calls = []

        def changed(descriptor, count):
            raw = original(descriptor, count)
            calls.append(count)
            path.write_bytes(b"x" * path.stat().st_size)
            return raw

        with patch.object(origin.os, "read", changed):
            with self.assertRaises(origin.ReportError):
                origin.source_snapshot(self.root, path.relative_to(self.root))
        self.assertTrue(calls)

    def test_actual_hostile_long_testcase_identity_rejects_in_bounded_child(self):
        self.process_timeout = 3
        source = PASS + "\nNestedTests.__qualname__ = 'test_a.' * 8000 + '!'\n"
        self.rejected([self.module(source)])

    def test_actual_hostile_long_fixture_identity_rejects_in_bounded_child(self):
        self.process_timeout = 3
        source = (
            PASS
            + """
unittest.suite._ErrorHolder.id = lambda self: 'setUpModule (scripts.' + 'test_a.' * 8000 + '!)'
def setUpModule(): raise RuntimeError('synthetic fixture error')
"""
        )
        self.rejected([self.module(source)])


if __name__ == "__main__":
    unittest.main()
