import copy
import hashlib
import importlib.util
import json
import re
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).with_name("local-social-fixture.py")
SPEC = importlib.util.spec_from_file_location("local_social_fixture", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)


class LocalSocialFixtureTests(unittest.TestCase):
    def persona_result(self) -> tuple[dict, dict]:
        _, suite = fixture.load_persona_suite(
            Path("test-fixtures/local-social-personas.v1.json")
        )
        personas = []
        for persona in suite["personas"]:
            personas.append(
                {
                    "alias": persona["alias"],
                    "identity_sha256": hashlib.sha256(
                        persona["alias"].encode("ascii")
                    ).hexdigest(),
                    "subscriptions": 1,
                    "attempts": [
                        {
                            "id": attempt["id"],
                            "flow": attempt["flow"],
                            "event_kind": fixture.FLOW_KINDS[attempt["flow"]],
                            "accepted": True,
                            "expected_failure_rejections": int(
                                attempt["expected_failure"]
                                == "transport_retry_relaunch"
                            ),
                        }
                        for attempt in persona["attempts"]
                    ],
                }
            )
        result = {
            "schema": "radroots.ios.local-social.persona-results.v1",
            "schema_version": 1,
            "run_id": "persona-result-test-001",
            "source_commit": "1" * 40,
            "source_tree": "2" * 40,
            "fixture_sha256": "3" * 64,
            "fixture_schema_sha256": "4" * 64,
            "result_schema_sha256": "5" * 64,
            "simulator": {
                "udid": "11111111-2222-3333-4444-555555555555",
                "os": "iOS 26.5",
                "architecture": "arm64",
            },
            "result_bundle_sha256": "6" * 64,
            "evidence_sha256": "7" * 64,
            "personas": personas,
            "flow_counts": {flow: 3 for flow in fixture.FLOW_KINDS},
            "accepted_events": 15,
            "event_kind_counts": {"1": 9, "31923": 3, "30402": 3},
            "accepted_uploads": 3,
            "retrievals": 3,
            "distinct_identities": 5,
            "unknown_attempts": 0,
            "duplicate_attempts": 0,
            "expected_failure_rejections": 1,
            "events_accepted_during_expected_failures": 0,
            "production_network_contacts": 0,
            "unintended_publications": 0,
            "final_candidate_data_loss": 0,
            "accessibility": {
                "locale": "en_US",
                "content_size": "accessibility-extra-extra-extra-large",
                "reduce_motion": True,
                "semantic_audit": "passed",
                "voiceover_user_observed": False,
            },
            "forward_repairs": [],
            "complete_matrix_rerun": True,
        }
        return suite, result

    def test_bip340_reference_signature_and_mutation(self) -> None:
        public_key = bytes.fromhex(
            "f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9"
        )
        message = bytes(32)
        signature = bytes.fromhex(
            "e907831f80848d1069a5371b402410364bdf1c5f8307b0084c55f1ce2dca8215"
            "25f66a4a85ea8b71e482a74f382d2ce5ebeee8fdb2172f477df4900d310536c0"
        )
        self.assertTrue(fixture.verify_bip340(public_key, message, signature))
        self.assertFalse(
            fixture.verify_bip340(public_key, message, signature[:-1] + b"\x00")
        )

    def test_fixture_rejects_duplicate_and_unknown_fields(self) -> None:
        source = Path("test-fixtures/local-social-personas.v1.json")
        payload = json.loads(source.read_text(encoding="utf-8"))
        payload["unexpected"] = True
        with self.assertRaisesRegex(ValueError, "field inventory"):
            fixture.validate_persona_suite(payload)

        with tempfile.TemporaryDirectory() as directory:
            duplicate = Path(directory, "duplicate.json")
            duplicate.write_text('{"schema":1,"schema":1}\n', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "duplicate JSON member"):
                fixture.read_json(duplicate)

    def test_fixture_freezes_exact_persona_and_flow_matrix(self) -> None:
        _, suite = fixture.load_persona_suite(
            Path("test-fixtures/local-social-personas.v1.json")
        )
        attempts = fixture.persona_attempts(suite)
        self.assertEqual(
            tuple(persona["alias"] for persona in suite["personas"]),
            fixture.PERSONA_ALIASES,
        )
        self.assertEqual(len(attempts), 15)
        self.assertEqual(
            {
                flow: sum(item["flow"] == flow for item in attempts.values())
                for flow in fixture.FLOW_KINDS
            },
            {flow: 3 for flow in fixture.FLOW_KINDS},
        )

    def test_legacy_fixture_control_remains_file_presence_based(self) -> None:
        body = b"legacy-photo"
        digest = fixture.hashlib.sha256(body).hexdigest()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            control = root / "control"
            control.touch()
            state = fixture.FixtureState(root / "evidence.json", control, 21100)
            with mock.patch.object(
                fixture, "valid_blossom_authorization", return_value=True
            ):
                accepted, _ = state.upload(body, "image/png", digest, "Nostr valid")
            self.assertTrue(accepted)
            self.assertIsNotNone(state.retrieve(digest))
            evidence = json.loads((root / "evidence.json").read_text(encoding="utf-8"))
            self.assertEqual(evidence["accepted_uploads"], 1)
            self.assertEqual(evidence["retrievals"], 1)

    def test_persona_retrieval_counts_only_its_own_uploaded_digest(self) -> None:
        body = b"persona-photo"
        digest = fixture.hashlib.sha256(body).hexdigest()
        _, suite = fixture.load_persona_suite(
            Path("test-fixtures/local-social-personas.v1.json")
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            control = root / "control.json"
            state = fixture.FixtureState(
                root / "evidence.json", control, 21100, suite
            )
            self.write_persona_control(control, "P01")
            with mock.patch.object(
                fixture, "valid_blossom_authorization", return_value=True
            ):
                accepted, _ = state.upload(body, "image/png", digest, "Nostr valid")
            self.assertTrue(accepted)
            self.assertIsNotNone(state.retrieve(digest))
            self.write_persona_control(control, "P02")
            self.assertIsNotNone(state.retrieve(digest))

            evidence = json.loads(
                (root / "evidence.json").read_text(encoding="utf-8")
            )
            self.assertEqual(evidence["retrievals"], 1)
            self.assertEqual(evidence["personas"][0]["retrievals"], 1)
            self.assertEqual(evidence["personas"][1]["retrievals"], 0)

    def test_transport_retry_drops_one_response_then_accepts(self) -> None:
        _, suite = fixture.load_persona_suite(
            Path("test-fixtures/local-social-personas.v1.json")
        )
        attempt = fixture.persona_attempts(suite)["P05-A01"]
        event = {
            "id": "1" * 64,
            "kind": fixture.FLOW_KINDS[attempt["flow"]],
            "pubkey": "2" * 64,
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            control = root / "control.json"
            self.write_persona_control(control, "P05")
            state = fixture.FixtureState(
                root / "evidence.json", control, 21100, suite
            )
            with mock.patch.object(
                fixture, "classify_attempt", return_value=attempt
            ):
                self.assertIsNone(state._publish_persona_event(event))
                self.assertTrue(state._publish_persona_event(event))

            evidence = json.loads(
                (root / "evidence.json").read_text(encoding="utf-8")
            )
            self.assertEqual(evidence["expected_failure_rejections"], 1)
            self.assertEqual(evidence["accepted_events"], 1)

    @staticmethod
    def write_persona_control(path: Path, alias: str) -> None:
        path.write_text(
            json.dumps(
                {
                    "schema": fixture.PERSONA_CONTROL_SCHEMA,
                    "active_persona": alias,
                    "blossom_enabled": True,
                }
            ),
            encoding="utf-8",
        )

    def test_result_contract_accepts_future_ios_and_rejects_drift(self) -> None:
        suite, result = self.persona_result()
        result_schema = json.loads(
            Path("test-fixtures/local-social-persona-results.v1.schema.json").read_text(
                encoding="utf-8"
            )
        )
        os_pattern = result_schema["properties"]["simulator"]["properties"]["os"][
            "pattern"
        ]
        self.assertIsNotNone(re.fullmatch(os_pattern, "iOS 26.5"))
        self.assertIsNone(re.fullmatch(os_pattern, "iOS 17.7"))
        self.assertIs(
            fixture.validate_persona_result(
                result, suite, "3" * 64, "4" * 64, "5" * 64
            ),
            result,
        )
        for mutation in (
            lambda value: value.update({"unexpected": True}),
            lambda value: value["simulator"].update({"os": "iOS 17.7"}),
            lambda value: value["accessibility"].update(
                {"voiceover_user_observed": True}
            ),
        ):
            changed = copy.deepcopy(result)
            mutation(changed)
            with self.assertRaises(ValueError):
                fixture.validate_persona_result(
                    changed, suite, "3" * 64, "4" * 64, "5" * 64
                )

    def test_control_is_bounded_and_deny_unknown(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory, "control.json")
            path.write_text(
                json.dumps(
                    {
                        "schema": fixture.PERSONA_CONTROL_SCHEMA,
                        "active_persona": "P03",
                        "blossom_enabled": True,
                    }
                ),
                encoding="utf-8",
            )
            self.assertEqual(fixture.read_control(path)["active_persona"], "P03")
            path.write_text(
                json.dumps(
                    {
                        "schema": fixture.PERSONA_CONTROL_SCHEMA,
                        "active_persona": "P03",
                        "blossom_enabled": True,
                        "secret": "forbidden",
                    }
                ),
                encoding="utf-8",
            )
            self.assertIsNone(fixture.read_control(path))

    def test_attempt_classification_accepts_only_exact_photo_wire_shape(self) -> None:
        marker = "rr-p01-a02-photo"
        digest = "a" * 64
        url = f"http://127.0.0.1:21101/{digest}.png"
        attempt = {"flow": "PhotoUpdate", "marker": marker}
        event = {
            "content": f"{marker}\n{url}",
            "tags": [
                [
                    "imeta",
                    f"url {url}",
                    f"x {digest}",
                    "m image/png",
                    "dim 1x1",
                    "size 1",
                    "alt Local qualification image",
                ]
            ],
        }

        self.assertIs(
            fixture.classify_attempt(event, {"P01-A02": attempt}), attempt
        )
        for mutation in (
            lambda value: value.update({"content": marker}),
            lambda value: value.update({"content": f"prefix {marker}\n{url}"}),
            lambda value: value.update(
                {"content": f"{marker}\nhttps://example.com/{digest}.png"}
            ),
            lambda value: value["tags"][0].remove(f"x {digest}"),
        ):
            changed = copy.deepcopy(event)
            mutation(changed)
            self.assertIsNone(
                fixture.classify_attempt(changed, {"P01-A02": attempt})
            )

        text_attempt = {"flow": "Update", "marker": "rr-p01-a01-update"}
        embedded = {"content": "prefix rr-p01-a01-update", "tags": []}
        self.assertIsNone(
            fixture.classify_attempt(embedded, {"P01-A01": text_attempt})
        )


if __name__ == "__main__":
    unittest.main()
