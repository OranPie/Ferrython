# test_phase114.py — email.mime, _weakrefset, _markupbase, _compat_pickle

# ── email.mime.base ──
from email.mime.base import MIMEBase

msg = MIMEBase('application', 'octet-stream')
assert msg.get_content_type() == 'application/octet-stream'
assert msg.get_content_maintype() == 'application'
assert msg.get_content_subtype() == 'octet-stream'
msg.set_payload("test data")
assert msg.get_payload() == "test data"

# Headers
msg['Subject'] = 'Test'
assert msg['Subject'] == 'Test'

# as_string
s = msg.as_string()
assert 'Content-Type' in s

# ── email.mime.multipart ──
from email.mime.multipart import MIMEMultipart

multi = MIMEMultipart()
ct = multi.get_content_type()
assert 'multipart' in ct

# Attach parts
from email.mime.text import MIMEText
text_part = MIMEText("Hello World", "plain")
multi.attach(text_part)

payload = multi.get_payload()
assert payload is not None

# ── _weakrefset ──
from _weakrefset import WeakSet

ws = WeakSet()
ws.add("hello")
ws.add("world")
assert len(ws) == 2
ws.discard("hello")
assert len(ws) == 1

# ── _compat_pickle ──
import _compat_pickle
assert 'copy_reg' in _compat_pickle.IMPORT_MAPPING
assert _compat_pickle.IMPORT_MAPPING['copy_reg'] == 'copyreg'

# ── _markupbase ──
from _markupbase import ParserBase
assert hasattr(ParserBase, 'getpos')
assert hasattr(ParserBase, 'updatepos')

print("phase114: all tests passed")
