# test_phase108.py — http.cookies, wsgiref, new module tests

# ── http.cookies ──
import http.cookies

c = http.cookies.SimpleCookie()
c["session"] = "abc123"
c["user"] = "alice"
assert "session" in c.keys(), f"Expected session in keys: {c.keys()}"
assert "user" in c.keys(), f"Expected user in keys: {c.keys()}"

morsel = c["session"]
assert morsel.key == "session", f"Expected key='session', got {morsel.key}"
assert morsel.value == "abc123", f"Expected value='abc123', got {morsel.value}"
print("PASS: http.cookies SimpleCookie")

# Load from string
c2 = http.cookies.SimpleCookie()
c2.load("name=Bob; age=30")
assert "name" in c2.keys()
assert "age" in c2.keys()
print("PASS: http.cookies load")

# Items
items = c.items()
assert len(items) == 2
print("PASS: http.cookies items")

# ── wsgiref.headers ──
from wsgiref.headers import Headers

h = Headers()
assert len(h) == 0
h["Content-Type"] = "text/html"
assert h["Content-Type"] == "text/html"
assert len(h) == 1

h["X-Custom"] = "value1"
assert len(h) == 2
assert "Content-Type" in h

# get_all
h.add_header("Accept", "text/html")
h.add_header("Accept", "application/json")
accepts = h.get_all("Accept")
assert len(accepts) == 2, f"Expected 2 Accept headers, got {len(accepts)}"
print("PASS: wsgiref.headers Headers")

# setdefault
val = h.setdefault("Server", "Ferrython")
assert val == "Ferrython"
val2 = h.setdefault("Server", "Other")
assert val2 == "Ferrython"  # Already set, returns existing
print("PASS: wsgiref.headers setdefault")

# keys/values/items
keys = h.keys()
assert "Content-Type" in keys
vals = h.values()
assert "text/html" in vals
print("PASS: wsgiref.headers keys/values/items")

# __str__
s = str(h)
assert "Content-Type: text/html" in s
print("PASS: wsgiref.headers __str__")

# __delitem__
del h["X-Custom"]
assert h.get("X-Custom") is None
print("PASS: wsgiref.headers __delitem__")

# ── wsgiref.util ──
from wsgiref.util import setup_testing_defaults, request_uri, guess_scheme, is_hop_by_hop

env = {}
setup_testing_defaults(env)
assert env["REQUEST_METHOD"] == "GET"
assert env["SERVER_NAME"] == "localhost"
assert env["PATH_INFO"] == "/"
assert env["wsgi.version"] == (1, 0)
print("PASS: wsgiref.util setup_testing_defaults")

uri = request_uri(env)
assert uri == "/", f"Expected '/', got {uri}"
env["QUERY_STRING"] = "x=1"
uri2 = request_uri(env)
assert "x=1" in uri2, f"Expected query string in URI: {uri2}"
print("PASS: wsgiref.util request_uri")

assert guess_scheme({}) == "http"
assert guess_scheme({"HTTPS": "on"}) == "https"
print("PASS: wsgiref.util guess_scheme")

assert is_hop_by_hop("connection") == True
assert is_hop_by_hop("Content-Type") == False
print("PASS: wsgiref.util is_hop_by_hop")

# ── wsgiref.validate ──
from wsgiref.validate import validator

def my_app(environ, start_response):
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"Hello World"]

validated = validator(my_app)
env3 = {}
setup_testing_defaults(env3)

collected = []
def mock_sr(status, headers, exc_info=None):
    collected.append(status)

result = validated(env3, mock_sr)
body_parts = []
for chunk in result:
    body_parts.append(chunk)
body = b"".join(body_parts)
assert body == b"Hello World", f"Expected b'Hello World', got {body}"
assert collected[0] == "200 OK"
print("PASS: wsgiref.validate validator")

# ── wsgiref.handlers ──
from wsgiref.handlers import BaseHandler, SimpleHandler

handler = BaseHandler()
assert handler.wsgi_version == (1, 0)
assert handler.wsgi_multithread == True
print("PASS: wsgiref.handlers BaseHandler")

# ── wsgiref.simple_server ──
from wsgiref.simple_server import WSGIServer, WSGIRequestHandler, demo_app

server = WSGIServer(("localhost", 0), WSGIRequestHandler)
server.set_app(demo_app)
assert server.get_app() is demo_app
print("PASS: wsgiref.simple_server WSGIServer")

print("\nAll phase 108 tests passed!")
