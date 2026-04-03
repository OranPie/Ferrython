# test_cpython_compat93.py - More string methods

passed93 = 0
total93 = 0

def check93(desc, got, expected):
    global passed93, total93
    total93 += 1
    if got == expected:
        passed93 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# str.partition
check93("partition found", "hello-world-foo".partition("-"), ("hello", "-", "world-foo"))
check93("partition not found", "hello".partition("-"), ("hello", "", ""))
check93("partition at start", "-hello".partition("-"), ("", "-", "hello"))
check93("partition at end", "hello-".partition("-"), ("hello", "-", ""))
check93("partition multi-char sep", "hello::world".partition("::"), ("hello", "::", "world"))

# str.rpartition
check93("rpartition found", "hello-world-foo".rpartition("-"), ("hello-world", "-", "foo"))
check93("rpartition not found", "hello".rpartition("-"), ("", "", "hello"))
check93("rpartition at start", "-hello".rpartition("-"), ("", "-", "hello"))
check93("rpartition at end", "hello-".rpartition("-"), ("hello", "-", ""))
check93("rpartition multi-char sep", "a::b::c".rpartition("::"), ("a::b", "::", "c"))

# str.expandtabs
check93("expandtabs default", "a\tb".expandtabs(), "a       b")
check93("expandtabs 4", "a\tb".expandtabs(4), "a   b")
check93("expandtabs 1", "a\tb".expandtabs(1), "a b")
check93("expandtabs no tabs", "hello".expandtabs(), "hello")
check93("expandtabs multiple", "a\tb\tc".expandtabs(4), "a   b   c")

# str.zfill
check93("zfill positive", "42".zfill(5), "00042")
check93("zfill negative", "-42".zfill(5), "-0042")
check93("zfill already long", "12345".zfill(3), "12345")
check93("zfill empty", "".zfill(3), "000")
check93("zfill plus sign", "+42".zfill(6), "+00042")
check93("zfill zero width", "abc".zfill(0), "abc")

# str.ljust
check93("ljust basic", "hi".ljust(5), "hi   ")
check93("ljust with fill", "hi".ljust(5, "*"), "hi***")
check93("ljust already long", "hello".ljust(3), "hello")
check93("ljust exact", "hi".ljust(2), "hi")

# str.rjust
check93("rjust basic", "hi".rjust(5), "   hi")
check93("rjust with fill", "hi".rjust(5, "*"), "***hi")
check93("rjust already long", "hello".rjust(3), "hello")

# str.center
check93("center basic", "hi".center(6), "  hi  ")
check93("center with fill", "hi".center(6, "*"), "**hi**")
check93("center odd padding", "hi".center(7, "-"), "---hi--")
check93("center already long", "hello".center(3), "hello")

# str.maketrans and str.translate
table1 = str.maketrans("abc", "xyz")
check93("translate simple", "abcdef".translate(table1), "xyzdef")

table2 = str.maketrans("", "", "aeiou")
check93("translate delete chars", "hello world".translate(table2), "hll wrld")

table3 = str.maketrans({"a": "X", "b": "Y"})
check93("translate with dict", "abc".translate(table3), "XYc")

table4 = str.maketrans({"a": None})
check93("translate delete with None", "banana".translate(table4), "bnn")

# str.encode / decode
check93("encode utf-8", "hello".encode("utf-8"), b"hello")
check93("decode utf-8", b"hello".decode("utf-8"), "hello")
check93("encode ascii", "abc".encode("ascii"), b"abc")
check93("decode ascii", b"abc".decode("ascii"), "abc")
check93("encode roundtrip", "test".encode("utf-8").decode("utf-8"), "test")

# str.splitlines
check93("splitlines basic", "a\nb\nc".splitlines(), ["a", "b", "c"])
check93("splitlines keepends", "a\nb\n".splitlines(True), ["a\n", "b\n"])
check93("splitlines mixed", "a\nb\r\nc".splitlines(), ["a", "b", "c"])
check93("splitlines empty", "".splitlines(), [])

# str.isdigit, isalpha, isalnum, isspace
check93("isdigit true", "12345".isdigit(), True)
check93("isdigit false", "123a".isdigit(), False)
check93("isalpha true", "hello".isalpha(), True)
check93("isalpha false", "hello1".isalpha(), False)
check93("isalnum true", "hello123".isalnum(), True)
check93("isalnum false", "hello 123".isalnum(), False)
check93("isspace true", "  \t\n".isspace(), True)
check93("isspace false", " a ".isspace(), False)

# str.title
check93("title basic", "hello world".title(), "Hello World")
check93("title apostrophe", "they're bill's friends".title(), "They'Re Bill'S Friends")

# str.swapcase
check93("swapcase mixed", "Hello World".swapcase(), "hELLO wORLD")
check93("swapcase all lower", "abc".swapcase(), "ABC")

# str.count
check93("count basic", "banana".count("an"), 2)
check93("count not found", "hello".count("xyz"), 0)
check93("count with range", "banana".count("a", 2, 5), 1)

# str.index and find
check93("find found", "hello world".find("world"), 6)
check93("find not found", "hello".find("xyz"), -1)
check93("rfind found", "abcabc".rfind("abc"), 3)

try:
    _ = "hello".index("xyz")
    check93("index not found raises ValueError", False, True)
except ValueError:
    check93("index not found raises ValueError", True, True)

print(f"Tests: {total93} | Passed: {passed93} | Failed: {total93 - passed93}")
