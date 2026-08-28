#!/usr/bin/env python3
"""Bounded loopback Nostr and Blossom fixture for iOS simulator tests."""

from __future__ import annotations

import argparse
import base64
import hashlib
import http.server
import json
import os
import re
import signal
import socket
import socketserver
import subprocess
import struct
import threading
import time
from pathlib import Path
from typing import Any

MAX_HTTP_BODY = 16 * 1024 * 1024
MAX_WEBSOCKET_MESSAGE = 2 * 1024 * 1024
MAX_EVENTS = 256
MAX_BLOBS = 16
MAX_JSON_BYTES = 64 * 1024
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
        self._production_network_contacts = 0
        self._unintended_publications = 0
        self._write_evidence()

    def publish(self, event: dict[str, Any]) -> bool | None:
        if not valid_nostr_event(event) or not verify_nostr_signature(event):
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
                    authorization, expected_hash
                )
            else:
                allowed = (
                    control is not None
                    and control["blossom_enabled"]
                    and persona in PHOTO_PERSONAS
                    and valid_blossom_authorization(authorization, expected_hash)
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
    if set(event) != {"id", "pubkey", "created_at", "kind", "tags", "content", "sig"}:
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


def valid_blossom_authorization(authorization: str, expected_hash: str) -> bool:
    if not authorization.startswith("Nostr "):
        return False
    payload = authorization.removeprefix("Nostr ")
    if (
        not payload
        or any(character.isspace() for character in payload)
        or "=" in payload
    ):
        return False
    try:
        raw = base64.urlsafe_b64decode(payload + "=" * (-len(payload) % 4))
        if len(raw) > 16 * 1024:
            return False
        if base64.urlsafe_b64encode(raw).rstrip(b"=").decode("ascii") != payload:
            return False
        event = json.loads(raw, object_pairs_hook=strict_object)
    except (ValueError, json.JSONDecodeError):
        return False
    if not valid_nostr_event(event) or not verify_nostr_signature(event):
        return False
    return any(
        isinstance(tag, list)
        and len(tag) >= 2
        and tag[0] == "x"
        and tag[1] == expected_hash
        for tag in event["tags"]
    )


class ReusableThreadingServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


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
    relay = ReusableThreadingServer(("127.0.0.1", arguments.relay_port), RelayHandler)
    blossom = http.server.ThreadingHTTPServer(
        ("127.0.0.1", arguments.blossom_port), BlossomHandler
    )
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
    print(
        "local-social persona fixtures verified: "
        f"fixture={hashlib.sha256(raw).hexdigest()} "
        f"fixture_schema={hashlib.sha256(fixture_schema).hexdigest()} "
        f"result_schema={hashlib.sha256(result_schema).hexdigest()}"
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


def simulator_metadata(udid: str) -> dict[str, str]:
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
    runtime, _ = matches[0]
    version = runtime.rsplit("iOS-", 1)[-1].replace("-", ".")
    architecture = subprocess.check_output(
        ["xcrun", "simctl", "spawn", udid, "uname", "-m"], text=True
    ).strip()
    if architecture != "arm64" or int(version.split(".")[0]) < 18:
        raise ValueError("simulator does not satisfy the development platform contract")
    return {"udid": udid.upper(), "os": f"iOS {version}", "architecture": architecture}


def validate_persona_evidence(value: Any, suite: dict[str, Any]) -> dict[str, Any]:
    keys = {
        "schema", "schema_version", "personas", "flow_counts", "accepted_events",
        "event_kind_counts", "upload_attempts", "accepted_uploads", "retrievals",
        "distinct_identities", "unknown_attempts", "duplicate_attempts",
        "expected_failure_rejections", "events_accepted_during_expected_failures",
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
        "production_network_contacts": 0,
        "unintended_publications": 0,
        "final_candidate_data_loss": 0,
    }
    if any(evidence.get(key) != expected for key, expected in expected_scalars.items()):
        raise ValueError("persona evidence totals are invalid")
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
    result_schema_raw = validate_schema_file(
        Path(arguments.result_schema).resolve(),
        "https://radroots.org/schemas/ios/local-social-persona-results.v1.schema.json",
    )
    evidence_path = Path(arguments.evidence).resolve()
    evidence_raw, evidence_value = read_json(evidence_path)
    evidence = validate_persona_evidence(evidence_value, suite)
    result_bundle = Path(arguments.result_bundle).resolve()
    if not result_bundle.is_dir():
        raise ValueError("XCUITest result bundle is unavailable")
    if not lowercase_hex(arguments.source_commit, 40) or not lowercase_hex(
        arguments.source_tree, 40
    ):
        raise ValueError("source identity is invalid")
    result = {
        "schema": "radroots.ios.local-social.persona-results.v1",
        "schema_version": 1,
        "run_id": arguments.run_id,
        "source_commit": arguments.source_commit,
        "source_tree": arguments.source_tree,
        "fixture_sha256": hashlib.sha256(fixture_raw).hexdigest(),
        "fixture_schema_sha256": hashlib.sha256(fixture_schema_raw).hexdigest(),
        "result_schema_sha256": hashlib.sha256(result_schema_raw).hexdigest(),
        "simulator": simulator_metadata(arguments.simulator_id),
        "result_bundle_sha256": directory_digest(result_bundle),
        "evidence_sha256": hashlib.sha256(evidence_raw).hexdigest(),
        "personas": [
            {
                "alias": persona["alias"],
                "identity_sha256": persona["identity_sha256"],
                "subscriptions": persona["subscriptions"],
                "attempts": persona["attempts"],
            }
            for persona in evidence["personas"]
        ],
        "flow_counts": evidence["flow_counts"],
        "accepted_events": evidence["accepted_events"],
        "event_kind_counts": evidence["event_kind_counts"],
        "accepted_uploads": evidence["accepted_uploads"],
        "retrievals": evidence["retrievals"],
        "distinct_identities": evidence["distinct_identities"],
        "unknown_attempts": evidence["unknown_attempts"],
        "duplicate_attempts": evidence["duplicate_attempts"],
        "expected_failure_rejections": evidence["expected_failure_rejections"],
        "events_accepted_during_expected_failures": evidence[
            "events_accepted_during_expected_failures"
        ],
        "production_network_contacts": evidence["production_network_contacts"],
        "unintended_publications": evidence["unintended_publications"],
        "final_candidate_data_loss": evidence["final_candidate_data_loss"],
        "accessibility": {
            "locale": "en_US",
            "content_size": "accessibility-extra-extra-extra-large",
            "reduce_motion": True,
            "semantic_audit": "passed",
            "voiceover_user_observed": False,
        },
        "forward_repairs": arguments.forward_repair_commit,
        "complete_matrix_rerun": True,
    }
    fixture_sha256 = hashlib.sha256(fixture_raw).hexdigest()
    fixture_schema_sha256 = hashlib.sha256(fixture_schema_raw).hexdigest()
    result_schema_sha256 = hashlib.sha256(result_schema_raw).hexdigest()
    validate_persona_result(
        result,
        suite,
        fixture_sha256,
        fixture_schema_sha256,
        result_schema_sha256,
    )
    output = Path(arguments.output).resolve()
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    verify_persona_result_file(
        output,
        suite,
        fixture_sha256,
        fixture_schema_sha256,
        result_schema_sha256,
    )
    print(
        "local-social persona result verified: "
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
    persona_command = commands.add_parser("verify-persona")
    persona_command.add_argument("--fixture", required=True)
    persona_command.add_argument("--fixture-schema", required=True)
    persona_command.add_argument("--result-schema", required=True)
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
    if arguments.command == "verify-persona-result":
        return verify_persona_result(arguments)
    return verify_persona(arguments)


if __name__ == "__main__":
    raise SystemExit(main())
