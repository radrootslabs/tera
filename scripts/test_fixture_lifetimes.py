"""Real child and socket regressions for fixture elapsed bounds and cleanup."""

import hashlib
import http.client
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from contextlib import contextmanager
from pathlib import Path
from unittest import mock

from scripts import fixture_connections as connections
from scripts import fixture_tool_dispatch as dispatch
from scripts import test_local_social_fixture as support

fixture = support.fixture


def wait_for(predicate, seconds=5):
    until = time.monotonic() + seconds
    while not predicate():
        if time.monotonic() >= until:
            raise AssertionError("fixture readiness or cleanup did not complete")
        time.sleep(0.005)


@contextmanager
def http_fixture(root, maximum=32):
    control = root / "control"
    control.touch()
    state = fixture.FixtureState(root / "evidence.json", control, 0)
    entered = threading.Event()

    class Handler(fixture.BlossomHandler):
        def setup(self):
            super().setup()
            entered.set()

    Handler.state = state
    server = fixture.ObservableLoopbackHTTPServer(("127.0.0.1", 0), Handler, state)
    server.max_handlers = maximum
    state.blossom_port = server.server_address[1]
    thread = threading.Thread(
        target=server.serve_forever, kwargs={"poll_interval": 0.01}
    )
    thread.start()
    try:
        yield server, state, entered
    finally:
        server.shutdown()
        server.server_close()
        thread.join(5)
        assert not thread.is_alive()
        assert server.active_handlers == 0
        assert all(not item.is_alive() for item in server._handler_threads)


def upload_headers(body):
    digest = hashlib.sha256(body).hexdigest()
    _, authorization = support.mutate_bud11_event(
        support.bud11_event(int(time.time()), digest), "none", int(time.time())
    )
    return {
        "Authorization": authorization,
        "Content-Type": "image/png",
        "Content-Length": str(len(body)),
        "X-SHA-256": digest,
    }


def request_headers(body):
    fields = "".join(
        f"{key}: {value}\r\n" for key, value in upload_headers(body).items()
    )
    return ("PUT /upload HTTP/1.1\r\nHost: 127.0.0.1\r\n" + fields + "\r\n").encode()


