"""Phase 6 tests: Mutable iterators, generator protocol, dunders, type system"""

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"FAIL: {name}")

# ── Mutable iterators ──
it = iter([10, 20, 30])
test("iter_next_1", next(it) == 10)
test("iter_next_2", next(it) == 20)
test("iter_next_3", next(it) == 30)
try:
    next(it)
    test("iter_exhausted", False)
except StopIteration:
    test("iter_exhausted", True)

# next with default
it = iter([1])
test("next_1", next(it) == 1)
test("next_default", next(it, "done") == "done")

# iter on string
it = iter("abc")
test("str_iter_1", next(it) == "a")
test("str_iter_2", next(it) == "b")
test("str_iter_3", next(it) == "c")

# iter on dict
d = {"x": 1, "y": 2}
it = iter(d)
first = next(it)
second = next(it)
test("dict_iter", first == "x" and second == "y")

# iter on range
it = iter(range(5))
test("range_iter_1", next(it) == 0)
test("range_iter_2", next(it) == 1)
test("range_iter_3", next(it) == 2)

# iter on tuple
it = iter((10, 20, 30))
test("tuple_iter", next(it) == 10 and next(it) == 20)

# ── Generator protocol ──
def accumulator():
    total = 0
    while True:
        val = yield total
        if val is None:
            break
        total += val

g = accumulator()
test("gen_send_init", next(g) == 0)
test("gen_send_10", g.send(10) == 10)
test("gen_send_20", g.send(20) == 30)
test("gen_send_5", g.send(5) == 35)

# Generator close
def counter():
    i = 0
    while True:
        yield i
        i += 1
g2 = counter()
test("gen_next_0", next(g2) == 0)
test("gen_next_1", next(g2) == 1)
g2.close()
try:
    next(g2)
    test("gen_close", False)
except StopIteration:
    test("gen_close", True)

# Generator with send pattern
def echo():
    while True:
        x = yield
        if x is None:
            break
        yield x * 2
g3 = echo()
next(g3)  # prime
test("gen_echo_1", g3.send(5) == 10)
next(g3)  # advance to next yield
test("gen_echo_2", g3.send(10) == 20)

# ── __getattr__ ──
class Proxy:
    def __init__(self):
        self.data = {"x": 10, "y": 20}
    def __getattr__(self, name):
        if name in self.data:
            return self.data[name]
        raise AttributeError(name)
p = Proxy()
test("getattr_x", p.x == 10)
test("getattr_y", p.y == 20)
test("getattr_data", p.data == {"x": 10, "y": 20})
try:
    p.z
    test("getattr_missing", False)
except AttributeError:
    test("getattr_missing", True)

# ── __setattr__ on class (via MRO) ──
class Logged:
    def __init__(self):
        # Bypass __setattr__ by directly setting in __init__ dict
        pass
    def __setattr__(self, name, value):
        # Custom setattr stores in uppercase
        super().__setattr__(name.upper(), value)
# Note: super().__setattr__ may not work yet, test basic case instead

# ── DeleteAttr ──
class Box:
    pass
b = Box()
b.value = 42
test("del_attr_before", b.value == 42)
del b.value
try:
    b.value
    test("del_attr_after", False)
except AttributeError:
    test("del_attr_after", True)

# ── __delitem__ ──
class MyList:
    def __init__(self):
        self.items = [1, 2, 3, 4, 5]
    def __delitem__(self, key):
        del self.items[key]
ml = MyList()
del ml[2]
test("delitem", ml.items == [1, 2, 4, 5])

# ── DupTopTwo (augmented subscript) ──
lst = [10, 20, 30]
lst[0] += 5
lst[2] *= 2
test("aug_subscr", lst == [15, 20, 60])

# Augmented attr
class Obj:
    def __init__(self):
        self.x = 0
o = Obj()
o.x += 5
o.x *= 3
test("aug_attr", o.x == 15)

# ── hasattr / getattr / setattr / delattr ──
class Attrs:
    x = 10
a = Attrs()
test("hasattr_yes", hasattr(a, "x"))
test("hasattr_no", not hasattr(a, "y"))
test("getattr_val", getattr(a, "x") == 10)
test("getattr_default", getattr(a, "y", 42) == 42)
setattr(a, "z", 99)
test("setattr", a.z == 99)
delattr(a, "z")
test("delattr", not hasattr(a, "z"))

# ── callable ──
test("callable_func", callable(print))
test("callable_lambda", callable(lambda: 0))
test("callable_int", not callable(42))
test("callable_str", not callable("hello"))
test("callable_list", not callable([]))

# ── pow with modulus ──
test("pow_basic", pow(2, 10) == 1024)
test("pow_mod", pow(2, 10, 100) == 24)
test("pow_mod2", pow(3, 4, 5) == 1)

# ── round ──
test("round_basic", round(3.7) == 4)
test("round_ndigits", round(3.14159, 2) == 3.14)
test("round_int", round(42) == 42)

# ── type() ──
test("type_int", type(42) == int)
test("type_str", type("hi") == str)
test("type_list", type([]) == list)
test("type_dict", type({}) == dict)
test("type_tuple", type(()) == tuple)
test("type_bool", type(True) == bool)
test("type_float", type(3.14) == float)

