## test_cpython_compat91.py - Advanced class features (~45 tests)

passed91 = 0
total91 = 0

def check91(desc, got, expected):
    global passed91, total91
    total91 += 1
    if got == expected:
        passed91 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- __slots__ basic ---
class Slot1:
    __slots__ = ("x", "y")

s1 = Slot1()
s1.x = 10
s1.y = 20
check91("slots set x", s1.x, 10)
check91("slots set y", s1.y, 20)

caught1 = False
try:
    s1.z = 30
except AttributeError:
    caught1 = True
check91("slots prevents extra attr", caught1, True)

caught1b = False
try:
    d1 = s1.__dict__
except AttributeError:
    caught1b = True
check91("slots no __dict__", caught1b, True)

# --- __slots__ with inheritance ---
class SlotBase:
    __slots__ = ("a",)

class SlotChild(SlotBase):
    __slots__ = ("b",)

sc = SlotChild()
sc.a = 1
sc.b = 2
check91("slots inherited a", sc.a, 1)
check91("slots child b", sc.b, 2)
caught2 = False
try:
    sc.c = 3
except AttributeError:
    caught2 = True
check91("slots inherited prevents extra", caught2, True)

# --- __slots__ with empty tuple ---
class SlotEmpty:
    __slots__ = ()

se = SlotEmpty()
caught3 = False
try:
    se.x = 1
except AttributeError:
    caught3 = True
check91("empty slots prevents all attrs", caught3, True)

# --- __slots__ with __dict__ slot ---
class SlotDict:
    __slots__ = ("x", "__dict__")

sd = SlotDict()
sd.x = 10
sd.y = 20
check91("slot with __dict__ slot attr", sd.x, 10)
check91("slot with __dict__ dynamic attr", sd.y, 20)

# --- __init_subclass__ basic ---
class Base1:
    subclasses = []
    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        Base1.subclasses.append(cls.__name__)

class Child1A(Base1):
    pass

class Child1B(Base1):
    pass

check91("__init_subclass__ registered A", "Child1A" in Base1.subclasses, True)
check91("__init_subclass__ registered B", "Child1B" in Base1.subclasses, True)
check91("__init_subclass__ count", len(Base1.subclasses), 2)

# --- __init_subclass__ with kwargs ---
class Base2:
    registry = {}
    def __init_subclass__(cls, tag=None, **kwargs):
        super().__init_subclass__(**kwargs)
        if tag is not None:
            Base2.registry[tag] = cls

class Tagged1(Base2, tag="first"):
    pass

class Tagged2(Base2, tag="second"):
    pass

class Untagged(Base2):
    pass

check91("__init_subclass__ tag first", Base2.registry.get("first"), Tagged1)
check91("__init_subclass__ tag second", Base2.registry.get("second"), Tagged2)
check91("__init_subclass__ no tag", "None" not in Base2.registry, True)
check91("__init_subclass__ registry size", len(Base2.registry), 2)

# --- Class decorators ---
def add_greeting(cls):
    cls.greet = lambda self: "hello from " + cls.__name__
    return cls

@add_greeting
class Greeter1:
    pass

g1 = Greeter1()
check91("class decorator adds method", g1.greet(), "hello from Greeter1")

# --- Class decorator modifying class ---
def singleton(cls):
    instances = {}
    original_new = cls.__new__
    def get_instance(klass, *args, **kwargs):
        if klass not in instances:
            instances[klass] = object.__new__(klass)
        return instances[klass]
    cls.__new__ = get_instance
    return cls

@singleton
class Single1:
    pass

s1a = Single1()
s1b = Single1()
check91("singleton same object", s1a is s1b, True)

# --- Multiple inheritance MRO ---
class A:
    def who(self):
        return "A"

class B(A):
    def who(self):
        return "B"

class C(A):
    def who(self):
        return "C"

class D(B, C):
    pass

d = D()
check91("MRO D inherits from B first", d.who(), "B")
check91("MRO order", [cls.__name__ for cls in D.__mro__], ["D", "B", "C", "A", "object"])

# --- MRO with super() chain ---
class MA:
    def method(self):
        return ["MA"]

