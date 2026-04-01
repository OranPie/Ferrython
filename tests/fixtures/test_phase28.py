# Phase 28: Dataclasses, annotations, exception subclass str
tests_passed = 0
tests_failed = 0
def test(name, got, expected):
    global tests_passed, tests_failed
    if got == expected:
        tests_passed += 1
    else:
        tests_failed += 1
        print(f"FAIL: {name}: got {got!r}, expected {expected!r}")

# ── Annotations ──
class Annotated:
    x: int = 10
    y: str = "hello"
    z: float = 3.14

test("class_annotations_exist", hasattr(Annotated, "__annotations__"), True)
ann = Annotated.__annotations__
test("annotation_x", "x" in ann, True)
test("annotation_y", "y" in ann, True)
test("annotation_z", "z" in ann, True)

# ── Basic Dataclass ──
from dataclasses import dataclass

@dataclass
class Point:
    x: int
    y: int

p = Point(3, 4)
test("dc_point_x", p.x, 3)
test("dc_point_y", p.y, 4)
test("dc_point_repr", repr(p), "Point(x=3, y=4)")
test("dc_point_eq", Point(3, 4) == Point(3, 4), True)
test("dc_point_neq", Point(3, 4) == Point(1, 2), False)

# ── Dataclass with defaults ──
@dataclass
class Config:
    name: str
    debug: bool = False
    level: int = 1

c1 = Config("prod")
test("dc_default_name", c1.name, "prod")
test("dc_default_debug", c1.debug, False)
test("dc_default_level", c1.level, 1)

c2 = Config("dev", True, 5)
test("dc_override_name", c2.name, "dev")
test("dc_override_debug", c2.debug, True)
test("dc_override_level", c2.level, 5)

test("dc_config_repr", repr(c1), "Config(name='prod', debug=False, level=1)")
test("dc_config_eq", Config("prod") == Config("prod"), True)
test("dc_config_neq", Config("prod") == Config("dev"), False)

# ── Exception subclass str ──
class AppError(Exception):
    pass

class DatabaseError(AppError):
    pass

e1 = AppError("connection failed")
test("exc_sub_str", str(e1), "connection failed")
test("exc_sub_args", e1.args, ("connection failed",))

e2 = DatabaseError("timeout")
test("exc_sub_nested_str", str(e2), "timeout")
test("exc_sub_nested_args", e2.args, ("timeout",))

# ── Exception catch by parent type ──
caught = None
try:
    raise DatabaseError("db error")
except AppError as e:
    caught = str(e)
test("exc_catch_parent", caught, "db error")

# ── Multiple exception args ──
e3 = AppError("error", 42, "details")
test("exc_multi_args", e3.args, ("error", 42, "details"))

# ── Empty exception ──
e4 = AppError()
test("exc_empty_str", str(e4), "")
test("exc_empty_args", e4.args, ())

# ── Dataclass with complex types ──
@dataclass 
class Pair:
    first: object
    second: object

pair = Pair([1, 2], {"a": 1})
test("dc_complex_first", pair.first, [1, 2])
test("dc_complex_second", pair.second, {"a": 1})

# ── Annotations without value ──
class Schema:
    name: str
    age: int

test("schema_annotations", "name" in Schema.__annotations__, True)
test("schema_annotations_age", "age" in Schema.__annotations__, True)

# ── asdict / astuple ──
from dataclasses import asdict, astuple

p2 = Point(10, 20)
d = asdict(p2)
test("asdict_x", d["x"], 10)
test("asdict_y", d["y"], 20)

t = astuple(p2)
test("astuple_0", t[0], 10)
test("astuple_1", t[1], 20)

print(f"Tests: {tests_passed + tests_failed} | Passed: {tests_passed} | Failed: {tests_failed}")
if tests_failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(f"{tests_failed} TESTS FAILED!")
