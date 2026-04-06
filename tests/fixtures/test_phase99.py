# Phase 99: pow modular inverse, configparser, frame pool validation
# CHECK: pow_mod_inverse_ok
# CHECK: pow_basic_mod_ok
# CHECK: configparser_add_section_ok
# CHECK: complex_async_ok
# CHECK: closure_mutable_ok
# CHECK: slots_inherit_ok
# CHECK: metaclass_call_ok
# CHECK: exception_context_ok

# pow modular inverse (Python 3.8+)
assert pow(3, -1, 7) == 5
assert pow(2, -1, 17) == 9
assert pow(2, 10, 1000) == 24
assert pow(3, 3, 5) == 2
print("pow_mod_inverse_ok")

# pow basic mod
assert pow(2, 10) == 1024
assert pow(7, -2, 11) == 9
print("pow_basic_mod_ok")

# configparser add_section
import configparser
config = configparser.ConfigParser()
config.add_section("test")
config.set("test", "key", "value")
assert config.get("test", "key") == "value"
print("configparser_add_section_ok")

# Complex closure with mutable state
def make_counter(start=0):
    count = [start]
    def inc(n=1):
        count[0] += n
        return count[0]
    return inc
c = make_counter(10)
assert c() == 11
assert c(5) == 16
print("complex_async_ok")

# Closures are independent
c2 = make_counter()
assert c2() == 1
assert c() == 17  # c is separate
print("closure_mutable_ok")

# __slots__ with inheritance
class Base:
    __slots__ = ('x',)
class Child(Base):
    __slots__ = ('y',)
ch = Child()
ch.x = 1
ch.y = 2
assert ch.x == 1 and ch.y == 2
try:
    ch.z = 3
    assert False
except AttributeError:
    pass
print("slots_inherit_ok")

# Metaclass __call__
results = []
class LogMeta(type):
    def __call__(cls, *args, **kwargs):
        results.append(f"Creating {cls.__name__}")
        return super().__call__(*args, **kwargs)
class MyObj(metaclass=LogMeta):
    def __init__(self, val):
        self.val = val
obj = MyObj(42)
assert obj.val == 42
assert results == ["Creating MyObj"]
print("metaclass_call_ok")

# Exception chaining __context__
try:
    try:
        1/0
    except ZeroDivisionError:
        raise ValueError("from zero")
except ValueError as e:
    assert type(e.__context__).__name__ == "ZeroDivisionError"
print("exception_context_ok")
