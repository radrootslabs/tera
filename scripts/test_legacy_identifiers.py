from __future__ import annotations

import copy
import json
import plistlib
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import legacy_identifiers as legacy  # noqa: E402
import package_contract as contract  # noqa: E402


class LegacyIdentifierTests(unittest.TestCase):
    def setUp(self) -> None:
        self.path = "Tera/Runtime/TeraExample.swift"
        self.sources = {
            self.path: 'import RadrootsKit\nlet oldKey = "radroots.mobile.settings.v1"\n'
        }
        self.policy = {
            "schema": "tera.legacy-identifiers.v1",
            "baseline_commit": "a" * 40,
            "categories": {
                "synthetic": {
                    "owner": "synthetic public producer",
                    "reason": "synthetic shared API and persisted reader",
                    "reader": "synthetic app consumer",
                    "removal_condition": "explicit versioned reader migration",
                }
            },
            "entries": [
                {
                    "identifier": identifier,
                    "category": "synthetic",
                    "occurrences": [{"path": self.path, "count": 1}],
                }
                for identifier in ("RadrootsKit", "radroots.mobile.settings.v1")
            ],
        }

    def test_shared_and_persisted_names_are_exact_exceptions(self) -> None:
        self.assertEqual(legacy.validate(self.policy, self.sources)["identifiers"], 2)

    def test_kotlin_binding_harness_keeps_exact_naming_boundaries(self) -> None:
        for extension in ("kt", "kts"):
            path = f"scripts/kotlin_smoke/source.{extension}"
            self.assertTrue(legacy.selected(path))
            policy = copy.deepcopy(self.policy)
            entry = next(
                e for e in policy["entries"] if e["identifier"] == "RadrootsKit"
            )
            entry["occurrences"].append({"path": path, "count": 1})
            with self.assertRaisesRegex(legacy.LegacyIdentifierError, "declaration"):
                legacy.validate(policy, {**self.sources, path: "fun RadrootsKit() {}"})

    def test_unknown_identifier_is_rejected_even_in_an_allowed_file(self) -> None:
        sources = {
            self.path: self.sources[self.path] + 'let old = "radroots.unapproved.v1"\n'
        }
        with self.assertRaisesRegex(legacy.LegacyIdentifierError, "unapproved"):
            legacy.validate(self.policy, sources)

    def test_known_identifier_cannot_expand_to_another_file(self) -> None:
        sources = {**self.sources, "Tera/New.swift": "import RadrootsKit\n"}
        with self.assertRaisesRegex(legacy.LegacyIdentifierError, "unapproved"):
            legacy.validate(self.policy, sources)

    def test_removed_or_duplicated_occurrences_require_review(self) -> None:
        for text in ("", self.sources[self.path] * 2):
            with self.subTest(text=text):
                with self.assertRaisesRegex(legacy.LegacyIdentifierError, "stale"):
                    legacy.validate(self.policy, {self.path: text})

    def test_shared_name_exception_cannot_authorize_an_app_declaration(self) -> None:
        for declaration in (
            "struct RadrootsKit {}",
            "public actor RadrootsKit {}",
            "pub struct RadrootsKit;",
            "private func RadrootsKit() {}",
            "private static var RadrootsKit: Int { 0 }",
        ):
            with self.subTest(declaration=declaration):
                with self.assertRaisesRegex(
                    legacy.LegacyIdentifierError, "declaration"
                ):
                    legacy.validate(self.policy, {self.path: declaration})

    def test_missing_owner_reader_or_removal_condition_is_rejected(self) -> None:
        for field in legacy.CATEGORY_FIELDS:
            with self.subTest(field=field):
                policy = copy.deepcopy(self.policy)
                policy["categories"]["synthetic"][field] = ""
                with self.assertRaisesRegex(legacy.LegacyIdentifierError, "absent"):
                    legacy.validate(policy, self.sources)

    def test_wildcard_and_traversal_exceptions_are_rejected(self) -> None:
        for path in ("Tera/*.swift", "../Tera/Runtime/TeraExample.swift"):
            with self.subTest(path=path):
                policy = copy.deepcopy(self.policy)
                policy["entries"][0]["occurrences"][0]["path"] = path
                with self.assertRaises(legacy.LegacyIdentifierError):
                    legacy.validate(policy, self.sources)

    def test_duplicate_policy_fields_and_rows_are_rejected(self) -> None:
        with self.assertRaisesRegex(legacy.LegacyIdentifierError, "duplicated"):
            json.loads(
                '{"schema": 1, "schema": 2}', object_pairs_hook=legacy.unique_object
            )
        policy = copy.deepcopy(self.policy)
        policy["entries"].append(policy["entries"][0])
        with self.assertRaisesRegex(legacy.LegacyIdentifierError, "duplicated"):
            legacy.validate(policy, self.sources)

    def test_source_parent_symlinks_and_oversized_files_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "outside").mkdir()
            (root / "Tera").symlink_to(root / "outside", target_is_directory=True)
            with self.assertRaisesRegex(legacy.LegacyIdentifierError, "symlink"):
                legacy.read_regular(root, "Tera/Example.swift")
            (root / "large").write_bytes(b"x" * (legacy.MAX_BYTES + 1))
            with self.assertRaisesRegex(legacy.LegacyIdentifierError, "byte bound"):
                legacy.read_regular(root, "large")

    def test_display_name_is_independent_of_legacy_namespace_exceptions(self) -> None:
        document = plistlib.loads((SCRIPTS.parent / "Tera/Info.plist").read_bytes())
        document["CFBundleDisplayName"] = "Radroots"
        with self.assertRaisesRegex(contract.PackageContractError, "display name"):
            contract._validate_app_plist(document)

    def test_current_source_has_only_reviewed_legacy_occurrences(self) -> None:
        result = legacy.verify(SCRIPTS.parent)
        self.assertGreater(result["identifiers"], 0)


if __name__ == "__main__":
    unittest.main()
