# Test phase 90: Pure Python stdlib modules (html, urllib.parse, email.mime.text, unittest)
import html
import urllib.parse
from email.mime.text import MIMEText
import unittest

passed = 0
failed = 0

def check(cond, msg):
    global passed, failed
    if cond:
        passed += 1
    else:
        failed += 1
        print("FAIL: " + msg)

# --- html.escape ---
# 1
check(html.escape("<b>hello</b>") == "&lt;b&gt;hello&lt;/b&gt;", "html.escape tags")
# 2
check(html.escape('a&b') == "a&amp;b", "html.escape ampersand")
# 3
check(html.escape('"quoted"') == "&quot;quoted&quot;", "html.escape quotes")
# 4 - quote=False should not escape quotes
check(html.escape('"hi"', False) == '"hi"', "html.escape quote=False")

# --- html.unescape ---
# 5
check(html.unescape("&lt;b&gt;") == "<b>", "html.unescape lt/gt")
# 6
check(html.unescape("&amp;") == "&", "html.unescape amp")
# 7
check(html.unescape("&quot;") == '"', "html.unescape quot")
# 8
check(html.unescape("&#39;") == "'", "html.unescape &#39;")

# --- urllib.parse.urlparse ---
# 9  (returns tuple: scheme, netloc, path, params, query, fragment)
r = urllib.parse.urlparse("https://example.com/path?q=1#frag")
check(r[0] == "https", "urlparse scheme")
# 10
check(r[1] == "example.com", "urlparse netloc")
# 11
check(r[2] == "/path", "urlparse path")
# 12
check(r[4] == "q=1", "urlparse query")
# 13
check(r[5] == "frag", "urlparse fragment")

# --- urllib.parse.urlencode ---
# 14
encoded = urllib.parse.urlencode({"key": "value", "a": "b"})
check("key=value" in encoded, "urlencode basic key=value")
# 15
check("a=b" in encoded, "urlencode basic a=b")

# --- urllib.parse.quote / unquote ---
# 16
check(urllib.parse.quote("hello world") == "hello%20world", "quote space")
# 17
check(urllib.parse.unquote("hello%20world") == "hello world", "unquote space")
# 18
check(urllib.parse.quote("/path/to") == "/path/to", "quote safe slash")
# 19
check(urllib.parse.quote("/path/to", safe="") == "%2Fpath%2Fto", "quote no safe")

# --- urllib.parse.parse_qs ---
# 20
qs = urllib.parse.parse_qs("a=1&b=2&a=3")
check("a" in qs, "parse_qs key a exists")
# 21
check("b" in qs, "parse_qs key b exists")

# --- email.mime.text.MIMEText ---
# 22
msg = MIMEText("Hello, world!")
s = msg.as_string()
check("text/plain" in s, "MIMEText content type in string")
# 23
check("Hello, world!" in s, "MIMEText body in string")

# --- unittest.TestCase ---
# 24
tc = unittest.TestCase()
tc.assertEqual(1, 1)
check(True, "TestCase.assertEqual no error")
# 25
tc.assertTrue(True)
tc.assertFalse(False)
tc.assertIn(1, [1, 2, 3])
tc.assertNotIn(4, [1, 2, 3])
tc.assertIsNone(None)
tc.assertIsNotNone(42)
tc.assertGreater(10, 5)
tc.assertLess(5, 10)
check(True, "TestCase multiple assertions passed")

print("test_phase90: " + str(passed) + " passed, " + str(failed) + " failed")
if failed > 0:
    raise Exception("test_phase90 had failures")
