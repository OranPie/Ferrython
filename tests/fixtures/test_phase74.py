# Phase 74 – Pure Python stdlib (reprlib) + Rust stdlib verification

passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL:", name, "got:", got, "expected:", expected)

# ── Task 1: reprlib (pure Python stdlib module from stdlib/Lib/) ──────

import reprlib

# Basic list repr — 8 items, maxlist=6 so it truncates
r = reprlib.repr([1, 2, 3, 4, 5, 6, 7, 8])
check("reprlib list truncates", '...' in r, True)

# Short list should not truncate
r2 = reprlib.repr([1, 2, 3])
check("reprlib short list", r2, '[1, 2, 3]')

# Repr object attributes
rep = reprlib.Repr()
check("reprlib maxlist", rep.maxlist, 6)
check("reprlib maxdict", rep.maxdict, 4)
check("reprlib maxlevel", rep.maxlevel, 6)

# Dict repr
d = {1: 'a', 2: 'b', 3: 'c'}
rd = rep.repr(d)
check("reprlib dict", '{' in rd, True)

# Tuple repr
check("reprlib tuple", rep.repr((1, 2)), '(1, 2)')
check("reprlib tuple single", rep.repr((1,)), '(1,)')

print("Task 1 (reprlib): OK")

# ── Task 2: string module (Rust builtin) ─────────────────────────────

from string import Template

t = Template("$name is $age")
result = t.substitute(name="Alice", age=30)
check("string Template substitute", result, "Alice is 30")

# safe_substitute with missing key
t2 = Template("$name is $missing")
result2 = t2.safe_substitute(name="Bob")
check("string Template safe_substitute", "$missing" in result2, True)
check("string Template safe_substitute name", "Bob" in result2, True)

# String constants
import string
check("string digits", string.digits, "0123456789")
check("string ascii_lowercase", string.ascii_lowercase, "abcdefghijklmnopqrstuvwxyz")

print("Task 2 (string): OK")

# ── Task 3: textwrap module (Rust builtin) ────────────────────────────

import textwrap

dedented = textwrap.dedent("    hello\n    world")
check("textwrap dedent", dedented, "hello\nworld")

wrapped = textwrap.fill("This is a long sentence that should be wrapped at some point", width=20)
check("textwrap fill has newlines", '\n' in wrapped, True)

print("Task 3 (textwrap): OK")

# ── Task 4: typing_extensions module (Rust builtin) ──────────────────

import typing_extensions

# Should at least import without error
check("typing_extensions imported", True, True)

print("Task 4 (typing_extensions): OK")

# ── Summary ───────────────────────────────────────────────────────────

print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
