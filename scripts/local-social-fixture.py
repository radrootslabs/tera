#!/usr/bin/env python3
"""Bounded loopback Nostr and Blossom fixture for iOS simulator tests."""

from __future__ import annotations

import argparse
import base64
import hashlib
import http.server
import json
import os
import signal
import socket
import socketserver
import struct
import threading
import time
from pathlib import Path
from typing import Any

MAX_HTTP_BODY = 16 * 1024 * 1024
MAX_WEBSOCKET_MESSAGE = 2 * 1024 * 1024
MAX_EVENTS = 256
MAX_BLOBS = 16


class FixtureState:
    def __init__(self, evidence: Path, control: Path, blossom_port: int) -> None:
        self._evidence = evidence
        self.control = control
        self.blossom_port = blossom_port
        self._lock = threading.Lock()
        self._events: dict[str, dict[str, Any]] = {}
        self._blobs: dict[str, tuple[bytes, str]] = {}
        self._upload_attempts = 0
        self._accepted_uploads = 0
        self._retrievals = 0
        self._subscriptions = 0
        self._write_evidence()

    def publish(self, event: dict[str, Any]) -> bool:
        if not valid_nostr_event(event):
            return False
        event_id = event["id"]
        with self._lock:
            if event_id not in self._events and len(self._events) >= MAX_EVENTS:
                return False
            self._events[event_id] = event
            self._write_evidence_locked()
        return True

    def query(self, filters: list[dict[str, Any]]) -> list[dict[str, Any]]:
        with self._lock:
            self._subscriptions += 1
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
        self, body: bytes, media_type: str, expected_hash: str
    ) -> tuple[bool, dict[str, Any]]:
        digest = hashlib.sha256(body).hexdigest()
        with self._lock:
            self._upload_attempts += 1
            allowed = self.control.is_file()
            capacity = digest in self._blobs or len(self._blobs) < MAX_BLOBS
            if allowed and capacity and digest == expected_hash:
                self._blobs[digest] = (body, media_type)
                self._accepted_uploads += 1
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
        with self._lock:
            value = self._blobs.get(digest)
            if value is not None:
                self._retrievals += 1
                self._write_evidence_locked()
            return value

    def _write_evidence(self) -> None:
        with self._lock:
            self._write_evidence_locked()

    def _write_evidence_locked(self) -> None:
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
        temporary = self._evidence.with_suffix(".tmp")
        temporary.write_text(
            json.dumps(payload, sort_keys=True) + "\n", encoding="utf-8"
        )
        os.replace(temporary, self._evidence)


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
        accepted, descriptor = self.state.upload(body, media_type, expected_hash)
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
    state = FixtureState(evidence, control, arguments.blossom_port)
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


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    commands = root.add_subparsers(dest="command", required=True)
    serve_command = commands.add_parser("serve")
    serve_command.add_argument("--relay-port", type=int, required=True)
    serve_command.add_argument("--blossom-port", type=int, required=True)
    serve_command.add_argument("--evidence", required=True)
    serve_command.add_argument("--ready", required=True)
    serve_command.add_argument("--control", required=True)
    verify_command = commands.add_parser("verify")
    verify_command.add_argument("--evidence", required=True)
    accessibility_command = commands.add_parser("verify-accessibility")
    accessibility_command.add_argument("--evidence", required=True)
    return root


def main() -> int:
    arguments = parser().parse_args()
    if arguments.command == "serve":
        return serve(arguments)
    if arguments.command == "verify":
        return verify(arguments)
    return verify_accessibility(arguments)


if __name__ == "__main__":
    raise SystemExit(main())