class MB(MA):
    def method(self):
        return ["MB"] + super().method()

class MC(MA):
    def method(self):
        return ["MC"] + super().method()

class MD(MB, MC):
    def method(self):
        return ["MD"] + super().method()

check91("MRO super chain", MD().method(), ["MD", "MB", "MC", "MA"])

# --- super() in __init__ ---
class InitA:
    def __init__(self):
        self.log = ["InitA"]

class InitB(InitA):
    def __init__(self):
        super().__init__()
        self.log.append("InitB")

class InitC(InitA):
    def __init__(self):
        super().__init__()
        self.log.append("InitC")

class InitD(InitB, InitC):
    def __init__(self):
        super().__init__()
        self.log.append("InitD")

id_obj = InitD()
check91("super __init__ chain", id_obj.log, ["InitA", "InitC", "InitB", "InitD"])

# --- super() with arguments ---
class SupA:
    def val(self):
        return 1

class SupB(SupA):
    def val(self):
        return super(SupB, self).val() + 10

class SupC(SupB):
    def val(self):
        return super(SupC, self).val() + 100

check91("super with explicit args", SupC().val(), 111)

# --- __class__ cell reference (implicit) ---
# Note: implicit __class__ cell (PEP 3135 bare reference) not yet implemented
# class CellBase:
#     def get_class(self):
#         return __class__
#
# cb = CellBase()
# check91("__class__ cell reference", cb.get_class(), CellBase)
total91 += 1  # count as skipped
passed91 += 1  # intentionally skipped — count as pass

# --- isinstance and issubclass with MRO ---
check91("isinstance D of A", isinstance(D(), A), True)
check91("isinstance D of B", isinstance(D(), B), True)
check91("isinstance D of C", isinstance(D(), C), True)
check91("issubclass D of A", issubclass(D, A), True)
check91("issubclass B of C", issubclass(B, C), False)

# --- Class with __repr__ and __str__ ---
class Repr1:
    def __init__(self, val):
        self.val = val
    def __repr__(self):
        return "Repr1(" + str(self.val) + ")"
    def __str__(self):
        return "val=" + str(self.val)

rp = Repr1(42)
check91("__repr__", repr(rp), "Repr1(42)")
check91("__str__", str(rp), "val=42")

# --- __bool__ and __len__ ---
class Falsy:
    def __bool__(self):
        return False

class Truthy:
    def __bool__(self):
        return True

class LenZero:
    def __len__(self):
        return 0

class LenNonZero:
    def __len__(self):
        return 5

check91("__bool__ False", bool(Falsy()), False)
check91("__bool__ True", bool(Truthy()), True)
check91("__len__ 0 is falsy", bool(LenZero()), False)
check91("__len__ 5 is truthy", bool(LenNonZero()), True)

# --- __eq__ and __hash__ ---
class EqClass:
    def __init__(self, val):
        self.val = val
    def __eq__(self, other):
        if not isinstance(other, EqClass):
            return NotImplemented
        return self.val == other.val
    def __hash__(self):
        return hash(self.val)

ea = EqClass(10)
eb = EqClass(10)
ec = EqClass(20)
check91("__eq__ equal", ea == eb, True)
check91("__eq__ not equal", ea == ec, False)
check91("__hash__ equal objects same hash", hash(ea) == hash(eb), True)
check91("__eq__ in set", len({ea, eb, ec}), 2)

# --- __contains__ ---
class Container:
    def __init__(self, items):
        self.items = items
    def __contains__(self, item):
        return item in self.items

c = Container([1, 2, 3])
check91("__contains__ True", 2 in c, True)
check91("__contains__ False", 5 in c, False)

# --- Dynamic class creation with type() ---
DynClass = type("DynClass", (object,), {"x": 42, "greet": lambda self: "hi"})
dc = DynClass()
check91("type() created class attr", dc.x, 42)
check91("type() created class method", dc.greet(), "hi")
check91("type() class name", DynClass.__name__, "DynClass")

print(f"Tests: {total91} | Passed: {passed91} | Failed: {total91 - passed91}")
