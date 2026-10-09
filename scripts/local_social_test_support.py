"""Scoped HTTP server ownership for the real local-social fixture tests."""

from contextlib import contextmanager
from threading import Thread

from scripts.fixture_connections import OwnedHTTPServer


@contextmanager
def blossom_server(handler, state):
    handler.state = state
    server = OwnedHTTPServer(("127.0.0.1", 0), handler)
    state.blossom_port = server.server_address[1]
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        if thread.is_alive():
            raise RuntimeError("fixture server teardown is incomplete")
