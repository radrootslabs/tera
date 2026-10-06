"""Native input guards and real filesystem/subprocess controls; no artifact claims."""

from __future__ import annotations

import copy
import json
import itertools
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import ffi_build as builder  # noqa: E402
import ffi_native as native  # noqa: E402
import ffi_source as source  # noqa: E402


# Literal cc 1.4.0 cargo_env_var_os inputs, independent of the production list.
CARGO_TRIM_OVERRIDES = (
    {"CARGO_TRIM_PATHS_SCOPE": ""},
    {"CARGO_TRIM_PATHS_SCOPE": "macro"},
    {"CARGO_TRIM_PATHS_REMAP": ""},
    {"CARGO_TRIM_PATHS_REMAP": "native.c=C137_REMAPPED"},
    {"CARGO_TRIM_PATHS_SCOPE": "", "CARGO_TRIM_PATHS_REMAP": ""},
    {"CARGO_TRIM_PATHS_SCOPE": "", "CARGO_TRIM_PATHS_REMAP": "native.c=C137_REMAPPED"},
    {"CARGO_TRIM_PATHS_SCOPE": "all", "CARGO_TRIM_PATHS_REMAP": ""},
    {
        "CARGO_TRIM_PATHS_SCOPE": "all",
        "CARGO_TRIM_PATHS_REMAP": "native.c=C137_REMAPPED",
    },
)


class NativePublicPathTests(unittest.TestCase):
    """Actual private aliases with the unchanged production public roots."""

    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory(prefix="C137_PRIVATE_PATH_SENTINEL-")
        self.addCleanup(temporary.cleanup)
        self.alias = Path(temporary.name).resolve() / "clang"
        self.alias.symlink_to("/usr/bin/clang")
        self.assertFalse(any(self.alias.is_relative_to(p) for p in native.PUBLIC_ROOTS))
        self.assertTrue(
            any(self.alias.resolve().is_relative_to(p) for p in native.PUBLIC_ROOTS)
        )

    def assert_private_rejection(self, path: Path) -> None:
        for helper in (native.public_path, native.public_tool, native.tool_identity):
            with self.subTest(helper=helper.__name__):
                with self.assertRaises(source.ProvenanceError) as failure:
                    helper(path if helper is native.tool_identity else str(path))
                self.assertNotIn(str(path), str(failure.exception))
                self.assertNotIn("C137_PRIVATE_PATH_SENTINEL", str(failure.exception))

    def test_real_private_alias_to_public_tool_rejects_without_private_diagnostic(
        self,
    ) -> None:
        self.assert_private_rejection(self.alias)

    def test_public_prefix_traversal_to_private_alias_rejects_without_diagnostic(
        self,
    ) -> None:
        path = Path("/usr/..") / self.alias.relative_to("/")
        self.assertTrue(path.is_relative_to(Path("/usr")))
        normalized = Path(os.path.normpath(path))
        self.assertFalse(any(normalized.is_relative_to(p) for p in native.PUBLIC_ROOTS))
        self.assert_private_rejection(path)

    def test_traversal_rejects_even_when_resolved_tool_is_public(self) -> None:
        self.assert_private_rejection(Path("/usr/bin/../bin/clang"))

    def test_real_public_tools_keep_selected_path_and_public_resolved_identity(
        self,
    ) -> None:
        for name in ("clang", "ranlib"):
            with self.subTest(tool=name):
                path = Path("/usr/bin") / name
                self.assertEqual(native.public_tool(str(path)), path)
                identity = native.tool_identity(path)
                self.assertEqual(identity["path"], str(path))
                self.assertEqual(identity["resolved_path"], str(path.resolve()))
                self.assertTrue(
                    any(
                        Path(identity["resolved_path"]).is_relative_to(p)
                        for p in native.PUBLIC_ROOTS
                    )
                )


