import typing

# 1. TypeVar enhancements
T = typing.TypeVar('T')
assert T.__name__ == 'T', f"name: {T.__name__}"
assert T.__covariant__ == False
assert T.__contravariant__ == False
r = T.__repr__()
assert r == '~T', f"repr: {r}"

# TypeVar with constraints
TC = typing.TypeVar('TC', int, str)
assert len(TC.__constraints__) == 2, f"constraints: {TC.__constraints__}"

# 2. Protocol + runtime_checkable
class MyProto(typing.Protocol):
    def speak(self):
        pass

typing.runtime_checkable(MyProto)
assert hasattr(MyProto, '__protocol_attrs__')
assert hasattr(MyProto, '_is_runtime_checkable')
assert hasattr(MyProto, '__instancecheck__')

# 3. NamedTuple already works (via process_namedtuple_class)
class Point(typing.NamedTuple):
    x: int
    y: int

p = Point(1, 2)
assert p.x == 1
assert p.y == 2

# 4. ClassVar and InitVar
assert hasattr(typing, 'ClassVar')
assert hasattr(typing, 'InitVar')

# 5. TYPE_CHECKING
assert typing.TYPE_CHECKING == False

# 6. overload
@typing.overload
def f(x: int) -> int: ...
assert callable(f)

# 7. cast
val = typing.cast(int, "hello")
assert val == "hello"

# 8. final/Final
assert hasattr(typing, 'Final')
assert hasattr(typing, 'final')

@typing.final
class Sealed:
    pass
assert Sealed is not None

print("ALL TYPING CHECKS PASSED")
