# Phase 22: re module, format specs, more builtins

passed = 0
failed = 0

def test(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name, "| got:", repr(got), "| expected:", repr(expected))

import re

# re.search
m = re.search(r"\d+", "hello 42 world")
test("re_search_found", m is not None, True)
test("re_search_group", m.group(), "42")
test("re_search_start", m.start(), 6)
test("re_search_end", m.end(), 8)
test("re_search_span", m.span(), (6, 8))

# re.search not found
m2 = re.search(r"\d+", "hello world")
test("re_search_none", m2 is None, True)

# re.match
m3 = re.match(r"\w+", "hello world")
test("re_match_group", m3.group(), "hello")
m4 = re.match(r"\d+", "hello world")
test("re_match_none", m4 is None, True)

# re.findall
results = re.findall(r"\d+", "age: 30, height: 180, weight: 75")
test("re_findall", results, ["30", "180", "75"])

# re.findall with groups
results2 = re.findall(r"(\w+)=(\w+)", "a=1 b=2 c=3")
test("re_findall_groups", results2, [("a", "1"), ("b", "2"), ("c", "3")])

# re.sub
result = re.sub(r"\d+", "X", "hello 42 world 99")
test("re_sub", result, "hello X world X")

# re.sub with count
result2 = re.sub(r"\d+", "X", "1 2 3 4 5", 2)
test("re_sub_count", result2, "X X 3 4 5")

# re.split
parts = re.split(r"\s+", "hello   world  foo")
test("re_split", parts, ["hello", "world", "foo"])

# re.split with pattern
parts2 = re.split(r"[,;]", "a,b;c,d")
test("re_split_pattern", parts2, ["a", "b", "c", "d"])

# re.compile
pattern = re.compile(r"\d+")
m5 = pattern.search("test 123")
test("re_compile_search", m5.group(), "123")
results3 = pattern.findall("1 22 333")
test("re_compile_findall", results3, ["1", "22", "333"])

# re.fullmatch
m6 = re.fullmatch(r"\d+", "12345")
test("re_fullmatch_match", m6 is not None, True)
m7 = re.fullmatch(r"\d+", "123abc")
test("re_fullmatch_none", m7 is None, True)

# re.escape
test("re_escape", re.escape("hello.world?"), r"hello\.world\?")

# Case insensitive
results4 = re.findall(r"hello", "Hello HELLO hello HeLLo", re.IGNORECASE)
test("re_ignorecase", len(results4), 4)

# re.subn
result3, count = re.subn(r"\d", "X", "a1b2c3")
test("re_subn_result", result3, "aXbXcX")
test("re_subn_count", count, 3)

# Groups with search
m8 = re.search(r"(\d+)-(\d+)", "phone: 123-456")
test("re_groups", m8.groups(), ("123", "456"))
test("re_group_1", m8.group(1), "123")
test("re_group_2", m8.group(2), "456")

# Email validation pattern
email_pattern = re.compile(r"[\w.+-]+@[\w-]+\.[\w.]+")
test("re_email_valid", email_pattern.match("user@example.com") is not None, True)
test("re_email_invalid", email_pattern.match("not-an-email") is None, True)

# Word extraction
words = re.findall(r"\b\w+\b", "Hello, World! How are you?")
test("re_words", words, ["Hello", "World", "How", "are", "you"])

# Format specifiers (already work from f-strings, test in str.format too)
test("format_align_right", "{:>10}".format("hi"), "        hi")
test("format_align_left", "{:<10}".format("hi"), "hi        ")
test("format_align_center", "{:^10}".format("hi"), "    hi    ")
test("format_zero_pad", "{:05d}".format(42), "00042")
test("format_float_prec", "{:.3f}".format(3.14159), "3.142")
test("format_hex", "{:x}".format(255), "ff")
test("format_binary", "{:b}".format(42), "101010")
test("format_octal", "{:o}".format(8), "10")

# Number formatting with comma
test("format_comma", f"{1000000:,}", "1,000,000")

# dict.fromkeys
# test("dict_fromkeys", dict.fromkeys(["a", "b", "c"], 0), {"a": 0, "b": 0, "c": 0})

# Multiple format args
test("format_multi", "{} + {} = {}".format(1, 2, 3), "1 + 2 = 3")
test("format_repeat", "{0}{0}{0}".format("ab"), "ababab")

