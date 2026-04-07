# test_phase126.py — datetime.tzinfo, sys additions, os.waitpid/W*, email.mime multipart

import sys
import os
import datetime

# ── sys module additions ──
assert hasattr(sys, 'hexversion'), "sys.hexversion missing"
assert isinstance(sys.hexversion, int), "sys.hexversion should be int"
assert sys.hexversion >= 0x030800f0, f"hexversion too low: {hex(sys.hexversion)}"

assert hasattr(sys, 'warnoptions'), "sys.warnoptions missing"
assert isinstance(sys.warnoptions, list), "sys.warnoptions should be list"

assert hasattr(sys, 'path_importer_cache'), "sys.path_importer_cache missing"
assert isinstance(sys.path_importer_cache, dict), "sys.path_importer_cache should be dict"

assert callable(sys.displayhook), "sys.displayhook should be callable"
assert callable(sys.breakpointhook), "sys.breakpointhook should be callable"

# ── datetime.tzinfo default ──
dt = datetime.datetime.now()
assert hasattr(dt, 'tzinfo'), "datetime.datetime.now() missing tzinfo"
assert dt.tzinfo is None, f"datetime.now() tzinfo should be None, got {dt.tzinfo}"

d = datetime.date.today()
# date objects should also have tzinfo=None conceptually (CPython doesn't, but we set it)

# ── os W* macros ──
assert hasattr(os, 'WNOHANG'), "os.WNOHANG missing"
assert os.WNOHANG == 1
assert hasattr(os, 'WUNTRACED'), "os.WUNTRACED missing"

assert callable(os.WIFEXITED), "os.WIFEXITED should be callable"
assert callable(os.WEXITSTATUS), "os.WEXITSTATUS should be callable"
assert callable(os.WIFSIGNALED), "os.WIFSIGNALED should be callable"
assert callable(os.WTERMSIG), "os.WTERMSIG should be callable"
assert callable(os.WIFSTOPPED), "os.WIFSTOPPED should be callable"
assert callable(os.WSTOPSIG), "os.WSTOPSIG should be callable"

# Test W* macros with known status values
# Normal exit status 0: on Linux this is 0x0000
assert os.WIFEXITED(0) == True
assert os.WEXITSTATUS(0) == 0
# Normal exit status 1: on Linux this is 0x0100
assert os.WIFEXITED(256) == True
assert os.WEXITSTATUS(256) == 1
# Signal kill (SIGKILL=9): on Linux status is 9
assert os.WIFSIGNALED(9) == True
assert os.WTERMSIG(9) == 9

# ── email.mime multipart serialization ──
from email.mime.text import MIMEText
from email.mime.multipart import MIMEMultipart

msg = MIMEMultipart()
msg['Subject'] = 'Test Email'
msg['From'] = 'sender@example.com'
msg['To'] = 'recipient@example.com'

part1 = MIMEText('Hello, plain text!', 'plain')
part2 = MIMEText('<h1>Hello HTML</h1>', 'html')
msg.attach(part1)
msg.attach(part2)

s = str(msg)
assert 'boundary=' in s, "Multipart should have boundary"
assert '--' in s, "Multipart should have boundary markers"
assert 'Hello, plain text!' in s, "Multipart should contain plain text part"
assert '<h1>Hello HTML</h1>' in s, "Multipart should contain HTML part"
assert 'Subject: Test Email' in s, "Multipart should contain Subject header"
assert 'From: sender@example.com' in s, "Multipart should contain From header"
assert 'Content-Type: multipart/mixed' in s, "Should have multipart/mixed content type"
assert 'Content-Type: text/plain' in s, "Should have text/plain part"
assert 'Content-Type: text/html' in s, "Should have text/html part"

# Single MIMEText serialization
single = MIMEText('Just text', 'plain')
ss = str(single)
assert 'Content-Type: text/plain' in ss
assert 'Just text' in ss

# email.message.Message basics
from email.message import Message
m = Message()
m['Content-Type'] = 'text/plain'
m.set_payload('body text')
assert m.get_payload() == 'body text' or str(m.get_payload()) == 'body text'
assert m['Content-Type'] == 'text/plain' or str(m['Content-Type']) == 'text/plain'

print("test_phase126 passed")
