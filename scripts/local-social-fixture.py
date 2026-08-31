#!/usr/bin/env python3
"""Bounded loopback Nostr and Blossom fixture for iOS simulator tests."""

from __future__ import annotations

import argparse
import base64
import hashlib
import http.server
import ipaddress
import json
import os
import re
import signal
import socket
import socketserver
import subprocess
import struct
import tempfile
import threading
import time
import unicodedata
from pathlib import Path
from typing import Any

MAX_HTTP_BODY = 16 * 1024 * 1024
MAX_WEBSOCKET_MESSAGE = 2 * 1024 * 1024
MAX_EVENTS = 256
MAX_BLOBS = 16
MAX_JSON_BYTES = 64 * 1024
BUD11_EVENT_KIND = 24_242
BUD11_CONTENT_MAX_BYTES = 4_096
BUD11_AUTHORIZATION_MAX_BYTES = 16 * 1024
BUD11_AUTHORIZATION_ENCODED_MAX_BYTES = 21_846
BUD11_MAX_LIFETIME_SECONDS = 300
BUD11_MAX_CREATED_AGE_SECONDS = 300
BUD11_SERVER_DOMAIN = "127.0.0.1"
BUD11_MUTATION_SCHEMA = "radroots.ios.local-social.bud11-mutations.v1"
BUD11_MUTATIONS = (
    ("canonical", "none", "http", True),
    ("wrong-scheme", "authorization_scheme", "http", False),
    ("padded-base64", "padded_base64", "http", False),
    ("wrong-kind", "wrong_kind", "http", False),
    ("empty-content", "empty_content", "http", False),
    ("oversized-content", "oversized_content", "http", False),
    ("leading-content-space", "leading_content_space", "http", False),
    ("control-content", "control_content", "http", False),
    ("missing-action", "missing_action", "http", False),
    ("wrong-action", "wrong_action", "http", False),
    ("duplicate-action", "duplicate_action", "http", False),
    ("wrong-hash", "wrong_hash", "http", False),
    ("duplicate-hash", "duplicate_hash", "http", False),
    ("missing-server", "missing_server", "http", False),
    ("wrong-server", "wrong_server", "http", False),
    ("uppercase-server", "uppercase_server", "http", False),
    ("duplicate-server", "duplicate_server", "http", False),
    ("missing-expiration", "missing_expiration", "http", False),
    ("noncanonical-expiration", "noncanonical_expiration", "http", False),
    ("expired", "expired", "http", False),
    ("created-at-not-past", "created_at_not_past", "http", False),
    ("lifetime-too-long", "lifetime_too_long", "http", False),
    ("unknown-tag", "unknown_tag", "http", False),
    ("event-id-mutation", "event_id", "http", False),
    ("signature-mutation", "signature", "http", False),
    ("relay-publication", "none", "relay", False),
)
PERSONA_ALIASES = ("P01", "P02", "P03", "P04", "P05")
FLOW_KINDS = {
    "Update": 1,
    "PhotoUpdate": 1,
    "Ask": 1,
    "Event": 31923,
    "FoodAvailability": 30402,
}
PHOTO_PERSONAS = frozenset(("P01", "P03", "P04"))
PERSONA_CONTROL_SCHEMA = "radroots.ios.local-social.persona-control.v1"
PERSONA_ATTEMPT_SCHEMA = "radroots.ios.local-social.persona-attempt-evidence.v1"
PERSONA_RESULT_V2_SCHEMA = "radroots.ios.local-social.persona-results.v2"
PERSONA_TEST_TARGET = "RadrootsUITests"
PERSONA_TEST_IDENTIFIER = "RadrootsUITests/testLocalSocialDeterministicPersonas"
PERSONA_TEST_ACTION = "test"
PERSONA_TEST_CONFIGURATION = "Debug"
PERSONA_XCRESULT_NODE_IDENTIFIER = (
    "RadrootsRemoteQualificationUITests/testLocalSocialDeterministicPersonas()"
)
PERSONA_XCRESULT_NODE_URL = (
    "test://com.apple.xcode/Radroots/RadrootsUITests/"
    "RadrootsRemoteQualificationUITests/testLocalSocialDeterministicPersonas"
)
MAX_XCRESULT_JSON_BYTES = 1024 * 1024
MAX_PERSONA_ATTACHMENT_BYTES = 64 * 1024
MAX_PERSONA_ATTACHMENTS_BYTES = 15 * MAX_PERSONA_ATTACHMENT_BYTES
PERSONA_ATTACHMENT_NAMES = tuple(
    f"radroots-local-social-P{persona:02d}-A{attempt:02d}.json"
    for persona in range(1, 6)
    for attempt in range(1, 4)
)
FORBIDDEN_EVIDENCE_KEYS = frozenset(
    ("private_key", "secret", "seed", "signed_event", "event_content", "raw_event")
)


def strict_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate JSON member")
        value[key] = item
    return value


def read_json(path: Path, maximum: int = MAX_JSON_BYTES) -> tuple[bytes, Any]:
    raw = path.read_bytes()
    if not raw or len(raw) > maximum:
        raise ValueError("JSON input is empty or exceeds its byte bound")
    value = json.loads(raw, object_pairs_hook=strict_object)
    return raw, value


def read_json_bounded(path: Path, maximum: int) -> tuple[bytes, Any]:
    with path.open("rb") as stream:
        raw = stream.read(maximum + 1)
    if not raw or len(raw) > maximum:
        raise ValueError("JSON input is empty or exceeds its byte bound")
    value = json.loads(raw, object_pairs_hook=strict_object)
    return raw, value


