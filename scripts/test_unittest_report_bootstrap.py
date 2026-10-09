"""Run genuine before-package and before-fixture dispatch integrity controls."""

from __future__ import annotations

import shutil
import unittest

from scripts import test_unittest_report as support

BODY = """from pathlib import Path
class ActualTests(unittest.TestCase):
    def setUp(self):
        Path('setup.executed').write_text(self.id())
    def test_authored_body(self):
        Path('body.executed').write_text(self.id())
        with self.subTest(index=1):
            BODY
    def tearDown(self):
        Path('teardown.executed').write_text(self.id())
"""
FORGED_RUN = """import unittest
def forged_run(self, result=None):
    result.startTest(self)
    result.addSuccess(self)
    result.stopTest(self)
    return result
unittest.TestCase.run = forged_run
"""
STDLIB_SHADOW = """import sys
from pathlib import Path
actual = Path(sys._stdlib_dir) / 'unittest'
__path__ = [str(actual)]
__package__ = 'unittest'
exec(compile((actual / '__init__.py').read_bytes(), str(actual / '__init__.py'), 'exec'), globals())
def forged_run(self, result=None):
    result.startTest(self)
    result.addSuccess(self)
    result.stopTest(self)
    return result
TestCase.run = forged_run
"""


def fixture_source(kind, restored, target="body"):
    """Keep replacement/restoration controls on the real fixture lifecycle."""
    source = BODY.replace("BODY", "self.fail('actual authored failure')")
    expression = (
        "ActualTests.test_authored_body"
        if target == "body"
        else "unittest.TestCase._callTestMethod"
    )
    action = (
        f"function = {expression}; original = function.__code__; "
        "function.__code__ = (lambda self, *args: None).__code__"
    )
    if kind == "class":
        cleanup = (
            "cls.addClassCleanup(setattr, function, '__code__', original); "
            if restored == "cleanup"
            else "cls.original = original; "
        )
        fixture = "    @classmethod\n    def setUpClass(cls):\n        "
        fixture += action.replace(
            "function.__code__ =", cleanup + "function.__code__ ="
        )
        fixture += "\n"
        if restored == "teardown":
            fixture += (
                "    @classmethod\n    def tearDownClass(cls):\n"
                f"        {expression}.__code__ = cls.original\n"
            )
        return source.replace("    def setUp(self):", fixture + "    def setUp(self):")
    fixture = "\ndef setUpModule():\n    global original\n    " + action + "\n"
    if restored == "cleanup":
        fixture += (
            "    unittest.addModuleCleanup(setattr, function, '__code__', original)\n"
        )
    elif restored == "teardown":
        fixture += f"def tearDownModule():\n    {expression}.__code__ = original\n"
    return source + fixture


