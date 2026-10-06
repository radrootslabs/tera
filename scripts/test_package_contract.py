from __future__ import annotations

import copy
import hashlib
import json
import plistlib
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

    def test_application_workspace_accepts_owned_rust(self) -> None:
        root = SCRIPTS.parent.resolve()
        for member in ("core/crates/tera_core", "core/crates/tera_ffi"):
            contract._validate_app_workspace(self.workspace(root, member), root)

    def test_application_workspace_rejects_hidden_sibling_dependency(self) -> None:
        root = SCRIPTS.parent.resolve()
        document = self.workspace(root)
        document["packages"][0]["dependencies"] = [
            {"path": str(root.parent / "lib/crates/storage")}
        ]
        with self.assertRaisesRegex(contract.PackageContractError, "escapes"):
            contract._validate_app_workspace(document, root)

    def test_mobile_defaults_exclude_wasm_and_execute_owned_runtime(self) -> None:
        root = SCRIPTS.parent.resolve()
        document = self.workspace(root)
        names = ("tera_core", "tera_ffi", "tera_wasm")
        document["packages"] = [
            {
                "id": name,
                "manifest_path": str(root / "core/crates" / name / "Cargo.toml"),
                "dependencies": [],
            }
            for name in names
        ]
        document["workspace_members"] = list(names)
        document["workspace_default_members"] = ["tera_core", "tera_ffi"]
        contract._validate_app_workspace(document, root)
        for defaults in (None, list(names), [], ["tera_core"], ["tera_ffi"]):
            with self.subTest(defaults=defaults):
                document["workspace_default_members"] = defaults
                with self.assertRaisesRegex(
                    contract.PackageContractError, "mobile defaults"
                ):
                    contract._validate_app_workspace(document, root)

    def test_application_workspace_rejects_foreign_member_root(self) -> None:
        root = SCRIPTS.parent.resolve()
        with self.assertRaisesRegex(contract.PackageContractError, "owned root"):
            contract._validate_app_workspace(
                self.workspace(root, "crates/source_lock"), root
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

    def test_current_package_contract_is_structurally_exact(self) -> None:
        version, revision = contract.verify(SCRIPTS.parent)
        self.assertEqual(version, "0.1.0-alpha")
        self.assertRegex(revision, r"^[0-9a-f]{40}$")

    def test_compatibility_baseline_rejects_bundle_or_custody_rename(self) -> None:
        root = SCRIPTS.parent
        base = contract.parse_xcconfig_assignments(
            (root / "Tera/Config/Base.xcconfig").read_text()
        )
        debug = contract.parse_xcconfig_assignments(
            (root / "Tera/Config/Debug.xcconfig").read_text()
        )
        for key in (
            "PRODUCT_BUNDLE_IDENTIFIER",
            "TERA_IOS_KEYCHAIN_SERVICE_PREFIX",
        ):
            with self.subTest(key=key):
                changed = {**debug, key: "org.tera.accidental-rename"}
                with self.assertRaisesRegex(
                    contract.PackageContractError, "identity compatibility"
                ):
                    contract._verify_installation_compatibility(root, base, changed)

    def test_comment_token_does_not_define_xcconfig_field(self) -> None:
        values = contract.parse_xcconfig_assignments(
            "// TERA_IOS_RUNTIME_MODE = production\n"
        )
        self.assertNotIn("TERA_IOS_RUNTIME_MODE", values)

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
                "packages:\n  TeraApp:\n    path: .\n"
                "targets:\n  RadrootsKit:\n    url: "
                + contract.APPLE_KIT_REMOTE
                + "\n",
                "RadrootsKit",
            )


class ProducerEpochTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "TeraFFI").mkdir()
        self.foundation = {
            "repository": contract.LIB_REMOTE,
            "revision": "a" * 40,
            "version": "0.1.0",
        }
        self.manifest = b'{"synthetic": "installed manifest"}\n'
        (self.root / "TeraFFI/provenance.json").write_bytes(self.manifest)

    def fixture(self, canonical: object, installed: object) -> None:
        field = (
            ""
            if canonical is None
            else f"source_date_epoch = {json.dumps(canonical)}\n"
        )
        (self.root / "TeraFFI/producer.toml").write_text("[build]\n" + field)
        values = {
            "schema": "tera.installed-source.v1",
            "repository": contract._read_toml(SCRIPTS.parent / "TeraFFI/producer.toml")[
                "repository"
            ],
            "source_tree": "b" * 40,
            "manifest_sha256": hashlib.sha256(self.manifest).hexdigest(),
        }
        if installed is not None:
            values["source_date_epoch"] = installed
        text = "".join(
            f"{key} = {json.dumps(value)}\n" for key, value in values.items()
        )
        text += "\n[foundation]\n" + "".join(
            f"{key} = {json.dumps(value)}\n" for key, value in self.foundation.items()
        )
        (self.root / "TeraFFI/source.lock").write_text(text)

    def test_installed_epoch_matches_coherent_canonical_producer_update(self) -> None:
        for epoch in (1787871027, 12345, 1):
            with self.subTest(epoch=epoch):
                self.fixture(epoch, epoch)
                contract._verify_owned_source_lock(self.root, self.foundation)

    def test_canonical_epoch_missing_and_invalid_values_reject(self) -> None:
        for epoch in (None, "1787871027", 0, -1, True):
            with self.subTest(epoch=epoch):
                self.fixture(epoch, 1787871027)
                with self.assertRaises(contract.PackageContractError):
                    contract._verify_owned_source_lock(self.root, self.foundation)

    def test_installed_epoch_mismatch_and_invalid_values_reject(self) -> None:
        for canonical, epoch in (
            (1787871027, 12345),
            (1787871027, None),
            (1787871027, "1787871027"),
            (1787871027, 0),
            (1787871027, -1),
            (1, True),
        ):
            with self.subTest(canonical=canonical, epoch=epoch):
                self.fixture(canonical, epoch)
                with self.assertRaises(contract.PackageContractError):
                    contract._verify_owned_source_lock(self.root, self.foundation)


if __name__ == "__main__":
    unittest.main()
