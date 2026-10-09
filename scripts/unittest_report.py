"""Standalone, bounded observations of genuine stdlib unittest callbacks.

Run from the capsule root with the frozen persona-verifier Python interpreter.
The machine report contains identities and callback facts only. Diagnostics use
stderr and never become report fields. No parent contract or pytest is loaded.
"""

from __future__ import annotations

import sys

startup_paths = tuple(sys.path)
try:
    if not __package__:
        sys.path[:] = [sys._stdlib_dir, sys._stdlib_dir + "/lib-dynload"]
    import hashlib
    import importlib.util
    import inspect
    import json
    import os
    import re
    import stat
    import unittest
    import weakref
    from pathlib import Path
    from types import FunctionType, MappingProxyType
except BaseException:
    print("unittest-report: standard library origin rejected", file=sys.stderr)
    raise SystemExit(1) from None
finally:
    if not __package__:
        sys.path[:] = startup_paths[1:]


def bootstrap_origin():
    """Load the owned helper source before a direct entry executes any package."""
    if __package__:
        from . import unittest_report_origin

        return unittest_report_origin
    path = Path(os.path.abspath(__file__)).with_name("unittest_report_origin.py")

    def metadata(value):
        return (
            value.st_dev,
            value.st_ino,
            value.st_mode,
            value.st_nlink,
            value.st_size,
            value.st_mtime_ns,
            value.st_ctime_ns,
        )

    directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        descriptor = os.open(
            path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory
        )
        try:
            before = os.fstat(descriptor)
            if not stat.S_ISREG(before.st_mode) or not 0 < before.st_size <= 2_000_000:
                raise ValueError("invalid bootstrap source")
            with os.fdopen(os.dup(descriptor), "rb") as source:
                raw = source.read(before.st_size + 1)
            if len(raw) != before.st_size or metadata(os.fstat(descriptor)) != metadata(
                before
            ):
                raise ValueError("bootstrap source changed")
            if metadata(
                os.stat(path.name, dir_fd=directory, follow_symlinks=False)
            ) != metadata(before):
                raise ValueError("bootstrap origin changed")
        finally:
            os.close(descriptor)
    finally:
        os.close(directory)
    spec = importlib.util.spec_from_file_location(
        "scripts.unittest_report_origin", path
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    exec(compile(raw, str(path), "exec"), module.__dict__)
    expected = module.source_metadata(before), hashlib.sha256(raw).hexdigest()
    module.require(
        module.source_identity(path.parent.parent, Path("scripts") / path.name)
        == expected,
        "bootstrap helper changed",
    )
    producer = sys.modules[__name__]
    producer.__spec__ = importlib.util.spec_from_file_location(__name__, __file__)
    sys.modules["scripts.unittest_report"] = producer
    return module


try:
    origin = bootstrap_origin()
    origin.standard_library_origins()
except BaseException:
    print("unittest-report: bootstrap source or origin rejected", file=sys.stderr)
    raise SystemExit(1) from None
ReportError = origin.ReportError
identity = origin.identity
load_modules = origin.load_modules
open_directory = origin.open_directory
owned_origin = origin.owned_origin
require = origin.require
selectors = origin.selectors
source_identity = origin.source_identity
source_metadata = origin.source_metadata

SCHEMA = "tera.unittest-callback-report.v1"
MAX_BYTES = 2_000_000
MAX_ITEMS = 4096
TEST_ID = re.compile(r"scripts(?:\.[A-Za-z_][A-Za-z0-9_]*){3,}")
FIXTURE_ID = re.compile(
    r"(?:setUpClass|tearDownClass|setUpModule|tearDownModule) "
    r"\((scripts(?:\.[A-Za-z_][A-Za-z0-9_]*)+)\)"
)
TESTCASE_ID = unittest.TestCase.id
TESTCASE_RUN = unittest.TestCase.run
TESTCASE_CALL = unittest.TestCase.__call__
TESTCASE_SETUP_CALL = unittest.TestCase._callSetUp
TESTCASE_METHOD_CALL = unittest.TestCase._callTestMethod
SUITE_RUN = unittest.TestSuite.run
SUITE_CALL = unittest.TestSuite.__call__
RESULT_METHODS = MappingProxyType(
    {
        name: method
        for name, method in vars(unittest.TestResult).items()
        if callable(method) and not name.startswith("__")
    }
)
CALLBACK_FIELDS = frozenset({"callbacks", "nonpassing", "rejected"})
ENCODER = json.JSONEncoder(sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def encoded_size(value):
    return sum(len(chunk) for chunk in ENCODER.iterencode(value))


class ReportBudget:
    def __init__(self):
        self.size = encoded_size(
            {"schema": SCHEMA, "test_ids": [], "tests_run": 0, "callbacks": []}
        )

    def add(self, size):
        require(self.size + size <= MAX_BYTES, "report byte bound exceeded")
        self.size += size

    def append(self, destination, value):
        require(len(destination) < MAX_ITEMS, "report item bound exceeded")
        self.add(encoded_size(value) + int(bool(destination)))
        destination.append(value)


class ReportOutput:
    """Exclusive output with bounded byte, position and identity fencing."""

    def __init__(self, path):
        require(
            isinstance(path, str) and 0 < len(path) <= 4096 and "\x00" not in path,
            "output path is invalid",
        )
        self.parent, self.name = os.path.split(path)
        require(self.name not in {"", ".", ".."}, "output basename is invalid")
        self.directory = open_directory(self.parent or ".")
        try:
            self.descriptor = os.open(
                self.name,
                os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                0o600,
                dir_fd=self.directory,
            )
        except BaseException:
            os.close(self.directory)
            raise
        try:
            self.information = os.fstat(self.descriptor)
            self.size = 0
            self.digest = hashlib.sha256()
            require(
                stat.S_ISREG(self.information.st_mode)
                and self.information.st_nlink == 1
                and self.information.st_size == 0,
                "output is not an exclusive regular file",
            )
        except BaseException:
            self.close()
            raise

    def verify_position(self, information):
        require(
            information.st_size == self.size
            and self.size <= MAX_BYTES
            and os.lseek(self.descriptor, 0, os.SEEK_CUR) == self.size,
            "output size or write position changed",
        )

    def verify_metadata(self):
        information = os.fstat(self.descriptor)
        require(
            source_metadata(information) == source_metadata(self.information),
            "output metadata changed",
        )
        self.verify_position(information)
        current = os.stat(self.name, dir_fd=self.directory, follow_symlinks=False)
        require(
            source_metadata(current) == source_metadata(information),
            "output identity changed",
        )
        return information

    def verify_directory(self):
        directory = open_directory(self.parent or ".")
        try:
            require(
                identity(os.fstat(directory)) == identity(os.fstat(self.directory)),
                "output directory changed",
            )
        finally:
            os.close(directory)

    def verify(self):
        self.verify_directory()
        before = self.verify_metadata()
        digest, offset = hashlib.sha256(), 0
        while offset < self.size:
            raw = os.pread(self.descriptor, min(65536, self.size - offset), offset)
            require(bool(raw), "output changed while reading")
            digest.update(raw)
            offset += len(raw)
        require(digest.digest() == self.digest.digest(), "output bytes changed")
        require(
            source_metadata(before) == source_metadata(self.verify_metadata()),
            "output changed while reading",
        )
        self.verify_directory()

    def write_piece(self, raw):
        while raw:
            self.verify_metadata()
            count = os.write(self.descriptor, raw)
            require(0 < count <= len(raw), "output write did not advance")
            self.size += count
            self.digest.update(raw[:count])
            information = os.fstat(self.descriptor)
            self.verify_position(information)
            require(
                source_metadata(information)[:3]
                == source_metadata(self.information)[:3],
                "output identity changed",
            )
            self.information = information
            raw = raw[count:]

    def write(self, document):
        require(encoded_size(document) <= MAX_BYTES, "report byte bound exceeded")
        self.verify()
        require(self.size == 0, "output was already written")
        total = 0
        for chunk in ENCODER.iterencode(document):
            raw = chunk.encode("ascii")
            total += len(raw)
            require(total <= MAX_BYTES, "report byte bound exceeded")
            self.write_piece(raw)
        os.fsync(self.descriptor)
        self.verify()

    def close(self):
        try:
            self.verify()
        finally:
            try:
                os.close(self.descriptor)
            finally:
                os.close(self.directory)


def bound_method(instance, name, implementation):
    method = getattr(instance, name)
    return (
        getattr(method, "__self__", None) is instance
        and getattr(method, "__func__", None) is implementation
    )


def suite_execution(suite):
    return type(suite).__call__ is SUITE_CALL and bound_method(suite, "run", SUITE_RUN)


def authored_method(test):
    descriptor = inspect.getattr_static(type(test), test._testMethodName)
    if not isinstance(descriptor, (FunctionType, staticmethod, classmethod)):
        return False
    expected = descriptor.__get__(test, type(test))
    actual = getattr(test, test._testMethodName)
    if isinstance(expected, FunctionType):
        return actual is expected
    return (
        getattr(actual, "__self__", None) is expected.__self__
        and getattr(actual, "__func__", None) is expected.__func__
    )


def testcase_execution(test):
    return authored_method(test) and all(
        bound_method(test, name, implementation)
        for name, implementation in (
            ("run", TESTCASE_RUN),
            ("__call__", TESTCASE_CALL),
            ("_callTestMethod", TESTCASE_METHOD_CALL),
        )
    )


def guard_setup(test, verify_execution=None):
    """Check dispatch after ordinary setup, inside the original stdlib run."""
    require(
        bound_method(test, "_callSetUp", TESTCASE_SETUP_CALL),
        "testcase setup dispatch is overridden",
    )
    value = canonical_id(test)
    method = getattr(test, test._testMethodName)
    executables = (getattr(method, "__func__", method), TESTCASE_METHOD_CALL)
    before = tuple(origin.callable_identity(item) for item in executables)

    def checked_setup():
        try:
            TESTCASE_SETUP_CALL(test)
            if vars(test).get("_callSetUp") is checked_setup:
                del test._callSetUp
            if verify_execution is not None:
                verify_execution(test)
            require(
                canonical_id(test) == value
                and testcase_execution(test)
                and tuple(origin.callable_identity(item) for item in executables)
                == before,
                "testcase execution changed during setup",
            )
        finally:
            if vars(test).get("_callSetUp") is checked_setup:
                del test._callSetUp

    test._callSetUp = checked_setup
    require(test._callSetUp is checked_setup, "testcase setup guard differs")


def iter_tests(suite):
    require(suite_execution(suite), "suite execution is overridden")
    pending = [iter(suite)]
    visited = set()
    while pending:
        current = next(pending[-1], None)
        if current is None:
            pending.pop()
        elif isinstance(current, unittest.TestSuite):
            require(
                id(current) not in visited
                and len(visited) < MAX_ITEMS
                and suite_execution(current),
                "suite inventory is cyclic or oversized",
            )
            visited.add(id(current))
            pending.append(iter(current))
        else:
            require(
                isinstance(current, unittest.TestCase),
                "inventory contains a non-testcase",
            )
            yield current


def canonical_id(test):
    require(
        type(test).id is TESTCASE_ID and unittest.TestCase.id is TESTCASE_ID,
        "testcase identity is overridden",
    )
    value = test.id()
    require(
        isinstance(value, str) and len(value) <= MAX_BYTES,
        "testcase identity is invalid",
    )
    require(TEST_ID.fullmatch(value) is not None, "testcase identity is not canonical")
    require(
        value == TESTCASE_ID(test)
        and value
        == type(test).__module__
        + "."
        + type(test).__qualname__
        + "."
        + test._testMethodName,
        "testcase identity differs from its actual class and method",
    )
    return value


def inventory(suite, names, budget):
    values, seen, represented = [], set(), set()
    for test in iter_tests(suite):
        value = canonical_id(test)
        module = type(test).__module__
        require(
            module in names and value.startswith(module + "."),
            "testcase belongs to an unselected module",
        )
        require(value not in seen, "testcase inventory is duplicated")
        budget.append(values, value)
        seen.add(value)
        represented.add(module)
    require(
        bool(values) and represented == set(names),
        "selected module inventory is empty or incomplete",
    )
    return values


def execution_identity(test):
    implementations = [inspect.getattr_static(type(test), test._testMethodName)]
    for name in ("run", "__call__", "_callSetUp", "_callTestMethod"):
        method = getattr(test, name)
        implementations.append(getattr(method, "__func__", method))
    implementations.extend(
        (
            TESTCASE_RUN,
            TESTCASE_CALL,
            TESTCASE_SETUP_CALL,
            TESTCASE_METHOD_CALL,
            SUITE_RUN,
            SUITE_CALL,
        )
    )
    return canonical_id(test), tuple(
        origin.callable_identity(item) for item in implementations
    )


def callback_result_type():
    # Selected TestCases can inspect the result, but do not own observed facts.
    facts = weakref.WeakKeyDictionary()

    class CallbackResult(unittest.TestResult):
        """Store bounded redacted callback facts, never formatted errors or reprs."""

        def __getattribute__(self, name):
            # A callback shadow must never replace the captured observer implementation.
            method = methods.get(name)
            if method is not None:
                if origin.callable_identity(method) != executables[name]:
                    facts[self]["rejected"] = True
                    raise ReportError("observer executable identity changed")
                namespace = object.__getattribute__(self, "__dict__")
                if any(key in namespace for key in methods) or any(
                    key in namespace for key in CALLBACK_FIELDS
                ):
                    object.__setattr__(self, "rejected", True)
                return method.__get__(self, type(self))
            return object.__getattribute__(self, name)

        def __setattr__(self, name, value):
            if name in methods:
                object.__setattr__(self, "rejected", True)
                return
            object.__setattr__(self, name, value)

        def __init__(self, test_ids, budget, stream, verify_execution=None):
            facts[self] = {
                "callbacks": [],
                "nonpassing": False,
                "rejected": False,
                "verify_execution": verify_execution,
            }
            super().__init__()
            self.inventory = set(test_ids)
            self.budget = budget
            self.stream = stream
            self.active = None
            self.started, self.completed, self.succeeded = set(), set(), set()
            self.ordinal = 0
            self.success = False
            self.nonpassing = False
            self.rejected = False
            self.run_started = False
            self.run_stopped = False

        def _get_callbacks(self):
            return tuple(MappingProxyType(event) for event in facts[self]["callbacks"])

        def _set_callbacks(self, value):
            del value
            facts[self]["rejected"] = True

        callbacks = property(_get_callbacks, _set_callbacks)
        del _get_callbacks, _set_callbacks

        def _get_nonpassing(self):
            return facts[self]["nonpassing"]

        def _set_nonpassing(self, value):
            if facts[self]["nonpassing"] and not value:
                facts[self]["rejected"] = True
            facts[self]["nonpassing"] |= bool(value)

        nonpassing = property(_get_nonpassing, _set_nonpassing)
        del _get_nonpassing, _set_nonpassing

        def _get_rejected(self):
            return facts[self]["rejected"]

        def _set_rejected(self, value):
            facts[self]["rejected"] |= bool(value)

        rejected = property(_get_rejected, _set_rejected)
        del _get_rejected, _set_rejected

        def event(self, callback, test_id=None, **fields):
            value = {"callback": callback, **fields}
            if test_id is not None:
                value["test_id"] = test_id
            try:
                self.budget.append(facts[self]["callbacks"], value)
            except ReportError:
                self.rejected = True
                self.stop()
                raise

        def startTestRun(self):
            require(not self.run_started, "test run started twice")
            self.run_started = True
            self.event("startTestRun")
            super().startTestRun()

        def stopTestRun(self):
            require(
                self.run_started and not self.run_stopped, "test run stop order differs"
            )
            self.run_stopped = True
            self.event("stopTestRun")
            super().stopTestRun()

        def startTest(self, test):
            verify_execution = facts[self]["verify_execution"]
            if verify_execution is not None:
                verify_execution(test)
            if type(test).run is TESTCASE_RUN:
                require(testcase_execution(test), "testcase execution is overridden")
                guard_setup(test, verify_execution)
            self.rejected |= (
                type(test).run is not TESTCASE_RUN
                or type(test).__call__ is not TESTCASE_CALL
                or unittest.TestCase.run is not TESTCASE_RUN
                or not testcase_execution(test)
            )
            require(
                self.run_started and not self.run_stopped,
                "testcase is outside run boundaries",
            )
            value = canonical_id(test)
            require(
                value in self.inventory
                and value not in self.started
                and self.active is None,
                "testcase start differs from inventory",
            )
            self.budget.add(len(str(self.testsRun + 1)) - len(str(self.testsRun)))
            self.event("startTest", value)
            super().startTest(test)
            self.active, self.ordinal, self.success = value, 0, False
            self.started.add(value)

        def stopTest(self, test):
            value = canonical_id(test)
            require(value == self.active, "testcase stop order differs")
            self.event("stopTest", value)
            self.completed.add(value)
            self.active = None
            super().stopTest(test)

        def addSuccess(self, test):
            value = canonical_id(test)
            require(
                value == self.active and not self.success,
                "testcase success order differs",
            )
            self.event("addSuccess", value)
            self.success = True
            self.succeeded.add(value)
            super().addSuccess(test)

        def addSubTest(self, test, subtest, err):
            del subtest
            value = canonical_id(test)
            require(value == self.active and not self.success, "subtest order differs")
            self.ordinal += 1
            self.event(
                "addSubTest", value, ordinal=self.ordinal, successful=err is None
            )
            if err is not None:
                self.nonpassing = True

        def nonpass(self, callback, test):
            self.nonpassing = True
            # _SubTest.id() includes raw parameter repr; its parent is the identity.
            parent = getattr(test, "test_case", test)
            if isinstance(parent, unittest.TestCase):
                value = canonical_id(parent)
            elif type(parent) is unittest.suite._ErrorHolder:
                value = self.fixture_id(parent)
            else:
                value = "unittest.redacted_noncase"
            self.event(callback, value)
            print("unittest-report: " + callback, file=self.stream)

        def fixture_id(self, test):
            value = test.id()
            require(
                isinstance(value, str) and len(value) <= MAX_BYTES,
                "fixture identity is invalid",
            )
            match = FIXTURE_ID.fullmatch(value)
            require(match is not None, "fixture identity is not canonical")
            require(
                any(item.startswith(match[1] + ".") for item in self.inventory),
                "fixture identity is outside inventory",
            )
            return value

        def addFailure(self, test, err):
            del err
            self.nonpass("addFailure", test)

        def addError(self, test, err):
            del err
            self.nonpass("addError", test)

        def addSkip(self, test, reason):
            del reason
            self.nonpass("addSkip", test)

        def addExpectedFailure(self, test, err):
            del err
            self.nonpass("addExpectedFailure", test)

        def addUnexpectedSuccess(self, test):
            self.nonpass("addUnexpectedSuccess", test)

        def wasSuccessful(self):
            return not self.nonpassing and not self.rejected

        def passing(self):
            boundaries = self.callbacks[:1] == (
                {"callback": "startTestRun"},
            ) and self.callbacks[-1:] == ({"callback": "stopTestRun"},)
            return (
                boundaries
                and self.testsRun > 0
                and self.wasSuccessful()
                and not self.shouldStop
                and self.active is None
                and self.started == self.completed == self.succeeded == self.inventory
                and self.testsRun == len(self.inventory)
            )

    methods = MappingProxyType(
        {
            **RESULT_METHODS,
            **{
                name: method
                for name, method in vars(CallbackResult).items()
                if callable(method) and not name.startswith("__")
            },
        }
    )
    executables = MappingProxyType(
        {name: origin.callable_identity(method) for name, method in methods.items()}
    )
    return CallbackResult


CallbackResult = callback_result_type()


def observe(suite, test_ids, budget, stream):
    verify_execution = origin.inventory_execution(
        iter_tests(suite), execution_identity, authored_method
    )
    result = CallbackResult(test_ids, budget, stream, verify_execution)
    result.startTestRun()
    try:
        suite(result)
    except BaseException:
        result.rejected = True
        result.stop()
        print("unittest-report: test run interrupted or rejected", file=stream)
    finally:
        try:
            result.stopTestRun()
        except ReportError:
            result.rejected = True
    return result, result.passing()


def execute(root, names, output, stream):
    verify_implementation = origin.runtime_guard([origin, sys.modules[__name__]])
    verify_framework = origin.runtime_guard(
        [unittest.case, unittest.suite, unittest.result]
    )
    producer = Path("scripts/unittest_report.py")
    initial = source_identity(root, producer)
    owned_origin(sys.modules[__name__], root / producer)
    helper = Path("scripts/unittest_report_origin.py")
    helper_initial = source_identity(root, helper)
    owned_origin(origin, root / helper)
    modules, fence = load_modules(root, names)
    verify_implementation()
    verify_framework()
    loader = unittest.TestLoader()
    suite = unittest.TestSuite(loader.loadTestsFromModule(module) for module in modules)
    require(not loader.errors, "testcase discovery failed")
    verify_framework()
    budget = ReportBudget()
    test_ids = inventory(suite, names, budget)
    result, passing = observe(suite, test_ids, budget, stream)
    fence.verify()
    for relative, before in [(producer, initial), (helper, helper_initial)]:
        require(
            source_identity(root, relative) == before,
            "source origin changed during execution",
        )
    output.write(
        {
            "schema": SCHEMA
            if all(name.count(".") == 1 for name in names)
            else "tera.unittest-callback-report.v2",
            "test_ids": test_ids,
            "tests_run": result.testsRun,
            "callbacks": [dict(event) for event in result.callbacks],
        }
    )
    fence.verify()
    for relative, before in [(producer, initial), (helper, helper_initial)]:
        require(
            source_identity(root, relative) == before,
            "source changed during report output",
        )
    verify_implementation()
    verify_framework()
    return 0 if passing else 1


def main(arguments=None):
    arguments = sys.argv[1:] if arguments is None else arguments
    output = None
    status = 1
    try:
        require(sys.version_info[:3] == (3, 14, 7), "frozen Python version differs")
        if __spec__.name == "scripts.unittest_report":
            print(
                "unittest-report: use the direct file entry before package initialization",
                file=sys.stderr,
            )
            return 1
        require(
            len(arguments) >= 3 and arguments[0] == "--report",
            "expected one explicit report output and selected modules",
        )
        names = selectors(arguments[2:])
        root = Path(os.path.abspath(__file__)).parent.parent
        require(
            identity(root.stat()) == identity(Path.cwd().stat()),
            "runner must use the capsule root",
        )
        output = ReportOutput(arguments[1])
        status = execute(root, names, output, sys.stderr)
    except BaseException:
        print(
            "unittest-report: input, origin, output, or lifecycle rejected",
            file=sys.stderr,
        )
    finally:
        if output is not None:
            try:
                output.close()
            except BaseException:
                print("unittest-report: output settlement rejected", file=sys.stderr)
                status = 1
    return status


if __name__ == "__main__":
    raise SystemExit(main())
