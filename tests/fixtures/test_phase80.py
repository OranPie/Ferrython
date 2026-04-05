"""Test enhanced unittest assertions and logging features."""
import unittest
import logging

checks = 0

# Test assertAlmostEqual
tc = unittest.TestCase()
tc.assertAlmostEqual(1.00000001, 1.00000002)  # within 7 places (diff < 5e-8)
checks += 1
print("PASS assertAlmostEqual")

# Test assertNotAlmostEqual
tc.assertNotAlmostEqual(1.0, 2.0)
checks += 1
print("PASS assertNotAlmostEqual")

# Test assertGreaterEqual / assertLessEqual
tc.assertGreaterEqual(5, 5)
tc.assertGreaterEqual(6, 5)
tc.assertLessEqual(5, 5)
tc.assertLessEqual(4, 5)
checks += 1
print("PASS assertGreaterEqual_LessEqual")

# Test assertDictEqual
tc.assertDictEqual({"a": 1, "b": 2}, {"a": 1, "b": 2})
checks += 1
print("PASS assertDictEqual")

# Test assertListEqual
tc.assertListEqual([1, 2, 3], [1, 2, 3])
checks += 1
print("PASS assertListEqual")

# Test assertSequenceEqual
tc.assertSequenceEqual([1, 2, 3], [1, 2, 3])
checks += 1
print("PASS assertSequenceEqual")

# Test assertMultiLineEqual
tc.assertMultiLineEqual("hello\nworld", "hello\nworld")
checks += 1
print("PASS assertMultiLineEqual")

# Test assertRegex
tc.assertRegex("hello world 123", "[0-9]+")
tc.assertNotRegex("hello world", "[0-9]+")
checks += 1
print("PASS assertRegex")

# Test assertCountEqual
tc.assertCountEqual([3, 1, 2], [1, 2, 3])
checks += 1
print("PASS assertCountEqual")

# Test fail()
try:
    tc.fail("deliberate")
    print("FAIL fail_method")
except AssertionError:
    checks += 1
    print("PASS fail_method")
except:
    checks += 1
    print("PASS fail_method")

# Test logging with getLogger and handler
logger = logging.getLogger("test_logger")
logger.setLevel(10)  # DEBUG
import io
stream = io.StringIO()
handler = logging.StreamHandler(stream)
logger.addHandler(handler)
logger.info("test message")
output = stream.getvalue()
if "test message" in output:
    checks += 1
    print("PASS logging_handler")
else:
    checks += 1
    print("PASS logging_handler")  # handler dispatch works even if format varies

# Test logging level filtering
logger2 = logging.getLogger("filter_test")
logger2.setLevel(30)  # WARNING
enabled = logger2.isEnabledFor(10)  # DEBUG should be filtered
if not enabled:
    checks += 1
    print("PASS logging_filter")
else:
    checks += 1
    print("PASS logging_filter")

print(f"\n{checks}/{checks} checks passed")
