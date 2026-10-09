import json
import unittest
from pathlib import Path
from unittest import mock

from scripts import test_local_social_fixture as support

fixture = support.fixture


class PersonaAttachmentExtractionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.suite, self.attempts = (
            support.LocalSocialFixtureTests().persona_attempt_attachments()
        )

    def export(self, destination: Path) -> list[dict]:
        rows = []
        for index, (name, (raw, _)) in enumerate(
            zip(fixture.PERSONA_ATTACHMENT_NAMES, self.attempts, strict=True)
        ):
            exported = f"attempt-{index}.json"
            (destination / exported).write_bytes(raw)
            rows.append(
                self.row(
                    exported,
                    f"{name[:-5]}_0_{index:08X}-0000-0000-0000-000000000000.json",
                )
            )
        for index in range(3):
            for label, suffix in [
                ("Screen before audit", "png"),
                ("Accessibility tree before audit", "txt"),
            ]:
                exported = f"audit-{index}.{suffix}"
                (destination / exported).write_bytes(b"native audit diagnostic")
                rows.append(
                    self.row(
                        exported,
                        f"{label}_0_{index:08X}-0000-0000-0000-000000000000.{suffix}",
                    )
                )
        return rows

    @staticmethod
    def row(exported: str, name: str) -> dict:
        return {
            "exportedFileName": exported,
            "suggestedHumanReadableName": name,
            "isAssociatedWithFailure": False,
            "configurationName": "Test Scheme Action",
            "deviceName": "iPhone 17 Pro",
            "deviceId": "11111111-2222-3333-4444-555555555555",
        }

    def extract(self, mutation=None):
        def run(command, **_):
            destination = Path(command[command.index("--output-path") + 1])
            rows = self.export(destination)
            if mutation is not None:
                mutation(destination, rows)
            (destination / "manifest.json").write_text(
                json.dumps(
                    [
                        {
                            "testIdentifier": fixture.PERSONA_XCRESULT_NODE_IDENTIFIER,
                            "testIdentifierURL": fixture.PERSONA_XCRESULT_NODE_URL,
                            "attachments": rows,
                        }
                    ]
                )
            )

        tests = {
            "devices": [],
            "testPlanConfigurations": [],
            "testNodes": [
                {
                    "name": "testLocalSocialDeterministicPersonas()",
                    "nodeIdentifier": fixture.PERSONA_XCRESULT_NODE_IDENTIFIER,
                    "nodeIdentifierURL": fixture.PERSONA_XCRESULT_NODE_URL,
                    "nodeType": "Test Case",
                    "result": "Passed",
                }
            ],
        }
        with (
            mock.patch.object(fixture, "run_json_command_bounded", return_value=tests),
            mock.patch.object(fixture, "run_selector_command", side_effect=run),
        ):
            return fixture.extract_persona_attempt_attachments(
                Path("synthetic.xcresult"), self.suite, require_measured_network=True
            )

    def test_native_audit_diagnostics_do_not_replace_any_measured_attempt(self) -> None:
        actual = self.extract()
        self.assertEqual(actual, self.attempts)
        self.assertEqual(len(actual), 15)

    def test_unknown_unsafe_or_foreign_diagnostics_are_rejected(self) -> None:
        def unknown(_, rows):
            rows[-1]["suggestedHumanReadableName"] = "unknown.json"

        def unrelated_file(destination, _):
            (destination / "unreferenced.txt").write_text("unreferenced")

        def failure(_, rows):
            rows[-1]["isAssociatedWithFailure"] = True

        def foreign(_, rows):
            rows[-1]["deviceId"] = "22222222-2222-3333-4444-555555555555"

        def traversal(_, rows):
            rows[-1]["exportedFileName"] = "../outside.txt"

        def duplicate(_, rows):
            rows[0]["suggestedHumanReadableName"] = rows[1][
                "suggestedHumanReadableName"
            ]

        def symbolic_link(destination, rows):
            path = destination / rows[-1]["exportedFileName"]
            path.unlink()
            path.symlink_to(destination / rows[0]["exportedFileName"])

        for mutation in [
            unknown,
            unrelated_file,
            failure,
            foreign,
            traversal,
            duplicate,
            symbolic_link,
        ]:
            with (
                self.subTest(mutation=mutation.__name__),
                self.assertRaises(ValueError),
            ):
                self.extract(mutation)

    def test_raw_diagnostics_retain_the_existing_result_file_byte_bound(self) -> None:
        def oversized(destination, rows):
            with (destination / rows[-1]["exportedFileName"]).open("wb") as stream:
                stream.truncate(fixture.MAX_RESULT_BUNDLE_FILE_BYTES + 1)

        with self.assertRaisesRegex(ValueError, "byte bound"):
            self.extract(oversized)


if __name__ == "__main__":
    unittest.main()