def exact_keys(value: Any, keys: set[str], name: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError(f"{name} has an invalid field inventory")
    return value


def validate_persona_suite(value: Any) -> dict[str, Any]:
    root = exact_keys(
        value,
        {"schema", "schema_version", "locale", "media_fixture_sha256", "personas"},
        "persona suite",
    )
    if (
        root["schema"] != "radroots.ios.local-social.personas.v1"
        or root["schema_version"] != 1
        or root["locale"] != "en_US"
        or not lowercase_hex(root["media_fixture_sha256"], 64)
        or not isinstance(root["personas"], list)
        or len(root["personas"]) != 5
    ):
        raise ValueError("persona suite header is invalid")
    expected = (
        ("P01", "age_18_27", "experienced_direct", ("Update", "PhotoUpdate", "Ask")),
        (
            "P02",
            "age_18_27",
            "novice_progressive_disclosure",
            ("Event", "FoodAvailability", "Update"),
        ),
        (
            "P03",
            "age_18_27",
            "nontechnical_validation_recovery",
            ("PhotoUpdate", "Ask", "Event"),
        ),
        (
            "P04",
            "adult_other",
            "novice_accessibility_keyboard",
            ("FoodAvailability", "Update", "PhotoUpdate"),
        ),
        (
            "P05",
            "adult_other",
            "general_transport_retry_relaunch",
            ("Ask", "Event", "FoodAvailability"),
        ),
    )
    attempts: list[dict[str, Any]] = []
    for persona_index, (persona_value, expected_value) in enumerate(
        zip(root["personas"], expected, strict=True), 1
    ):
        persona = exact_keys(
            persona_value,
            {"alias", "synthetic_age_band", "interaction_profile", "attempts"},
            "persona",
        )
        alias, age_band, profile, flows = expected_value
        if (
            persona["alias"] != alias
            or persona["synthetic_age_band"] != age_band
            or persona["interaction_profile"] != profile
            or not isinstance(persona["attempts"], list)
            or len(persona["attempts"]) != 3
        ):
            raise ValueError("persona matrix is not exact")
        for attempt_index, (attempt_value, flow) in enumerate(
            zip(persona["attempts"], flows, strict=True), 1
        ):
            attempt = exact_keys(
                attempt_value,
                {"id", "order", "flow", "marker", "expected_failure"},
                "attempt",
            )
            expected_id = f"{alias}-A{attempt_index:02d}"
            expected_order = (persona_index - 1) * 3 + attempt_index
            if (
                attempt["id"] != expected_id
                or attempt["order"] != expected_order
                or attempt["flow"] != flow
                or not isinstance(attempt["marker"], str)
                or not 1 <= len(attempt["marker"].encode("ascii")) <= 24
                or attempt["marker"]
                != f"rr-{alias.lower()}-a{attempt_index:02d}-{flow_marker(flow)}"
                or attempt["expected_failure"]
                not in {"none", "validation_recovery", "transport_retry_relaunch"}
            ):
                raise ValueError("persona attempt is not exact")
            attempts.append(attempt)
    if (
        [item["expected_failure"] for item in attempts].count("validation_recovery")
        != 1
    ):
        raise ValueError("persona suite must contain one validation-recovery vector")
    if (
        [item["expected_failure"] for item in attempts].count(
            "transport_retry_relaunch"
        )
        != 1
    ):
        raise ValueError("persona suite must contain one transport-retry vector")
    if any(
        sum(item["flow"] == flow for item in attempts) != 3 for flow in FLOW_KINDS
    ):
        raise ValueError("each Add flow must have exactly three attempts")
    return root


def validate_bud11_mutation_corpus(value: Any) -> dict[str, Any]:
    root = exact_keys(value, {"schema", "schema_version", "vectors"}, "BUD-11 corpus")
    if (
        root["schema"] != BUD11_MUTATION_SCHEMA
        or root["schema_version"] != 1
        or not isinstance(root["vectors"], list)
    ):
        raise ValueError("BUD-11 corpus header is invalid")
    observed = []
    for value in root["vectors"]:
        vector = exact_keys(
            value,
            {"id", "mutation", "surface", "expected_accepted"},
            "BUD-11 vector",
        )
        if (
            not isinstance(vector["id"], str)
            or not isinstance(vector["mutation"], str)
            or vector["surface"] not in {"http", "relay"}
            or type(vector["expected_accepted"]) is not bool
        ):
            raise ValueError("BUD-11 vector is invalid")
        observed.append(
            (
                vector["id"],
                vector["mutation"],
                vector["surface"],
                vector["expected_accepted"],
            )
        )
    if tuple(observed) != BUD11_MUTATIONS:
        raise ValueError("BUD-11 corpus inventory is invalid")
    return root


def load_bud11_mutation_corpus(path: Path) -> tuple[bytes, dict[str, Any]]:
    raw, value = read_json(path)
    corpus = validate_bud11_mutation_corpus(value)
    canonical = (
        "{\n"
        f'  "schema": {json.dumps(value.get("schema"))},\n'
        f'  "schema_version": {json.dumps(value.get("schema_version"))},\n'
        '  "vectors": [\n'
        + ",\n".join(
            "    " + json.dumps(vector, ensure_ascii=False)
            for vector in value.get("vectors", [])
        )
        + "\n  ]\n}\n"
    ).encode("utf-8")
    if raw != canonical:
        raise ValueError("BUD-11 corpus is noncanonical")
    return raw, corpus


def flow_marker(flow: str) -> str:
    return {
        "Update": "update",
        "PhotoUpdate": "photo",
        "Ask": "ask",
        "Event": "event",
        "FoodAvailability": "food",
    }[flow]


def load_persona_suite(path: Path) -> tuple[bytes, dict[str, Any]]:
    raw, value = read_json(path)
    return raw, validate_persona_suite(value)


def validate_schema_file(path: Path, expected_id: str) -> bytes:
    raw, value = read_json(path)
    root = exact_keys(value, set(value), "schema")
    if (
        root.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
        or root.get("$id") != expected_id
        or root.get("type") != "object"
        or root.get("additionalProperties") is not False
    ):
        raise ValueError("schema boundary is invalid")
    return raw


def persona_attempts(suite: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {
        attempt["id"]: {**attempt, "persona": persona["alias"]}
        for persona in suite["personas"]
        for attempt in persona["attempts"]
    }


def read_control(path: Path) -> dict[str, Any] | None:
    if not path.is_file():
        return None
    try:
        _, value = read_json(path, 1024)
        control = exact_keys(
            value,
            {"schema", "active_persona", "blossom_enabled"},
            "persona control",
        )
    except (OSError, UnicodeError, ValueError, json.JSONDecodeError):
        return None
    if (
        control["schema"] != PERSONA_CONTROL_SCHEMA
        or control["active_persona"] not in PERSONA_ALIASES
        or type(control["blossom_enabled"]) is not bool
    ):
        return None
    return control


class FixtureState:
    def __init__(
        self,
        evidence: Path,
        control: Path,
        blossom_port: int,
        suite: dict[str, Any] | None = None,
    ) -> None:
        self._evidence = evidence
        self.control = control
        self.blossom_port = blossom_port
        self._suite = suite
        self._attempts = persona_attempts(suite) if suite is not None else {}
        self._lock = threading.Lock()
        self._events: dict[str, dict[str, Any]] = {}
        self._blobs: dict[str, tuple[bytes, str]] = {}
        self._upload_attempts = 0
        self._accepted_uploads = 0
        self._retrievals = 0
        self._subscriptions = 0
        self._subscriptions_by_persona = {alias: 0 for alias in PERSONA_ALIASES}
        self._accepted_attempts: dict[str, dict[str, Any]] = {}
        self._identity_by_persona: dict[str, str] = {}
        self._persona_by_identity: dict[str, str] = {}
        self._accepted_uploads_by_persona = {alias: 0 for alias in PERSONA_ALIASES}
        self._uploaded_digests_by_persona = {
            alias: set() for alias in PERSONA_ALIASES
        }
        self._retrievals_by_persona = {alias: set() for alias in PERSONA_ALIASES}
        self._unknown_attempts = 0
        self._duplicate_attempts = 0
        self._expected_failure_rejections = 0
        self._transport_rejected_attempts: set[str] = set()
        self._events_accepted_during_expected_failures = 0
        self._accepted_connections = 0
        self._rejected_connections = 0
        self._non_loopback_attempts = 0
        self._production_network_contacts = 0
        self._unintended_publications = 0
        self._write_evidence()

    def observe_connection(self, host: str) -> bool:
        try:
            permitted = ipaddress.ip_address(host).is_loopback
        except ValueError:
            permitted = False
        with self._lock:
            if permitted:
                self._accepted_connections += 1
            else:
                self._rejected_connections += 1
                self._non_loopback_attempts += 1
                self._production_network_contacts += 1
            self._write_evidence_locked()
        return permitted

    def publish(self, event: dict[str, Any]) -> bool | None:
        if not valid_nostr_event(event) or not verify_nostr_signature(event):
            return False
        if event["kind"] == BUD11_EVENT_KIND:
            if self._suite is not None:
                with self._lock:
                    self._unintended_publications += 1
                    self._write_evidence_locked()
            return False
        if self._suite is not None:
            return self._publish_persona_event(event)
        event_id = event["id"]
        with self._lock:
            if event_id not in self._events and len(self._events) >= MAX_EVENTS:
                return False
            self._events[event_id] = event
            self._write_evidence_locked()
        return True

    def _publish_persona_event(self, event: dict[str, Any]) -> bool | None:
        attempt = classify_attempt(event, self._attempts)
        control = read_control(self.control)
        with self._lock:
            if attempt is None or control is None:
                self._unknown_attempts += 1
                self._unintended_publications += 1
                self._write_evidence_locked()
                return False
            attempt_id = attempt["id"]
            persona = attempt["persona"]
            if control["active_persona"] != persona:
                self._unknown_attempts += 1
                self._unintended_publications += 1
                self._write_evidence_locked()
                return False
            if event["kind"] != FLOW_KINDS[attempt["flow"]]:
                self._unintended_publications += 1
                self._write_evidence_locked()
                return False
            public_key = event["pubkey"]
            existing_identity = self._identity_by_persona.get(persona)
            existing_persona = self._persona_by_identity.get(public_key)
            if (
                (existing_identity is not None and existing_identity != public_key)
                or (existing_persona is not None and existing_persona != persona)
            ):
                self._unintended_publications += 1
                self._write_evidence_locked()
                return False
            self._identity_by_persona[persona] = public_key
            self._persona_by_identity[public_key] = persona
            if (
                attempt["expected_failure"] == "transport_retry_relaunch"
                and attempt_id not in self._transport_rejected_attempts
            ):
                self._transport_rejected_attempts.add(attempt_id)
                self._expected_failure_rejections += 1
                self._write_evidence_locked()
                return None
            if attempt_id in self._accepted_attempts:
                self._duplicate_attempts += 1
                self._write_evidence_locked()
                return False
            if len(self._events) >= MAX_EVENTS:
                return False
            self._events[event["id"]] = event
            self._accepted_attempts[attempt_id] = {
                "id": attempt_id,
                "flow": attempt["flow"],
                "event_kind": event["kind"],
                "accepted": True,
                "expected_failure_rejections": int(
                    attempt_id in self._transport_rejected_attempts
                ),
            }
            self._write_evidence_locked()
        return True

    def query(self, filters: list[dict[str, Any]]) -> list[dict[str, Any]]:
        control = read_control(self.control)
        with self._lock:
            self._subscriptions += 1
            if control is not None:
                self._subscriptions_by_persona[control["active_persona"]] += 1
            events = list(self._events.values())
            self._write_evidence_locked()
        selected = [
            event for event in events if any(matches(event, item) for item in filters)
        ]
        selected.sort(key=lambda item: (item.get("created_at", 0), item.get("id", "")))
        limits = [
            item["limit"]
            for item in filters
            if isinstance(item, dict) and type(item.get("limit")) is int
        ]
        count = min(max(max(limits, default=len(selected)), 0), MAX_EVENTS)
        return selected[-count:] if count else []

    def upload(
        self,
        body: bytes,
        media_type: str,
        expected_hash: str,
        authorization: str,
    ) -> tuple[bool, dict[str, Any]]:
        digest = hashlib.sha256(body).hexdigest()
        control = read_control(self.control)
        with self._lock:
            self._upload_attempts += 1
            persona = control["active_persona"] if control is not None else None
            if self._suite is None:
                allowed = self.control.is_file() and valid_blossom_authorization(
                    authorization, expected_hash, BUD11_SERVER_DOMAIN
                )
            else:
                allowed = (
                    control is not None
                    and control["blossom_enabled"]
                    and persona in PHOTO_PERSONAS
                    and valid_blossom_authorization(
                        authorization, expected_hash, BUD11_SERVER_DOMAIN
                    )
                )
            capacity = digest in self._blobs or len(self._blobs) < MAX_BLOBS
            if allowed and capacity and digest == expected_hash:
                self._blobs[digest] = (body, media_type)
                self._accepted_uploads += 1
                if persona is not None:
                    self._accepted_uploads_by_persona[persona] += 1
                    self._uploaded_digests_by_persona[persona].add(digest)
            self._write_evidence_locked()
        descriptor = {
            "url": f"http://127.0.0.1:{self.blossom_port}/{digest}.png",
            "sha256": digest,
            "size": len(body),
            "type": media_type,
            "uploaded": int(time.time()),
        }
        return allowed and capacity and digest == expected_hash, descriptor

    def retrieve(self, digest: str) -> tuple[bytes, str] | None:
        control = read_control(self.control)
        with self._lock:
            value = self._blobs.get(digest)
            if value is not None:
                if self._suite is None:
                    self._retrievals += 1
                elif control is not None:
                    persona = control["active_persona"]
                    if digest in self._uploaded_digests_by_persona[persona]:
                        before = len(self._retrievals_by_persona[persona])
                        self._retrievals_by_persona[persona].add(digest)
                        self._retrievals += (
                            len(self._retrievals_by_persona[persona]) - before
                        )
                self._write_evidence_locked()
            return value

    def _write_evidence(self) -> None:
        with self._lock:
            self._write_evidence_locked()

    def _write_evidence_locked(self) -> None:
        if self._suite is None:
            payload = {
                "schema": "radroots-ios-local-social-fixture-evidence-v1",
                "schema_version": 1,
                "accepted_events": len(self._events),
                "event_kinds": sorted(
                    event["kind"]
                    for event in self._events.values()
                    if isinstance(event.get("kind"), int)
                ),
                "subscriptions": self._subscriptions,
                "upload_attempts": self._upload_attempts,
                "accepted_uploads": self._accepted_uploads,
                "retrievals": self._retrievals,
                "accepted_connections": self._accepted_connections,
                "rejected_connections": self._rejected_connections,
                "non_loopback_attempts": self._non_loopback_attempts,
                "production_network_contacts": self._production_network_contacts,
            }
        else:
            payload = self._persona_evidence()
        temporary = self._evidence.with_suffix(".tmp")
        temporary.write_text(
            json.dumps(payload, sort_keys=True) + "\n", encoding="utf-8"
        )
        os.replace(temporary, self._evidence)

    def _persona_evidence(self) -> dict[str, Any]:
        personas = []
        for persona in self._suite["personas"]:
            alias = persona["alias"]
            public_key = self._identity_by_persona.get(alias)
            personas.append(
                {
                    "alias": alias,
                    "identity_sha256": identity_digest(public_key),
                    "subscriptions": self._subscriptions_by_persona[alias],
                    "accepted_uploads": self._accepted_uploads_by_persona[alias],
                    "retrievals": len(self._retrievals_by_persona[alias]),
                    "attempts": [
                        self._accepted_attempts[attempt["id"]]
                        for attempt in persona["attempts"]
                        if attempt["id"] in self._accepted_attempts
                    ],
                }
            )
        kind_counts = {
            str(kind): sum(event["kind"] == kind for event in self._events.values())
            for kind in (1, 31923, 30402)
        }
        flow_counts = {
            flow: sum(item["flow"] == flow for item in self._accepted_attempts.values())
            for flow in FLOW_KINDS
        }
        return {
            "schema": "radroots.ios.local-social.persona-evidence.v1",
            "schema_version": 1,
            "personas": personas,
            "flow_counts": flow_counts,
            "accepted_events": len(self._events),
            "event_kind_counts": kind_counts,
            "upload_attempts": self._upload_attempts,
            "accepted_uploads": self._accepted_uploads,
            "retrievals": self._retrievals,
            "distinct_identities": len(self._persona_by_identity),
            "unknown_attempts": self._unknown_attempts,
            "duplicate_attempts": self._duplicate_attempts,
            "expected_failure_rejections": self._expected_failure_rejections,
            "events_accepted_during_expected_failures": (
                self._events_accepted_during_expected_failures
            ),
            "accepted_connections": self._accepted_connections,
            "rejected_connections": self._rejected_connections,
            "non_loopback_attempts": self._non_loopback_attempts,
            "production_network_contacts": self._production_network_contacts,
            "unintended_publications": self._unintended_publications,
            "final_candidate_data_loss": 0,
        }


def matches(event: dict[str, Any], item: dict[str, Any]) -> bool:
    if not isinstance(item, dict):
        return False
    if isinstance(item.get("ids"), list) and event.get("id") not in item["ids"]:
        return False
    if (
        isinstance(item.get("authors"), list)
        and event.get("pubkey") not in item["authors"]
    ):
        return False
    if isinstance(item.get("kinds"), list) and event.get("kind") not in item["kinds"]:
        return False
    created_at = event.get("created_at")
    if not isinstance(created_at, int):
        return False
    if isinstance(item.get("since"), int) and created_at < item["since"]:
        return False
    if isinstance(item.get("until"), int) and created_at > item["until"]:
        return False
    tags = event.get("tags")
    if not isinstance(tags, list):
        return False
    for key, expected in item.items():
        if not key.startswith("#") or not isinstance(expected, list):
            continue
        name = key[1:]
        if not any(
            isinstance(tag, list)
            and len(tag) >= 2
            and tag[0] == name
            and tag[1] in expected
            for tag in tags
        ):
            return False
    return True


def valid_nostr_event(event: dict[str, Any]) -> bool:
    if not isinstance(event, dict) or set(event) != {
        "id",
        "pubkey",
        "created_at",
        "kind",
        "tags",
        "content",
        "sig",
    }:
        return False
    if not lowercase_hex(event["id"], 64) or not lowercase_hex(event["pubkey"], 64):
        return False
    if not lowercase_hex(event["sig"], 128):
        return False
    if type(event["created_at"]) is not int or event["created_at"] < 0:
        return False
    if type(event["kind"]) is not int or not 0 <= event["kind"] <= 0xFFFF_FFFF:
        return False
    if not isinstance(event["content"], str) or not isinstance(event["tags"], list):
        return False
    if not all(
        isinstance(tag, list) and all(isinstance(value, str) for value in tag)
        for tag in event["tags"]
    ):
        return False
    preimage = json.dumps(
        [
            0,
            event["pubkey"],
            event["created_at"],
            event["kind"],
            event["tags"],
            event["content"],
        ],
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(preimage).hexdigest() == event["id"]


def lowercase_hex(value: Any, length: int) -> bool:
    return (
        isinstance(value, str)
        and len(value) == length
        and all(character in "0123456789abcdef" for character in value)
    )


def classify_attempt(
    event: dict[str, Any], attempts: dict[str, dict[str, Any]]
) -> dict[str, Any] | None:
    values = [event.get("content")]
    for tag in event.get("tags", []):
        values.extend(tag)
    matches = [
        attempt
        for attempt in attempts.values()
        if (
            photo_attempt_matches(event, attempt)
            if attempt.get("flow") == "PhotoUpdate"
            else attempt["marker"] in values
        )
    ]
    return matches[0] if len(matches) == 1 else None


def photo_attempt_matches(event: dict[str, Any], attempt: dict[str, Any]) -> bool:
    if attempt.get("flow") != "PhotoUpdate":
        return False
    content = event.get("content")
    if not isinstance(content, str):
        return False
    lines = content.split("\n")
    if len(lines) != 2 or lines[0] != attempt.get("marker"):
        return False
    url = lines[1]
    match = re.fullmatch(
        r"http://127\.0\.0\.1:([1-9][0-9]{0,4})/([0-9a-f]{64})\.png", url
    )
    if match is None or int(match.group(1)) > 65535:
        return False
    digest = match.group(2)
    imeta = [
        tag
        for tag in event.get("tags", [])
        if isinstance(tag, list) and tag[:1] == ["imeta"]
    ]
    return len(imeta) == 1 and all(
        field in imeta[0]
        for field in (f"url {url}", f"x {digest}", "m image/png")
    )


def identity_digest(public_key: str | None) -> str:
    if public_key is None:
        return "0" * 64
    return hashlib.sha256(
        b"radroots.ios.local-social.persona-identity.v1\0"
        + bytes.fromhex(public_key)
    ).hexdigest()


SECP256K1_FIELD = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F
SECP256K1_ORDER = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
SECP256K1_GENERATOR = (
    0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798,
    0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8,
)


def point_add(
    left: tuple[int, int] | None, right: tuple[int, int] | None
) -> tuple[int, int] | None:
    if left is None:
        return right
    if right is None:
        return left
    if left[0] == right[0] and left[1] != right[1]:
        return None
    if left == right:
        if left[1] == 0:
            return None
        slope = (3 * left[0] * left[0]) * pow(2 * left[1], -1, SECP256K1_FIELD)
    else:
        slope = (right[1] - left[1]) * pow(
            right[0] - left[0], -1, SECP256K1_FIELD
        )
    slope %= SECP256K1_FIELD
    x = (slope * slope - left[0] - right[0]) % SECP256K1_FIELD
    y = (slope * (left[0] - x) - left[1]) % SECP256K1_FIELD
    return x, y


def point_multiply(value: int, point: tuple[int, int]) -> tuple[int, int] | None:
    result = None
    current: tuple[int, int] | None = point
    while value:
        if value & 1:
            result = point_add(result, current)
        current = point_add(current, current)
        value >>= 1
    return result


def tagged_hash(tag: str, payload: bytes) -> bytes:
    tag_hash = hashlib.sha256(tag.encode("ascii")).digest()
    return hashlib.sha256(tag_hash + tag_hash + payload).digest()


def verify_bip340(public_key: bytes, message: bytes, signature: bytes) -> bool:
    if len(public_key) != 32 or len(message) != 32 or len(signature) != 64:
        return False
    x = int.from_bytes(public_key, "big")
    r = int.from_bytes(signature[:32], "big")
    s = int.from_bytes(signature[32:], "big")
    if x >= SECP256K1_FIELD or r >= SECP256K1_FIELD or s >= SECP256K1_ORDER:
        return False
    y_squared = (pow(x, 3, SECP256K1_FIELD) + 7) % SECP256K1_FIELD
    y = pow(y_squared, (SECP256K1_FIELD + 1) // 4, SECP256K1_FIELD)
    if pow(y, 2, SECP256K1_FIELD) != y_squared:
        return False
    if y & 1:
        y = SECP256K1_FIELD - y
    challenge = int.from_bytes(
        tagged_hash("BIP0340/challenge", signature[:32] + public_key + message),
        "big",
    ) % SECP256K1_ORDER
    negative = (x, (-y) % SECP256K1_FIELD)
    candidate = point_add(
        point_multiply(s, SECP256K1_GENERATOR),
        point_multiply(challenge, negative),
    )
    return candidate is not None and candidate[1] % 2 == 0 and candidate[0] == r


def verify_nostr_signature(event: dict[str, Any]) -> bool:
    try:
        return verify_bip340(
            bytes.fromhex(event["pubkey"]),
            bytes.fromhex(event["id"]),
            bytes.fromhex(event["sig"]),
        )
    except (KeyError, TypeError, ValueError):
        return False


def valid_bud11_server_domain(value: Any) -> bool:
    if (
        not isinstance(value, str)
        or not value
        or len(value) > 253
        or not value.isascii()
        or value.lower() != value
    ):
        return False
    try:
        address = ipaddress.ip_address(value)
    except ValueError:
        labels = value.split(".")
        return not all(label.isdigit() for label in labels) and all(
            1 <= len(label) <= 63
            and re.fullmatch(r"[a-z0-9](?:[a-z0-9-]*[a-z0-9])?", label)
            is not None
            for label in labels
        )
    return isinstance(address, ipaddress.IPv4Address) and str(address) == value


def valid_bud11_content(value: Any) -> bool:
    return (
        isinstance(value, str)
        and value
        and len(value.encode("utf-8")) <= BUD11_CONTENT_MAX_BYTES
        and value.strip() == value
        and not any(
            unicodedata.category(character) == "Cc"
            and character not in "\t\n\r"
            for character in value
        )
    )


def canonical_unsigned_decimal(value: Any) -> int | None:
    if (
        not isinstance(value, str)
        or not value
        or len(value) > 20
        or any(character not in "0123456789" for character in value)
        or (len(value) > 1 and value.startswith("0"))
    ):
        return None
    parsed = int(value)
    return parsed if parsed <= 0xFFFF_FFFF_FFFF_FFFF else None


def valid_blossom_authorization(
    authorization: str,
    expected_hash: str,
    expected_server: str,
    now_unix_s: int | None = None,
) -> bool:
    if not lowercase_hex(expected_hash, 64) or not valid_bud11_server_domain(
        expected_server
    ):
        return False
    if not authorization.startswith("Nostr "):
        return False
    payload = authorization.removeprefix("Nostr ")
    if (
        not payload
        or len(payload) > BUD11_AUTHORIZATION_ENCODED_MAX_BYTES
        or any(character.isspace() for character in payload)
        or "=" in payload
        or re.fullmatch(r"[A-Za-z0-9_-]+", payload) is None
    ):
        return False
    try:
        raw = base64.urlsafe_b64decode(payload + "=" * (-len(payload) % 4))
        if len(raw) > BUD11_AUTHORIZATION_MAX_BYTES:
            return False
        if base64.urlsafe_b64encode(raw).rstrip(b"=").decode("ascii") != payload:
            return False
        event = json.loads(raw, object_pairs_hook=strict_object)
    except (ValueError, json.JSONDecodeError):
        return False
    if not valid_nostr_event(event) or not verify_nostr_signature(event):
        return False
    if event["kind"] != BUD11_EVENT_KIND or not valid_bud11_content(event["content"]):
        return False
    if len(event["tags"]) != 4 or event["tags"][0] != ["t", "upload"]:
        return False
    expiration_tag = event["tags"][1]
    if (
        len(expiration_tag) != 2
        or expiration_tag[0] != "expiration"
        or event["tags"][2] != ["x", expected_hash]
        or event["tags"][3] != ["server", expected_server]
    ):
        return False
    expiration = canonical_unsigned_decimal(expiration_tag[1])
    now = int(time.time()) if now_unix_s is None else now_unix_s
    return (
        type(now) is int
        and now >= 0
        and expiration is not None
        and event["created_at"] < now < expiration
        and now - event["created_at"] <= BUD11_MAX_CREATED_AGE_SECONDS
        and 0 < expiration - event["created_at"] <= BUD11_MAX_LIFETIME_SECONDS
    )


class ObservableLoopbackServerMixin:
    _fixture_state: FixtureState

    def verify_request(
        self, request: socket.socket, client_address: tuple[str, int]
    ) -> bool:
        del request
        return self._fixture_state.observe_connection(client_address[0])


class ReusableThreadingServer(
    ObservableLoopbackServerMixin, socketserver.ThreadingTCPServer
):
    allow_reuse_address = True
    daemon_threads = True

    def __init__(
        self,
        server_address: tuple[str, int],
        handler: type[socketserver.BaseRequestHandler],
        state: FixtureState,
    ) -> None:
        self._fixture_state = state
        super().__init__(server_address, handler)


class ObservableLoopbackHTTPServer(
    ObservableLoopbackServerMixin, http.server.ThreadingHTTPServer
):
    def __init__(
        self,
        server_address: tuple[str, int],
        handler: type[http.server.BaseHTTPRequestHandler],
        state: FixtureState,
    ) -> None:
        self._fixture_state = state
        super().__init__(server_address, handler)


class LoopbackConnectionFactory:
    @staticmethod
    def relay(port: int, state: FixtureState) -> ReusableThreadingServer:
        return ReusableThreadingServer(("127.0.0.1", port), RelayHandler, state)

    @staticmethod
    def blossom(port: int, state: FixtureState) -> ObservableLoopbackHTTPServer:
        return ObservableLoopbackHTTPServer(
            ("127.0.0.1", port), BlossomHandler, state
        )


class RelayHandler(socketserver.BaseRequestHandler):
    state: FixtureState

    def handle(self) -> None:
        self.request.settimeout(15)
        headers = read_until(self.request, b"\r\n\r\n", 16 * 1024)
        lines = headers.decode("ascii").split("\r\n")
        if not lines or lines[0] != "GET / HTTP/1.1":
            return
        fields = {}
        for line in lines[1:]:
            if ":" in line:
                key, value = line.split(":", 1)
                fields[key.lower()] = value.strip()
        key = fields.get("sec-websocket-key")
        if not key or fields.get("upgrade", "").lower() != "websocket":
            return
        accept = base64.b64encode(
            hashlib.sha1(
                (key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()
            ).digest()
        ).decode()
        self.request.sendall(
            (
                "HTTP/1.1 101 Switching Protocols\r\n"
                "Upgrade: websocket\r\n"
                "Connection: Upgrade\r\n"
                f"Sec-WebSocket-Accept: {accept}\r\n\r\n"
            ).encode("ascii")
        )
        while True:
            frame = read_frame(self.request)
            if frame is None:
                return
            opcode, payload = frame
            if opcode == 0x8:
                return
            if opcode == 0x9:
                send_frame(self.request, 0xA, payload)
                continue
            if opcode != 0x1:
                continue
            try:
                message = json.loads(payload)
            except (UnicodeDecodeError, json.JSONDecodeError):
                continue
            if not isinstance(message, list) or not message:
                continue
            if (
                message[0] == "EVENT"
                and len(message) == 2
                and isinstance(message[1], dict)
            ):
                event_id = message[1].get("id", "")
                accepted = self.state.publish(message[1])
                if accepted is None:
                    return
                send_json(
                    self.request,
                    ["OK", event_id, accepted, "" if accepted else "invalid"],
                )
            elif (
                message[0] == "REQ"
                and len(message) >= 3
                and isinstance(message[1], str)
            ):
                subscription = message[1]
                filters = [value for value in message[2:] if isinstance(value, dict)]
                for event in self.state.query(filters):
                    send_json(self.request, ["EVENT", subscription, event])
                send_json(self.request, ["EOSE", subscription])


def read_until(stream: socket.socket, marker: bytes, maximum: int) -> bytes:
    value = bytearray()
    while marker not in value:
        chunk = stream.recv(4096)
        if not chunk:
            raise ConnectionError("socket closed")
        value.extend(chunk)
        if len(value) > maximum:
            raise ValueError("request too large")
    return bytes(value)


def read_exact(stream: socket.socket, length: int) -> bytes:
    value = bytearray()
    while len(value) < length:
        chunk = stream.recv(length - len(value))
        if not chunk:
            raise ConnectionError("socket closed")
        value.extend(chunk)
    return bytes(value)


def read_frame(stream: socket.socket) -> tuple[int, bytes] | None:
    try:
        first, second = read_exact(stream, 2)
    except (ConnectionError, OSError, TimeoutError):
        return None
    if first & 0x80 == 0 or second & 0x80 == 0:
        return None
    length = second & 0x7F
    if length == 126:
        length = struct.unpack("!H", read_exact(stream, 2))[0]
    elif length == 127:
        length = struct.unpack("!Q", read_exact(stream, 8))[0]
    if length > MAX_WEBSOCKET_MESSAGE:
        return None
    mask = read_exact(stream, 4)
    payload = bytearray(read_exact(stream, length))
    for index in range(length):
        payload[index] ^= mask[index % 4]
    return first & 0x0F, bytes(payload)


def send_frame(stream: socket.socket, opcode: int, payload: bytes) -> None:
    length = len(payload)
    header = bytearray([0x80 | opcode])
    if length < 126:
        header.append(length)
    elif length <= 0xFFFF:
        header.append(126)
        header.extend(struct.pack("!H", length))
    else:
        header.append(127)
        header.extend(struct.pack("!Q", length))
    stream.sendall(header + payload)


def send_json(stream: socket.socket, value: Any) -> None:
    send_frame(stream, 0x1, json.dumps(value, separators=(",", ":")).encode())


class BlossomHandler(http.server.BaseHTTPRequestHandler):
    state: FixtureState
    protocol_version = "HTTP/1.1"

    def do_PUT(self) -> None:  # noqa: N802
        component = self.path.removeprefix("/")
        if (
            not component.endswith(".png")
            or len(component) != 68
            or any(character not in "0123456789abcdef" for character in component[:-4])
        ):
            self.send_error(404)
            return
        try:
            length = int(self.headers.get("Content-Length", "-1"))
        except ValueError:
            length = -1
        authorization = self.headers.get("Authorization", "")
        expected_hash = component[:-4]
        media_type = self.headers.get("Content-Type", "")
        if (
            length < 1
            or length > MAX_HTTP_BODY
            or not authorization.startswith("Nostr ")
            or media_type != "image/png"
        ):
            self.send_error(400)
            return
        body = self.rfile.read(length)
        accepted, descriptor = self.state.upload(
            body, media_type, expected_hash, authorization
        )
        if not accepted:
            self.respond(503, b'{"error":"fixture_retry_required"}', "application/json")
            return
        self.respond(
            200,
            json.dumps(descriptor, separators=(",", ":")).encode(),
            "application/json",
        )

    def do_GET(self) -> None:  # noqa: N802
        component = self.path.removeprefix("/")
        if component.endswith(".png") and lowercase_hex(component[:-4], 64):
            value = self.state.retrieve(component[:-4])
            if value is not None:
                self.respond(200, value[0], value[1])
                return
        self.respond(404, b'{"error":"not_found"}', "application/json")

    def respond(self, status: int, body: bytes, content_type: str) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: Any) -> None:
        return


def serve(arguments: argparse.Namespace) -> int:
    evidence = Path(arguments.evidence).resolve()
    ready = Path(arguments.ready).resolve()
    control = Path(arguments.control).resolve()
    for path in (evidence, ready, control):
        path.parent.mkdir(parents=True, exist_ok=True)
    control.unlink(missing_ok=True)
    ready.unlink(missing_ok=True)
    suite = None
    if arguments.persona_fixture is not None:
        _, suite = load_persona_suite(Path(arguments.persona_fixture).resolve())
    state = FixtureState(evidence, control, arguments.blossom_port, suite)
    RelayHandler.state = state
    BlossomHandler.state = state
    relay = LoopbackConnectionFactory.relay(arguments.relay_port, state)
    blossom = LoopbackConnectionFactory.blossom(arguments.blossom_port, state)
    threads = [
        threading.Thread(target=relay.serve_forever, daemon=True),
        threading.Thread(target=blossom.serve_forever, daemon=True),
    ]
    for thread in threads:
        thread.start()
    ready.write_text(
        json.dumps(
            {
                "schema": "radroots-ios-local-social-fixture-ready-v1",
                "relay": f"ws://127.0.0.1:{arguments.relay_port}",
                "blossom": f"http://127.0.0.1:{arguments.blossom_port}",
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    stopped = threading.Event()

    def stop(_signum: int, _frame: Any) -> None:
        stopped.set()

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    stopped.wait()
    relay.shutdown()
    blossom.shutdown()
    relay.server_close()
    blossom.server_close()
    for thread in threads:
        thread.join(timeout=5)
    return 0


def verify(arguments: argparse.Namespace) -> int:
    payload = json.loads(Path(arguments.evidence).read_text(encoding="utf-8"))
    if (
        payload.get("schema") != "radroots-ios-local-social-fixture-evidence-v1"
        or payload.get("accepted_events", 0) < 5
        or payload.get("subscriptions", 0) < 1
        or payload.get("upload_attempts", 0) < 1
        or payload.get("accepted_uploads", 0) < 1
        or payload.get("retrievals", 0) < 1
        or payload.get("non_loopback_attempts") != 0
        or payload.get("production_network_contacts") != 0
    ):
        raise SystemExit("local-social fixture evidence is incomplete")
    print("local-social fixture evidence verified")
    return 0


def verify_accessibility(arguments: argparse.Namespace) -> int:
    payload = json.loads(Path(arguments.evidence).read_text(encoding="utf-8"))
    if (
        payload.get("schema") != "radroots-ios-local-social-fixture-evidence-v1"
        or payload.get("accepted_events") != 0
        or payload.get("upload_attempts") != 0
        or payload.get("accepted_uploads") != 0
        or payload.get("retrievals") != 0
        or payload.get("subscriptions", 0) < 1
    ):
        raise SystemExit("local-social accessibility fixture evidence is invalid")
    print("local-social accessibility fixture evidence verified")
    return 0


def verify_persona_fixture(arguments: argparse.Namespace) -> int:
    raw, _ = load_persona_suite(Path(arguments.fixture).resolve())
    fixture_schema = validate_schema_file(
        Path(arguments.fixture_schema).resolve(),
        "https://radroots.org/schemas/ios/local-social-personas.v1.schema.json",
    )
    result_schema = validate_schema_file(
        Path(arguments.result_schema).resolve(),
        "https://radroots.org/schemas/ios/local-social-persona-results.v1.schema.json",
    )
    attempt_schema = validate_schema_file(
        Path(arguments.attempt_schema).resolve(),
        "https://radroots.org/schemas/ios/local-social-persona-attempt-evidence.v1.schema.json",
    )
    result_v2_schema = validate_schema_file(
        Path(arguments.result_v2_schema).resolve(),
        "https://radroots.org/schemas/ios/local-social-persona-results.v2.schema.json",
    )
    print(
        "local-social persona fixtures verified: "
        f"fixture={hashlib.sha256(raw).hexdigest()} "
        f"fixture_schema={hashlib.sha256(fixture_schema).hexdigest()} "
        f"result_schema={hashlib.sha256(result_schema).hexdigest()} "
        f"attempt_schema={hashlib.sha256(attempt_schema).hexdigest()} "
        f"result_v2_schema={hashlib.sha256(result_v2_schema).hexdigest()}"
    )
    return 0


def verify_bud11_corpus(arguments: argparse.Namespace) -> int:
    raw, _ = load_bud11_mutation_corpus(Path(arguments.corpus).resolve())
    schema = validate_schema_file(
        Path(arguments.schema).resolve(),
        "https://radroots.org/schemas/ios/bud11-upload-authorization-mutations.v1.schema.json",
    )
    print(
        "BUD-11 mutation corpus verified: "
        f"corpus={hashlib.sha256(raw).hexdigest()} "
        f"schema={hashlib.sha256(schema).hexdigest()}"
    )
    return 0


def directory_digest(path: Path) -> str:
    digest = hashlib.sha256()
    for item in sorted(
        path.rglob("*"), key=lambda value: value.relative_to(path).as_posix()
    ):
        relative = item.relative_to(path).as_posix().encode("utf-8")
        if item.is_symlink():
            raise ValueError("result bundle contains a symbolic link")
        if item.is_dir():
            digest.update(b"d\0" + relative + b"\0")
            continue
        if not item.is_file():
            raise ValueError("result bundle contains an unsupported entry")
        digest.update(b"f\0" + relative + b"\0")
        with item.open("rb") as stream:
            while chunk := stream.read(64 * 1024):
                digest.update(chunk)
    return digest.hexdigest()


def simulator_metadata(udid: str, result_bundle: Path) -> dict[str, str]:
    devices = json.loads(
        subprocess.check_output(["xcrun", "simctl", "list", "devices", "--json"])
    )
    matches = [
        (runtime, device)
        for runtime, values in devices.get("devices", {}).items()
        for device in values
        if device.get("udid") == udid
    ]
    if len(matches) != 1:
        raise ValueError("simulator identity is unavailable")
    runtime, simulator = matches[0]
    runtime_version = runtime.rsplit("iOS-", 1)[-1].replace("-", ".")
    summary = json.loads(
        subprocess.check_output(
            [
                "xcrun",
                "xcresulttool",
                "get",
                "test-results",
                "summary",
                "--path",
                str(result_bundle),
            ]
        )
    )
    result_devices = [
        row.get("device")
        for row in summary.get("devicesAndConfigurations", [])
        if isinstance(row, dict)
        and isinstance(row.get("device"), dict)
        and isinstance(row["device"].get("deviceId"), str)
        and row["device"]["deviceId"].upper() == udid.upper()
    ]
    if len(result_devices) != 1:
        raise ValueError("result bundle simulator identity is unavailable")
    result_device = result_devices[0]
    version = result_device.get("osVersion")
    architecture = result_device.get("architecture")
    if (
        summary.get("result") != "Passed"
        or summary.get("failedTests") != 0
        or summary.get("passedTests") != 1
        or summary.get("skippedTests") != 0
        or simulator.get("isAvailable") is not True
        or result_device.get("platform") != "iOS Simulator"
        or version != runtime_version
        or not isinstance(version, str)
        or re.fullmatch(r"[1-9][0-9]*(\.[0-9]+){1,2}", version) is None
        or int(version.split(".", 1)[0]) < 18
        or architecture != "arm64"
    ):
        raise ValueError("simulator does not satisfy the development platform contract")
    return {"udid": udid.upper(), "os": f"iOS {version}", "architecture": architecture}


def validate_persona_evidence(value: Any, suite: dict[str, Any]) -> dict[str, Any]:
    keys = {
        "schema", "schema_version", "personas", "flow_counts", "accepted_events",
        "event_kind_counts", "upload_attempts", "accepted_uploads", "retrievals",
        "distinct_identities", "unknown_attempts", "duplicate_attempts",
        "expected_failure_rejections", "events_accepted_during_expected_failures",
        "accepted_connections", "rejected_connections", "non_loopback_attempts",
        "production_network_contacts",
        "unintended_publications",
        "final_candidate_data_loss",
    }
    evidence = exact_keys(value, keys, "persona evidence")
    if (
        evidence["schema"] != "radroots.ios.local-social.persona-evidence.v1"
        or evidence["schema_version"] != 1
    ):
        raise ValueError("persona evidence header is invalid")
    expected_scalars = {
        "accepted_events": 15,
        "accepted_uploads": 3,
        "retrievals": 3,
        "distinct_identities": 5,
        "unknown_attempts": 0,
        "duplicate_attempts": 0,
        "expected_failure_rejections": 1,
        "events_accepted_during_expected_failures": 0,
        "non_loopback_attempts": 0,
        "production_network_contacts": 0,
        "unintended_publications": 0,
        "final_candidate_data_loss": 0,
    }
    if any(evidence.get(key) != expected for key, expected in expected_scalars.items()):
        raise ValueError("persona evidence totals are invalid")
    if (
        type(evidence["accepted_connections"]) is not int
        or not 1 <= evidence["accepted_connections"] <= 61_440
        or type(evidence["rejected_connections"]) is not int
        or not 0 <= evidence["rejected_connections"] <= 61_440
    ):
        raise ValueError("persona connection evidence is invalid")
    if evidence["flow_counts"] != {flow: 3 for flow in FLOW_KINDS}:
        raise ValueError("persona flow counts are invalid")
    if evidence["event_kind_counts"] != {"1": 9, "31923": 3, "30402": 3}:
        raise ValueError("persona event kind counts are invalid")
    if not isinstance(evidence["personas"], list) or len(evidence["personas"]) != 5:
        raise ValueError("persona evidence inventory is invalid")
    identity_digests = set()
    for persona, expected in zip(evidence["personas"], suite["personas"], strict=True):
        exact_keys(
            persona,
            {
                "alias",
                "identity_sha256",
                "subscriptions",
                "accepted_uploads",
                "retrievals",
                "attempts",
            },
            "persona evidence row",
        )
        if (
            persona["alias"] != expected["alias"]
            or not lowercase_hex(persona["identity_sha256"], 64)
            or persona["identity_sha256"] == "0" * 64
            or type(persona["subscriptions"]) is not int
            or not 1 <= persona["subscriptions"] <= 4096
            or persona["accepted_uploads"] != int(persona["alias"] in PHOTO_PERSONAS)
            or persona["retrievals"] != int(persona["alias"] in PHOTO_PERSONAS)
            or not isinstance(persona["attempts"], list)
            or len(persona["attempts"]) != 3
        ):
            raise ValueError("persona evidence row is invalid")
        identity_digests.add(persona["identity_sha256"])
        for attempt, expected_attempt in zip(
            persona["attempts"], expected["attempts"], strict=True
        ):
            exact_keys(
                attempt,
                {"id", "flow", "event_kind", "accepted", "expected_failure_rejections"},
                "attempt evidence row",
            )
            if (
                attempt["id"] != expected_attempt["id"]
                or attempt["flow"] != expected_attempt["flow"]
                or attempt["event_kind"] != FLOW_KINDS[expected_attempt["flow"]]
                or attempt["accepted"] is not True
                or attempt["expected_failure_rejections"]
                != int(
                    expected_attempt["expected_failure"]
                    == "transport_retry_relaunch"
                )
            ):
                raise ValueError("attempt evidence row is invalid")
    if len(identity_digests) != 5:
        raise ValueError("persona identities are not distinct")
    return evidence


def valid_ios_runtime(value: Any) -> bool:
    if not isinstance(value, str) or not re.fullmatch(
        r"iOS ([1-9][0-9]*)(\.[0-9]+){1,2}", value
    ):
        return False
    return int(value.split()[1].split(".", 1)[0]) >= 18


def canonical_evidence_bytes(value: Any) -> bytes:
    return json.dumps(
        value, ensure_ascii=True, separators=(",", ":"), sort_keys=True
    ).encode("utf-8")


def evidence_contains_forbidden_key(value: Any) -> bool:
    if isinstance(value, dict):
        return any(
            key.lower() in FORBIDDEN_EVIDENCE_KEYS
            or evidence_contains_forbidden_key(item)
            for key, item in value.items()
        )
    if isinstance(value, list):
        return any(evidence_contains_forbidden_key(item) for item in value)
    return False


def persona_invocation() -> dict[str, str]:
    return {
        "target": PERSONA_TEST_TARGET,
        "identifier": PERSONA_TEST_IDENTIFIER,
        "action": PERSONA_TEST_ACTION,
        "configuration": PERSONA_TEST_CONFIGURATION,
    }


def validate_persona_attempt_evidence(
    value: Any,
    suite: dict[str, Any],
    *,
    require_measured_network: bool,
) -> dict[str, Any]:
    keys = {
        "schema",
        "schema_version",
        "test_invocation",
        "source",
        "app_build_sha256",
        "simulator",
        "run_id",
        "persona_run_id",
        "persona_alias",
        "attempt_id",
        "attempt_order",
        "flow",
        "expected_failure",
        "public_identity_sha256",
        "endpoint_policy_sha256",
        "ui_observation",
        "network_observation",
        "accessibility",
        "artifact_digests",
    }
    attempt = exact_keys(value, keys, "persona attempt evidence")
    if evidence_contains_forbidden_key(attempt):
        raise ValueError("persona attempt evidence contains a forbidden field")
    if (
        attempt["schema"] != PERSONA_ATTEMPT_SCHEMA
        or attempt["schema_version"] != 1
        or attempt["test_invocation"] != persona_invocation()
        or not lowercase_hex(attempt["app_build_sha256"], 64)
        or not lowercase_hex(attempt["public_identity_sha256"], 64)
        or attempt["public_identity_sha256"] == "0" * 64
        or not lowercase_hex(attempt["endpoint_policy_sha256"], 64)
        or not re.fullmatch(
            r"[a-z0-9][a-z0-9-]{6,62}[a-z0-9]", attempt["run_id"]
        )
    ):
        raise ValueError("persona attempt evidence header is invalid")
    source = exact_keys(attempt["source"], {"commit", "tree"}, "attempt source")
    if not lowercase_hex(source["commit"], 40) or not lowercase_hex(
        source["tree"], 40
    ):
        raise ValueError("persona attempt source identity is invalid")
    simulator = exact_keys(
        attempt["simulator"], {"udid", "os", "architecture"}, "attempt simulator"
    )
    if (
        not isinstance(simulator["udid"], str)
        or re.fullmatch(r"[A-F0-9-]{36}", simulator["udid"]) is None
        or not valid_ios_runtime(simulator["os"])
        or simulator["architecture"] != "arm64"
    ):
        raise ValueError("persona attempt simulator is invalid")
    expected_attempts = {
        candidate["id"]: (persona, candidate)
        for persona in suite["personas"]
        for candidate in persona["attempts"]
    }
    expected = expected_attempts.get(attempt["attempt_id"])
    if expected is None:
        raise ValueError("persona attempt identity is unknown")
    expected_persona, expected_attempt = expected
    alias = expected_persona["alias"]
    if (
        attempt["persona_alias"] != alias
        or attempt["attempt_order"] != expected_attempt["order"]
        or attempt["flow"] != expected_attempt["flow"]
        or attempt["expected_failure"] != expected_attempt["expected_failure"]
        or re.fullmatch(
            rf"persona-{alias.lower()}-[0-9a-f]{{24}}", attempt["persona_run_id"]
        )
        is None
    ):
        raise ValueError("persona attempt binding is invalid")
    ui = exact_keys(
        attempt["ui_observation"],
        {
            "validation_attempted",
            "validation_rejected",
            "retry_attempts",
            "relaunches",
            "retention_verified",
            "today_projection_verified",
        },
        "attempt UI observation",
    )
    validation = expected_attempt["expected_failure"] == "validation_recovery"
    retry = expected_attempt["expected_failure"] == "transport_retry_relaunch"
    if (
        ui["validation_attempted"] is not validation
        or ui["validation_rejected"] is not validation
        or ui["retry_attempts"] != int(retry)
        or ui["relaunches"] != int(retry)
        or ui["retention_verified"] is not True
        or ui["today_projection_verified"] is not True
    ):
        raise ValueError("persona attempt UI observation is invalid")
    network = attempt["network_observation"]
    if network == {"state": "pending_step_258"}:
        if require_measured_network:
            raise ValueError("persona attempt network evidence is not measured")
    else:
        network = exact_keys(
            network,
            {
                "state",
                "accepted_connections",
                "rejected_connections",
                "non_loopback_attempts",
                "subscriptions",
                "accepted_events",
                "accepted_uploads",
                "retrievals",
                "unintended_publications",
                "events_accepted_during_expected_failure",
                "final_candidate_data_loss",
            },
            "attempt network observation",
        )
        if network["state"] != "measured" or any(
            type(network[key]) is not int or not 0 <= network[key] <= 4096
            for key in network
            if key != "state"
        ):
            raise ValueError("persona attempt network observation is invalid")
        expected_media = int(attempt["flow"] == "PhotoUpdate")
        if (
            network["accepted_events"] != 1
            or network["accepted_uploads"] != expected_media
            or network["retrievals"] != expected_media
            or network["non_loopback_attempts"] != 0
            or network["unintended_publications"] != 0
            or network["events_accepted_during_expected_failure"] != 0
            or network["final_candidate_data_loss"] != 0
        ):
            raise ValueError("persona attempt measured result is invalid")
    accessibility = exact_keys(
        attempt["accessibility"],
        {
            "locale",
            "content_size",
            "reduce_motion",
            "progressive_disclosure",
            "labels_values_traits",
            "keyboard_focus",
            "visible_actions",
            "voiceover_user_observed",
        },
        "attempt accessibility observation",
    )
    if (
        accessibility["locale"] != "en_US"
        or accessibility["content_size"]
        != "accessibility-extra-extra-extra-large"
        or accessibility["reduce_motion"] is not True
        or any(
            type(accessibility[key]) is not bool
            for key in (
                "progressive_disclosure",
                "labels_values_traits",
                "keyboard_focus",
                "visible_actions",
                "voiceover_user_observed",
            )
        )
        or accessibility["visible_actions"] is not True
        or accessibility["voiceover_user_observed"] is not False
        or accessibility["progressive_disclosure"]
        is not (expected_persona["interaction_profile"] == "novice_progressive_disclosure")
        or accessibility["keyboard_focus"]
        is not (expected_persona["interaction_profile"] == "novice_accessibility_keyboard")
    ):
        raise ValueError("persona attempt accessibility evidence is invalid")
    artifacts = attempt["artifact_digests"]
    if not isinstance(artifacts, list) or len(artifacts) > 4:
        raise ValueError("persona attempt artifact inventory is invalid")
    roles: set[str] = set()
    for artifact in artifacts:
        artifact = exact_keys(artifact, {"role", "sha256"}, "attempt artifact")
        if (
            artifact["role"]
            not in {"media_input", "uploaded_blob", "retrieved_blob", "ui_snapshot"}
            or artifact["role"] in roles
            or not lowercase_hex(artifact["sha256"], 64)
        ):
            raise ValueError("persona attempt artifact is invalid")
        roles.add(artifact["role"])
    return attempt


def exact_persona_test_node(value: Any) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != {
        "devices",
        "testNodes",
        "testPlanConfigurations",
    }:
        raise ValueError("xcresult test inventory is invalid")
    matches: list[dict[str, Any]] = []

    def visit(node: Any) -> None:
        if not isinstance(node, dict):
            raise ValueError("xcresult test node is invalid")
        if node.get("nodeType") == "Test Case" and (
            node.get("nodeIdentifier") == PERSONA_XCRESULT_NODE_IDENTIFIER
            or node.get("nodeIdentifierURL") == PERSONA_XCRESULT_NODE_URL
        ):
            matches.append(node)
        children = node.get("children", [])
        if not isinstance(children, list):
            raise ValueError("xcresult child inventory is invalid")
        for child in children:
            visit(child)

    nodes = value["testNodes"]
    if not isinstance(nodes, list):
        raise ValueError("xcresult test inventory is invalid")
    for node in nodes:
        visit(node)
    if len(matches) != 1:
        raise ValueError("xcresult exact persona test is unavailable")
    match = matches[0]
    if (
        match.get("nodeIdentifier") != PERSONA_XCRESULT_NODE_IDENTIFIER
        or match.get("nodeIdentifierURL") != PERSONA_XCRESULT_NODE_URL
        or match.get("name") != "testLocalSocialDeterministicPersonas()"
        or match.get("nodeType") != "Test Case"
        or match.get("result") != "Passed"
    ):
        raise ValueError("xcresult exact persona test did not pass")
    return match


def run_json_command_bounded(command: list[str], maximum: int) -> Any:
    with tempfile.TemporaryFile() as output:
        subprocess.run(command, stdout=output, check=True)
        size = output.tell()
        if size <= 0 or size > maximum:
            raise ValueError("command JSON output exceeds its byte bound")
        output.seek(0)
        return json.loads(output.read(), object_pairs_hook=strict_object)


def load_exported_persona_attachments(
    export_directory: Path,
    suite: dict[str, Any],
    *,
    require_measured_network: bool,
) -> list[tuple[bytes, dict[str, Any]]]:
    manifest_raw, manifest_value = read_json_bounded(
        export_directory / "manifest.json", MAX_XCRESULT_JSON_BYTES
    )
    del manifest_raw
    if not isinstance(manifest_value, list) or len(manifest_value) != 1:
        raise ValueError("xcresult attachment manifest is invalid")
    group = exact_keys(
        manifest_value[0],
        {"testIdentifier", "testIdentifierURL", "attachments"},
        "xcresult attachment group",
    )
    if (
        group["testIdentifier"] != PERSONA_XCRESULT_NODE_IDENTIFIER
        or group["testIdentifierURL"] != PERSONA_XCRESULT_NODE_URL
        or not isinstance(group["attachments"], list)
        or len(group["attachments"]) != 15
    ):
        raise ValueError("xcresult attachment test binding is invalid")
    attachments_by_name: dict[str, tuple[bytes, dict[str, Any]]] = {}
    total_bytes = 0
    allowed_manifest_keys = {
        "exportedFileName",
        "suggestedHumanReadableName",
        "isAssociatedWithFailure",
        "configurationName",
        "deviceName",
        "deviceId",
        "timestamp",
        "repetitionNumber",
        "arguments",
    }
    for row_value in group["attachments"]:
        if not isinstance(row_value, dict) or not {
            "exportedFileName",
            "suggestedHumanReadableName",
            "isAssociatedWithFailure",
            "configurationName",
            "deviceName",
            "deviceId",
        }.issubset(row_value) or not set(row_value).issubset(allowed_manifest_keys):
            raise ValueError("xcresult attachment row is invalid")
        exported = row_value["exportedFileName"]
        name = row_value["suggestedHumanReadableName"]
        if (
            name not in PERSONA_ATTACHMENT_NAMES
            or name in attachments_by_name
            or not isinstance(exported, str)
            or not 1 <= len(exported.encode("utf-8")) <= 255
            or Path(exported).name != exported
            or row_value["isAssociatedWithFailure"] is not False
            or row_value["configurationName"] != "Test Scheme Action"
            or not isinstance(row_value["deviceName"], str)
            or not row_value["deviceName"]
            or not isinstance(row_value["deviceId"], str)
        ):
            raise ValueError("xcresult attempt attachment identity is invalid")
        path = export_directory / exported
        if path.is_symlink() or not path.is_file():
            raise ValueError("xcresult attempt attachment is not a regular file")
        raw, value = read_json_bounded(path, MAX_PERSONA_ATTACHMENT_BYTES)
        if raw != canonical_evidence_bytes(value):
            raise ValueError("persona attempt attachment is noncanonical")
        attempt = validate_persona_attempt_evidence(
            value, suite, require_measured_network=require_measured_network
        )
        if name != f"radroots-local-social-{attempt['attempt_id']}.json":
            raise ValueError("xcresult attachment name does not bind its attempt")
        total_bytes += len(raw)
        if total_bytes > MAX_PERSONA_ATTACHMENTS_BYTES:
            raise ValueError("xcresult attempt attachments exceed their aggregate bound")
        attachments_by_name[name] = (raw, attempt)
    if tuple(sorted(attachments_by_name)) != tuple(sorted(PERSONA_ATTACHMENT_NAMES)):
        raise ValueError("xcresult persona attachment inventory is incomplete")
    return [attachments_by_name[name] for name in PERSONA_ATTACHMENT_NAMES]


def extract_persona_attempt_attachments(
    result_bundle: Path,
    suite: dict[str, Any],
    *,
    require_measured_network: bool,
) -> list[tuple[bytes, dict[str, Any]]]:
    tests = run_json_command_bounded(
        [
            "xcrun",
            "xcresulttool",
            "get",
            "test-results",
            "tests",
            "--path",
            str(result_bundle),
        ],
        MAX_XCRESULT_JSON_BYTES,
    )
    exact_persona_test_node(tests)
    with tempfile.TemporaryDirectory() as directory:
        export_directory = Path(directory)
        subprocess.run(
            [
                "xcrun",
                "xcresulttool",
                "export",
                "attachments",
                "--test-id",
                PERSONA_XCRESULT_NODE_URL,
                "--path",
                str(result_bundle),
                "--output-path",
                str(export_directory),
            ],
            check=True,
        )
        return load_exported_persona_attachments(
            export_directory,
            suite,
            require_measured_network=require_measured_network,
        )


def reconstruct_persona_result_v2(
    suite: dict[str, Any],
    attachments: list[tuple[bytes, dict[str, Any]]],
    *,
    fixture_sha256: str,
    fixture_schema_sha256: str,
    attempt_schema_sha256: str,
    result_schema_sha256: str,
    result_bundle_sha256: str,
    forward_repairs: list[str],
) -> dict[str, Any]:
    if len(attachments) != 15:
        raise ValueError("persona result requires exactly 15 attempt attachments")
    attempts = [
        validate_persona_attempt_evidence(value, suite, require_measured_network=True)
        for _, value in attachments
    ]
    expected_ids = [
        candidate["id"]
        for persona in suite["personas"]
        for candidate in persona["attempts"]
    ]
    if [attempt["attempt_id"] for attempt in attempts] != expected_ids:
        raise ValueError("persona result attempt inventory is not exact")
    first = attempts[0]
    shared_fields = (
        "test_invocation",
        "source",
        "app_build_sha256",
        "simulator",
        "run_id",
        "endpoint_policy_sha256",
    )
    if any(
        attempt[field] != first[field]
        for attempt in attempts[1:]
        for field in shared_fields
    ):
        raise ValueError("persona attempt evidence spans multiple run identities")
    for digest in (
        fixture_sha256,
        fixture_schema_sha256,
        attempt_schema_sha256,
        result_schema_sha256,
        result_bundle_sha256,
    ):
        if not lowercase_hex(digest, 64):
            raise ValueError("persona result digest identity is invalid")
    if (
        not isinstance(forward_repairs, list)
        or len(forward_repairs) > 16
        or len(set(forward_repairs)) != len(forward_repairs)
        or any(not lowercase_hex(revision, 40) for revision in forward_repairs)
    ):
        raise ValueError("persona result forward-repair inventory is invalid")
    identity_by_persona: dict[str, str] = {}
    persona_rows = []
    for persona in suite["personas"]:
        alias = persona["alias"]
        rows = [attempt for attempt in attempts if attempt["persona_alias"] == alias]
        identities = {row["public_identity_sha256"] for row in rows}
        if len(rows) != 3 or len(identities) != 1:
            raise ValueError("persona result identity reuse is invalid")
        identity = identities.pop()
        identity_by_persona[alias] = identity
        subscriptions = sum(
            row["network_observation"]["subscriptions"] for row in rows
        )
        if subscriptions < 1:
            raise ValueError("persona result is missing a subscription")
        persona_rows.append(
            {
                "alias": alias,
                "public_identity_sha256": identity,
                "subscriptions": subscriptions,
                "attempt_ids": [row["attempt_id"] for row in rows],
            }
        )
    if len(set(identity_by_persona.values())) != 5:
        raise ValueError("persona result identities are not distinct")
    network_rows = [attempt["network_observation"] for attempt in attempts]
    accepted_events = sum(row["accepted_events"] for row in network_rows)
    flow_counts = {
        flow: sum(attempt["flow"] == flow for attempt in attempts)
        for flow in FLOW_KINDS
    }
    event_kind_counts = {
        str(kind): sum(
            FLOW_KINDS[attempt["flow"]] == kind
            and attempt["network_observation"]["accepted_events"] == 1
            for attempt in attempts
        )
        for kind in (1, 31923, 30402)
    }
    result = {
        "schema": PERSONA_RESULT_V2_SCHEMA,
        "schema_version": 2,
        "run_id": first["run_id"],
        "test_invocation": first["test_invocation"],
        "source": first["source"],
        "app_build_sha256": first["app_build_sha256"],
        "endpoint_policy_sha256": first["endpoint_policy_sha256"],
        "fixture_sha256": fixture_sha256,
        "fixture_schema_sha256": fixture_schema_sha256,
        "attempt_schema_sha256": attempt_schema_sha256,
        "result_schema_sha256": result_schema_sha256,
        "simulator": first["simulator"],
        "result_bundle_sha256": result_bundle_sha256,
        "attachments": [
            {
                "attempt_id": attempt["attempt_id"],
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
            for raw, attempt in attachments
        ],
        "personas": persona_rows,
        "flow_counts": flow_counts,
        "accepted_events": accepted_events,
        "event_kind_counts": event_kind_counts,
        "accepted_uploads": sum(row["accepted_uploads"] for row in network_rows),
        "retrievals": sum(row["retrievals"] for row in network_rows),
        "distinct_identities": len(set(identity_by_persona.values())),
        "unknown_attempts": len(
            set(expected_ids) - {row["attempt_id"] for row in attempts}
        ),
        "duplicate_attempts": len(attempts) - len({row["attempt_id"] for row in attempts}),
        "expected_failure_rejections": sum(
            attempt["ui_observation"]["validation_rejected"]
            or attempt["ui_observation"]["retry_attempts"] > 0
            for attempt in attempts
        ),
        "events_accepted_during_expected_failures": sum(
            row["events_accepted_during_expected_failure"] for row in network_rows
        ),
        "accepted_connections": sum(row["accepted_connections"] for row in network_rows),
        "rejected_connections": sum(row["rejected_connections"] for row in network_rows),
        "non_loopback_attempts": sum(row["non_loopback_attempts"] for row in network_rows),
        "unintended_publications": sum(
            row["unintended_publications"] for row in network_rows
        ),
        "final_candidate_data_loss": sum(
            row["final_candidate_data_loss"] for row in network_rows
        ),
        "accessibility": {
            "locale": "en_US",
            "content_size": "accessibility-extra-extra-extra-large",
            "reduce_motion": True,
            "semantic_attempts": sum(
                attempt["accessibility"]["labels_values_traits"] for attempt in attempts
            ),
            "keyboard_focus_attempts": sum(
                attempt["accessibility"]["keyboard_focus"] for attempt in attempts
            ),
            "voiceover_user_observed": False,
        },
        "forward_repairs": forward_repairs,
        "complete_matrix_rerun": True,
    }
    if (
        result["flow_counts"] != {flow: 3 for flow in FLOW_KINDS}
        or result["accepted_events"] != 15
        or result["event_kind_counts"] != {"1": 9, "31923": 3, "30402": 3}
        or result["accepted_uploads"] != 3
        or result["retrievals"] != 3
        or result["distinct_identities"] != 5
        or result["unknown_attempts"] != 0
        or result["duplicate_attempts"] != 0
        or result["expected_failure_rejections"] != 2
        or result["events_accepted_during_expected_failures"] != 0
        or result["accepted_connections"] < 1
        or result["non_loopback_attempts"] != 0
        or result["unintended_publications"] != 0
        or result["final_candidate_data_loss"] != 0
        or result["accessibility"]["semantic_attempts"] < 3
        or result["accessibility"]["keyboard_focus_attempts"] != 3
    ):
        raise ValueError("persona result reconstructed outcome is invalid")
    return result


def validate_persona_result(
    value: Any,
    suite: dict[str, Any],
    fixture_sha256: str,
    fixture_schema_sha256: str,
    result_schema_sha256: str,
) -> dict[str, Any]:
    keys = {
        "schema",
        "schema_version",
        "run_id",
        "source_commit",
        "source_tree",
        "fixture_sha256",
        "fixture_schema_sha256",
        "result_schema_sha256",
        "simulator",
        "result_bundle_sha256",
        "evidence_sha256",
        "personas",
        "flow_counts",
        "accepted_events",
        "event_kind_counts",
        "accepted_uploads",
        "retrievals",
        "distinct_identities",
        "unknown_attempts",
        "duplicate_attempts",
        "expected_failure_rejections",
        "events_accepted_during_expected_failures",
        "production_network_contacts",
        "unintended_publications",
        "final_candidate_data_loss",
        "accessibility",
        "forward_repairs",
        "complete_matrix_rerun",
    }
    result = exact_keys(value, keys, "persona result")
    if (
        result["schema"] != "radroots.ios.local-social.persona-results.v1"
        or result["schema_version"] != 1
        or not re.fullmatch(r"[a-z0-9][a-z0-9-]{6,62}[a-z0-9]", result["run_id"])
        or not lowercase_hex(result["source_commit"], 40)
        or not lowercase_hex(result["source_tree"], 40)
    ):
        raise ValueError("persona result header is invalid")
    expected_digests = {
        "fixture_sha256": fixture_sha256,
        "fixture_schema_sha256": fixture_schema_sha256,
        "result_schema_sha256": result_schema_sha256,
    }
    if any(result[key] != digest for key, digest in expected_digests.items()):
        raise ValueError("persona result contract digest is invalid")
    if not lowercase_hex(result["result_bundle_sha256"], 64) or not lowercase_hex(
        result["evidence_sha256"], 64
    ):
        raise ValueError("persona result evidence digest is invalid")
    simulator = exact_keys(
        result["simulator"], {"udid", "os", "architecture"}, "simulator"
    )
    if (
        not isinstance(simulator["udid"], str)
        or re.fullmatch(r"[A-F0-9-]{36}", simulator["udid"]) is None
        or not valid_ios_runtime(simulator["os"])
        or simulator["architecture"] != "arm64"
    ):
        raise ValueError("persona result simulator is invalid")
    accessibility = exact_keys(
        result["accessibility"],
        {
            "locale",
            "content_size",
            "reduce_motion",
            "semantic_audit",
            "voiceover_user_observed",
        },
        "accessibility result",
    )
    if accessibility != {
        "locale": "en_US",
        "content_size": "accessibility-extra-extra-extra-large",
        "reduce_motion": True,
        "semantic_audit": "passed",
        "voiceover_user_observed": False,
    }:
        raise ValueError("persona accessibility result is invalid")
    repairs = result["forward_repairs"]
    if (
        not isinstance(repairs, list)
        or len(repairs) > 16
        or len(set(repairs)) != len(repairs)
        or any(not lowercase_hex(commit, 40) for commit in repairs)
        or result["complete_matrix_rerun"] is not True
    ):
        raise ValueError("persona rerun evidence is invalid")
    expected_scalars = {
        "accepted_events": 15,
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
    }
    if any(result.get(key) != expected for key, expected in expected_scalars.items()):
        raise ValueError("persona result totals are invalid")
    if result["flow_counts"] != {flow: 3 for flow in FLOW_KINDS} or result[
        "event_kind_counts"
    ] != {"1": 9, "31923": 3, "30402": 3}:
        raise ValueError("persona result event inventory is invalid")
    if not isinstance(result["personas"], list) or len(result["personas"]) != 5:
        raise ValueError("persona result inventory is invalid")
    identities = set()
    for persona, expected in zip(result["personas"], suite["personas"], strict=True):
        exact_keys(
            persona,
            {"alias", "identity_sha256", "subscriptions", "attempts"},
            "persona result row",
        )
        if (
            persona["alias"] != expected["alias"]
            or not lowercase_hex(persona["identity_sha256"], 64)
            or persona["identity_sha256"] == "0" * 64
            or type(persona["subscriptions"]) is not int
            or not 1 <= persona["subscriptions"] <= 4096
            or not isinstance(persona["attempts"], list)
            or len(persona["attempts"]) != 3
        ):
            raise ValueError("persona result row is invalid")
        identities.add(persona["identity_sha256"])
        for attempt, expected_attempt in zip(
            persona["attempts"], expected["attempts"], strict=True
        ):
            exact_keys(
                attempt,
                {
                    "id",
                    "flow",
                    "event_kind",
                    "accepted",
                    "expected_failure_rejections",
                },
                "attempt result row",
            )
            if (
                attempt["id"] != expected_attempt["id"]
                or attempt["flow"] != expected_attempt["flow"]
                or attempt["event_kind"] != FLOW_KINDS[expected_attempt["flow"]]
                or attempt["accepted"] is not True
                or attempt["expected_failure_rejections"]
                != int(
                    expected_attempt["expected_failure"]
                    == "transport_retry_relaunch"
                )
            ):
                raise ValueError("attempt result row is invalid")
    if len(identities) != 5:
        raise ValueError("persona result identities are not distinct")
    return result


def verify_persona(arguments: argparse.Namespace) -> int:
    fixture_raw, suite = load_persona_suite(Path(arguments.fixture).resolve())
    fixture_schema_raw = validate_schema_file(
        Path(arguments.fixture_schema).resolve(),
        "https://radroots.org/schemas/ios/local-social-personas.v1.schema.json",
    )
    attempt_schema_raw = validate_schema_file(
        Path(arguments.attempt_schema).resolve(),
        "https://radroots.org/schemas/ios/local-social-persona-attempt-evidence.v1.schema.json",
    )
    result_schema_raw = validate_schema_file(
        Path(arguments.result_v2_schema).resolve(),
        "https://radroots.org/schemas/ios/local-social-persona-results.v2.schema.json",
    )
    evidence_path = Path(arguments.evidence).resolve()
    _, evidence_value = read_json(evidence_path)
    evidence = validate_persona_evidence(evidence_value, suite)
    result_bundle = Path(arguments.result_bundle).resolve()
    if not result_bundle.is_dir():
        raise ValueError("XCUITest result bundle is unavailable")
    if not lowercase_hex(arguments.source_commit, 40) or not lowercase_hex(
        arguments.source_tree, 40
    ):
        raise ValueError("source identity is invalid")
    attachments = extract_persona_attempt_attachments(
        result_bundle, suite, require_measured_network=True
    )
    simulator = simulator_metadata(arguments.simulator_id, result_bundle)
    result = reconstruct_persona_result_v2(
        suite,
        attachments,
        fixture_sha256=hashlib.sha256(fixture_raw).hexdigest(),
        fixture_schema_sha256=hashlib.sha256(fixture_schema_raw).hexdigest(),
        attempt_schema_sha256=hashlib.sha256(attempt_schema_raw).hexdigest(),
        result_schema_sha256=hashlib.sha256(result_schema_raw).hexdigest(),
        result_bundle_sha256=directory_digest(result_bundle),
        forward_repairs=arguments.forward_repair_commit,
    )
    if (
        result["run_id"] != arguments.run_id
        or result["source"]
        != {"commit": arguments.source_commit, "tree": arguments.source_tree}
        or result["simulator"] != simulator
        or result["accepted_events"] != evidence["accepted_events"]
        or result["accepted_uploads"] != evidence["accepted_uploads"]
        or result["retrievals"] != evidence["retrievals"]
        or result["non_loopback_attempts"] != evidence["non_loopback_attempts"]
        or result["unintended_publications"] != evidence["unintended_publications"]
        or result["final_candidate_data_loss"] != evidence["final_candidate_data_loss"]
        or result["accepted_connections"] > evidence["accepted_connections"]
        or result["rejected_connections"] > evidence["rejected_connections"]
    ):
        raise ValueError("persona aggregate does not match fixture evidence")
    output = Path(arguments.output).resolve()
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    raw, reloaded = read_json(output)
    if raw != (json.dumps(reloaded, indent=2) + "\n").encode("utf-8") or reloaded != result:
        raise ValueError("persona v2 result is noncanonical")
    print(
        "local-social measured persona result verified: "
        f"{hashlib.sha256(output.read_bytes()).hexdigest()}"
    )
    return 0


def verify_persona_result_file(
    path: Path,
    suite: dict[str, Any],
    fixture_sha256: str,
    fixture_schema_sha256: str,
    result_schema_sha256: str,
) -> dict[str, Any]:
    raw, value = read_json(path)
    canonical = (json.dumps(value, indent=2) + "\n").encode("utf-8")
    if raw != canonical:
        raise ValueError("persona result is noncanonical")
    return validate_persona_result(
        value,
        suite,
        fixture_sha256,
        fixture_schema_sha256,
        result_schema_sha256,
    )


def verify_persona_result(arguments: argparse.Namespace) -> int:
    fixture_raw, suite = load_persona_suite(Path(arguments.fixture).resolve())
    fixture_schema_raw = validate_schema_file(
        Path(arguments.fixture_schema).resolve(),
        "https://radroots.org/schemas/ios/local-social-personas.v1.schema.json",
    )
    result_schema_raw = validate_schema_file(
        Path(arguments.result_schema).resolve(),
        "https://radroots.org/schemas/ios/local-social-persona-results.v1.schema.json",
    )
    verify_persona_result_file(
        Path(arguments.result).resolve(),
        suite,
        hashlib.sha256(fixture_raw).hexdigest(),
        hashlib.sha256(fixture_schema_raw).hexdigest(),
        hashlib.sha256(result_schema_raw).hexdigest(),
    )
    print("local-social persona result contract verified")
    return 0


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    commands = root.add_subparsers(dest="command", required=True)
    serve_command = commands.add_parser("serve")
    serve_command.add_argument("--relay-port", type=int, required=True)
    serve_command.add_argument("--blossom-port", type=int, required=True)
    serve_command.add_argument("--evidence", required=True)
    serve_command.add_argument("--ready", required=True)
    serve_command.add_argument("--control", required=True)
    serve_command.add_argument("--persona-fixture")
    verify_command = commands.add_parser("verify")
    verify_command.add_argument("--evidence", required=True)
    accessibility_command = commands.add_parser("verify-accessibility")
    accessibility_command.add_argument("--evidence", required=True)
    fixture_command = commands.add_parser("verify-persona-fixture")
    fixture_command.add_argument("--fixture", required=True)
    fixture_command.add_argument("--fixture-schema", required=True)
    fixture_command.add_argument("--result-schema", required=True)
    fixture_command.add_argument("--attempt-schema", required=True)
    fixture_command.add_argument("--result-v2-schema", required=True)
    bud11_command = commands.add_parser("verify-bud11-corpus")
    bud11_command.add_argument("--corpus", required=True)
    bud11_command.add_argument("--schema", required=True)
    persona_command = commands.add_parser("verify-persona")
    persona_command.add_argument("--fixture", required=True)
    persona_command.add_argument("--fixture-schema", required=True)
    persona_command.add_argument("--result-schema", required=True)
    persona_command.add_argument("--attempt-schema", required=True)
    persona_command.add_argument("--result-v2-schema", required=True)
    persona_command.add_argument("--evidence", required=True)
    persona_command.add_argument("--result-bundle", required=True)
    persona_command.add_argument("--output", required=True)
    persona_command.add_argument("--source-commit", required=True)
    persona_command.add_argument("--source-tree", required=True)
    persona_command.add_argument("--run-id", required=True)
    persona_command.add_argument("--simulator-id", required=True)
    persona_command.add_argument("--forward-repair-commit", action="append", default=[])
    result_command = commands.add_parser("verify-persona-result")
    result_command.add_argument("--fixture", required=True)
    result_command.add_argument("--fixture-schema", required=True)
    result_command.add_argument("--result-schema", required=True)
    result_command.add_argument("--result", required=True)
    return root


def main() -> int:
    arguments = parser().parse_args()
    if arguments.command == "serve":
        return serve(arguments)
    if arguments.command == "verify":
        return verify(arguments)
    if arguments.command == "verify-accessibility":
        return verify_accessibility(arguments)
    if arguments.command == "verify-persona-fixture":
        return verify_persona_fixture(arguments)
    if arguments.command == "verify-bud11-corpus":
        return verify_bud11_corpus(arguments)
    if arguments.command == "verify-persona-result":
        return verify_persona_result(arguments)
    return verify_persona(arguments)


if __name__ == "__main__":
    raise SystemExit(main())
