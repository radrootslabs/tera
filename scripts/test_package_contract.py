from __future__ import annotations

import copy
import hashlib
import json
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import package_contract as contract  # noqa: E402


class PackageContractTests(unittest.TestCase):
    def workspace(self, root: Path, member: str = "core/crates/tera_core") -> dict:
        return {
            "workspace_root": str(root),
            "workspace_members": ["app"],
            "packages": [
                {
                    "id": "app",
                    "manifest_path": str(root / member / "Cargo.toml"),
                    "dependencies": [],
                }
            ],
        }

    def test_application_workspace_accepts_owned_rust_and_transition_shim(self) -> None:
        root = SCRIPTS.parent.resolve()
        for member in ("core/crates/tera_core", "crates/source_lock"):
            contract._validate_app_workspace(self.workspace(root, member), root)

    def test_application_workspace_rejects_hidden_sibling_dependency(self) -> None:
        root = SCRIPTS.parent.resolve()
        document = self.workspace(root)
        document["packages"][0]["dependencies"] = [
            {"path": str(root.parent / "lib/crates/storage")}
        ]
        with self.assertRaisesRegex(contract.PackageContractError, "escapes"):
            contract._validate_app_workspace(document, root)

    def test_application_workspace_rejects_foreign_member_root(self) -> None:
        root = SCRIPTS.parent.resolve()
        with self.assertRaisesRegex(contract.PackageContractError, "owned root"):
            contract._validate_app_workspace(
                self.workspace(root, "private/runtime"), root
            )

    def test_application_workspace_rejects_implicit_parent_workspace(self) -> None:
        root = SCRIPTS.parent.resolve()
        document = self.workspace(root)
        document["workspace_root"] = str(root.parent)
        with self.assertRaisesRegex(contract.PackageContractError, "workspace root"):
            contract._validate_app_workspace(document, root)

    def test_application_workspace_rejects_symlink_escape(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "core").symlink_to(root.parent, target_is_directory=True)
            with self.assertRaisesRegex(contract.PackageContractError, "escapes"):
                contract._validate_app_workspace(self.workspace(root), root)

    def test_forbidden_roots_still_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("docs", ".github", ".act"):
                with self.subTest(name=name):
                    forbidden = root / name
                    forbidden.mkdir()
                    with self.assertRaisesRegex(
                        contract.PackageContractError, "forbidden"
                    ):
                        contract._verify_repository_layout(root)
                    forbidden.rmdir()

    def test_installed_artifact_guard_rejects_tampered_binary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root / "RadrootsFFI/scripts/verify-installed-artifacts.sh"
            script.parent.mkdir(parents=True)
            shutil.copyfile(
                SCRIPTS.parent / "RadrootsFFI/scripts" / script.name, script
            )
            library = (
                root
                / "Radroots/Frameworks/RadrootsFFI.xcframework/ios-arm64/libradroots_mobile_ffi.a"
            )
            library.parent.mkdir(parents=True)
            fixture = b"synthetic artifact verifier fixture, not executable code"
            library.write_bytes(fixture)
            (root / "RadrootsFFI/source.lock").write_text(
                "override RADROOTS_FIELD_FFI_DEVICE_SHA256 := "
                + hashlib.sha256(fixture).hexdigest()
                + "\n"
            )
            for data, expected in (
                (fixture, "missing simulator FFI library"),
                (fixture + b"tampered", "stale device FFI library"),
            ):
                library.write_bytes(data)
                result = subprocess.run(
                    ["sh", str(script)],
                    check=False,
                    capture_output=True,
                    text=True,
                    timeout=10,
                )
                self.assertEqual(result.returncode, 1)
                self.assertIn(expected, result.stderr)

    def test_current_package_contract_is_structurally_exact(self) -> None:
        version, revision = contract.verify(SCRIPTS.parent)
        self.assertEqual(version, "0.1.0-alpha")
        self.assertRegex(revision, r"^[0-9a-f]{40}$")

    def test_comment_token_does_not_define_xcconfig_field(self) -> None:
        values = contract.parse_xcconfig_assignments(
            "// RADROOTS_FIELD_IOS_RUNTIME_MODE = production\n"
        )
        self.assertNotIn("RADROOTS_FIELD_IOS_RUNTIME_MODE", values)

    def test_dead_metadata_text_does_not_define_cargo_repository(self) -> None:
        document = {
            "workspace": {
                "package": {},
                "metadata": {
                    "example": 'repository = "https://github.com/radrootslabs/tera"'
                },
            }
        }
        package = contract._mapping(document["workspace"]["package"], "package")
        with self.assertRaisesRegex(contract.PackageContractError, "differs"):
            contract._exact(
                package.get("repository"),
                "https://github.com/radrootslabs/tera",
                "Cargo repository",
            )

    def test_plist_value_token_does_not_define_required_key(self) -> None:
        document = plistlib.loads(
            plistlib.dumps(
                {
                    "Comment": (
                        "NSCameraUsageDescription NSFaceIDUsageDescription "
                        "NSLocalNetworkUsageDescription"
                    ),
                    "NSAppTransportSecurity": {"NSAllowsLocalNetworking": True},
                }
            )
        )
        with self.assertRaisesRegex(contract.PackageContractError, "purpose"):
            contract._validate_app_plist(document)

    def test_duplicate_source_lock_assignment_is_rejected(self) -> None:
        with self.assertRaisesRegex(contract.PackageContractError, "duplicated"):
            contract.parse_make_assignments(
                "override RADROOTS_FIELD_LIB_GIT_REV := " + "a" * 40 + "\n"
                "override RADROOTS_FIELD_LIB_GIT_REV := " + "b" * 40 + "\n"
            )

    def test_source_lock_dead_assignment_is_rejected(self) -> None:
        with self.assertRaisesRegex(contract.PackageContractError, "unsupported"):
            contract.parse_make_assignments(
                "ifneq ($(UNREACHABLE),)\n"
                "override RADROOTS_FIELD_LIB_GIT_REV := " + "a" * 40 + "\nendif\n"
            )

    def test_duplicate_xcconfig_assignment_is_rejected(self) -> None:
        with self.assertRaisesRegex(contract.PackageContractError, "duplicated"):
            contract.parse_xcconfig_assignments("FIELD = one\nFIELD = two\n")

    def test_package_lock_rejects_pin_drift(self) -> None:
        document = json.loads(
            (SCRIPTS.parent / "Package.resolved").read_text(encoding="utf-8")
        )
        apple_revision = next(
            pin["state"]["revision"]
            for pin in document["pins"]
            if pin["location"] == contract.APPLE_KIT_REMOTE
        )
        drifted = copy.deepcopy(document)
        drifted["pins"][0]["state"]["revision"] = "f" * 40
        with self.assertRaises(contract.PackageContractError):
            contract.validate_resolved(drifted, apple_revision)

    def test_project_package_comment_does_not_define_entry(self) -> None:
        with self.assertRaisesRegex(contract.PackageContractError, "absent"):
            contract.parse_project_package(
                "packages:\n#   RadrootsKit:\n#     url: "
                + contract.APPLE_KIT_REMOTE
                + "\n",
                "RadrootsKit",
            )

    def test_project_package_dead_section_does_not_define_entry(self) -> None:
        with self.assertRaisesRegex(contract.PackageContractError, "absent"):
            contract.parse_project_package(
                "packages:\n  RadrootsApp:\n    path: .\n"
                "targets:\n  RadrootsKit:\n    url: "
                + contract.APPLE_KIT_REMOTE
                + "\n",
                "RadrootsKit",
            )


if __name__ == "__main__":
    unittest.main()
