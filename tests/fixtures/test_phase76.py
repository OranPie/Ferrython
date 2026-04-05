# Phase 76: More stdlib modules — unittest improvements + json enhancements
passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL:", name, "got:", got, "expected:", expected)

def check_true(name, condition):
    global passed, failed
    if condition:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL:", name)

# ── unittest TestCase assert methods ──
import unittest

class MyTest(unittest.TestCase):
    pass

tc = MyTest()

# assertEqual
tc.assertEqual(1, 1)
tc.assertEqual("hello", "hello")
tc.assertEqual([1, 2], [1, 2])
check("assertEqual_pass", True, True)

caught = False
try:
    tc.assertEqual(1, 2)
except AssertionError:
    caught = True
except:
    caught = True
check("assertEqual_fail_raises", caught, True)

# assertNotEqual
tc.assertNotEqual(1, 2)
tc.assertNotEqual("a", "b")
check("assertNotEqual_pass", True, True)

caught2 = False
try:
    tc.assertNotEqual(1, 1)
except AssertionError:
    caught2 = True
except:
    caught2 = True
check("assertNotEqual_fail_raises", caught2, True)

# assertTrue / assertFalse
tc.assertTrue(True)
tc.assertTrue(1)
tc.assertTrue("nonempty")
check("assertTrue_pass", True, True)

tc.assertFalse(False)
tc.assertFalse(0)
tc.assertFalse("")
check("assertFalse_pass", True, True)

caught3 = False
try:
    tc.assertTrue(False)
except AssertionError:
    caught3 = True
except:
    caught3 = True
check("assertTrue_fail_raises", caught3, True)

caught4 = False
try:
    tc.assertFalse(True)
except AssertionError:
    caught4 = True
except:
    caught4 = True
check("assertFalse_fail_raises", caught4, True)

# assertIsNone / assertIsNotNone
tc.assertIsNone(None)
check("assertIsNone_pass", True, True)

tc.assertIsNotNone(42)
tc.assertIsNotNone("hello")
check("assertIsNotNone_pass", True, True)

caught5 = False
try:
    tc.assertIsNone(42)
except AssertionError:
    caught5 = True
except:
    caught5 = True
check("assertIsNone_fail_raises", caught5, True)

caught6 = False
try:
    tc.assertIsNotNone(None)
except AssertionError:
    caught6 = True
except:
    caught6 = True
check("assertIsNotNone_fail_raises", caught6, True)

# assertIn / assertNotIn
tc.assertIn(1, [1, 2, 3])
tc.assertIn("a", "abc")
check("assertIn_pass", True, True)

tc.assertNotIn(4, [1, 2, 3])
tc.assertNotIn("x", "abc")
check("assertNotIn_pass", True, True)

caught7 = False
try:
    tc.assertIn(99, [1, 2, 3])
except AssertionError:
    caught7 = True
except:
    caught7 = True
check("assertIn_fail_raises", caught7, True)

# assertGreater / assertLess
tc.assertGreater(10, 5)
tc.assertLess(5, 10)
check("assertGreater_less_pass", True, True)

caught8 = False
try:
    tc.assertGreater(1, 10)
except AssertionError:
    caught8 = True
except:
    caught8 = True
check("assertGreater_fail_raises", caught8, True)

# ── json pretty printing and features ──
import json

# json.dumps with indent
data = {"name": "Alice", "age": 30}
pretty = json.dumps(data, indent=2)
check_true("json_indent_has_newlines", "\n" in pretty)
check_true("json_indent_has_name", "Alice" in pretty)

# json.dumps with sort_keys
data2 = {"banana": 2, "apple": 1}
sorted_json = json.dumps(data2, sort_keys=True)
# With sort_keys, "apple" should come before "banana"
apple_pos = sorted_json.find("apple")
banana_pos = sorted_json.find("banana")
check_true("json_sort_keys", apple_pos < banana_pos)

# json.loads with various types
check("json_loads_int", json.loads("42"), 42)
check("json_loads_float", json.loads("3.14"), 3.14)
check("json_loads_string", json.loads('"hello"'), "hello")
check("json_loads_bool_true", json.loads("true"), True)
check("json_loads_bool_false", json.loads("false"), False)
check("json_loads_null", json.loads("null"), None)
check("json_loads_array", json.loads("[1, 2, 3]"), [1, 2, 3])

obj = json.loads('{"key": "value"}')
check("json_loads_object", obj["key"], "value")

# json.loads + json.dumps roundtrip
original = {"list": [1, 2, 3], "nested": {"a": True, "b": None}}
roundtrip = json.loads(json.dumps(original))
check("json_roundtrip_list", roundtrip["list"], [1, 2, 3])
check("json_roundtrip_nested_a", roundtrip["nested"]["a"], True)
check("json_roundtrip_nested_b", roundtrip["nested"]["b"], None)

# JSONEncoder / JSONDecoder
encoder = json.JSONEncoder()
encoded = encoder.encode({"x": 1})
check_true("json_encoder_works", '"x"' in encoded)

decoder = json.JSONDecoder()
decoded = decoder.decode('{"y": 2}')
check("json_decoder_works", decoded["y"], 2)

# JSONDecodeError exists
check_true("json_decode_error_exists", hasattr(json, "JSONDecodeError"))

# json.dumps with separators
compact = json.dumps({"a": 1, "b": 2}, separators=(",", ":"))
check_true("json_separators_no_spaces", " " not in compact)

# ── final report ──
print("Phase 76 Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed > 0:
    raise Exception("TESTS FAILED: " + str(failed))
print("ALL PHASE 76 TESTS PASSED!")
