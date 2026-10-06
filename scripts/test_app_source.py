"""Real Git and descriptor-relative authored-app capture controls."""

from __future__ import annotations

import json
import os
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

import app_source as app  # noqa: E402
import ffi_source as source  # noqa: E402


class AppSourceTests(unittest.TestCase):
    def setUp(self) -> None:
        evidence = os.environ.get("TERA_C138_TEST_EVIDENCE")
        if evidence:
            self.root = Path(tempfile.mkdtemp(prefix="app-", dir=evidence))
        else:
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            self.root = Path(temporary.name).resolve()
        self.git("init", "--quiet")
        self.write("Package.swift", "// fixture package\n")
        self.write("project.yml", "name: Tera\n")
        self.write(
            "Tera/Runtime/TeraGeneratedBody.swift", "private func body() -> Int { 1 }\n"
        )
        self.write("Tera/Resources/message.txt", "resource one\n")
        self.write("Tera/Config/Debug.xcconfig", "VALUE = one\n")
        self.git("add", ".")

    def write(self, relative: str, text: str | bytes) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(text.encode() if isinstance(text, str) else text)

    def git(self, *arguments: str) -> str:
        return subprocess.check_output(
            ["git", *arguments], cwd=self.root, stderr=subprocess.DEVNULL, text=True
        ).strip()

    def snapshot(self) -> dict:
        return app.capture(self.root)

    def test_unchanged_staged_source_reproduces_projected_git_tree(self) -> None:
        first = self.snapshot()
        self.assertEqual(first, self.snapshot())
        self.assertEqual(first["tree"], self.git("write-tree"))
        self.assertEqual(json.loads(app.encoded(first)), first)

    def test_authored_runtime_generated_prefix_is_included(self) -> None:
        self.assertIn("Tera/Runtime/TeraGeneratedBody.swift", self.snapshot()["files"])

    def test_intent_to_add_empty_and_nonempty_inputs_reject(self) -> None:
        for body in (b"", b"private func intent() {}\n"):
            with self.subTest(empty=not body):
                self.write("Tera/intent.swift", body)
                self.git("add", "--intent-to-add", "Tera/intent.swift")
                with self.assertRaises(source.ProvenanceError):
                    self.snapshot()
                self.git("update-index", "--force-remove", "Tera/intent.swift")
                (self.root / "Tera/intent.swift").unlink()

    def test_coherent_staging_of_empty_and_nonempty_inputs_matches_git_tree(
        self,
    ) -> None:
        for body in (b"", b"private func staged() {}\n"):
            with self.subTest(empty=not body):
                self.write("Tera/staged.swift", body)
                self.git("add", "--intent-to-add", "Tera/staged.swift")
                self.git("add", "Tera/staged.swift")
                self.assertEqual(self.snapshot()["tree"], self.git("write-tree"))

    def test_private_body_resource_and_configuration_edits_change_identity(
        self,
    ) -> None:
        for relative in (
            "Tera/Runtime/TeraGeneratedBody.swift",
            "Tera/Resources/message.txt",
            "Tera/Config/Debug.xcconfig",
            "Package.swift",
            "project.yml",
        ):
            with self.subTest(relative=relative):
                before = self.snapshot()
                self.write(
                    relative, (self.root / relative).read_bytes() + b"// changed\n"
                )
                self.git("add", relative)
                self.assertNotEqual(before["tree"], self.snapshot()["tree"])

    def test_staged_addition_and_removal_change_identity(self) -> None:
        before = self.snapshot()
        self.write("Tera/Runtime/Added.swift", "private func added() {}\n")
        self.git("add", "Tera")
        added = self.snapshot()
        self.assertNotEqual(before["tree"], added["tree"])
        self.git("rm", "-f", "Tera/Runtime/Added.swift")
        self.assertEqual(before, self.snapshot())

    def test_generated_outputs_are_excluded_without_evidence_cycle(self) -> None:
        before = self.snapshot()
        for relative in (
            "Tera/Generated/Bindings.swift",
            "Tera/Frameworks/Fixture.a",
            "TeraFFI/provenance.json",
            "TeraFFI/source/native.json",
            "api/TeraApp.symbols.json",
            "Tera.xcodeproj/project.pbxproj",
            "release/provenance.json",
        ):
            self.write(relative, "generated fixture\n")
        self.git("add", ".")
        self.assertEqual(before, self.snapshot())
        self.write("Tera/Generated/Ignored.swift", "untracked excluded binding\n")
        self.assertEqual(before, self.snapshot())

    def test_unstaged_untracked_ignored_and_missing_inputs_reject(self) -> None:
        body = self.root / "Tera/Runtime/TeraGeneratedBody.swift"
        original = body.read_bytes()
        body.write_bytes(original + b"// unstaged\n")
        with self.assertRaisesRegex(source.ProvenanceError, "differs from staged"):
            self.snapshot()
        body.write_bytes(original)
        extra = self.root / "Tera/extra.swift"
        extra.write_text("untracked\n")
        with self.assertRaisesRegex(source.ProvenanceError, "untracked or ignored"):
            self.snapshot()
        extra.unlink()
        self.write(".gitignore", "Tera/ignored.swift\n")
        self.write("Tera/ignored.swift", "ignored\n")
        with self.assertRaisesRegex(source.ProvenanceError, "untracked or ignored"):
            self.snapshot()
        (self.root / "Tera/ignored.swift").unlink()
        self.git("rm", "-f", "Package.swift")
        with self.assertRaisesRegex(source.ProvenanceError, "missing from index"):
            self.snapshot()

    def test_unreadable_input_rejects(self) -> None:
        path = self.root / "Tera/Resources/message.txt"
        path.chmod(0)
        self.addCleanup(path.chmod, 0o644)
        with self.assertRaisesRegex(source.ProvenanceError, "read"):
            self.snapshot()

    def test_symlink_and_directory_replacement_reject_before_foreign_read(self) -> None:
        body = self.root / "Tera/Runtime/TeraGeneratedBody.swift"
        original = body.read_bytes()
        foreign = self.root / "foreign"
        foreign.mkdir()
        (foreign / body.name).write_bytes(b"FOREIGN_BODY_SENTINEL")
        body.unlink()
        body.symlink_to(foreign / body.name)
        with self.assertRaisesRegex(source.ProvenanceError, "safely read"):
            self.snapshot()
        body.unlink()
        body.write_bytes(original)
        opening = os.open
        observed = []
        reading = os.read
        replaced = False

        def replace_directory(name, flags, *args, **kwargs):
            nonlocal replaced
            descriptor = opening(name, flags, *args, **kwargs)
            if name == "Runtime" and not replaced:
                replaced = True
                (self.root / "Tera/Runtime").rename(self.root / "preserved-runtime")
                (self.root / "Tera/Runtime").symlink_to(
                    foreign, target_is_directory=True
                )
            return descriptor

        def observe_read(descriptor, size):
            data = reading(descriptor, size)
            observed.append(data)
            return data

        with (
            patch.object(os, "open", side_effect=replace_directory),
            patch.object(os, "read", side_effect=observe_read),
        ):
            with self.assertRaises(source.ProvenanceError):
                self.snapshot()
        self.assertTrue(replaced)
        self.assertNotIn(b"FOREIGN_BODY_SENTINEL", b"".join(observed))

    def test_scoped_index_and_member_drift_reject(self) -> None:
        reading = app.read_authored_source
        for mutation in ("index", "untracked", "ignored", "worktree"):
            with self.subTest(mutation=mutation):
                changed = False
                relative = "Tera/Resources/message.txt"
                original = (self.root / relative).read_bytes()

                def change_after_read(root, path, deadline):
                    nonlocal changed
                    result = reading(root, path, deadline)
                    if not changed:
                        changed = True
                        if mutation == "index":
                            self.write(relative, "new staged resource\n")
                            self.git("add", relative)
                        elif mutation in ("untracked", "ignored"):
                            self.write(
                                ".gitignore",
                                "Tera/drift.swift\n" if mutation == "ignored" else "",
                            )
                            self.write("Tera/drift.swift", "new authored member\n")
                        else:
                            self.write("Package.swift", "// unstaged drift\n")
                    return result

                with patch.object(
                    app, "read_authored_source", side_effect=change_after_read
                ):
                    with self.assertRaises(source.ProvenanceError):
                        self.snapshot()
                self.write(relative, original)
                self.write("Package.swift", "// fixture package\n")
                (self.root / "Tera/drift.swift").unlink(missing_ok=True)
                self.git("add", "Package.swift", relative)

    def test_mode_conflict_and_unsupported_index_entries_reject(self) -> None:
        relative = "Tera/Runtime/TeraGeneratedBody.swift"
        path = self.root / relative
        path.chmod(0o755)
        with self.assertRaisesRegex(source.ProvenanceError, "differs from staged"):
            self.snapshot()
        path.chmod(0o644)
        blob = self.git("rev-parse", ":" + relative)
        self.git("update-index", "--force-remove", relative)
        row = f"100644 {blob} 1\t{relative}\n".encode()
        subprocess.run(
            ["git", "update-index", "--index-info"],
            input=row,
            cwd=self.root,
            check=True,
            capture_output=True,
        )
        with self.assertRaisesRegex(source.ProvenanceError, "unresolved"):
            self.snapshot()
        self.git("add", relative)
        path.unlink()
        path.symlink_to("../Resources/message.txt")
        self.git("add", relative)
        with self.assertRaisesRegex(source.ProvenanceError, "unsupported mode"):
            self.snapshot()

    def test_git_selection_overrides_cannot_substitute_app_index(self) -> None:
        for name in (*app.GIT_OVERRIDES, "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0"):
            with self.subTest(name=name), patch.dict(os.environ, {name: ""}):
                with self.assertRaisesRegex(
                    source.ProvenanceError, "selection override"
                ):
                    self.snapshot()

    def test_entry_and_file_bounds_accept_exact_and_reject_one_over(self) -> None:
        self.assertEqual(app.MAX_INPUTS, 2048)
        self.assertEqual(app.MAX_BYTES, 2 * 1024 * 1024)
        self.write("Tera/Resources/message.txt", b"x" * app.MAX_BYTES)
        self.git("add", "Tera/Resources/message.txt")
        self.assertEqual(
            self.snapshot()["files"]["Tera/Resources/message.txt"]["bytes"],
            app.MAX_BYTES,
        )
        self.write("Tera/Resources/message.txt", b"x" * (app.MAX_BYTES + 1))
        with self.assertRaisesRegex(source.ProvenanceError, "byte bound"):
            self.snapshot()
        self.write("Tera/Resources/message.txt", "bounded\n")
        for index in range(app.MAX_INPUTS - 5):
            self.write(f"Tera/Resources/member-{index:04}.txt", "x")
        self.git("add", "Tera")
        deadline = time.monotonic() + app.MAX_SECONDS
        self.assertEqual(
            app.scoped_inventory(self.root, deadline).count(b"\0"), app.MAX_INPUTS
        )
        self.write("Tera/Resources/one-over.txt", "x")
        self.git("add", "Tera")
        with self.assertRaisesRegex(source.ProvenanceError, "inventory exceeds bounds"):
            self.snapshot()

    def test_deadline_and_encoded_record_bounds_reject(self) -> None:
        self.assertEqual(app.MAX_SECONDS, 120)
        with self.assertRaisesRegex(source.ProvenanceError, "deadline"):
            app.scoped_inventory(self.root, time.monotonic() - 1)
        value = {"value": "x" * (app.MAX_BYTES - len(app.encoded({"value": ""})))}
        self.assertEqual(len(app.encoded(value)), app.MAX_BYTES)
        value["value"] += "x"
        with self.assertRaisesRegex(source.ProvenanceError, "record exceeds"):
            app.encoded(value)
        command = app.git
        timeouts = []

        def slow_command(root, argv, deadline):
            timeouts.append(app.remaining(deadline))
            result = command(root, argv, deadline)
            time.sleep(0.02)
            return result

        with (
            patch.object(app, "MAX_SECONDS", 0.03),
            patch.object(app, "git", side_effect=slow_command),
        ):
            with self.assertRaisesRegex(source.ProvenanceError, "deadline"):
                self.snapshot()
        self.assertGreaterEqual(len(timeouts), 1)
        self.assertTrue(all(value <= 0.03 for value in timeouts))

    def test_git_output_is_bounded_and_owned_process_is_reaped(self) -> None:
        tool = self.root / "git"
        pid_file = self.root / "git-child.pid"
        for size, admitted in ((app.MAX_BYTES, True), (app.MAX_BYTES + 1, False)):
            tool.write_text(
                f"#!{sys.executable}\nimport os, sys\nfrom pathlib import Path\n"
                f"Path({str(pid_file)!r}).write_text(str(os.getpid()))\n"
                f"sys.stdout.buffer.write(b'x' * {size})\n"
            )
            tool.chmod(0o700)
            with patch.dict(os.environ, {"PATH": str(self.root)}):
                if admitted:
                    self.assertEqual(
                        len(
                            app.git(
                                self.root,
                                ["--version"],
                                time.monotonic() + app.MAX_SECONDS,
                            )
                        ),
                        size,
                    )
                else:
                    with self.assertRaisesRegex(
                        source.ProvenanceError, "output exceeds byte bound"
                    ):
                        app.git(
                            self.root, ["--version"], time.monotonic() + app.MAX_SECONDS
                        )
            with self.assertRaises(ProcessLookupError):
                os.kill(int(pid_file.read_text()), 0)


if __name__ == "__main__":
    unittest.main()
