"""Real socket checks for idle versus incomplete fixture WebSocket frames."""

import importlib.util
import json
import select
import socket
import threading
import unittest
from contextlib import contextmanager
from pathlib import Path
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "local_social_relay_fixture", Path(__file__).with_name("local-social-fixture.py")
)
assert SPEC is not None and SPEC.loader is not None
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)


def masked_frame(opcode, payload):
    assert len(payload) < 126
    mask = b"test"
    body = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
    return bytes([0x80 | opcode, 0x80 | len(payload)]) + mask + body


@contextmanager
def relay_socket(wait):
    client, server = socket.socketpair()
    errors = []
    finished = threading.Event()
    state = mock.Mock()
    state.query.return_value = []

    class ScaledReadSocket:
        def settimeout(self, value):
            assert value == 15
            server.settimeout(0.05)

        def recv(self, length):
            return server.recv(length)

        def sendall(self, value):
            server.sendall(value)

        def fileno(self):
            return server.fileno()

    handler = type("RelaySocketTestHandler", (fixture.RelayHandler,), {"state": state})

    def serve():
        try:
            handler(ScaledReadSocket(), ("127.0.0.1", 0), None)
        except (ConnectionError, OSError) as error:
            errors.append(type(error))
        finally:
            server.close()
            finished.set()

    with mock.patch.object(fixture, "select", mock.Mock(select=wait), create=True):
        thread = threading.Thread(target=serve)
        thread.start()
        client.settimeout(1)
        try:
            client.sendall(
                b"GET / HTTP/1.1\r\nUpgrade: websocket\r\n"
                b"Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
            )
            response = client.recv(4096)
            assert response.startswith(b"HTTP/1.1 101")
            yield client, state, finished, errors
        finally:
            client.close()
            thread.join(1)
            assert not thread.is_alive(), "Fixture handler leaked after peer close"


class LocalSocialRelaySocketTests(unittest.TestCase):
    def test_idle_connection_survives_timeout_then_answers_exact_subscription(self):
        idle = threading.Event()
        calls = []

        def wait(read, write, error, timeout):
            self.assertEqual(timeout, 15)
            calls.append(timeout)
            if len(calls) == 1:
                result = select.select(read, write, error, 0.05)
                self.assertFalse(result[0])
                idle.set()
                return result
            return select.select(read, write, error, 1)

        with relay_socket(wait) as (client, state, finished, errors):
            self.assertTrue(idle.wait(1), "Idle frame boundary was not reached")
            self.assertFalse(finished.is_set(), "Healthy idle connection was closed")
            state.query.assert_not_called()
            client.sendall(masked_frame(1, b'["REQ","idle-read",{"limit":1}]'))
            first, second = fixture.read_exact(client, 2)
            self.assertEqual(first, 0x81)
            self.assertLess(second, 126)
            payload = fixture.read_exact(client, second)
            self.assertEqual(json.loads(payload), ["EOSE", "idle-read"])
            state.query.assert_called_once_with([{"limit": 1}])
            state.publish.assert_not_called()
            client.sendall(masked_frame(8, b""))
            self.assertTrue(finished.wait(1))
            self.assertEqual(errors, [])
        self.assertGreaterEqual(len(calls), 2)

    def test_partial_frame_still_expires_without_query_or_publication(self):
        def wait(read, write, error, timeout):
            self.assertEqual(timeout, 15)
            return select.select(read, write, error, 1)

        with relay_socket(wait) as (client, state, finished, errors):
            client.sendall(b"\x81")
            self.assertTrue(finished.wait(1), "Partial frame read deadline was lost")
            self.assertEqual(client.recv(1), b"")
            state.query.assert_not_called()
            state.publish.assert_not_called()
            self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
