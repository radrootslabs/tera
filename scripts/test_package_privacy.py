from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import package_contract as contract  # noqa: E402
import package_privacy as privacy  # noqa: E402


class PrivacyContractTests(unittest.TestCase):
    def test_real_manifest_and_permission_contact_inputs_match_the_product(
        self,
    ) -> None:
        root = SCRIPTS.parent
        contract._validate_privacy(
            contract._read_plist(root / "Tera/Resources/PrivacyInfo.xcprivacy")
        )
        contract._validate_app_plist(contract._read_plist(root / "Tera/Info.plist"))

    def test_tracking_and_linkage_cannot_hide_behind_numeric_boolean_aliases(
        self,
    ) -> None:
        for numeric in [0, 0.0, 1, 1.0]:
            for target in ["tracking", "linked", "entry_tracking"]:
                with self.subTest(numeric=numeric, target=target):
                    value = privacy.manifest()
                    if target == "tracking":
                        value["NSPrivacyTracking"] = numeric
                    elif target == "linked":
                        value["NSPrivacyCollectedDataTypes"][0][
                            "NSPrivacyCollectedDataTypeLinked"
                        ] = numeric
                    else:
                        value["NSPrivacyCollectedDataTypes"][0][
                            "NSPrivacyCollectedDataTypeTracking"
                        ] = numeric
                    with self.assertRaises(contract.PackageContractError):
                        contract._validate_privacy(value)

    def test_missing_data_type_reason_or_tracking_declaration_fails_closed(
        self,
    ) -> None:
        base = privacy.manifest()
        for key in base:
            with self.subTest(key=key):
                value = copy.deepcopy(base)
                del value[key]
                with self.assertRaises(contract.PackageContractError):
                    contract._validate_privacy(value)
        for inventory in ["NSPrivacyCollectedDataTypes", "NSPrivacyAccessedAPITypes"]:
            for index in range(len(base[inventory])):
                with self.subTest(inventory=inventory, index=index):
                    value = copy.deepcopy(base)
                    value[inventory].pop(index)
                    with self.assertRaises(contract.PackageContractError):
                        contract._validate_privacy(value)

    def test_duplicate_foreign_purpose_or_unreviewed_api_reason_fails_closed(
        self,
    ) -> None:
        for inventory in ["NSPrivacyCollectedDataTypes", "NSPrivacyAccessedAPITypes"]:
            value = privacy.manifest()
            value[inventory].append(copy.deepcopy(value[inventory][0]))
            with self.assertRaises(contract.PackageContractError):
                contract._validate_privacy(value)
        for kind, key, replacement in [
            (
                "NSPrivacyCollectedDataTypes",
                "NSPrivacyCollectedDataTypePurposes",
                ["NSPrivacyCollectedDataTypePurposeAnalytics"],
            ),
            (
                "NSPrivacyAccessedAPITypes",
                "NSPrivacyAccessedAPITypeReasons",
                ["3B52.1"],
            ),
        ]:
            value = privacy.manifest()
            value[kind][0][key] = replacement
            with self.assertRaises(contract.PackageContractError):
                contract._validate_privacy(value)

    def test_stale_permission_and_foreign_contact_cannot_pass_nonempty_string_checks(
        self,
    ) -> None:
        root = SCRIPTS.parent
        base = contract._read_plist(root / "Tera/Info.plist")
        for key, bad in [
            ("NSCameraUsageDescription", "Add a farm photo"),
            ("TeraSupportEmail", "operator@example.invalid"),
            ("TeraSupportEmail", None),
        ]:
            value = {**base, key: bad}
            with self.assertRaises(contract.PackageContractError):
                contract._validate_app_plist(value)

    def test_unsupported_or_nonfinite_values_return_a_stable_rejection(self) -> None:
        for value in [b"private", float("nan"), float("inf")]:
            document = privacy.manifest()
            document["unexpected"] = value
            with self.assertRaises(contract.PackageContractError):
                contract._validate_privacy(document)
