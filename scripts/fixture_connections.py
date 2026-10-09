"""Elapsed read bounds and ownership for the isolated loopback fixture."""

from __future__ import annotations

import http.server
import io
import socket
import threading
import time

READ_SECONDS = 15
TEARDOWN_SECONDS = 5
MAX_HANDLERS = 32


def deadline() -> float:
    return time.monotonic() + READ_SECONDS


def remaining(until: float) -> float:
    value = until - time.monotonic()
    if value <= 0:
        raise TimeoutError("fixture read deadline exceeded")
    return value


def recv_exact(stream, length: int, until=None) -> bytes:
    until = deadline() if until is None else until
    value = bytearray()
    while len(value) < length:
        stream.settimeout(remaining(until))
        chunk = stream.recv(length - len(value))
        if not chunk:
            raise ConnectionError("socket closed")
        value.extend(chunk)
    return bytes(value)


def recv_until(stream, marker: bytes, maximum: int) -> bytes:
    until = deadline()
    value = bytearray()
    while marker not in value:
        stream.settimeout(remaining(until))
        chunk = stream.recv(4096)
        if not chunk:
            raise ConnectionError("socket closed")
        value.extend(chunk)
        if len(value) > maximum:
            raise ValueError("request too large")
    return bytes(value)


class DeadlineReader(io.RawIOBase):
    def __init__(self, stream: socket.socket, until: float):
        super().__init__()
        self.stream, self.until = stream, until

    def readable(self) -> bool:
        return True

    def readinto(self, buffer) -> int:
        self.stream.settimeout(remaining(self.until))
        return self.stream.recv_into(buffer)


def http_reader(stream: socket.socket) -> io.BufferedReader:
    return io.BufferedReader(DeadlineReader(stream, deadline()))


class OwnedConnectionsMixin:
    """Register work before starting it, then close and join only owned work."""

    max_handlers = MAX_HANDLERS

    def __init__(self, *args, **kwargs):
        self._owned_lock = threading.Lock()
        self._owned = {}
        self._handler_threads = set()
        self._closing = False
        super().__init__(*args, **kwargs)

    @property
    def active_handlers(self) -> int:
        with self._owned_lock:
            return len(self._owned)

    def process_request(self, request, client_address):
        with self._owned_lock:
            if self._closing or len(self._owned) >= self.max_handlers:
                self.shutdown_request(request)
                return
            thread = threading.Thread(
                target=self._owned_request, args=(request, client_address), daemon=True
            )
            self._owned[request] = thread
            self._handler_threads = {
                item for item in self._handler_threads if item.is_alive()
            }
            self._handler_threads.add(thread)
            try:
                thread.start()
            except BaseException:
                self._owned.pop(request)
                self._handler_threads.remove(thread)
                self.shutdown_request(request)
                raise

    def _owned_request(self, request, client_address):
        try:
            try:
                self.finish_request(request, client_address)
            except (ConnectionError, TimeoutError):
                pass
            except OSError:
                if not self._closing:
                    self.handle_error(request, client_address)
            except Exception:
                self.handle_error(request, client_address)
        finally:
            self.shutdown_request(request)
            with self._owned_lock:
                self._owned.pop(request, None)

    def server_close(self):
        with self._owned_lock:
            self._closing = True
            owned = list(self._owned.items())
            threads = list(self._handler_threads)
        super().server_close()
        for request, _ in owned:
            try:
                request.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            request.close()
        until = time.monotonic() + TEARDOWN_SECONDS
        for thread in threads:
            thread.join(max(0, until - time.monotonic()))
        if any(thread.is_alive() for thread in threads):
            raise RuntimeError("fixture owned handler teardown is incomplete")


class OwnedHTTPServer(OwnedConnectionsMixin, http.server.ThreadingHTTPServer):
    pass
