from __future__ import annotations

import copy
import hashlib
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

import ffi_notices as notices  # noqa: E402
import ffi_source as source  # noqa: E402
import package_contract as contract  # noqa: E402


class RedistributionTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.package = self.root / "provider"
        self.package.mkdir()
        (self.package / "Cargo.toml").write_text("# synthetic metadata fixture\n")
        self.data = b"synthetic complete redistribution material\n"
        self.path = self.package / "NOTICE"
        self.path.write_bytes(self.data)
        self.foundation = {"repository": contract.LIB_REMOTE, "revision": "b" * 40}
        self.metadata = {
            "packages": [
                {
                    "name": "radroots_domain",
                    "version": "0.1.0-alpha",
                    "source": f"git+{contract.LIB_REMOTE}?rev={'b' * 40}#{'b' * 40}",
                    "manifest_path": str(self.package / "Cargo.toml"),
                }
            ]
        }

    def capture(self) -> dict:
        with (
            patch.object(contract, "_cargo_workspace", return_value=self.metadata),
            patch.dict(
                notices.CONTRACT, {"sha256": hashlib.sha256(self.data).hexdigest()}
            ),
        ):
            return notices.capture(self.root, self.foundation)

    def test_exact_selected_material_is_copied_without_transformation(self) -> None:
        binding = self.capture()
        with (
            patch.object(contract, "_cargo_workspace", return_value=self.metadata),
            patch.dict(notices.CONTRACT, {"sha256": binding["sha256"]}),
        ):
            destination = self.root / "candidate"
            notices.install(self.root, destination, binding, self.foundation)
        self.assertEqual((destination / notices.ARTIFACT_PATH).read_bytes(), self.data)
        self.assertEqual(notices.verify_material(self.path, binding), self.data)

    def test_provider_must_be_unique_and_exact_git_source(self) -> None:
        original = copy.deepcopy(self.metadata["packages"][0])
        for changes in (
            {"source": None},
            {"source": "registry+synthetic"},
            {"version": "0.2.0"},
        ):
            self.metadata["packages"] = [{**original, **changes}]
            with self.assertRaises(source.ProvenanceError):
                self.capture()
        for packages in ([], [original, original]):
            self.metadata["packages"] = packages
            with self.assertRaisesRegex(source.ProvenanceError, "ambiguous"):
                self.capture()

    def test_empty_oversized_non_utf8_and_symlink_material_is_rejected(self) -> None:
        for data in (b"", b"x" * (notices.MAX_BYTES + 1), b"\xff"):
            self.path.write_bytes(data)
            with self.assertRaises(source.ProvenanceError):
                notices.read_material(self.path)
        self.path.unlink()
        self.path.symlink_to(self.package / "Cargo.toml")
        with self.assertRaisesRegex(source.ProvenanceError, "unavailable"):
            notices.read_material(self.path)

    def test_truncation_and_same_length_substitution_are_rejected(self) -> None:
        binding = self.capture()
        for data in (self.data[:-1], b"x" * len(self.data)):
            self.path.write_bytes(data)
            with self.assertRaisesRegex(source.ProvenanceError, "differs"):
                notices.verify_material(self.path, binding)

    def test_non_regular_material_is_rejected_without_waiting_for_a_writer(
        self,
    ) -> None:
        self.path.unlink()
        os.mkfifo(self.path)
        result = subprocess.run(
            [
                sys.executable,
                "-c",
                "from pathlib import Path; import ffi_notices; "
                "ffi_notices.read_material(Path(__import__('sys').argv[1]))",
                str(self.path),
            ],
            cwd=SCRIPTS,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=5,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"redistribution material is not bounded", result.stderr)

    def test_binding_and_inventory_cannot_change_notice_identity(self) -> None:
        binding = self.capture()
        row = {
            "path": notices.ARTIFACT_PATH,
            "bytes": binding["bytes"],
            "sha256": binding["sha256"],
        }
        notices.verify_inventory([row], binding, notices.ARTIFACT_PATH)
        for files in ([], [row, row], [{**row, "sha256": "f" * 64}]):
            with self.assertRaises(contract.PackageContractError):
                notices.verify_inventory(files, binding, notices.ARTIFACT_PATH)
        with self.assertRaises(contract.PackageContractError):
            notices.verify_expected(
                binding,
                {**self.foundation, "revision": "c" * 40},
                {"sha256": binding["sha256"]},
            )
        with self.assertRaises(contract.PackageContractError):
            notices.validate_contract(
                {**notices.CONTRACT, "maximum_bytes": notices.MAX_BYTES + 1}
            )


if __name__ == "__main__":
    unittest.main()