class NativeInputTests(unittest.TestCase):
    def test_literal_cargo_trim_inputs_reject_source_policy(self) -> None:
        for overrides in CARGO_TRIM_OVERRIDES:
            with (
                self.subTest(overrides=overrides),
                patch.dict(os.environ, overrides, clear=True),
            ):
                with self.assertRaisesRegex(
                    source.ProvenanceError, "^ungoverned inherited native build input$"
                ):
                    source.reject_build_overrides()

    def test_literal_cargo_trim_inputs_reject_inspection_environment(self) -> None:
        for overrides in CARGO_TRIM_OVERRIDES:
            with (
                self.subTest(overrides=overrides),
                patch.dict(os.environ, overrides, clear=True),
            ):
                with self.assertRaisesRegex(
                    source.ProvenanceError, "^ungoverned inherited native build input$"
                ):
                    native.inspection_environment()

    def test_literal_cargo_trim_inputs_reject_build_before_config_read(self) -> None:
        config = source.producer_contract(SCRIPTS.parent)
        for overrides in CARGO_TRIM_OVERRIDES:
            with (
                self.subTest(overrides=overrides),
                patch.dict(os.environ, overrides, clear=True),
                patch.object(
                    builder.contract, "_read_toml", return_value={"revision": "a" * 40}
                ) as read,
            ):
                with self.assertRaisesRegex(
                    source.ProvenanceError, "^ungoverned inherited native build input$"
                ):
                    builder.build_environment(SCRIPTS.parent, Path("/external"), config)
                read.assert_not_called()

    def test_cc_cargo_trim_names_do_not_invent_host_or_target_variants(self) -> None:
        for name in ("CARGO_TRIM_PATHS_SCOPE", "CARGO_TRIM_PATHS_REMAP"):
            for variant in (
                "HOST_" + name,
                "TARGET_" + name,
                name + "_aarch64-apple-ios",
                name + "_aarch64_apple_ios",
            ):
                with self.subTest(variant=variant):
                    native.reject_inherited({variant: "inactive cc input"})

    def test_every_supported_input_variant_rejects_empty_and_nonempty(self) -> None:
        for name in sorted(native.inherited_names()):
            for value in ("", "unrecorded"):
                with self.subTest(name=name, empty=not value):
                    with patch.dict(os.environ, {name: value}, clear=True):
                        with self.assertRaisesRegex(
                            source.ProvenanceError, "ungoverned"
                        ):
                            source.reject_build_overrides()

    def test_tool_precedence_and_additive_flag_inputs_are_never_flattened(self) -> None:
        for target in native.TARGET_SDKS:
            for base in native.TOOL_INPUTS:
                names = (
                    base,
                    "HOST_" + base,
                    "TARGET_" + base,
                    base + "_" + target.replace("-", "_"),
                    base + "_" + target,
                )
                for winner in names:
                    environment = {name: "lower" for name in names}
                    environment[winner] = ""
                    with self.subTest(target=target, base=base, winner=winner):
                        with self.assertRaises(source.ProvenanceError):
                            native.reject_inherited(environment)

    def test_inactive_cpp_and_other_target_controls_do_not_invent_inputs(self) -> None:
        environment = {
            "CXX": "inactive",
            "CPPFLAGS": "inactive",
            "CC_wasm32_unknown_unknown": "inactive",
        }
        native.reject_inherited(environment)

    def test_ring_pregeneration_is_rejected_for_every_presence_sensitive_value(
        self,
    ) -> None:
        for value in ("", "0", "1", "malformed"):
            with self.subTest(value=value), self.assertRaises(source.ProvenanceError):
                native.reject_inherited({"RING_PREGENERATE_ASM": value})

    def test_uv_fallback_is_bound_by_digest_and_other_loader_controls_reject(
        self,
    ) -> None:
        base = {"PATH": "/public/tools"}
        first = native.inherited_identity(base)
        second = native.inherited_identity(
            {**base, "DYLD_FALLBACK_LIBRARY_PATH": "sentinel"}
        )
        self.assertNotEqual(first, second)
        self.assertNotIn("sentinel", json.dumps(second))
        for key in (
            "DYLD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_FRAMEWORK_PATH",
        ):
            with self.subTest(key=key), self.assertRaises(source.ProvenanceError):
                native.inherited_identity({**base, key: ""})

    def test_missing_or_unbounded_path_is_rejected(self) -> None:
        for value in ("", "x" * (source.MAX_BYTES + 1)):
            with (
                self.subTest(empty=not value),
                self.assertRaises(source.ProvenanceError),
            ):
                native.inherited_identity({"PATH": value})

    def test_extbuild_allowed_generator_debug_policy_is_preserved(self) -> None:
        with patch.dict(
            os.environ,
            {
                "EXT_BUILD_RUN_ACTIVE": "1",
                "CARGO_PROFILE_DEV_DEBUG": "line-tables-only",
            },
            clear=True,
        ):
            source.reject_build_overrides()
            self.assertEqual(
                source.allowed_profile_overrides(),
                {"CARGO_PROFILE_DEV_DEBUG": "line-tables-only"},
            )


class NativeBindingTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.tools = self.root / "bin"
        self.tools.mkdir()
        for name in (
            "xcrun",
            "clang",
            "ld",
            "ar",
            "libtool",
            "swift-frontend",
            "xcodebuild",
            "swiftformat",
            "swift-format",
            "swift",
        ):
            tool = self.tools / name
            tool.write_text("synthetic tool identity: " + name)
            tool.chmod(0o755)
        for alias, target in (
            ("ranlib", "libtool"),
            ("swiftc", "swift-frontend"),
            ("swift-symbolgraph-extract", "swift-frontend"),
        ):
            (self.tools / alias).symlink_to(target)
        self.sdks = {}
        self.resources = self.root / "resources"
        self.resources.mkdir()
        (self.resources / "stddef.h").write_text("/* clang resources */")
        for name in native.TARGET_SDKS.values():
            sdk = self.root / name
            sdk.mkdir()
            (sdk / "SDKSettings.json").write_text('{"Version":"26.5"}')
            (sdk / "header.h").write_text("/* governed SDK */")
            self.sdks[name] = sdk
        self.environment = {
            "PATH": str(self.tools),
            "EXT_BUILD_RUN_ACTIVE": "1",
            "CARGO_PROFILE_DEV_DEBUG": "line-tables-only",
            "DYLD_FALLBACK_LIBRARY_PATH": "private-sentinel",
        }
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, self.environment, clear=True).start()
        patch.object(native, "PUBLIC_ROOTS", (self.root,)).start()
        patch.object(source, "command", side_effect=self.query).start()

    def query(self, root: Path, argv: list[str], **kwargs) -> bytes:
        self.assertEqual(root, self.root)
        self.assertNotIn("DYLD_FALLBACK_LIBRARY_PATH", kwargs["environment"])
        if argv[0] == "/usr/bin/xcode-select":
            return str(self.root).encode()
        if "-print-resource-dir" in argv:
            return str(self.resources).encode()
        sdk = argv[argv.index("--sdk") + 1]
        if "--find" in argv:
            return str(self.tools / argv[-1]).encode()
        if "--show-sdk-path" in argv:
            return str(self.sdks[sdk]).encode()
        if "--show-sdk-version" in argv:
            return b"26.5"
        if "--show-sdk-build-version" in argv:
            return b"test-sdk-build"
        self.fail("unexpected synthetic query")

    def capture(self, target: str) -> dict:
        return native.capture(self.root, target)

    def test_public_fixture_aliases_retain_actual_argv0_dispatch(self) -> None:
        # These existing synthetic tools use explicitly substituted fixture roots.
        dispatch = (
            f"#!{sys.executable}\nimport os,sys\nprint(os.path.basename(sys.argv[0]))\n"
        )
        for name in ("libtool", "swift-frontend"):
            (self.tools / name).write_text(dispatch)
        for name in ("ranlib", "swiftc", "swift-symbolgraph-extract"):
            with self.subTest(tool=name):
                alias = self.tools / name
                selected = native.public_tool(str(alias))
                self.assertEqual(selected, alias)
                self.assertNotEqual(selected, alias.resolve())
                identity = native.tool_identity(selected)
                self.assertEqual(identity["path"], str(alias))
                self.assertEqual(identity["resolved_path"], str(alias.resolve()))
                result = subprocess.run(
                    [str(selected)],
                    check=True,
                    capture_output=True,
                    text=True,
                    timeout=30,
                    env=native.inspection_environment(),
                )
                self.assertEqual(result.stdout.strip(), name)

    def test_literal_cargo_trim_capture_rejects_before_tool_queries(self) -> None:
        for overrides in CARGO_TRIM_OVERRIDES:
            with self.subTest(overrides=overrides), patch.dict(os.environ, overrides):
                source.command.reset_mock()
                with self.assertRaisesRegex(
                    source.ProvenanceError, "^ungoverned inherited native build input$"
                ):
                    self.capture(native.HOST)
                source.command.assert_not_called()

    def test_literal_cargo_trim_introduction_rejects_after_initial_capture(
        self,
    ) -> None:
        context = self.capture(native.HOST)
        for overrides in CARGO_TRIM_OVERRIDES:
            with self.subTest(overrides=overrides), patch.dict(os.environ, overrides):
                source.command.reset_mock()
                with self.assertRaisesRegex(
                    source.ProvenanceError, "^ungoverned inherited native build input$"
                ):
                    native.verify(self.root, context)
                source.command.assert_not_called()

    def test_literal_cargo_trim_in_execution_dictionary_rejects_with_clean_ambient(
        self,
    ) -> None:
        context = self.capture("aarch64-apple-ios")
        host = self.capture(native.HOST)
        for overrides in CARGO_TRIM_OVERRIDES:
            with self.subTest(overrides=overrides):
                self.assertNotIn("CARGO_TRIM_PATHS_SCOPE", os.environ)
                self.assertNotIn("CARGO_TRIM_PATHS_REMAP", os.environ)
                environment = {
                    **self.environment,
                    **overrides,
                    "CARGO_ENCODED_RUSTFLAGS": "--remap-path-prefix=/fixture=/tera",
                    "IPHONEOS_DEPLOYMENT_TARGET": "18.0",
                }
                source.command.reset_mock()
                with self.assertRaisesRegex(
                    source.ProvenanceError, "^ungoverned inherited native build input$"
                ):
                    native.execution_environment(self.root, environment, context, host)
                source.command.assert_not_called()

    def test_clean_owned_build_environment_retains_governed_flags_and_deployment(
        self,
    ) -> None:
        config = source.producer_contract(SCRIPTS.parent)
        environment = builder.build_environment(SCRIPTS.parent, self.root, config)
        host = self.capture(native.HOST)
        for target in native.TARGET_SDKS:
            with self.subTest(target=target):
                context = self.capture(target)
                bound = native.execution_environment(
                    self.root, environment, context, host
                )
                self.assertTrue(
                    bound["CARGO_ENCODED_RUSTFLAGS"].startswith(
                        environment["CARGO_ENCODED_RUSTFLAGS"] + "\x1f"
                    )
                )
                self.assertEqual(
                    bound["SOURCE_DATE_EPOCH"], environment["SOURCE_DATE_EPOCH"]
                )
                self.assertEqual(bound["CARGO_PROFILE_DEV_DEBUG"], "line-tables-only")
                self.assertNotIn("DYLD_FALLBACK_LIBRARY_PATH", bound)
                self.assertNotIn("MACOSX_DEPLOYMENT_TARGET", bound)
                if target == native.HOST:
                    self.assertNotIn("IPHONEOS_DEPLOYMENT_TARGET", bound)
                else:
                    self.assertEqual(bound["IPHONEOS_DEPLOYMENT_TARGET"], "18.0")

    def test_all_targets_and_independent_generator_bind_real_alias_and_sdk_bytes(
        self,
    ) -> None:
        host = self.capture(native.HOST)
        for target, sdk in native.TARGET_SDKS.items():
            with self.subTest(target=target):
                context = self.capture(target)
                self.assertEqual(context, self.capture(target))
                self.assertEqual(context["sdk"]["path"], str(self.sdks[sdk]))
                self.assertEqual(
                    context["tools"]["ranlib"]["path"], str(self.tools / "ranlib")
                )
                self.assertEqual(
                    context["tools"]["ranlib"]["resolved_path"],
                    str(self.tools / "libtool"),
                )
                self.assertEqual(
                    context["tools"]["swiftc"]["path"], str(self.tools / "swiftc")
                )
                bound = native.execution_environment(
                    self.root, self.environment, context, host
                )
                self.assertEqual(bound["SDKROOT"], str(self.sdks[sdk]))
                self.assertEqual(bound["PATH"], self.environment["PATH"])
                self.assertEqual(bound["CARGO_PROFILE_DEV_DEBUG"], "line-tables-only")
                self.assertNotIn("DYLD_FALLBACK_LIBRARY_PATH", bound)
                self.assertNotIn("MACOSX_DEPLOYMENT_TARGET", bound)
                for selected in (target, native.HOST):
                    self.assertEqual(bound["CC_" + selected], str(self.tools / "clang"))
                    self.assertEqual(bound["AR_" + selected], str(self.tools / "ar"))
                    self.assertEqual(
                        bound["RANLIB_" + selected], str(self.tools / "ranlib")
                    )
                if target == native.HOST:
                    self.assertNotIn("IPHONEOS_DEPLOYMENT_TARGET", bound)
                else:
                    self.assertEqual(bound["IPHONEOS_DEPLOYMENT_TARGET"], "18.0")

    def test_changed_tools_sdk_and_selection_are_fenced_before_execution(self) -> None:
        context = self.capture("aarch64-apple-ios")
        for path in (
            self.tools / "clang",
            self.tools / "ld",
            self.tools / "ar",
            self.tools / "libtool",
            self.tools / "swift-frontend",
            self.tools / "xcodebuild",
            self.tools / "swiftformat",
            self.sdks["iphoneos"] / "header.h",
            self.resources / "stddef.h",
        ):
            original = path.read_bytes()
            path.write_bytes(original + b"changed")
            with (
                self.subTest(tool=path.name),
                self.assertRaisesRegex(source.ProvenanceError, "drift"),
            ):
                native.verify(self.root, context)
            path.write_bytes(original)
        changed = copy.deepcopy(context)
        changed["sdk"]["path"] = str(self.sdks["macosx"])
        with self.assertRaisesRegex(source.ProvenanceError, "drift"):
            native.verify(self.root, changed)

    def test_environment_presence_and_path_changes_are_fenced(self) -> None:
        context = self.capture(native.HOST)
        for overrides in (
            {"PATH": str(self.tools) + ":/different"},
            {"DYLD_FALLBACK_LIBRARY_PATH": "changed"},
            {"LIBSQLITE3_SYS_USE_PKG_CONFIG": ""},
            {"DEVELOPER_DIR": ""},
            {"CARGO_PROFILE_DEV_DEBUG": "different"},
        ):
            with (
                self.subTest(keys=sorted(overrides)),
                patch.dict(os.environ, overrides),
            ):
                with self.assertRaises(source.ProvenanceError):
                    native.verify(self.root, context)

    def test_native_stage_executes_captured_environment_in_a_real_child(self) -> None:
        host = self.capture(native.HOST)
        for target in (*native.TARGET_SDKS, native.HOST):
            context = self.capture(target)
            log = builder.run_native(
                self.root,
                self.root,
                "child-" + target,
                [
                    sys.executable,
                    "-c",
                    "import json,os; print(json.dumps(dict(os.environ)))",
                ],
                self.environment,
                context,
                host,
            )
            observed = json.loads(log.read_text())
            self.assertEqual(observed["SDKROOT"], context["sdk"]["path"])
            self.assertEqual(
                observed["CC_" + target], context["tools"]["compiler"]["path"]
            )
            self.assertEqual(
                observed[
                    "CARGO_TARGET_" + target.upper().replace("-", "_") + "_LINKER"
                ],
                context["tools"]["compiler"]["path"],
            )
            self.assertIn(
                context["tools"]["linker"]["path"], observed["CARGO_ENCODED_RUSTFLAGS"]
            )

    def test_target_and_host_sdk_changes_bind_both_c_and_rust_cache_inputs(
        self,
    ) -> None:
        target = self.capture("aarch64-apple-ios")
        host = self.capture(native.HOST)
        original = native.execution_environment(
            self.root, self.environment, target, host
        )
        repeated = native.execution_environment(
            self.root, self.environment, target, host
        )
        self.assertEqual(original, repeated)
        for sdk, selected in (("iphoneos", "target"), ("macosx", "host")):
            with self.subTest(selected=selected):
                (self.sdks[sdk] / "header.h").write_text("changed SDK context " + sdk)
                target = self.capture("aarch64-apple-ios")
                host = self.capture(native.HOST)
                changed = native.execution_environment(
                    self.root, self.environment, target, host
                )
                for key in (
                    "CFLAGS_aarch64-apple-ios",
                    "CFLAGS_aarch64-apple-darwin",
                    "CARGO_ENCODED_RUSTFLAGS",
                ):
                    self.assertNotEqual(changed[key], original[key])
                self.assertIn(
                    "--no-default-config", changed["CFLAGS_aarch64-apple-ios"]
                )
                self.assertEqual(changed["PATH"], original["PATH"])
                original = changed

    def test_postbuild_sdk_drift_rejects_real_child_result(self) -> None:
        context = self.capture(native.HOST)
        path = self.sdks["macosx"] / "header.h"
        code = (
            "from pathlib import Path; Path("
            + repr(str(path))
            + ").write_text('changed')"
        )
        with self.assertRaisesRegex(source.ProvenanceError, "drift"):
            builder.run_native(
                self.root,
                self.root,
                "postbuild-drift",
                [sys.executable, "-c", code],
                self.environment,
                context,
                context,
            )
        self.assertTrue((self.root / "postbuild-drift.txt").is_file())

    def test_bad_tool_path_or_permissions_reject_without_echoing_path(self) -> None:
        with self.assertRaisesRegex(source.ProvenanceError, "not absolute"):
            native.public_path("private-sentinel")
        with self.assertRaisesRegex(source.ProvenanceError, "not public"):
            native.public_path("/bin/sh")
        path = self.tools / "ar"
        path.chmod(0o644)
        with self.assertRaisesRegex(source.ProvenanceError, "not executable"):
            native.tool_identity(path)

    def test_sdk_symlink_escape_and_nonregular_file_reject(self) -> None:
        sdk = self.sdks["iphoneos"]
        (sdk / "link").symlink_to(self.tools / "clang")
        with self.assertRaisesRegex(source.ProvenanceError, "escapes"):
            native.sdk_identity(sdk)
        (sdk / "link").unlink()
        os.mkfifo(sdk / "fifo")
        with self.assertRaisesRegex(source.ProvenanceError, "regular"):
            native.sdk_identity(sdk)

    def test_sdk_node_and_aggregate_byte_limits_exact_and_one_over(self) -> None:
        sdk = self.sdks["iphoneos"]
        size = sum(path.stat().st_size for path in sdk.iterdir())
        with (
            patch.object(native, "MAX_NATIVE_NODES", 2),
            patch.object(native, "MAX_NATIVE_BYTES", size),
        ):
            self.assertEqual(native.sdk_identity(sdk)["nodes"], 2)
            self.assertEqual(native.sdk_identity(sdk)["bytes"], size)
        with patch.object(native, "MAX_NATIVE_NODES", 1):
            with self.assertRaisesRegex(source.ProvenanceError, "inspection bound"):
                native.sdk_identity(sdk)
        with patch.object(native, "MAX_NATIVE_BYTES", size - 1):
            with self.assertRaisesRegex(source.ProvenanceError, "byte bound"):
                native.sdk_identity(sdk)

    def test_sdk_inspection_deadline_is_bounded(self) -> None:
        with patch.object(
            native.time,
            "monotonic",
            side_effect=itertools.chain([0], itertools.repeat(120)),
        ):
            native.sdk_identity(self.sdks["iphoneos"])
        with patch.object(native.time, "monotonic", side_effect=[0, 121]):
            with self.assertRaisesRegex(source.ProvenanceError, "inspection bound"):
                native.sdk_identity(self.sdks["iphoneos"])

    def test_long_stream_is_fenced_within_chunks(self) -> None:
        path = self.sdks["iphoneos"] / "header.h"
        with patch.object(native.time, "monotonic", side_effect=[0, 0, 121]):
            with self.assertRaisesRegex(source.ProvenanceError, "inspection bound"):
                native.file_identity(path)

    def test_stream_detects_content_mutation_at_descriptor_settlement(self) -> None:
        path = self.sdks["iphoneos"] / "header.h"
        stamp = native.file_stamp

        def changed_stamp(selected):
            selected.write_bytes(b"changed while reading")
            return stamp(selected)

        with patch.object(native, "file_stamp", side_effect=changed_stamp):
            with self.assertRaisesRegex(
                source.ProvenanceError, "changed while reading"
            ):
                native.file_identity(path)

    def test_failed_sdk_walk_rejects_incomplete_inventory(self) -> None:
        with patch.object(
            native.os, "scandir", side_effect=PermissionError("private-sentinel")
        ):
            with self.assertRaisesRegex(source.ProvenanceError, "enumeration failed"):
                native.sdk_identity(self.sdks["iphoneos"])

    def observed_sdk_directory(self) -> tuple[Path, dict, object]:
        directory = self.root / "wide-sdk"
        directory.mkdir()
        for index in range(32):
            (directory / f"member{index}").write_bytes(b"synthetic SDK byte")
        observed = {"entries": 0}
        scanner = native.os.scandir

        class CountingScandir:
            def __init__(self, selected):
                self.iterator = scanner(selected)

            def __iter__(self):
                return self

            def __next__(self):
                entry = next(self.iterator)
                observed["entries"] += 1
                return entry

            def __enter__(self):
                return self

            def __exit__(self, *args):
                self.iterator.close()

        return directory, observed, CountingScandir

    def test_sdk_wide_directory_rejects_before_buffering_beyond_node_cap(self) -> None:
        directory, observed, scanner = self.observed_sdk_directory()
        with (
            patch.object(native, "MAX_NATIVE_NODES", 2),
            patch.object(native.os, "scandir", scanner),
        ):
            with self.assertRaisesRegex(source.ProvenanceError, "inspection bound"):
                native.sdk_inventory(directory, float("inf"))
        self.assertLessEqual(observed["entries"], 3)

    def test_sdk_deadline_is_checked_during_real_directory_enumeration(self) -> None:
        directory, observed, scanner = self.observed_sdk_directory()
        with (
            patch.object(native.os, "scandir", scanner),
            patch.object(
                native.time, "monotonic", side_effect=lambda: observed["entries"]
            ),
        ):
            with self.assertRaisesRegex(source.ProvenanceError, "inspection bound"):
                native.sdk_inventory(directory, 2)
        self.assertLessEqual(observed["entries"], 3)

    def test_inventoried_directory_replaced_by_foreign_symlink_is_not_traversed(
        self,
    ) -> None:
        sdk = self.sdks["iphoneos"]
        directory = sdk / "child"
        directory.mkdir()
        (directory / "member").write_text("SDK member")
        foreign = self.root / "foreign"
        foreign.mkdir()
        (foreign / "sentinel").write_text("must not be enumerated")
        stamp = native.file_stamp

        def replace_directory(path):
            value = stamp(path)
            if path == directory and not path.is_symlink():
                path.rename(self.root / "preserved-child")
                path.symlink_to(foreign, target_is_directory=True)
            return value

        with patch.object(native, "file_stamp", side_effect=replace_directory):
            with self.assertRaises(source.ProvenanceError):
                native.sdk_inventory(sdk, float("inf"))

    def test_tool_query_receives_only_remaining_aggregate_deadline(self) -> None:
        with patch.object(native.time, "monotonic", return_value=100):
            native.query(self.root, ["/usr/bin/xcode-select", "-p"], {}, 101)
        self.assertEqual(source.command.call_args.kwargs["timeout"], 1)
        with patch.object(native.time, "monotonic", return_value=102):
            with self.assertRaisesRegex(source.ProvenanceError, "inspection bound"):
                native.query(self.root, ["/usr/bin/xcode-select", "-p"], {}, 101)

    def test_legitimate_internal_sdk_links_and_capture_inventory_drift(self) -> None:
        sdk = self.sdks["iphoneos"]
        (sdk / "alias.h").symlink_to("header.h")
        self.assertEqual(native.sdk_identity(sdk)["nodes"], 3)
        actual_entry = native.sdk_entry

        def changing_entry(root, path, deadline):
            value = actual_entry(root, path, deadline)
            if path.name == "header.h":
                (root / "new.h").write_text("inventory drift")
            return value

        with patch.object(native, "sdk_entry", side_effect=changing_entry):
            with self.assertRaisesRegex(source.ProvenanceError, "inventory changed"):
                native.sdk_identity(sdk)

    def test_sdk_root_and_member_replacement_during_capture_reject(self) -> None:
        sdk = self.sdks["iphoneos"]
        actual_entry = native.sdk_entry

        def replacing_entry(root, path, deadline):
            value = actual_entry(root, path, deadline)
            if path.name == "header.h":
                path.unlink()
                path.write_text("/* governed SDK */")
            return value

        with patch.object(native, "sdk_entry", side_effect=replacing_entry):
            with self.assertRaisesRegex(source.ProvenanceError, "inventory changed"):
                native.sdk_identity(sdk)

    def test_sdk_root_replacement_during_capture_rejects_equal_bytes(self) -> None:
        sdk = self.sdks["iphoneos"]
        actual_entry = native.sdk_entry

        def replace_root(root, path, deadline):
            value = actual_entry(root, path, deadline)
            if path.name == "header.h":
                root.rename(root.with_name("preserved-sdk"))
                root.mkdir()
                for original in root.with_name("preserved-sdk").iterdir():
                    (root / original.name).write_bytes(original.read_bytes())
            return value

        with patch.object(native, "sdk_entry", side_effect=replace_root):
            with self.assertRaisesRegex(source.ProvenanceError, "inventory changed"):
                native.sdk_identity(sdk)

    def test_tool_nofollow_identity_rejects_symlink_input(self) -> None:
        with self.assertRaises(OSError):
            native.file_identity(self.tools / "ranlib")


if __name__ == "__main__":
    unittest.main()
