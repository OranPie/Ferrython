# Phase 140: marshal, __builtins__, bytearray.__setitem__, Fraction(Decimal), missing modules
import marshal
assert marshal.loads(marshal.dumps(42)) == 42
assert marshal.loads(marshal.dumps("hello")) == "hello"
assert marshal.loads(marshal.dumps([1, 2, 3])) == [1, 2, 3]
assert marshal.loads(marshal.dumps({"a": 1})) == {"a": 1}
assert marshal.loads(marshal.dumps((1, 2))) == (1, 2)
assert marshal.loads(marshal.dumps(3.14)) == 3.14
assert marshal.loads(marshal.dumps(True)) is True
assert marshal.loads(marshal.dumps(None)) is None
assert marshal.loads(marshal.dumps(b"bytes")) == b"bytes"

import tabnanny
import pyclbr
import asyncio.streams

assert "__builtins__" in dir()

ba = bytearray(b"hello")
ba.__setitem__(0, 72)
assert bytes(ba) == b"Hello"

from fractions import Fraction
from decimal import Decimal
assert Fraction(Decimal("0.25")) == Fraction(1, 4)
assert Fraction(3.14159).limit_denominator(100) == Fraction(311, 99)

from email.message import EmailMessage
msg = EmailMessage()
msg.set_content("Hello World")
assert "Hello World" in msg.get_content()

print("phase140: all checks passed")
