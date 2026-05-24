# test_phase157.py - frameless callable-instance recursion guard

import sys

old_limit = sys.getrecursionlimit()
try:
    sys.setrecursionlimit(20)

    class A:
        pass

    A.__call__ = A()
    a = A()

    try:
        a()
    except RecursionError as exc:
        assert "maximum recursion depth exceeded" in str(exc)
    else:
        raise AssertionError("callable instance recursion did not raise RecursionError")
finally:
    sys.setrecursionlimit(old_limit)

print("test_phase157 passed")
