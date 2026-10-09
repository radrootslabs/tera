"""Standalone nofollow source reads and canonical module ancestry admission."""

from __future__ import annotations

import hashlib
import importlib.machinery
import importlib.util
import os
import re
import stat
import sys
import types
from pathlib import Path

MAX_BYTES = 2_000_000
MAX_ITEMS = 4096
MODULE = re.compile(r"scripts(?:\.[A-Za-z_][A-Za-z0-9_]*)*\.test_[A-Za-z0-9_]+")


class ReportError(Exception):
    """A stable, secret-free report rejection."""


def require(condition, message):
    if not condition:
        raise ReportError(message)


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


def source_snapshot(root, relative, *, allow_empty=False):
    directory = open_directory(str(root / relative.parent))
    try:
        descriptor = os.open(
            relative.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory
        )
        try:
            before = os.fstat(descriptor)
            require(
                stat.S_ISREG(before.st_mode)
                and int(not allow_empty) <= before.st_size <= MAX_BYTES,
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
    return source_snapshot(root, relative, allow_empty=relative.name == "__init__.py")[
        0
    ]


def owned_origin(module, expected):
    require(
        getattr(module, "__file__", None) == str(expected),
        "loaded module file origin differs",
    )
    require(
        getattr(getattr(module, "__spec__", None), "origin", None) == str(expected),
        "loaded module specification origin differs",
    )


def directory_identity(root, relative):
    descriptor = open_directory(str(root / relative))
    try:
        information = os.fstat(descriptor)
        return identity(information), information.st_mode
    finally:
        os.close(descriptor)


def initializer_snapshot(root, directory):
    relative = directory / "__init__.py"
    descriptor = open_directory(str(root / directory))
    try:
        try:
            os.stat(relative.name, dir_fd=descriptor, follow_symlinks=False)
        except FileNotFoundError:
            return None
        return source_snapshot(root, relative, allow_empty=True)[0]
    finally:
        os.close(descriptor)


class SourceFence:
    """Fence all selected bytes and ancestry before any selected code executes."""

    def __init__(self, root, names):
        self.root = root
        self.sources, self.directories, self.initializers, self.loaded = {}, {}, {}, {}
        for name in names:
            components = name.split(".")
            for length in range(1, len(components)):
                directory = Path(*components[:length])
                self.directories[directory] = directory_identity(root, directory)
                self.initializers[directory] = initializer_snapshot(root, directory)
            relative = Path(*components).with_suffix(".py")
            self.sources[relative] = source_identity(root, relative)
        self.verify()

    def verify(self):
        for relative, before in self.sources.items():
            require(
                source_identity(self.root, relative) == before,
                "selected source changed",
            )
        for directory, before in self.directories.items():
            require(
                directory_identity(self.root, directory) == before,
                "package directory changed",
            )
            require(
                initializer_snapshot(self.root, directory)
                == self.initializers[directory],
                "package initializer changed",
            )
        for name, module in self.loaded.items():
            require(sys.modules.get(name) is module, "loaded module identity changed")
            self.module_origin(name, module)

    def module_origin(self, name, module):
        directory = Path(*name.split("."))
        if directory not in self.directories:
            owned_origin(module, (self.root / directory).with_suffix(".py"))
            require(
                module.__name__ == name
                and module.__spec__.name == name
                and type(module.__spec__.loader)
                is importlib.machinery.SourceFileLoader,
                "selected module identity or loader differs",
            )
            return
        self.package_spec(name, module.__spec__)
        require(
            list(getattr(module, "__path__", [])) == [str(self.root / directory)],
            "loaded package search path differs",
        )
        initializer = self.initializers[directory]
        expected = str(self.root / directory / "__init__.py") if initializer else None
        require(
            getattr(module, "__file__", None) == expected, "loaded package file differs"
        )

    def package_spec(self, name, spec):
        directory = Path(*name.split("."))
        initializer = self.initializers[directory]
        require(spec is not None and spec.name == name, "package specification differs")
        expected = str(self.root / directory / "__init__.py") if initializer else None
        require(spec.origin == expected, "package origin differs")
        require(
            list(spec.submodule_search_locations or []) == [str(self.root / directory)],
            "package specification search path differs",
        )
        expected_loader = (
            importlib.machinery.SourceFileLoader
            if initializer
            else importlib.machinery.NamespaceLoader
        )
        require(type(spec.loader) is expected_loader, "package loader differs")

    def package(self, name):
        directory = Path(*name.split("."))
        self.verify()
        if name in sys.modules:
            module = sys.modules[name]
            self.module_origin(name, module)
        else:
            spec = importlib.machinery.PathFinder.find_spec(
                name, [str(self.root / directory.parent)]
            )
            require(spec is not None, "package resolution failed")
            module = importlib.util.module_from_spec(spec)
            self.package_spec(name, module.__spec__)
            sys.modules[name] = module
            if name == "scripts":
                for child in ("unittest_report", "unittest_report_origin"):
                    loaded = sys.modules.get(name + "." + child)
                    if loaded is not None:
                        owned_origin(loaded, self.root / directory / (child + ".py"))
                        setattr(module, child, loaded)
            initializer = self.initializers[directory]
            if initializer:
                before, raw = source_snapshot(
                    self.root, directory / "__init__.py", allow_empty=True
                )
                require(
                    before == initializer, "package initializer changed before import"
                )
                exec(
                    compile(raw, str(self.root / directory / "__init__.py"), "exec"),
                    module.__dict__,
                )
            self.module_origin(name, module)
        self.loaded[name] = module
        self.verify()

    def selected(self, name):
        components = name.split(".")
        for length in range(1, len(components)):
            self.package(".".join(components[:length]))
        relative = Path(*components).with_suffix(".py")
        spec = importlib.util.find_spec(name)
        require(
            spec is not None and spec.origin == str(self.root / relative),
            "selected module resolution origin differs",
        )
        require(
            type(spec.loader) is importlib.machinery.SourceFileLoader,
            "selected module loader differs",
        )
        module = importlib.util.module_from_spec(spec)
        self.verify()
        sys.modules[name] = module
        before, raw = source_snapshot(self.root, relative)
        require(
            before == self.sources[relative], "selected source changed before import"
        )
        exec(compile(raw, str(self.root / relative), "exec"), module.__dict__)
        self.module_origin(name, module)
        self.loaded[name] = module
        self.verify()
        return module


def load_modules(root, names):
    fence = SourceFence(root, names)
    modules = [fence.selected(name) for name in names]
    return modules, fence


def callable_identity(value):
    if isinstance(value, property):
        return (
            value,
            tuple(
                callable_identity(method)
                for method in (value.fget, value.fset, value.fdel)
            ),
            None,
        )
    if isinstance(value, (staticmethod, classmethod)):
        value = value.__func__
    return value, getattr(value, "__code__", None), getattr(value, "__defaults__", None)


def implementation_identity(module):
    values = {}
    for name, value in vars(module).items():
        if isinstance(value, types.FunctionType):
            values[name] = callable_identity(value)
        elif isinstance(value, type):
            values[name] = (
                value,
                {key: callable_identity(method) for key, method in vars(value).items()},
            )
        elif name.isupper():
            values[name] = value, dict(getattr(value, "__dict__", {}))
    return values


def extension_child_origin(name, module):
    """Only genuine spec-less children generated by the verified pyexpat parent."""
    return (
        name
        in {
            "pyexpat.errors",
            "pyexpat.model",
            "xml.parsers.expat.errors",
            "xml.parsers.expat.model",
        }
        and module is getattr(sys.modules.get("pyexpat"), name.rsplit(".", 1)[1], None)
        and getattr(module, "__spec__", None) is None
        and getattr(module, "__file__", None) is None
    )


def standard_library_origins():
    """Check interpreter-owned stdlib dependencies before implementation capture."""
    directory = Path(sys._stdlib_dir)
    for name, module in tuple(sys.modules.items()):
        if name.split(".", 1)[0] in sys.stdlib_module_names:
            source = getattr(getattr(module, "__spec__", None), "origin", None)
            require(
                source in {"built-in", "frozen"}
                or (isinstance(source, str) and Path(source).is_relative_to(directory))
                or extension_child_origin(name, module),
                "standard library module origin differs",
            )


def inventory_execution(tests, capture, authored):
    """Retain private executable baselines before ordinary module/class fixtures."""
    check = require
    before = {test: capture(test) for test in tests}

    def verify(test):
        check(
            test in before and authored(test) and capture(test) == before[test],
            "inventoried testcase executable changed",
        )

    return verify


def runtime_guard(modules):
    """Capture callable implementation identities before selected code imports."""
    snapshot, check = implementation_identity, require
    before = [(module, snapshot(module)) for module in modules]

    def verify():
        for module, expected in before:
            check(sys.modules.get(module.__name__) is module, "producer module changed")
            check(
                snapshot(module) == expected,
                "producer callback or writer implementation changed",
            )

    return verify