# ── isinstance ──
test("isinstance_int", isinstance(42, int))
test("isinstance_str", isinstance("hi", str))
test("isinstance_tuple", isinstance(42, (int, str)))
test("isinstance_tuple2", isinstance("hi", (int, str)))
test("isinstance_tuple_no", not isinstance(3.14, (int, str)))

class Animal:
    pass
class Dog(Animal):
    pass
d = Dog()
test("isinstance_class", isinstance(d, Dog))
test("isinstance_parent", isinstance(d, Animal))
test("isinstance_not", not isinstance(d, int))

# ── issubclass ──
test("issubclass_yes", issubclass(Dog, Animal))
test("issubclass_self", issubclass(Dog, Dog))
test("issubclass_no", not issubclass(Animal, Dog))

# ── repr ──
test("repr_int", repr(42) == "42")
test("repr_str", repr("hello") == "'hello'")
test("repr_list", repr([1, 2, 3]) == "[1, 2, 3]")
test("repr_bool", repr(True) == "True")
test("repr_none", repr(None) == "None")
test("repr_tuple", repr((1, 2)) == "(1, 2)")

# ── id ──
a = [1, 2, 3]
b = a
c = [1, 2, 3]
test("id_same", id(a) == id(b))
test("id_diff", id(a) != id(c))

# ── chr / ord ──
test("chr", chr(65) == "A")
test("ord", ord("A") == 65)
test("chr_ord_round", chr(ord("Z")) == "Z")

# ── hex / oct / bin ──
test("hex", hex(255) == "0xff")
test("oct", oct(8) == "0o10")
test("bin", bin(10) == "0b1010")

# ── map / filter / zip ──
test("map", list(map(lambda x: x*2, [1,2,3])) == [2,4,6])
test("filter", list(filter(lambda x: x > 2, [1,2,3,4,5])) == [3,4,5])
test("zip", list(zip([1,2,3], ["a","b","c"])) == [(1,"a"),(2,"b"),(3,"c")])

# ── enumerate ──
test("enumerate", list(enumerate(["a","b","c"])) == [(0,"a"),(1,"b"),(2,"c")])

# ── sorted ──
test("sorted", sorted([3,1,2]) == [1,2,3])

# ── any / all ──
test("any_true", any([False, False, True]))
test("any_false", not any([False, False, False]))
test("all_true", all([True, True, True]))
test("all_false", not all([True, False, True]))

# ── abs / divmod ──
test("abs_neg", abs(-5) == 5)
test("abs_pos", abs(5) == 5)
test("abs_float", abs(-3.14) == 3.14)
test("divmod", divmod(17, 5) == (3, 2))

# ── bool ──
test("bool_1", bool(1) == True)
test("bool_0", bool(0) == False)
test("bool_empty_str", bool("") == False)
test("bool_str", bool("x") == True)
test("bool_empty_list", bool([]) == False)
test("bool_list", bool([1]) == True)

# ── Type conversions ──
test("int_str", int("42") == 42)
test("float_str", float("3.14") == 3.14)
test("str_int", str(42) == "42")
test("int_float", int(3.14) == 3)
test("float_int", float(42) == 42.0)

# ── Dict methods ──
d = {"a": 1, "b": 2, "c": 3}
test("dict_get", d.get("a") == 1)
test("dict_get_default", d.get("z", 0) == 0)
test("dict_pop", d.pop("c") == 3 and "c" not in d)
d.update({"d": 4})
test("dict_update", d["d"] == 4)
d.setdefault("e", 5)
test("dict_setdefault", d["e"] == 5)
d.setdefault("a", 999)
test("dict_setdefault_exist", d["a"] == 1)
keys = list(d.keys())
vals = list(d.values())
items = list(d.items())
test("dict_keys", "a" in keys)
test("dict_vals", 1 in vals)

# ── String methods comprehensive ──
test("title", "hello world".title() == "Hello World")
test("swapcase", "Hello".swapcase() == "hELLO")
test("center", "hi".center(6, "-") == "--hi--")
test("ljust", "hi".ljust(5, ".") == "hi...")
test("rjust", "hi".rjust(5, ".") == "...hi")
test("zfill", "42".zfill(5) == "00042")
test("lower", "HELLO".lower() == "hello")
test("upper", "hello".upper() == "HELLO")
test("strip", "  hi  ".strip() == "hi")
test("lstrip", "  hi  ".lstrip() == "hi  ")
test("rstrip", "  hi  ".rstrip() == "  hi")
test("split", "a,b,c".split(",") == ["a", "b", "c"])
test("join", "-".join(["a", "b", "c"]) == "a-b-c")
test("startswith", "hello".startswith("he"))
test("endswith", "hello".endswith("lo"))
test("replace", "aabaa".replace("a", "x") == "xxbxx")
test("find", "hello".find("ll") == 2)
test("find_miss", "hello".find("zz") == -1)
test("count", "hello".count("l") == 2)
test("isdigit", "123".isdigit())
test("isalpha", "abc".isalpha())

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
