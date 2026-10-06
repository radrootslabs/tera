"""Real pinned Cargo/cc rebuild controls; disposable contexts are not admission."""

from __future__ import annotations

import copy
import hashlib
import importlib
import json
import os
import shlex
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

builder = importlib.import_module("ffi_build")
native = importlib.import_module("ffi_native")


class NativeCargoCacheTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        if not os.environ.get("EXT_BUILD_RUN_ACTIVE"):
            raise RuntimeError("real native cache tests require extbuild")
        cls.capsule = SCRIPTS.parent
        cls.platform = native.capture(cls.capsule, native.HOST)

    def setUp(self) -> None:
        project, self.target_root = builder.build_roots(self.capsule)
        directory = project / "target/tera_ffi/native_cache_tests"
        directory.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix="fixture-", dir=directory)).resolve()
        self.environment = native.inspection_environment()
        self.target_before = self.environment["CARGO_TARGET_DIR"]
        self.package = "tera_native_cache_" + self.root.name.replace("-", "_")
        (self.root / "src").mkdir()
        (self.root / "Cargo.toml").write_text(
            f'[package]\nname="{self.package}"\nversion="0.0.0"\nedition="2024"\n'
            '[build-dependencies]\ncc="=1.4.0"\nfind-msvc-tools="=0.1.9"\nshlex="=2.0.1"\n'
        )
        (self.root / "build.rs").write_text(
            'fn main() { println!("cargo:rerun-if-changed=native.c"); '
            'cc::Build::new().file("native.c").compile("native_cache_control"); }\n'
        )
        (self.root / "native.c").write_text(
            "int native_value(void) { return C137_COMPILER_VALUE + C137_SDK_VALUE; }\n"
        )
        (self.root / "src/main.rs").write_text(
            'unsafe extern "C" { fn native_value() -> i32; } '
            'fn main() { println!("{}", unsafe { native_value() }); }\n'
        )
        self.wrapper = self.root / "clang"
        self.invocations = self.root / "compiler_invocations.txt"
        self.sdk = self.root / "MacOSX.sdk"
        self.sdk.mkdir()
        self.header = self.sdk / "native_value.h"
        self.header.write_text("#define C137_SDK_VALUE 0\n")
        self.write_compiler(1)
        self.observations = []
        argv = [
            "cargo",
            "generate-lockfile",
            "--offline",
            "--manifest-path",
            str(self.root / "Cargo.toml"),
        ]
        with (self.root / "lock.txt").open("wb") as log:
            subprocess.run(
                argv,
                cwd=self.root,
                env=self.environment,
                stdout=log,
                stderr=subprocess.STDOUT,
                check=True,
                timeout=120,
            )
        self.addCleanup(self.save_observations)

    def write_compiler(self, value: int) -> None:
        self.wrapper.write_text(
            '#!/bin/sh\ncase " $* " in *native.c*) printf "compiled\\n" >> '
            + shlex.quote(str(self.invocations))
            + ";; esac\nexec "
            + shlex.quote(self.platform["tools"]["compiler"]["path"])
            + f' -DC137_COMPILER_VALUE={value} "$@" -isysroot '
            + shlex.quote(self.platform["sdk"]["path"])
            + "\n"
        )
        self.wrapper.chmod(0o755)

    def context(self, target: str = native.HOST) -> dict:
        context = copy.deepcopy(self.platform)
        context["target"] = target
        context["inherited"]["path_sha256"] = hashlib.sha256(
            self.environment["PATH"].encode()
        ).hexdigest()
        context["tools"]["compiler"] = {
            "path": str(self.wrapper),
            "resolved_path": str(self.wrapper),
            **native.file_identity(self.wrapper),
        }
        context["platform_sdk"] = context["sdk"]
        context["sdk"] = native.sdk_identity(self.sdk)
        context["compiler_arguments"] += [
            "-isysroot",
            context["platform_sdk"]["path"],
            "-include",
            str(self.header),
        ]
        context["linker_driver_arguments"] += [
            "-isysroot",
            context["platform_sdk"]["path"],
        ]
        return context

    def fence(self, root: Path, context: dict) -> None:
        self.assertEqual(root, self.root)
        self.assertEqual(
            hashlib.sha256(self.environment["PATH"].encode()).hexdigest(),
            context["inherited"]["path_sha256"],
        )
        compiler = context["tools"]["compiler"]
        self.assertEqual(
            native.file_identity(Path(compiler["resolved_path"])),
            {key: compiler[key] for key in ("sha256", "bytes")},
        )
        self.assertEqual(
            native.sdk_identity(Path(context["sdk"]["path"])), context["sdk"]
        )
        self.assertEqual(
            native.sdk_identity(Path(context["platform_sdk"]["path"])),
            context["platform_sdk"],
        )
        self.assertEqual(
            native.sdk_identity(Path(context["compiler_resources"]["path"])),
            context["compiler_resources"],
        )
        for key in ("linker", "archiver", "ranlib"):
            tool = context["tools"][key]
            self.assertEqual(
                native.file_identity(Path(tool["resolved_path"])),
                {name: tool[name] for name in ("sha256", "bytes")},
            )

    def execute(
        self, name: str, context: dict, host: dict, *, generator: bool = False
    ) -> dict:
        argv = [
            "cargo",
            "run",
            "--manifest-path",
            str(self.root / "Cargo.toml"),
            "--offline",
            "--locked",
            "--release",
            "-vv",
        ]
        if not generator:
            argv += ["--target", native.HOST]
        with patch.object(native, "verify", side_effect=self.fence):
            started = time.monotonic()
            log = builder.run_native(
                self.root, self.root, name, argv, self.environment, context, host
            )
            elapsed = time.monotonic() - started
        raw = log.read_text()
        relative = "release" if generator else native.HOST + "/release"
        binary = self.target_root / relative / self.package
        observed = {
            "name": name,
            "argv": argv,
            "exit_code": 0,
            "value": int(raw.splitlines()[-1]),
            "log": str(log),
            "compiler_calls": len(self.invocations.read_text().splitlines()),
            "binary_mtime_ns": binary.stat().st_mtime_ns,
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "context": context,
            "host": host,
            "cargo_target_dir": self.environment["CARGO_TARGET_DIR"],
            "elapsed_seconds": elapsed,
        }
        self.assertEqual(observed["cargo_target_dir"], self.target_before)
        self.observations.append(observed)
        return observed

    def save_observations(self) -> None:
        (self.root / "observations.json").write_text(
            json.dumps(
                {
                    "fixture": "Real clang/ld/ar/ranlib and platform SDK, pinned cc1.4.0 C archive and executed Rust binary. Disposable compiler wrapper and SDK include-header context replace public selection; real input bytes are fenced before/after each invocation. No production capture/public admission assertion or Tera artifact qualification.",
                    "root": str(self.root),
                    "observations": self.observations,
                },
                sort_keys=True,
                indent=2,
            )
            + "\n"
        )
        print("native cache observations: " + str(self.root / "observations.json"))

    def assert_reused(self, first: dict, second: dict) -> None:
        self.assertEqual(second["compiler_calls"], first["compiler_calls"])
        self.assertEqual(second["binary_mtime_ns"], first["binary_mtime_ns"])
        self.assertEqual(second["binary_sha256"], first["binary_sha256"])

    def test_same_path_compiler_bytes_rebuild_real_cargo_cc_and_reuse(
        self,
    ) -> None:
        original = self.context()
        first = self.execute("first", original, original)
        self.assertEqual(first["value"], 1)
        self.assert_reused(first, self.execute("stable-first", original, original))
        self.write_compiler(2)
        compiler = self.context()
        second = self.execute("changed-compiler", compiler, compiler)
        self.assertEqual(second["value"], 2)
        self.assertGreater(second["compiler_calls"], first["compiler_calls"])
        self.assert_reused(second, self.execute("stable-compiler", compiler, compiler))

    def test_same_path_sdk_header_bytes_rebuild_real_cargo_cc_and_reuse(self) -> None:
        original = self.context()
        first = self.execute("sdk-first", original, original)
        self.assertEqual(first["value"], 1)
        self.assert_reused(first, self.execute("sdk-stable-first", original, original))
        self.header.write_text("#define C137_SDK_VALUE 3\n")
        sdk = self.context()
        third = self.execute("changed-sdk", sdk, sdk)
        self.assertEqual(third["value"], 4)
        self.assertGreater(third["compiler_calls"], first["compiler_calls"])
        self.assert_reused(third, self.execute("stable-sdk", sdk, sdk))

    def test_metadata_suppressed_cc_builder_rebuilds_via_cargo_fingerprint(
        self,
    ) -> None:
        (self.root / "build.rs").write_text(
            'fn main() { println!("cargo:rerun-if-changed=native.c"); '
            'cc::Build::new().cargo_metadata(false).file("native.c").compile("native_cache_control"); '
            'println!("cargo:rustc-link-search=native={}", std::env::var("OUT_DIR").unwrap()); '
            'println!("cargo:rustc-link-lib=static=native_cache_control"); }\n'
        )
        original = self.context()
        first = self.execute("metadata-first", original, original)
        self.assertEqual(first["value"], 1)
        self.assert_reused(first, self.execute("metadata-stable", original, original))
        self.write_compiler(2)
        changed = self.context()
        second = self.execute("metadata-compiler-changed", changed, changed)
        self.assertEqual(second["value"], 2)
        self.assertGreater(second["compiler_calls"], first["compiler_calls"])
        self.assert_reused(
            second, self.execute("metadata-rebuilt-stable", changed, changed)
        )

    def test_independent_host_context_invalidates_real_generator_cache(self) -> None:
        target = self.context("aarch64-apple-ios")
        # Keep this independently selected target context unchanged while only
        # the host compiler fixture changes. The executed fixture is Darwin.
        target["tools"]["compiler"] = copy.deepcopy(self.platform["tools"]["compiler"])
        target["sdk"] = target["platform_sdk"]
        target["compiler_arguments"] = self.platform["compiler_arguments"]
        host = self.context()
        first = self.execute("generator-first", target, host, generator=True)
        self.assertEqual(first["value"], 1)
        self.assert_reused(
            first, self.execute("generator-stable", target, host, generator=True)
        )
        self.write_compiler(2)
        changed_host = self.context()
        current = self.execute(
            "generator-host-changed", target, changed_host, generator=True
        )
        self.assertEqual(current["value"], 2)
        self.assertGreater(current["compiler_calls"], first["compiler_calls"])
        self.assert_reused(
            current,
            self.execute("generator-host-stable", target, changed_host, generator=True),
        )


if __name__ == "__main__":
    unittest.main()
