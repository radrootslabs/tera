from __future__ import annotations

import copy
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import ffi_artifacts as artifacts  # noqa: E402
import ffi_installed as installed  # noqa: E402
import ffi_provenance as provenance  # noqa: E402
import ffi_source as source  # noqa: E402
import test_ffi_artifacts as fixtures  # noqa: E402


class InstalledArtifactTests(unittest.TestCase):
    def setUp(self) -> None:
        fixture = fixtures.NativeArtifactTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.manifest = installed.installed_manifest(
            artifacts.check(fixture.root, fixture.records)
        )
        for relative, destination in installed.installed_paths().items():
            path = self.root / destination
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(fixture.root / relative, path)
        (self.root / installed.MANIFEST).write_bytes(provenance.encoded(self.manifest))

    def test_installed_inventory_retains_exact_candidate_file_identities(self) -> None:
        installed.verify_files(self.root, self.manifest)
        self.assertEqual(len(self.manifest["files"]), 13)
        self.assertEqual(
            self.manifest["disposition"], "local_installed_not_release_qualified"
        )

    def test_missing_or_tampered_installed_library_is_rejected(self) -> None:
        library = (
            self.root / "Tera/Frameworks/TeraFFI.xcframework/ios-arm64/libtera_ffi.a"
        )
        library.write_bytes(b"tampered synthetic verifier fixture")
        with self.assertRaisesRegex(source.ProvenanceError, "bytes are stale"):
            installed.verify_files(self.root, self.manifest)
        library.unlink()
        with self.assertRaisesRegex(source.ProvenanceError, "missing"):
            installed.verify_files(self.root, self.manifest)

    def test_extra_generated_source_cannot_survive_an_install(self) -> None:
        (self.root / "Tera/Generated/LegacyBindings.swift").write_text("// synthetic\n")
        with self.assertRaisesRegex(source.ProvenanceError, "inventory differs"):
            installed.verify_files(self.root, self.manifest)

    def test_manifest_cannot_substitute_an_installed_file_hash(self) -> None:
        changed = copy.deepcopy(self.manifest)
        changed["files"][0]["sha256"] = "f" * 64
        with self.assertRaisesRegex(source.ProvenanceError, "manifest differs"):
            installed.verify_files(self.root, changed)

    def test_symlinked_installed_source_is_rejected(self) -> None:
        path = self.root / "Tera/Generated/TeraKitBindings.swift"
        path.unlink()
        path.symlink_to(self.root / installed.MANIFEST)
        with self.assertRaisesRegex(source.ProvenanceError, "symlink"):
            installed.verify_files(self.root, self.manifest)

    def test_symlinked_parent_is_rejected_before_any_installation_swap(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            outside = Path(temporary) / "Tera"
            (self.root / "Tera").rename(outside)
            (self.root / "Tera").symlink_to(outside, target_is_directory=True)
            original = (outside / "Generated/TeraKitBindings.swift").read_bytes()
            previous = self.root / "previous-unused"
            with self.assertRaisesRegex(
                source.ProvenanceError, "path contains a symlink"
            ):
                installed.replace_installation(
                    self.root, self.root / "stage-unused", previous
                )
            self.assertFalse(previous.exists())
            self.assertEqual(
                (outside / "Generated/TeraKitBindings.swift").read_bytes(), original
            )

    def test_failed_final_source_check_restores_the_prior_installation(self) -> None:
        staging = self.root / "stage"
        previous = self.root / "previous"
        before = {
            p.relative_to(self.root): p.read_bytes()
            for p in self.root.rglob("*")
            if p.is_file()
        }
        for relative, data in before.items():
            path = staging / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data + b"new synthetic generation")
        (staging / installed.LOCK).write_text("new lock\n")
        with patch.object(
            installed, "check", side_effect=source.ProvenanceError("source changed")
        ):
            with self.assertRaisesRegex(source.ProvenanceError, "source changed"):
                installed.replace_installation(self.root, staging, previous)
        for relative, data in before.items():
            self.assertEqual((self.root / relative).read_bytes(), data)
        self.assertFalse((self.root / installed.LOCK).exists())


if __name__ == "__main__":
    unittest.main()
