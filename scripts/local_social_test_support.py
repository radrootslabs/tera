"""Scoped HTTP server ownership for the real local-social fixture tests."""

from contextlib import contextmanager
from http.server import ThreadingHTTPServer
from threading import Thread


@contextmanager
def blossom_server(handler, state):
    handler.state = state
    server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    state.blossom_port = server.server_address[1]
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
