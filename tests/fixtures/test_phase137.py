# Phase 137: Pickle class instances, typing repr, Counter ops, logging.handlers, urlencode, limit_denominator
import sys
checks = []

# 1. pickle class round-trip
import pickle
class Pt:
    def __init__(self, x, y):
        self.x = x
        self.y = y
p = Pt(3, 4)
p2 = pickle.loads(pickle.dumps(p))
checks.append(("pickle_class_rt", p2.x == 3 and p2.y == 4))

# 2. pickle protocol 0 basic types
data = {"key": [1, 2.5, True, None, "hello"]}
checks.append(("pickle_dict", pickle.loads(pickle.dumps(data)) == data))

# 3. typing repr
import typing
checks.append(("typing_optional_repr", str(typing.Optional[int]) == "typing.Optional[int]"))
checks.append(("typing_list_repr", str(typing.List[int]) == "typing.List[int]"))

# 4. logging.handlers import
import logging.handlers
checks.append(("logging_handlers", hasattr(logging.handlers, "RotatingFileHandler")))

# 5. logging.config import
import logging.config
checks.append(("logging_config", hasattr(logging.config, "dictConfig")))

# 6. urlencode space as +
import urllib.parse
checks.append(("urlencode_plus", urllib.parse.urlencode({"q": "hello world"}) == "q=hello+world"))

# 7. Counter | (union = max)
from collections import Counter
c1 = Counter(a=3, b=1)
c2 = Counter(a=1, b=2)
result = c1 | c2
checks.append(("counter_union", dict(result) == {"a": 3, "b": 2}))

# 8. Counter & (intersection = min)
result2 = c1 & c2
checks.append(("counter_intersect", dict(result2) == {"a": 1, "b": 1}))

# 9. Counter subtraction
result3 = c1 - c2
checks.append(("counter_sub", dict(result3) == {"a": 2}))

# 10. Fraction.limit_denominator
from fractions import Fraction
f = Fraction(3.14159).limit_denominator(100)
checks.append(("frac_limit_denom", str(f) == "311/99"))

# 11. dict | merge (PEP 584)
d1 = {"a": 1, "b": 2}
d2 = {"b": 3, "c": 4}
checks.append(("dict_merge", d1 | d2 == {"a": 1, "b": 3, "c": 4}))

# 12. Flag bitwise ops
from enum import Flag, IntFlag
class Perm(Flag):
    R = 4
    W = 2
    X = 1
rw = Perm.R | Perm.W
checks.append(("flag_or", rw.value == 6))
checks.append(("flag_contains", Perm.R in rw))

# 13. IntFlag arithmetic
class C(IntFlag):
    RED = 1
    GREEN = 2
    BLUE = 4
checks.append(("intflag_add", C.RED + C.GREEN == 3))

# 14. cached_property
from functools import cached_property
class Exp:
    count = 0
    @cached_property
    def val(self):
        Exp.count += 1
        return 42
e = Exp()
_ = e.val
_ = e.val
checks.append(("cached_property", Exp.count == 1))

# 15. dataclass order
from dataclasses import dataclass
@dataclass(order=True)
class Student:
    grade: int
    name: str
s1 = Student(90, "Alice")
s2 = Student(85, "Bob")
checks.append(("dc_order", s2 < s1))

# 16. contextlib.suppress
from contextlib import suppress
with suppress(FileNotFoundError):
    open("/nonexistent")
checks.append(("suppress", True))

# 17. ast.parse
import ast
tree = ast.parse("x = 1")
checks.append(("ast_parse", "Assign" in ast.dump(tree)))

# 18. operator.methodcaller
import operator
upper = operator.methodcaller("upper")
checks.append(("methodcaller", upper("hello") == "HELLO"))

# 19. heapq.merge
import heapq
checks.append(("heapq_merge", list(heapq.merge([1,3,5],[2,4,6])) == [1,2,3,4,5,6]))

# 20. itertools.tee
import itertools
a, b = itertools.tee(iter([1,2,3]), 2)
checks.append(("itertools_tee", list(a) == [1,2,3] and list(b) == [1,2,3]))

# --- report ---
for name, ok in checks:
    if not ok:
        print(f"FAIL {name}")
        sys.exit(1)
print(f"phase137: {len(checks)} checks passed")
