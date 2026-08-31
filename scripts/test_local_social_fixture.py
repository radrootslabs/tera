import base64
import copy
import hashlib
import http.client
import importlib.util
import json
import re
import secrets
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).with_name("local-social-fixture.py")
SPEC = importlib.util.spec_from_file_location("local_social_fixture", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)


def sign_bud11_event(event: dict) -> dict:
    secret = secrets.randbelow(fixture.SECP256K1_ORDER - 1) + 1
    public_point = fixture.point_multiply(secret, fixture.SECP256K1_GENERATOR)
    assert public_point is not None
    if public_point[1] & 1:
        secret = fixture.SECP256K1_ORDER - secret
    public_key = public_point[0].to_bytes(32, "big")
    signed = copy.deepcopy(event)
    signed["pubkey"] = public_key.hex()
    preimage = json.dumps(
        [
            0,
            signed["pubkey"],
            signed["created_at"],
            signed["kind"],
            signed["tags"],
            signed["content"],
        ],
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    message = hashlib.sha256(preimage).digest()
    signed["id"] = message.hex()
    auxiliary = secrets.token_bytes(32)
    masked_secret = bytes(
        left ^ right
        for left, right in zip(
            secret.to_bytes(32, "big"),
            fixture.tagged_hash("BIP0340/aux", auxiliary),
            strict=True,
        )
    )
    nonce = int.from_bytes(
        fixture.tagged_hash("BIP0340/nonce", masked_secret + public_key + message),
        "big",
    ) % fixture.SECP256K1_ORDER
    assert nonce != 0
    nonce_point = fixture.point_multiply(nonce, fixture.SECP256K1_GENERATOR)
    assert nonce_point is not None
    if nonce_point[1] & 1:
        nonce = fixture.SECP256K1_ORDER - nonce
    challenge = int.from_bytes(
        fixture.tagged_hash(
            "BIP0340/challenge",
            nonce_point[0].to_bytes(32, "big") + public_key + message,
        ),
        "big",
    ) % fixture.SECP256K1_ORDER
    signature = nonce_point[0].to_bytes(32, "big") + (
        (nonce + challenge * secret) % fixture.SECP256K1_ORDER
    ).to_bytes(32, "big")
    signed["sig"] = signature.hex()
    assert fixture.valid_nostr_event(signed)
    assert fixture.verify_nostr_signature(signed)
    return signed


def bud11_event(now_unix_s: int, digest: str) -> dict:
    return {
        "created_at": now_unix_s - 5,
        "kind": fixture.BUD11_EVENT_KIND,
        "tags": [
            ["t", "upload"],
            ["expiration", str(now_unix_s + 295)],
            ["x", digest],
            ["server", fixture.BUD11_SERVER_DOMAIN],
        ],
        "content": "Upload exact Radroots image",
    }


def mutate_bud11_event(event: dict, mutation: str, now_unix_s: int) -> tuple[dict, str]:
    changed = copy.deepcopy(event)
    if mutation == "none":
        pass
    elif mutation == "wrong_kind":
        changed["kind"] = 1
    elif mutation == "empty_content":
        changed["content"] = ""
    elif mutation == "oversized_content":
        changed["content"] = "x" * (fixture.BUD11_CONTENT_MAX_BYTES + 1)
    elif mutation == "leading_content_space":
        changed["content"] = " Upload exact Radroots image"
    elif mutation == "control_content":
        changed["content"] = "Upload\0image"
    elif mutation == "missing_action":
        changed["tags"].pop(0)
    elif mutation == "wrong_action":
        changed["tags"][0][1] = "delete"
    elif mutation == "duplicate_action":
        changed["tags"].insert(1, ["t", "upload"])
    elif mutation == "wrong_hash":
        changed["tags"][2][1] = "f" * 64
    elif mutation == "duplicate_hash":
        changed["tags"].insert(3, copy.deepcopy(changed["tags"][2]))
    elif mutation == "missing_server":
        changed["tags"].pop(3)
    elif mutation == "wrong_server":
        changed["tags"][3][1] = "media.example"
    elif mutation == "uppercase_server":
        changed["tags"][3][1] = "Media.Example"
    elif mutation == "duplicate_server":
        changed["tags"].append(copy.deepcopy(changed["tags"][3]))
    elif mutation == "missing_expiration":
        changed["tags"].pop(1)
    elif mutation == "noncanonical_expiration":
        changed["tags"][1][1] = "0" + changed["tags"][1][1]
    elif mutation == "expired":
        changed["tags"][1][1] = str(now_unix_s)
    elif mutation == "created_at_not_past":
        changed["created_at"] = now_unix_s + 60
        changed["tags"][1][1] = str(now_unix_s + 360)
    elif mutation == "lifetime_too_long":
        changed["tags"][1][1] = str(changed["created_at"] + 301)
    elif mutation == "unknown_tag":
        changed["tags"].append(["client", "radroots"])
    elif mutation in {"event_id", "signature", "authorization_scheme", "padded_base64"}:
        pass
    else:
        raise AssertionError(f"unsupported mutation: {mutation}")
    signed = sign_bud11_event(changed)
    if mutation == "event_id":
        signed["id"] = ("0" if signed["id"][0] != "0" else "1") + signed["id"][1:]
    elif mutation == "signature":
        signed["sig"] = ("0" if signed["sig"][0] != "0" else "1") + signed["sig"][1:]
    raw = json.dumps(signed, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    header = "Nostr " + base64.urlsafe_b64encode(raw).rstrip(b"=").decode("ascii")
    if mutation == "authorization_scheme":
        header = "Bearer " + header.removeprefix("Nostr ")
    elif mutation == "padded_base64":
        header += "="
    return signed, header


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

    def persona_attempt_attachments(
        self,
        *,
        measured: bool = True,
    ) -> tuple[dict, list[tuple[bytes, dict]]]:
        _, suite = fixture.load_persona_suite(
            Path("test-fixtures/local-social-personas.v1.json")
        )
        attachments = []
        for persona in suite["personas"]:
            identity = hashlib.sha256(persona["alias"].encode("ascii")).hexdigest()
            for index, attempt in enumerate(persona["attempts"]):
                validation = attempt["expected_failure"] == "validation_recovery"
                retry = attempt["expected_failure"] == "transport_retry_relaunch"
                network = {"state": "pending_step_258"}
                if measured:
                    network = {
                        "state": "measured",
                        "accepted_connections": 1,
                        "rejected_connections": int(retry),
                        "non_loopback_attempts": 0,
                        "subscriptions": int(index == 0),
                        "accepted_events": 1,
                        "accepted_uploads": int(attempt["flow"] == "PhotoUpdate"),
                        "retrievals": int(attempt["flow"] == "PhotoUpdate"),
                        "unintended_publications": 0,
                        "events_accepted_during_expected_failure": 0,
                        "final_candidate_data_loss": 0,
                    }
                value = {
                    "schema": fixture.PERSONA_ATTEMPT_SCHEMA,
                    "schema_version": 1,
                    "test_invocation": fixture.persona_invocation(),
                    "source": {"commit": "1" * 40, "tree": "2" * 40},
                    "app_build_sha256": "3" * 64,
                    "simulator": {
                        "udid": "11111111-2222-3333-4444-555555555555",
                        "os": "iOS 26.5",
                        "architecture": "arm64",
                    },
                    "run_id": "persona-result-test-001",
                    "persona_run_id": (
                        f"persona-{persona['alias'].lower()}-" + "a" * 24
                    ),
                    "persona_alias": persona["alias"],
                    "attempt_id": attempt["id"],
                    "attempt_order": attempt["order"],
                    "flow": attempt["flow"],
                    "expected_failure": attempt["expected_failure"],
                    "public_identity_sha256": identity,
                    "endpoint_policy_sha256": "4" * 64,
                    "ui_observation": {
                        "validation_attempted": validation,
                        "validation_rejected": validation,
                        "retry_attempts": int(retry),
                        "relaunches": int(retry),
                        "retention_verified": True,
                        "today_projection_verified": True,
                    },
                    "network_observation": network,
                    "accessibility": {
                        "locale": "en_US",
                        "content_size": "accessibility-extra-extra-extra-large",
                        "reduce_motion": True,
                        "progressive_disclosure": persona["interaction_profile"]
                        == "novice_progressive_disclosure",
                        "labels_values_traits": persona["interaction_profile"]
                        == "novice_accessibility_keyboard",
                        "keyboard_focus": persona["interaction_profile"]
                        == "novice_accessibility_keyboard",
                        "visible_actions": True,
                        "voiceover_user_observed": False,
                    },
                    "artifact_digests": [],
                }
                raw = fixture.canonical_evidence_bytes(value)
                attachments.append((raw, value))
        return suite, attachments

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

    def test_bud11_corpus_is_exact_bounded_and_contains_no_sensitive_evidence(self) -> None:
        path = Path("test-fixtures/bud11-upload-authorization-mutations.v1.json")
        raw, corpus = fixture.load_bud11_mutation_corpus(path)
        self.assertLessEqual(len(raw), fixture.MAX_JSON_BYTES)
        self.assertEqual(len(corpus["vectors"]), len(fixture.BUD11_MUTATIONS))
        self.assertNotRegex(
            raw.decode("utf-8"),
            r'"(?:private_key|secret|seed|authorization|event|sig|pubkey)"\s*:',
        )

    def test_bud11_corpus_rejects_unknown_duplicate_and_missing_vectors(self) -> None:
        path = Path("test-fixtures/bud11-upload-authorization-mutations.v1.json")
        value = json.loads(path.read_text(encoding="utf-8"))
        value["unknown"] = True
        with self.assertRaisesRegex(ValueError, "field inventory"):
            fixture.validate_bud11_mutation_corpus(value)
        value = json.loads(path.read_text(encoding="utf-8"))
        value["vectors"].pop()
        with self.assertRaisesRegex(ValueError, "inventory"):
            fixture.validate_bud11_mutation_corpus(value)
        with tempfile.TemporaryDirectory() as directory:
            duplicate = Path(directory, "duplicate.json")
            duplicate.write_text(
                '{"schema":"x","schema":"x","schema_version":1,"vectors":[]}\n',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "duplicate JSON member"):
                fixture.load_bud11_mutation_corpus(duplicate)

    def test_bud11_scalar_bounds_and_server_domain_grammar_are_fail_closed(self) -> None:
        self.assertEqual(fixture.canonical_unsigned_decimal("0"), 0)
        self.assertEqual(
            fixture.canonical_unsigned_decimal(str(0xFFFF_FFFF_FFFF_FFFF)),
            0xFFFF_FFFF_FFFF_FFFF,
        )
        for value in ["", "00", "+1", "١", "18446744073709551616", "1" * 10_000]:
            self.assertIsNone(fixture.canonical_unsigned_decimal(value))
        for value in ["127.0.0.1", "media.example", "a-b.example"]:
            self.assertTrue(fixture.valid_bud11_server_domain(value), value)
        for value in [
            "127.000.0.1",
            "123",
            "Media.Example",
            "https://media.example",
            "media.example:443",
            "-media.example",
            "media-.example",
        ]:
            self.assertFalse(fixture.valid_bud11_server_domain(value), value)
        oversized = "Nostr " + "a" * (
            fixture.BUD11_AUTHORIZATION_ENCODED_MAX_BYTES + 1
        )
        self.assertFalse(
            fixture.valid_blossom_authorization(
                oversized,
                "a" * 64,
                fixture.BUD11_SERVER_DOMAIN,
                1,
            )
        )

    def test_bud11_mutation_corpus_matches_strict_admission_and_relay_denial(self) -> None:
        _, corpus = fixture.load_bud11_mutation_corpus(
            Path("test-fixtures/bud11-upload-authorization-mutations.v1.json")
        )
        now = int(time.time())
        digest = hashlib.sha256(b"canonical-bud11-photo").hexdigest()
        for vector in corpus["vectors"]:
            with self.subTest(vector=vector["id"]):
                event, header = mutate_bud11_event(
                    bud11_event(now, digest), vector["mutation"], now
                )
                if vector["surface"] == "http":
                    accepted = fixture.valid_blossom_authorization(
                        header,
                        digest,
                        fixture.BUD11_SERVER_DOMAIN,
                        now,
                    )
                else:
                    with tempfile.TemporaryDirectory() as directory:
                        root = Path(directory)
                        state = fixture.FixtureState(
                            root / "evidence.json", root / "control", 21100
                        )
                        accepted = bool(state.publish(event))
                self.assertEqual(accepted, vector["expected_accepted"])

    def test_local_blossom_http_runtime_enforces_the_shared_bud11_corpus(self) -> None:
        _, corpus = fixture.load_bud11_mutation_corpus(
            Path("test-fixtures/bud11-upload-authorization-mutations.v1.json")
        )
        body = b"canonical-bud11-photo"
        digest = hashlib.sha256(body).hexdigest()
        now = int(time.time())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            control = root / "control"
            control.touch()
            state = fixture.FixtureState(root / "evidence.json", control, 0)
            fixture.BlossomHandler.state = state
            server = fixture.http.server.ThreadingHTTPServer(
                ("127.0.0.1", 0), fixture.BlossomHandler
            )
            state.blossom_port = server.server_address[1]
            thread = fixture.threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                for vector in corpus["vectors"]:
                    if vector["surface"] != "http":
                        continue
                    with self.subTest(vector=vector["id"]):
                        _, header = mutate_bud11_event(
                            bud11_event(now, digest), vector["mutation"], now
                        )
                        connection = http.client.HTTPConnection(
                            "127.0.0.1", state.blossom_port, timeout=5
                        )
                        connection.request(
                            "PUT",
                            f"/{digest}.png",
                            body=body,
                            headers={
                                "Authorization": header,
                                "Content-Type": "image/png",
                                "Content-Length": str(len(body)),
                            },
                        )
                        response = connection.getresponse()
                        response.read()
                        connection.close()
                        self.assertEqual(
                            response.status == 200, vector["expected_accepted"]
                        )
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
            evidence = json.loads((root / "evidence.json").read_text(encoding="utf-8"))
            self.assertEqual(evidence["accepted_uploads"], 1)

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

    def test_json_reads_enforce_maximum_plus_one_before_decoding(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory, "bounded.json")
            path.write_bytes(b'{"x":1}')
            self.assertEqual(fixture.read_json(path, 7)[1], {"x": 1})
            path.write_bytes(b'{"x":1}\n')
            with self.assertRaisesRegex(ValueError, "byte bound"):
                fixture.read_json(path, 7)

    def test_schemas_pass_meta_validation_and_match_semantic_corpora(self) -> None:
        fixture_raw, suite = fixture.load_persona_suite(
            Path("test-fixtures/local-social-personas.v1.json")
        )
        self.assertTrue(fixture_raw)
        _, persona_schema = fixture.load_schema_file(
            Path("test-fixtures/local-social-personas.v1.schema.json"),
            "https://radroots.org/schemas/ios/local-social-personas.v1.schema.json",
        )
        fixture.validate_schema_instance(persona_schema, suite, "persona fixture")
        _, corpus = fixture.load_bud11_mutation_corpus(
            Path("test-fixtures/bud11-upload-authorization-mutations.v1.json")
        )
        _, corpus_schema = fixture.load_schema_file(
            Path(
                "test-fixtures/bud11-upload-authorization-mutations.v1.schema.json"
            ),
            "https://radroots.org/schemas/ios/bud11-upload-authorization-mutations.v1.schema.json",
        )
        fixture.validate_schema_instance(corpus_schema, corpus, "BUD-11 corpus")

        changed = copy.deepcopy(suite)
        changed["unexpected"] = True
        with self.assertRaisesRegex(ValueError, "disagrees"):
            fixture.validate_schema_instance(persona_schema, changed, "persona fixture")

        with tempfile.TemporaryDirectory() as directory:
            invalid = Path(directory, "invalid.schema.json")
            invalid.write_text(
                json.dumps(
                    {
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "$id": "https://radroots.org/schemas/ios/invalid.json",
                        "type": "object",
                        "additionalProperties": False,
                        "properties": {"x": {"type": "not-a-json-schema-type"}},
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "meta-validation"):
                fixture.load_schema_file(
                    invalid, "https://radroots.org/schemas/ios/invalid.json"
                )
            invalid.write_text(
                json.dumps(
                    {
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "$id": "https://radroots.org/schemas/ios/invalid.json",
                        "type": "object",
                        "additionalProperties": False,
                        "properties": {
                            "x": {"$ref": "https://example.com/external.json"}
                        },
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "external reference"):
                fixture.load_schema_file(
                    invalid, "https://radroots.org/schemas/ios/invalid.json"
                )

    def test_result_bundle_digest_is_framed_bounded_and_pinned(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "a.txt").write_bytes(b"A")
            (root / "nested").mkdir()
            (root / "nested" / "b.bin").write_bytes(b"\x00B")
            self.assertEqual(
                fixture.directory_digest(root, 3, 2, 3),
                "25fb5f36c3b044de2716ddfbbd95c2e5eb39cf38a5bcf2feb793cd57ce27147a",
            )
            with self.assertRaisesRegex(ValueError, "entry bound"):
                fixture.directory_digest(root, 2, 2, 3)
            with self.assertRaisesRegex(ValueError, "byte bound"):
                fixture.directory_digest(root, 3, 1, 3)
            with self.assertRaisesRegex(ValueError, "byte bound"):
                fixture.directory_digest(root, 3, 2, 2)

            alternate = root / "alternate"
            alternate.mkdir()
            (alternate / "a").write_bytes(b".txtA")
            self.assertNotEqual(
                fixture.directory_digest(root, 5, 8, 16),
                fixture.directory_digest(alternate, 1, 8, 8),
            )

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
        fixture.validate_schema_instance(result_schema, result, "persona v1 result")
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

    def test_attempt_evidence_is_strict_bound_and_secret_free(self) -> None:
        suite, attachments = self.persona_attempt_attachments(measured=False)
        for raw, value in attachments:
            self.assertLessEqual(len(raw), fixture.MAX_PERSONA_ATTACHMENT_BYTES)
            self.assertIs(
                fixture.validate_persona_attempt_evidence(
                    value, suite, require_measured_network=False
                ),
                value,
            )
            self.assertNotRegex(
                raw.decode("utf-8"),
                r'"(?:private_key|secret|seed|signed_event|event_content|raw_event)"',
            )
            with self.assertRaisesRegex(ValueError, "not measured"):
                fixture.validate_persona_attempt_evidence(
                    value, suite, require_measured_network=True
                )

    def test_attempt_evidence_rejects_one_field_identity_and_outcome_drift(self) -> None:
        suite, attachments = self.persona_attempt_attachments()
        _, canonical = attachments[0]
        mutations = (
            lambda value: value.update({"unknown": True}),
            lambda value: value["test_invocation"].update(
                {"identifier": "RadrootsUITests/testOther"}
            ),
            lambda value: value["source"].update({"tree": "A" * 40}),
            lambda value: value.update({"app_build_sha256": "0" * 63}),
            lambda value: value.update({"run_id": "x"}),
            lambda value: value.update({"persona_alias": "P02"}),
            lambda value: value.update({"attempt_order": 2}),
            lambda value: value["ui_observation"].update(
                {"today_projection_verified": False}
            ),
            lambda value: value["network_observation"].update(
                {"non_loopback_attempts": 1}
            ),
            lambda value: value["accessibility"].update(
                {"voiceover_user_observed": True}
            ),
            lambda value: value.update({"private_key": "canary"}),
        )
        for mutation in mutations:
            changed = copy.deepcopy(canonical)
            mutation(changed)
            with self.assertRaises(ValueError):
                fixture.validate_persona_attempt_evidence(
                    changed, suite, require_measured_network=True
                )

    def test_persona_result_v2_is_reconstructed_only_from_measured_attempts(self) -> None:
        suite, attachments = self.persona_attempt_attachments()
        result = fixture.reconstruct_persona_result_v2(
            suite,
            attachments,
            fixture_sha256="5" * 64,
            fixture_schema_sha256="6" * 64,
            attempt_schema_sha256="7" * 64,
            result_schema_sha256="8" * 64,
            result_bundle_sha256="9" * 64,
            forward_repairs=[],
        )
        self.assertEqual(result["schema"], fixture.PERSONA_RESULT_V2_SCHEMA)
        self.assertEqual(result["accepted_events"], 15)
        self.assertEqual(result["expected_failure_rejections"], 2)
        self.assertEqual(result["accepted_uploads"], 3)
        self.assertEqual(result["retrievals"], 3)
        self.assertEqual(result["non_loopback_attempts"], 0)
        self.assertEqual(len(result["attachments"]), 15)
        _, attempt_schema = fixture.load_schema_file(
            Path(
                "test-fixtures/local-social-persona-attempt-evidence.v1.schema.json"
            ),
            "https://radroots.org/schemas/ios/local-social-persona-attempt-evidence.v1.schema.json",
        )
        for _, attempt in attachments:
            fixture.validate_schema_instance(
                attempt_schema, attempt, "persona attempt evidence"
            )
        _, result_schema = fixture.load_schema_file(
            Path("test-fixtures/local-social-persona-results.v2.schema.json"),
            "https://radroots.org/schemas/ios/local-social-persona-results.v2.schema.json",
        )
        fixture.validate_schema_instance(result_schema, result, "persona v2 result")

        _, pending = self.persona_attempt_attachments(measured=False)
        with self.assertRaisesRegex(ValueError, "not measured"):
            fixture.reconstruct_persona_result_v2(
                suite,
                pending,
                fixture_sha256="5" * 64,
                fixture_schema_sha256="6" * 64,
                attempt_schema_sha256="7" * 64,
                result_schema_sha256="8" * 64,
                result_bundle_sha256="9" * 64,
                forward_repairs=[],
            )

    def test_xcresult_test_identity_and_attachment_inventory_are_exact(self) -> None:
        suite, attachments = self.persona_attempt_attachments()
        tests = {
            "devices": [],
            "testNodes": [
                {
                    "children": [
                        {
                            "name": "testLocalSocialDeterministicPersonas()",
                            "nodeIdentifier": fixture.PERSONA_XCRESULT_NODE_IDENTIFIER,
                            "nodeIdentifierURL": fixture.PERSONA_XCRESULT_NODE_URL,
                            "nodeType": "Test Case",
                            "result": "Passed",
                        }
                    ],
                    "name": "Radroots",
                    "nodeType": "Test Plan",
                    "result": "Passed",
                }
            ],
            "testPlanConfigurations": [],
        }
        self.assertEqual(
            fixture.exact_persona_test_node(tests)["result"], "Passed"
        )
        for mutation in (
            lambda value: value["testNodes"][0]["children"][0].update(
                {"result": "Failed"}
            ),
            lambda value: value["testNodes"][0]["children"][0].update(
                {"nodeIdentifierURL": "test://wrong"}
            ),
            lambda value: value["testNodes"].append(
                copy.deepcopy(value["testNodes"][0])
            ),
        ):
            changed = copy.deepcopy(tests)
            mutation(changed)
            with self.assertRaises(ValueError):
                fixture.exact_persona_test_node(changed)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            rows = []
            for name, (raw, _) in zip(
                fixture.PERSONA_ATTACHMENT_NAMES, attachments, strict=True
            ):
                exported = "exported-" + name
                (root / exported).write_bytes(raw)
                rows.append(
                    {
                        "exportedFileName": exported,
                        "suggestedHumanReadableName": name,
                        "isAssociatedWithFailure": False,
                        "configurationName": "Test Scheme Action",
                        "deviceName": "iPhone 17 Pro",
                        "deviceId": "11111111-2222-3333-4444-555555555555",
                    }
                )
            manifest = [
                {
                    "testIdentifier": fixture.PERSONA_XCRESULT_NODE_IDENTIFIER,
                    "testIdentifierURL": fixture.PERSONA_XCRESULT_NODE_URL,
                    "attachments": rows,
                }
            ]
            (root / "manifest.json").write_text(
                json.dumps(manifest), encoding="utf-8"
            )
            loaded = fixture.load_exported_persona_attachments(
                root, suite, require_measured_network=True
            )
            self.assertEqual(len(loaded), 15)
            self.assertEqual(loaded[0][1]["attempt_id"], "P01-A01")

            unexpected = root / "unexpected.json"
            unexpected.write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "entry bound"):
                fixture.load_exported_persona_attachments(
                    root, suite, require_measured_network=True
                )
            unexpected.unlink()

            rows[0]["suggestedHumanReadableName"] = rows[1][
                "suggestedHumanReadableName"
            ]
            (root / "manifest.json").write_text(
                json.dumps(manifest), encoding="utf-8"
            )
            with self.assertRaises(ValueError):
                fixture.load_exported_persona_attachments(
                    root, suite, require_measured_network=True
                )

    def test_xcresult_attachment_read_rejects_maximum_plus_one(self) -> None:
        suite, attachments = self.persona_attempt_attachments()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            name = fixture.PERSONA_ATTACHMENT_NAMES[0]
            exported = "oversized.json"
            (root / exported).write_bytes(
                b"{" + b" " * fixture.MAX_PERSONA_ATTACHMENT_BYTES + b"}"
            )
            rows = [
                {
                    "exportedFileName": exported,
                    "suggestedHumanReadableName": name,
                    "isAssociatedWithFailure": False,
                    "configurationName": "Test Scheme Action",
                    "deviceName": "iPhone 17 Pro",
                    "deviceId": "11111111-2222-3333-4444-555555555555",
                }
            ]
            for index, other_name in enumerate(
                fixture.PERSONA_ATTACHMENT_NAMES[1:], 1
            ):
                other_exported = f"exported-{index}.json"
                (root / other_exported).write_bytes(attachments[index][0])
                rows.append(
                    {
                        "exportedFileName": other_exported,
                        "suggestedHumanReadableName": other_name,
                        "isAssociatedWithFailure": False,
                        "configurationName": "Test Scheme Action",
                        "deviceName": "iPhone 17 Pro",
                        "deviceId": "11111111-2222-3333-4444-555555555555",
                    }
                )
            (root / "manifest.json").write_text(
                json.dumps(
                    [
                        {
                            "testIdentifier": fixture.PERSONA_XCRESULT_NODE_IDENTIFIER,
                            "testIdentifierURL": fixture.PERSONA_XCRESULT_NODE_URL,
                            "attachments": rows,
                        }
                    ]
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "byte bound"):
                fixture.load_exported_persona_attachments(
                    root, suite, require_measured_network=True
                )

    def test_simulator_metadata_uses_exact_result_bundle_device(self) -> None:
        udid = "11111111-2222-3333-4444-555555555555"
        result_bundle = Path("result.xcresult")
        simctl_devices = {
            "devices": {
                "com.apple.CoreSimulator.SimRuntime.iOS-26-5": [
                    {
                        "udid": udid,
                        "isAvailable": True,
                        "name": "iPhone 17 Pro",
                    }
                ]
            }
        }
        result_summary = {
            "failedTests": 0,
            "passedTests": 1,
            "result": "Passed",
            "skippedTests": 0,
            "devicesAndConfigurations": [
                {
                    "device": {
                        "architecture": "arm64",
                        "deviceId": udid,
                        "osVersion": "26.5",
                        "platform": "iOS Simulator",
                    }
                }
            ],
        }

        with mock.patch.object(
            fixture,
            "run_json_command_bounded",
            side_effect=[simctl_devices, result_summary],
        ) as run_json:
            self.assertEqual(
                fixture.simulator_metadata(udid.lower(), result_bundle),
                {"udid": udid, "os": "iOS 26.5", "architecture": "arm64"},
            )
        commands = [call.args[0] for call in run_json.call_args_list]
        self.assertEqual(commands[0][:3], ["xcrun", "simctl", "list"])
        self.assertEqual(commands[1][:3], ["xcrun", "xcresulttool", "get"])
        self.assertNotIn("spawn", commands[0] + commands[1])

        for mutation in (
            lambda value: value["devicesAndConfigurations"][0]["device"].update(
                {"architecture": "x86_64"}
            ),
            lambda value: value["devicesAndConfigurations"][0]["device"].update(
                {"deviceId": "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"}
            ),
            lambda value: value["devicesAndConfigurations"][0]["device"].update(
                {"osVersion": "17.7"}
            ),
            lambda value: value["devicesAndConfigurations"][0]["device"].update(
                {"platform": "macOS"}
            ),
            lambda value: value.update({"result": "Failed", "failedTests": 1}),
        ):
            changed = copy.deepcopy(result_summary)
            mutation(changed)
            with (
                mock.patch.object(
                    fixture,
                    "run_json_command_bounded",
                    side_effect=[simctl_devices, changed],
                ),
                self.assertRaises(ValueError),
            ):
                fixture.simulator_metadata(udid, result_bundle)

    def test_subprocess_json_is_bounded_before_decoding(self) -> None:
        command = [
            fixture.sys.executable,
            "-c",
            "import sys; sys.stdout.write('{\\\"x\\\":1}')",
        ]
        self.assertEqual(fixture.run_json_command_bounded(command, 7), {"x": 1})
        with self.assertRaisesRegex(ValueError, "byte bound"):
            fixture.run_json_command_bounded(command, 6)

    def test_verifier_toolchain_identity_is_exact(self) -> None:
        fixture.verify_toolchain_identity()
        with (
            mock.patch.object(fixture.sys, "version_info", (3, 14, 6)),
            self.assertRaisesRegex(RuntimeError, "Python identity"),
        ):
            fixture.verify_toolchain_identity()
        with (
            mock.patch.object(
                fixture.importlib.metadata, "version", return_value="4.25.1"
            ),
            self.assertRaisesRegex(RuntimeError, "schema dependency"),
        ):
            fixture.verify_toolchain_identity()

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

    def test_observable_loopback_factory_counts_and_rejects_connections(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            state = fixture.FixtureState(root / "evidence.json", root / "control", 0)
            server = fixture.LoopbackConnectionFactory.relay(0, state)
            try:
                self.assertTrue(
                    server.verify_request(mock.Mock(), ("127.0.0.1", 20_000))
                )
                self.assertFalse(
                    server.verify_request(mock.Mock(), ("192.0.2.1", 20_001))
                )
            finally:
                server.server_close()
            _, evidence = fixture.read_json(root / "evidence.json")
            self.assertEqual(evidence["accepted_connections"], 1)
            self.assertEqual(evidence["rejected_connections"], 1)
            self.assertEqual(evidence["non_loopback_attempts"], 1)
            self.assertEqual(evidence["production_network_contacts"], 1)

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