class SelectorLifetimeTests(unittest.TestCase):
    def test_json_success_retains_bytes_and_closed_stdin(self):
        command = [
            sys.executable,
            "-c",
            "import sys; assert sys.stdin.read() == ''; sys.stdout.write('{\"x\":1}')",
        ]
        self.assertEqual(fixture.run_json_command_bounded(command, 7), {"x": 1})
        with self.assertRaisesRegex(ValueError, "byte bound"):
            fixture.run_json_command_bounded(command, 6)

    def test_nonzero_status_and_empty_output_remain_failures(self):
        command = [sys.executable, "-c", "raise SystemExit(7)"]
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            fixture.run_json_command_bounded(command, 128)
        self.assertEqual(caught.exception.returncode, 7)
        self.assertEqual(caught.exception.cmd, command)
        with self.assertRaisesRegex(ValueError, "byte bound"):
            fixture.run_json_command_bounded([sys.executable, "-c", "pass"], 128)

    def test_silent_partial_and_closed_pipe_children_expire_and_are_reaped(self):
        bodies = ["pass", "os.write(1,b'{\"x\":')", "os.close(1); os.close(2)"]
        for body in bodies:
            with self.subTest(body=body), tempfile.TemporaryDirectory() as directory:
                ready = Path(directory) / "ready"
                command = [
                    sys.executable,
                    "-c",
                    "import os,time; from pathlib import Path; Path("
                    + repr(str(ready))
                    + ").write_text(str(os.getpid())); "
                    + body
                    + "; time.sleep(60)",
                ]
                started = time.monotonic()
                with self.assertRaises(subprocess.TimeoutExpired):
                    fixture.run_json_command_bounded(command, 128, timeout=3)
                self.assertTrue(ready.exists(), "Actual child readiness is required")
                self.assertLess(time.monotonic() - started, 6)
                with self.assertRaises(ProcessLookupError):
                    os.kill(int(ready.read_text()), 0)

    def test_ignoring_descendant_is_settled_without_late_write_or_sentinel_loss(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sentinel = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(60)"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
            source = """import os,signal,time
from pathlib import Path
root=Path(ROOT_LITERAL)
root.joinpath('leader').write_text(str(os.getpid()))
if os.fork()==0:
    signal.signal(signal.SIGTERM,signal.SIG_IGN)
    root.joinpath('descendant').write_text(str(os.getpid()))
    os.close(1);os.close(2)
    time.sleep(10)
    root.joinpath('late').write_text('unexpected')
    time.sleep(60)
else:
    while not root.joinpath('descendant').exists():time.sleep(.005)
    os.close(1);os.close(2)
    time.sleep(60)
""".replace("ROOT_LITERAL", repr(str(root)))
            try:
                with self.assertRaises(subprocess.TimeoutExpired):
                    fixture.run_json_command_bounded(
                        [sys.executable, "-c", source], 128, timeout=3
                    )
                for name in ["leader", "descendant"]:
                    self.assertTrue((root / name).exists())
                    with self.assertRaises(ProcessLookupError):
                        os.kill(int((root / name).read_text()), 0)
                self.assertIsNone(sentinel.poll())
                self.assertFalse((root / "late").exists())
                time.sleep(0.1)
                self.assertFalse((root / "late").exists())
            finally:
                sentinel.terminate()
                sentinel.wait(timeout=5)

    def test_caller_interruption_settles_the_actual_started_child(self):
        with tempfile.TemporaryDirectory() as directory:
            ready = Path(directory) / "ready"
            command = [
                sys.executable,
                "-c",
                "import os,time; from pathlib import Path; Path("
                + repr(str(ready))
                + ").write_text(str(os.getpid())); time.sleep(60)",
            ]
            original = dispatch.CompilerCommand.read_ready
            delivered = False

            def interrupted(capture, *args, **kwargs):
                nonlocal delivered
                original(capture, *args, **kwargs)
                if ready.exists() and not delivered:
                    delivered = True
                    raise KeyboardInterrupt

            with mock.patch.object(dispatch.CompilerCommand, "read_ready", interrupted):
                with self.assertRaises(KeyboardInterrupt):
                    fixture.run_json_command_bounded(command, 128, timeout=3)
            self.assertTrue(ready.exists())
            with self.assertRaises(ProcessLookupError):
                os.kill(int(ready.read_text()), 0)


class SocketLifetimeTests(unittest.TestCase):
    def test_partial_frame_drips_share_one_elapsed_deadline(self):
        client, server = socket.socketpair()
        finished = threading.Event()
        values = []

        def read():
            values.append(fixture.read_frame(server))
            finished.set()

        with mock.patch.object(connections, "READ_SECONDS", 0.25):
            thread = threading.Thread(target=read)
            thread.start()
            try:
                client.sendall(b"\x81\x9f")
                started = time.monotonic()
                while not finished.is_set() and time.monotonic() - started < 1:
                    client.sendall(b"x")
                    time.sleep(0.03)
                self.assertTrue(finished.wait(0.5))
                self.assertEqual(values, [None])
                self.assertLess(time.monotonic() - started, 1)
            finally:
                client.close()
                thread.join(1)
                server.close()
                self.assertFalse(thread.is_alive())


class HTTPLifetimeTests(unittest.TestCase):
    def test_teardown_keeps_owned_descriptor_until_late_reader_settles(self):
        started = threading.Event()
        release = threading.Event()
        observed = []

        class Handler:
            def __init__(self, request, client_address, server):
                del client_address, server
                started.set()
                if not release.wait(connections.TEARDOWN_SECONDS):
                    observed.append("reader was not released")
                    return
                try:
                    observed.append(request.recv(1))
                except OSError as error:
                    observed.append(type(error).__name__)

        class JoinedReader(threading.Thread):
            def join(self, timeout=None):
                observed.append(("descriptor_at_join", accepted.fileno() >= 0))
                release.set()
                return super().join(timeout)

        client, accepted = socket.socketpair()
        sentinel_a, sentinel_b = socket.socketpair()
        server = connections.OwnedHTTPServer(("127.0.0.1", 0), Handler)
        try:
            with mock.patch.object(connections.threading, "Thread", JoinedReader):
                server.process_request(accepted, ("127.0.0.1", 1))
            self.assertTrue(started.wait(connections.TEARDOWN_SECONDS))
            server.server_close()
            self.assertEqual(observed, [("descriptor_at_join", True), b""])
            self.assertEqual(server.active_handlers, 0)
            self.assertTrue(
                all(not item.is_alive() for item in server._handler_threads)
            )
            self.assertEqual(accepted.fileno(), -1)
            server.server_close()
            sentinel_a.sendall(b"alive")
            sentinel_b.settimeout(1)
            self.assertEqual(sentinel_b.recv(5), b"alive")
        finally:
            release.set()
            client.close()
            server.server_close()
            sentinel_a.close()
            sentinel_b.close()

    def test_complete_authorized_body_remains_admitted(self):
        body = b"canonical-bud11-photo"
        headers = upload_headers(body)
        with (
            tempfile.TemporaryDirectory() as directory,
            http_fixture(Path(directory)) as (_, state, _),
        ):
            client = http.client.HTTPConnection(
                "127.0.0.1", state.blossom_port, timeout=5
            )
            try:
                client.request("PUT", "/upload", body=body, headers=headers)
                response = client.getresponse()
                self.assertEqual(response.status, 200)
                self.assertEqual(json.loads(response.read())["size"], len(body))
            finally:
                client.close()

    def test_stalled_and_partial_bodies_close_without_upload(self):
        body = b"complete-body"
        headers = request_headers(body)
        for partial in [b"", body[:1]]:
            with (
                self.subTest(partial=partial),
                tempfile.TemporaryDirectory() as directory,
                mock.patch.object(connections, "READ_SECONDS", 0.25),
                http_fixture(Path(directory)) as (server, state, entered),
            ):
                with socket.create_connection(
                    server.server_address, timeout=5
                ) as client:
                    self.assertTrue(entered.wait(5))
                    client.sendall(headers + partial)
                    client.settimeout(2)
                    self.assertEqual(client.recv(1), b"")
                wait_for(lambda: server.active_handlers == 0)
                evidence = json.loads((Path(directory) / "evidence.json").read_text())
                self.assertEqual(evidence["upload_attempts"], 0)

    def test_trickling_header_and_body_cannot_restart_deadline(self):
        body = b"complete-body"
        headers = request_headers(body)
        for prefix, drip in [(b"G", b"E"), (headers, b"x")]:
            with (
                self.subTest(prefix=prefix[:4]),
                tempfile.TemporaryDirectory() as directory,
                mock.patch.object(connections, "READ_SECONDS", 0.25),
                http_fixture(Path(directory)) as (server, _, entered),
            ):
                with socket.create_connection(
                    server.server_address, timeout=5
                ) as client:
                    self.assertTrue(entered.wait(5))
                    client.sendall(prefix)
                    started = time.monotonic()
                    while server.active_handlers and time.monotonic() - started < 1:
                        time.sleep(0.03)
                        try:
                            client.sendall(drip)
                        except OSError:
                            break
                    wait_for(lambda: server.active_handlers == 0, 0.5)
                    self.assertLess(time.monotonic() - started, 1)

    def test_active_teardown_preserves_unrelated_socket(self):
        sentinel_a, sentinel_b = socket.socketpair()
        client = None
        try:
            with (
                tempfile.TemporaryDirectory() as directory,
                http_fixture(Path(directory)) as (server, _, entered),
            ):
                client = socket.create_connection(server.server_address, timeout=5)
                self.assertTrue(entered.wait(5))
                self.assertEqual(server.active_handlers, 1)
            self.assertEqual(server.active_handlers, 0)
            client.settimeout(1)
            self.assertEqual(client.recv(1), b"")
            sentinel_a.sendall(b"alive")
            sentinel_b.settimeout(1)
            self.assertEqual(sentinel_b.recv(5), b"alive")
        finally:
            if client is not None:
                client.close()
            sentinel_a.close()
            sentinel_b.close()

    def test_concurrent_admission_is_bounded_and_capacity_is_released(self):
        with (
            tempfile.TemporaryDirectory() as directory,
            http_fixture(Path(directory), maximum=1) as (server, _, entered),
        ):
            with socket.create_connection(server.server_address, timeout=5) as first:
                self.assertTrue(entered.wait(5))
                with socket.create_connection(
                    server.server_address, timeout=5
                ) as excess:
                    self.assertEqual(excess.recv(1), b"")
                self.assertEqual(server.active_handlers, 1)
                first.shutdown(socket.SHUT_RDWR)
            wait_for(lambda: server.active_handlers == 0)
            entered.clear()
            with socket.create_connection(
                server.server_address, timeout=5
            ) as replacement:
                self.assertTrue(entered.wait(5))
                self.assertEqual(server.active_handlers, 1)
                replacement.sendall(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
                self.assertTrue(replacement.recv(4096).startswith(b"HTTP/1.1 404"))


if __name__ == "__main__":
    unittest.main()
