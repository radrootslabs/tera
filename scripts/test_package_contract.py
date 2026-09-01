from __future__ import annotations

import copy
import json
import plistlib
import sys
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import package_contract as contract  # noqa: E402


class PackageContractTests(unittest.TestCase):
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
