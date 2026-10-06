"""Standalone, bounded observations of genuine stdlib unittest callbacks.

Run from the capsule root with the frozen persona-verifier Python interpreter.
The machine report contains identities and callback facts only. Diagnostics use
stderr and never become report fields. No parent contract or pytest is loaded.
"""

from __future__ import annotations

import hashlib
import importlib
import importlib.machinery
import importlib.util
import json
import os
import re
import stat
import sys
import unittest
from pathlib import Path

SCHEMA = "tera.unittest-callback-report.v1"
MAX_BYTES = 2_000_000
MAX_ITEMS = 4096
MODULE = re.compile(r"scripts\.test_[A-Za-z0-9_]+")
TEST_ID = re.compile(
    r"scripts\.test_[A-Za-z0-9_]+\.[A-Za-z_][A-Za-z0-9_]*\.[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*"
)
FIXTURE_ID = re.compile(
    r"(?:setUpClass|tearDownClass|setUpModule|tearDownModule) "
    r"\((scripts\.test_[A-Za-z0-9_]+(?:\.[A-Za-z_][A-Za-z0-9_]*)*)\)"
)
ENCODER = json.JSONEncoder(sort_keys=True, separators=(",", ":"), ensure_ascii=True)


class ReportError(Exception):
    """A stable, secret-free report rejection."""


def require(condition, message):
    if not condition:
        raise ReportError(message)


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


def selectors(values):
    require(0 < len(values) <= MAX_ITEMS, "selected module count is invalid")
    seen = set()
    for value in values:
        require(
            isinstance(value, str) and len(value) <= 255, "selected module is invalid"
        )
        require(MODULE.fullmatch(value) is not None, "selected module is not canonical")
        require(value not in seen, "selected module is duplicated")
        seen.add(value)
    return values


def open_directory(path):
    """Walk every component with nofollow; retain the actual directory descriptor."""
    require(
        isinstance(path, str) and 0 < len(path) <= 4096 and "\x00" not in path,
        "output path is invalid",
    )
    absolute = path.startswith("/")
    descriptor = os.open("/" if absolute else ".", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in path.split("/"):
            if not component or component == ".":
                continue
            child = os.open(
                component,
                os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                dir_fd=descriptor,
            )
            os.close(descriptor)
            descriptor = child
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def identity(information):
    return (information.st_dev, information.st_ino)


def source_metadata(information):
    return (
        identity(information),
        information.st_mode,
        information.st_nlink,
        information.st_size,
        information.st_mtime_ns,
        information.st_ctime_ns,
    )


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


def source_snapshot(root, relative):
    directory = open_directory(str(root / relative.parent))
    try:
        descriptor = os.open(
            relative.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory
        )
        try:
            before = os.fstat(descriptor)
            require(
                stat.S_ISREG(before.st_mode) and 0 < before.st_size <= MAX_BYTES,
                "source origin is not a bounded regular file",
            )
            digest = hashlib.sha256()
            chunks = []
            remaining = before.st_size
            while remaining:
                chunk = os.read(descriptor, min(65536, remaining))
                require(bool(chunk), "source origin changed while reading")
                digest.update(chunk)
                chunks.append(chunk)
                remaining -= len(chunk)
            after = os.fstat(descriptor)
            require(
                source_metadata(before) == source_metadata(after),
                "source origin changed while reading",
            )
            current = os.stat(relative.name, dir_fd=directory, follow_symlinks=False)
            require(
                source_metadata(current) == source_metadata(before),
                "source origin identity changed",
            )
            return (source_metadata(before), digest.hexdigest()), b"".join(chunks)
        finally:
            os.close(descriptor)
    finally:
        os.close(directory)


def source_identity(root, relative):
    return source_snapshot(root, relative)[0]


def owned_origin(module, expected):
    require(
        getattr(module, "__file__", None) == str(expected),
        "loaded module file origin differs",
    )
    require(
        getattr(getattr(module, "__spec__", None), "origin", None) == str(expected),
        "loaded module specification origin differs",
    )


def load_modules(root, names):
    modules, sources = [], {}
    for name in names:
        relative = Path("scripts") / (name.split(".")[1] + ".py")
        before, raw = source_snapshot(root, relative)
        spec = importlib.util.find_spec(name)
        require(
            spec is not None and spec.origin == str(root / relative),
            "selected module resolution origin differs",
        )
        require(
            type(spec.loader) is importlib.machinery.SourceFileLoader,
            "selected module loader differs",
        )
        module = importlib.util.module_from_spec(spec)
        require(
            source_identity(root, relative) == before,
            "selected module changed before import",
        )
        sys.modules[name] = module
        exec(compile(raw, str(root / relative), "exec"), module.__dict__)
        owned_origin(module, root / relative)
        require(
            source_identity(root, relative) == before,
            "selected module changed during import",
        )
        sources[relative] = before
        modules.append(module)
    return modules, sources


def iter_tests(suite):
    pending = [iter(suite)]
    visited = set()
    while pending:
        current = next(pending[-1], None)
        if current is None:
            pending.pop()
        elif isinstance(current, unittest.TestSuite):
            require(
                id(current) not in visited and len(visited) < MAX_ITEMS,
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
    value = test.id()
    require(
        isinstance(value, str) and len(value) <= MAX_BYTES,
        "testcase identity is invalid",
    )
    require(TEST_ID.fullmatch(value) is not None, "testcase identity is not canonical")
    require(
        value == unittest.TestCase.id(test),
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


class CallbackResult(unittest.TestResult):
    """Store bounded redacted callback facts, never formatted errors or reprs."""

    def __init__(self, test_ids, budget, stream):
        super().__init__()
        self.callbacks = []
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

    def event(self, callback, test_id=None, **fields):
        value = {"callback": callback, **fields}
        if test_id is not None:
            value["test_id"] = test_id
        try:
            self.budget.append(self.callbacks, value)
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
            value == self.active and not self.success, "testcase success order differs"
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
        self.event("addSubTest", value, ordinal=self.ordinal, successful=err is None)
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
        boundaries = self.callbacks[:1] == [
            {"callback": "startTestRun"}
        ] and self.callbacks[-1:] == [{"callback": "stopTestRun"}]
        return (
            boundaries
            and self.testsRun > 0
            and self.wasSuccessful()
            and not self.shouldStop
            and self.active is None
            and self.started == self.completed == self.succeeded == self.inventory
            and self.testsRun == len(self.inventory)
        )


def observe(suite, test_ids, budget, stream):
    result = CallbackResult(test_ids, budget, stream)
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
    producer = Path("scripts/unittest_report.py")
    initial = source_identity(root, producer)
    owned_origin(sys.modules[__name__], root / producer)
    modules, sources = load_modules(root, names)
    sources[producer] = initial
    loader = unittest.TestLoader()
    suite = unittest.TestSuite(loader.loadTestsFromModule(module) for module in modules)
    require(not loader.errors, "testcase discovery failed")
    budget = ReportBudget()
    test_ids = inventory(suite, names, budget)
    result, passing = observe(suite, test_ids, budget, stream)
    for relative, before in sources.items():
        require(
            source_identity(root, relative) == before,
            "source origin changed during execution",
        )
    output.write(
        {
            "schema": SCHEMA,
            "test_ids": test_ids,
            "tests_run": result.testsRun,
            "callbacks": result.callbacks,
        }
    )
    return 0 if passing else 1


def main(arguments=None):
    arguments = sys.argv[1:] if arguments is None else arguments
    output = None
    status = 1
    try:
        require(sys.version_info[:3] == (3, 14, 7), "frozen Python version differs")
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
