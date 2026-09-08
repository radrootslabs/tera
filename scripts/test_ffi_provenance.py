from __future__ import annotations

import copy
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

import ffi_provenance as provenance  # noqa: E402
import ffi_source as source  # noqa: E402
import package_contract as contract  # noqa: E402


class ProducerSourceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.git("init", "--quiet")
        (self.root / "core").mkdir()
        (self.root / "core/lib.rs").write_text("pub fn value() -> u8 { 1 }\n")
        (self.root / "Cargo.lock").write_text("# synthetic source-capture fixture\n")
        self.git("add", "core", "Cargo.lock")
        self.inputs = ["core", "Cargo.lock"]

    def git(self, *args: str) -> str:
        return subprocess.check_output(
            ["git", *args], cwd=self.root, text=True, stderr=subprocess.DEVNULL
        ).strip()

    def snapshot(self) -> dict:
        return source.source_snapshot(self.root, self.inputs)

    def test_tree_is_real_and_deterministic_without_an_introducing_commit(self) -> None:
        first = self.snapshot()
        self.assertEqual(first, self.snapshot())
        self.assertEqual(self.git("write-tree"), first["tree"])
        self.assertEqual(self.git("cat-file", "-t", first["tree"]), "tree")
        # Generated evidence never changes its own source tree identity.
        (self.root / "provenance.json").write_bytes(provenance.encoded(first))
        self.git("add", "provenance.json")
        self.assertEqual(first, self.snapshot())

    def test_worktree_change_requires_an_explicit_staged_transaction(self) -> None:
        first = self.snapshot()
        (self.root / "core/lib.rs").write_text("pub fn value() -> u8 { 2 }\n")
        with self.assertRaisesRegex(source.ProvenanceError, "differs from staged"):
            self.snapshot()
        self.git("add", "core/lib.rs")
        self.assertNotEqual(first["tree"], self.snapshot()["tree"])

    def test_untracked_and_missing_source_fail_closed(self) -> None:
        (self.root / "core/extra.rs").write_text("// untracked input\n")
        with self.assertRaisesRegex(source.ProvenanceError, "untracked"):
            self.snapshot()
        (self.root / "core/extra.rs").unlink()
        (self.root / "Cargo.lock").unlink()
        self.git("add", "-u", "Cargo.lock")
        with self.assertRaisesRegex(source.ProvenanceError, "missing from index"):
            self.snapshot()

    def test_symlink_and_file_mode_changes_are_rejected(self) -> None:
        path = self.root / "core/lib.rs"
        path.chmod(0o755)
        with self.assertRaisesRegex(source.ProvenanceError, "differs from staged"):
            self.snapshot()
        path.unlink()
        path.symlink_to("../Cargo.lock")
        with self.assertRaisesRegex(source.ProvenanceError, "symlink"):
            self.snapshot()
        self.git("add", "core/lib.rs")
        with self.assertRaisesRegex(source.ProvenanceError, "unsupported mode"):
            self.snapshot()

    def test_source_byte_bound_prevents_unbounded_capture(self) -> None:
        (self.root / "core/lib.rs").write_bytes(b"x" * (source.MAX_BYTES + 1))
        with self.assertRaisesRegex(contract.PackageContractError, "byte limit"):
            self.snapshot()

    def test_gitignore_cannot_hide_a_producer_input(self) -> None:
        (self.root / ".gitignore").write_text("core/hidden.rs\n")
        (self.root / "core/hidden.rs").write_text("// ignored source input\n")
        with self.assertRaisesRegex(source.ProvenanceError, "ignored source"):
            self.snapshot()

    def test_provenance_rejects_every_source_or_build_tuple_mutation(self) -> None:
        expected = {
            "source": self.snapshot(),
            "cargo_lock_sha256": "a" * 64,
            "foundation": {"revision": "b" * 40},
            "build": {
                "target": "aarch64-apple-ios",
                "rustc": "rustc 1.97.1",
                "features": [],
                "feature_graph": ["tera_core|mobile-social"],
            },
        }
        provenance.verify_record(provenance.encoded(expected), expected)
        for section, key, value in (
            ("source", "tree", "c" * 40),
            ("foundation", "revision", "d" * 40),
            ("build", "target", "aarch64-apple-ios-sim"),
            ("build", "rustc", "rustc 1.96.0"),
            ("build", "features", ["extra"]),
            ("build", "feature_graph", []),
        ):
            with self.subTest(section=section, key=key):
                changed = copy.deepcopy(expected)
                changed[section][key] = value
                with self.assertRaisesRegex(
                    source.ProvenanceError, "exact source/build tuple"
                ):
                    provenance.verify_record(provenance.encoded(changed), expected)
        changed = {**expected, "cargo_lock_sha256": "f" * 64}
        with self.assertRaises(source.ProvenanceError):
            provenance.verify_record(provenance.encoded(changed), expected)

    def test_output_rejects_repository_paths_escapes_and_symlinks(self) -> None:
        external = self.root / "external"
        external.mkdir()
        repository = self.root / "repository"
        repository.mkdir()
        with patch.dict(
            os.environ,
            {
                "EXT_BUILD_RUN_ACTIVE": "1",
                "EXT_BUILD_PROJECT_DIR": str(external),
            },
        ):
            for output in (repository / "evidence.json", external / "../escape.json"):
                with self.assertRaises(source.ProvenanceError):
                    provenance.output_path(repository, "aarch64-apple-ios", str(output))
            (external / "link").symlink_to(repository, target_is_directory=True)
            with self.assertRaises(source.ProvenanceError):
                provenance.output_path(
                    repository, "aarch64-apple-ios", str(external / "link/value.json")
                )

    def test_ungoverned_build_flags_are_rejected(self) -> None:
        for key in (
            "RUSTFLAGS",
            "RUSTC_WRAPPER",
            "CARGO_BUILD_RUSTFLAGS",
            "CARGO_TARGET_AARCH64_APPLE_IOS_LINKER",
        ):
            with self.subTest(key=key), patch.dict(os.environ, {key: "synthetic"}):
                with self.assertRaisesRegex(source.ProvenanceError, "ungoverned"):
                    source.reject_build_overrides()

    def test_local_cargo_build_configuration_cannot_escape_source_identity(
        self,
    ) -> None:
        (self.root / ".cargo").mkdir()
        config = self.root / ".cargo/config.toml"
        config.write_text('[build]\nrustflags = ["--cfg", "unrecorded"]\n')
        with self.assertRaisesRegex(source.ProvenanceError, "Cargo configuration"):
            source.reject_cargo_configuration(self.root)

    def test_current_contract_separates_foundation_and_application(self) -> None:
        config = source.producer_contract(SCRIPTS.parent)
        self.assertEqual(config["ffi"]["package"], "tera_ffi")
        self.assertEqual(config["repository"], "https://github.com/radrootslabs/tera")
        cargo = contract._read_toml(SCRIPTS.parent / "Cargo.toml")
        lock = contract._read_toml(SCRIPTS.parent / config["foundation_lock"])
        changed = copy.deepcopy(cargo)
        changed["workspace"]["dependencies"]["radroots_sdk"]["rev"] = "f" * 40
        with self.assertRaisesRegex(
            contract.PackageContractError, "foundation Cargo revision"
        ):
            source.validate_foundation(changed, lock)


if __name__ == "__main__":
    unittest.main()