class BootstrapTests(unittest.TestCase):
    setUp = support.StandaloneReportTests.setUp
    pid_absent = staticmethod(support.StandaloneReportTests.pid_absent)
    run_report = support.StandaloneReportTests.run_report

    def probe_record(self, arguments, modules):
        self.last_record = support.StandaloneReportTests.probe_record(
            self, arguments, modules
        )
        if self.last_record is not None:
            shutil.copytree(
                self.root / "scripts",
                self.last_record / "scripts_before",
                symlinks=True,
            )
        return self.last_record

    def run_process(self, modules, output=None):
        result = support.StandaloneReportTests.run_process(self, modules, output)
        if self.last_record is not None:
            shutil.copytree(
                self.root / "scripts", self.last_record / "scripts_after", symlinks=True
            )
            for marker in self.root.glob("*.executed"):
                shutil.copy2(marker, self.last_record / marker.name)
        return result

    def prepare(self, source, *, nested=False, initializer=None):
        self.output.unlink(missing_ok=True)
        for marker in self.root.glob("*.executed"):
            marker.unlink()
        root_init = self.root / "scripts/__init__.py"
        root_init.unlink(missing_ok=True)
        if initializer is not None:
            root_init.write_text(initializer)
        name = "scripts.controls.test_actual" if nested else "scripts.test_actual"
        path = self.root.joinpath(*name.split(".")).with_suffix(".py")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("import unittest\n" + source)
        return name

    def test_namespace_empty_and_ordinary_root_packages_execute_actual_body(self):
        ordinary = (
            "from pathlib import Path\n"
            "from . import unittest_report, unittest_report_origin\n"
            "Path('initializer.executed').write_text('actual initializer')\n"
        )
        for nested in (False, True):
            for initializer in (None, "", ordinary):
                with self.subTest(nested=nested, initializer=initializer):
                    module = self.prepare(
                        BODY.replace("BODY", "self.assertTrue(True)"),
                        nested=nested,
                        initializer=initializer,
                    )
                    result, document = self.run_report([module])
                    self.assertEqual(result.returncode, 0, result.stderr)
                    expected = module + ".ActualTests.test_authored_body"
                    self.assertEqual(document["test_ids"], [expected])
                    for name in ("setup", "body", "teardown"):
                        self.assertEqual(
                            (self.root / (name + ".executed")).read_text(), expected
                        )
                    if initializer:
                        self.assertTrue((self.root / "initializer.executed").is_file())

    def test_root_initializer_self_mutation_and_forged_run_are_rejected(self):
        controls = (
            FORGED_RUN,
            "from pathlib import Path\nPath(__file__).write_text('# mutated\\n')\n",
            "from pathlib import Path\nPath(__file__).chmod(0o600)\n",
        )
        for nested in (False, True):
            for initializer in controls:
                with self.subTest(nested=nested, initializer=initializer):
                    module = self.prepare(
                        BODY.replace("BODY", "self.fail('actual failure')"),
                        nested=nested,
                        initializer=initializer,
                    )
                    result = self.run_process([module])
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse((self.root / "body.executed").exists())

    def test_class_and_module_fixture_code_cannot_skip_inventoried_body(self):
        for nested in (False, True):
            for kind in ("class", "module"):
                for restored in (None, "cleanup", "teardown"):
                    for target in ("body", "dispatch"):
                        with self.subTest(
                            nested=nested, kind=kind, restored=restored, target=target
                        ):
                            module = self.prepare(
                                fixture_source(kind, restored, target), nested=nested
                            )
                            result, document = self.run_report([module])
                            self.assertNotEqual(result.returncode, 0)
                            self.assertFalse((self.root / "body.executed").exists())
                            self.assertFalse(
                                document
                                and any(
                                    event["callback"] == "addSuccess"
                                    for event in document["callbacks"]
                                )
                            )

    def test_genuine_body_failure_keeps_actual_setup_body_and_teardown(self):
        for nested in (False, True):
            with self.subTest(nested=nested):
                module = self.prepare(
                    BODY.replace("BODY", "self.fail('actual authored failure')"),
                    nested=nested,
                )
                result, document = self.run_report([module])
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue((self.root / "body.executed").exists())
                self.assertTrue((self.root / "teardown.executed").exists())
                self.assertIn(
                    "addSubTest", [e["callback"] for e in document["callbacks"]]
                )

    def test_live_legacy_module_entry_requires_trusted_direct_entry(self):
        module = self.prepare(BODY.replace("BODY", "self.assertTrue(True)"))
        self.report_entry = ["-m", "scripts.unittest_report"]
        result = self.run_process([module])
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.root / "body.executed").exists())
        self.assertFalse(self.output.exists() and self.output.stat().st_size)
        self.assertNotIn("actual authored failure", result.stderr)
        self.assertIn("direct file entry", result.stderr)

    def test_bootstrap_helper_failure_has_static_safe_diagnostics(self):
        for source in (
            "P139S_SYNTHETIC_DIAGNOSTIC_MARKER = '\n",
            "raise RuntimeError('P139S_SYNTHETIC_DIAGNOSTIC_MARKER')\n",
        ):
            with self.subTest(source=source):
                module = self.prepare(BODY.replace("BODY", "self.assertTrue(True)"))
                (self.root / "scripts/unittest_report_origin.py").write_text(source)
                result = self.run_process([module])
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertEqual(
                    result.stderr,
                    "unittest-report: bootstrap source or origin rejected\n",
                )
                self.assertFalse((self.root / "body.executed").exists())
                self.assertFalse(self.output.exists())

    def test_local_stdlib_shadow_cannot_skip_actual_authored_failure(self):
        for nested in (False, True):
            for dependency in ("unittest", "hashlib", "inspect", "json"):
                with self.subTest(nested=nested, dependency=dependency):
                    module = self.prepare(
                        BODY.replace("BODY", "self.fail('actual authored failure')"),
                        nested=nested,
                    )
                    (self.root / "scripts" / (dependency + ".py")).write_text(
                        STDLIB_SHADOW
                    )
                    result, document = self.run_report([module])
                    self.assertNotEqual(result.returncode, 0)
                    self.assertTrue((self.root / "body.executed").is_file())
                    self.assertFalse(
                        any(
                            e["callback"] == "addSuccess" for e in document["callbacks"]
                        )
                    )

    def test_library_preload_accepts_real_pyexpat_children_and_rejects_forgery(self):
        for forged in (None, "alias", "parent_source"):
            with self.subTest(forged=forged):
                module = self.prepare(BODY.replace("BODY", "self.assertTrue(True)"))
                source = "import xml.parsers.expat, sys, types; "
                if forged == "alias":
                    source += "sys.modules['pyexpat.errors'] = types.ModuleType('pyexpat.errors'); "
                elif forged == "parent_source":
                    source += "sys.modules['pyexpat'].__spec__.origin = 'foreign.py'; "
                source += "import scripts.unittest_report"
                self.report_entry = ["-c", source]
                result = self.run_process([module])
                self.assertEqual(result.returncode == 0, forged is None, result.stderr)
                self.assertFalse(self.output.exists())
                self.assertFalse((self.root / "body.executed").exists())


if __name__ == "__main__":
    unittest.main()
