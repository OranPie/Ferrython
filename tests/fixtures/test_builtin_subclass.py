# Test builtin type subclass instantiation and methods

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL: " + name)

# -- tuple subclass --
class MyTuple(tuple):
    pass

t = MyTuple([1,2,3])
test("tuple subclass type", type(t).__name__ == "MyTuple")
test("tuple subclass isinstance", isinstance(t, tuple))
test("tuple subclass len", len(t) == 3)
test("tuple subclass getitem", t[0] == 1)
test("tuple subclass iter", list(t) == [1, 2, 3])
test("tuple subclass sum", sum(t) == 6)
test("tuple subclass repr", repr(t) == "(1, 2, 3)")

# -- list subclass --
class MyList(list):
    pass

l = MyList([1,2,3])
test("list subclass type", type(l).__name__ == "MyList")
test("list subclass isinstance", isinstance(l, list))
test("list subclass len", len(l) == 3)
test("list subclass getitem", l[0] == 1)
test("list subclass iter", [x for x in l] == [1, 2, 3])
l.append(4)
test("list subclass append", list(l) == [1, 2, 3, 4])
l.extend([5, 6])
test("list subclass extend", list(l) == [1, 2, 3, 4, 5, 6])
test("list subclass pop", l.pop() == 6)

# -- int subclass --
class MyInt(int):
    pass

n = MyInt(42)
test("int subclass type", type(n).__name__ == "MyInt")
test("int subclass isinstance", isinstance(n, int))
test("int subclass value", n == 42)
test("int subclass arithmetic", n + 1 == 43)

# -- str subclass --
class MyStr(str):
    pass

s = MyStr("hello")
test("str subclass type", type(s).__name__ == "MyStr")
test("str subclass isinstance", isinstance(s, str))
test("str subclass value", s == "hello")
test("str subclass upper", s.upper() == "HELLO")
test("str subclass len", len(s) == 5)

# -- float subclass --
class MyFloat(float):
    pass

f = MyFloat(3.14)
test("float subclass type", type(f).__name__ == "MyFloat")
test("float subclass isinstance", isinstance(f, float))
test("float subclass value", f == 3.14)

# -- multi-level subclass --
class MySubList(MyList):
    pass

sl = MySubList([10, 20])
test("multi-level subclass type", type(sl).__name__ == "MySubList")
test("multi-level subclass isinstance list", isinstance(sl, list))
test("multi-level subclass isinstance MyList", isinstance(sl, MyList))
test("multi-level subclass len", len(sl) == 2)
sl.append(30)
test("multi-level subclass append", list(sl) == [10, 20, 30])

# -- subclass with custom __init__ --
class NamedTuple(tuple):
    def __init__(self, items, name="unnamed"):
        self.name = name

nt = NamedTuple([1, 2, 3], name="point")
test("subclass custom init name", nt.name == "point")
test("subclass custom init iter", list(nt) == [1, 2, 3])

print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