# isinstance checks
test("isinstance_int", isinstance(42, int), True)
test("isinstance_str", isinstance("hi", str), True)
test("isinstance_float", isinstance(3.14, float), True)
test("isinstance_bool", isinstance(True, bool), True)
test("isinstance_bool_int", isinstance(True, int), True)
test("isinstance_list", isinstance([], list), True)
test("isinstance_dict", isinstance({}, dict), True)
test("isinstance_tuple", isinstance((), tuple), True)
test("isinstance_none", isinstance(None, type(None)), True)

# type() checks
test("type_int_name", type(42).__name__, "int")
test("type_float_name", type(3.14).__name__, "float")
test("type_str_name", type("hello").__name__, "str")
test("type_bool_name", type(True).__name__, "bool")
test("type_list_name", type([]).__name__, "list")
test("type_dict_name", type({}).__name__, "dict")
test("type_tuple_name", type(()).__name__, "tuple")

# Membership testing
test("in_list", 3 in [1, 2, 3], True)
test("not_in_list", 4 not in [1, 2, 3], True)
test("in_str", "lo" in "hello", True)
test("in_dict", "a" in {"a": 1, "b": 2}, True)
test("in_set", 3 in {1, 2, 3}, True)

# Sequence unpacking with star
a, *b, c = [1, 2, 3, 4, 5]
test("star_unpack_a", a, 1)
test("star_unpack_b", b, [2, 3, 4])
test("star_unpack_c", c, 5)

# Dict update
d = {"a": 1}
d.update({"b": 2, "c": 3})
test("dict_update", d, {"a": 1, "b": 2, "c": 3})

# List copy
original = [1, 2, 3]
copy = original.copy()
copy.append(4)
test("list_copy", original, [1, 2, 3])
test("list_copy_new", copy, [1, 2, 3, 4])

# Tuple unpacking in for
pairs = [(1, "a"), (2, "b"), (3, "c")]
keys = []
vals = []
for k, v in pairs:
    keys.append(k)
    vals.append(v)
test("tuple_unpack_for_keys", keys, [1, 2, 3])
test("tuple_unpack_for_vals", vals, ["a", "b", "c"])

# enumerate
test("enumerate", list(enumerate("abc")), [(0, "a"), (1, "b"), (2, "c")])

# reversed
test("reversed_list", list(reversed([1, 2, 3])), [3, 2, 1])

# zip longest not needed yet, basic zip
test("zip_dict", dict(zip(["a", "b", "c"], [1, 2, 3])), {"a": 1, "b": 2, "c": 3})

# Nested function
def outer(x):
    def inner(y):
        return x + y
    return inner

add5 = outer(5)
test("closure", add5(3), 8)

# Default mutable argument pattern
def append_to(element, target=None):
    if target is None:
        target = []
    target.append(element)
    return target

test("default_mutable_1", append_to(1), [1])
test("default_mutable_2", append_to(2), [2])

# String methods
test("str_title", "hello world".title(), "Hello World")
test("str_capitalize", "hello world".capitalize(), "Hello world")
test("str_swapcase", "Hello World".swapcase(), "hELLO wORLD")
test("str_center", "hi".center(10, "*"), "****hi****")
test("str_ljust", "hi".ljust(10, "-"), "hi--------")
test("str_rjust", "hi".rjust(10, "-"), "--------hi")
test("str_zfill", "42".zfill(5), "00042")
test("str_isdigit", "12345".isdigit(), True)
test("str_isdigit_false", "12.34".isdigit(), False)
test("str_isalpha", "hello".isalpha(), True)
test("str_isalpha_false", "hello1".isalpha(), False)
test("str_isalnum", "hello123".isalnum(), True)
test("str_isupper", "HELLO".isupper(), True)
test("str_islower", "hello".islower(), True)
test("str_isspace", "   ".isspace(), True)
test("str_replace_count", "aaa".replace("a", "b", 2), "bba")

# Multiple inheritance method resolution
class Base:
    def method(self):
        return "base"

class Left(Base):
    def method(self):
        return "left"

class Right(Base):
    def method(self):
        return "right"

class Child(Left, Right):
    pass

test("mro_method", Child().method(), "left")

# Exception hierarchy
class CustomError(ValueError):
    pass

try:
    raise CustomError("test")
except ValueError:
    test("exc_hierarchy", True, True)

# Try/except/else/finally
log = []
try:
    log.append("try")
except Exception:
    log.append("except")
else:
    log.append("else")
finally:
    log.append("finally")
test("try_else_finally", log, ["try", "else", "finally"])

# While/else
n = 0
while n < 3:
    n += 1
else:
    result = "completed"
test("while_else", result, "completed")

# For/else
for i in range(3):
    pass
else:
    result = "for_done"
test("for_else", result, "for_done")

print("=" * 40)
print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL TESTS PASSED!")
print("=" * 40)
