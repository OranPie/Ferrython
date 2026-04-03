## test_cpython_compat88.py - Descriptor protocol (~45 tests)

passed88 = 0
total88 = 0

def check88(desc, got, expected):
    global passed88, total88
    total88 += 1
    if got == expected:
        passed88 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- Basic __get__ descriptor ---
class Desc1:
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return 42

class Owner1:
    attr = Desc1()

o1 = Owner1()
check88("basic __get__ on instance", o1.attr, 42)
check88("__get__ on class returns descriptor", isinstance(Owner1.attr, Desc1), True)

# --- __get__ with obj info ---
class Desc2:
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return obj._val * 2

class Owner2:
    x = Desc2()
    def __init__(self, val):
        self._val = val

o2a = Owner2(5)
o2b = Owner2(10)
check88("__get__ uses instance data a", o2a.x, 10)
check88("__get__ uses instance data b", o2b.x, 20)

# --- __set__ descriptor ---
class Desc3:
    def __init__(self):
        self.store = {}
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return self.store.get(id(obj), "unset")
    def __set__(self, obj, value):
        self.store[id(obj)] = value

class Owner3:
    attr = Desc3()

o3 = Owner3()
check88("__get__ before __set__", o3.attr, "unset")
o3.attr = "hello"
check88("__set__ then __get__", o3.attr, "hello")
o3.attr = "world"
check88("__set__ overwrites", o3.attr, "world")

# --- __delete__ descriptor ---
class Desc4:
    def __init__(self):
        self.store = {}
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return self.store.get(id(obj), "missing")
    def __set__(self, obj, value):
        self.store[id(obj)] = value
    def __delete__(self, obj):
        self.store.pop(id(obj), None)

class Owner4:
    attr = Desc4()

o4 = Owner4()
o4.attr = "present"
check88("before delete", o4.attr, "present")
del o4.attr
check88("after delete", o4.attr, "missing")

# --- Data descriptor vs instance dict priority ---
class DataDesc:
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return "from-descriptor"
    def __set__(self, obj, value):
        pass

class Owner5:
    attr = DataDesc()

o5 = Owner5()
o5.__dict__["attr"] = "from-instance"
check88("data descriptor beats instance dict", o5.attr, "from-descriptor")

# --- Non-data descriptor vs instance dict priority ---
class NonDataDesc:
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return "from-nd-descriptor"

class Owner6:
    attr = NonDataDesc()

o6 = Owner6()
check88("non-data descriptor when no instance attr", o6.attr, "from-nd-descriptor")
o6.__dict__["attr"] = "from-instance"
check88("instance dict beats non-data descriptor", o6.attr, "from-instance")

# --- property decorator basic ---
class Prop1:
    def __init__(self):
        self._x = 0
    @property
    def x(self):
        return self._x
    @x.setter
    def x(self, val):
        self._x = val * 2
    @x.deleter
    def x(self):
        self._x = -1

p1 = Prop1()
check88("property getter default", p1.x, 0)
p1.x = 5
check88("property setter doubles", p1.x, 10)
del p1.x
check88("property deleter sets -1", p1.x, -1)

# --- property read-only ---
class Prop2:
    @property
    def ro(self):
        return 99

p2 = Prop2()
check88("read-only property get", p2.ro, 99)
caught_p2 = False
try:
    p2.ro = 100
except AttributeError:
    caught_p2 = True
check88("read-only property set raises AttributeError", caught_p2, True)

# --- property with computed value ---
class Prop3:
    def __init__(self, a, b):
        self.a = a
        self.b = b
    @property
    def total(self):
        return self.a + self.b

p3 = Prop3(3, 7)
check88("computed property", p3.total, 10)
p3.a = 10
check88("computed property after change", p3.total, 17)

# --- classmethod ---
class CM1:
    count = 0
    @classmethod
    def inc(cls):
        cls.count += 1
        return cls.count

check88("classmethod first call", CM1.inc(), 1)
check88("classmethod second call", CM1.inc(), 2)
cm1_inst = CM1()
check88("classmethod via instance", cm1_inst.inc(), 3)

# --- classmethod receives subclass ---
class Base1:
    name = "Base"
    @classmethod
    def who(cls):
        return cls.name

class Sub1(Base1):
    name = "Sub"

check88("classmethod on base", Base1.who(), "Base")
check88("classmethod on subclass", Sub1.who(), "Sub")

# --- staticmethod ---
class SM1:
    @staticmethod
    def add(a, b):
        return a + b

check88("staticmethod via class", SM1.add(2, 3), 5)
sm1_inst = SM1()
check88("staticmethod via instance", sm1_inst.add(4, 5), 9)

# --- Descriptor __set_name__ ---
class Desc5:
    def __set_name__(self, owner, name):
        self.public_name = name
        self.private_name = "_" + name
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return getattr(obj, self.private_name, "default")
    def __set__(self, obj, value):
        setattr(obj, self.private_name, value)

class Owner7:
    field = Desc5()

o7 = Owner7()
check88("__set_name__ default get", o7.field, "default")
o7.field = "hello"
check88("__set_name__ set then get", o7.field, "hello")
check88("__set_name__ stored in private", o7._field, "hello")
check88("__set_name__ public name", Owner7.field.public_name, "field")

# --- Multiple descriptors on one class ---
class Desc6:
    def __init__(self, default):
        self.default = default
        self.store = {}
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return self.store.get(id(obj), self.default)
    def __set__(self, obj, value):
        self.store[id(obj)] = value

class Owner8:
    x = Desc6(0)
    y = Desc6(1)

o8 = Owner8()
check88("multi desc x default", o8.x, 0)
check88("multi desc y default", o8.y, 1)
o8.x = 10
o8.y = 20
check88("multi desc x after set", o8.x, 10)
check88("multi desc y after set", o8.y, 20)

# --- Descriptor via __get__ receives objtype ---
class Desc7:
    def __get__(self, obj, objtype=None):
        return objtype

class Owner9:
    attr = Desc7()

o9 = Owner9()
check88("__get__ objtype from instance", o9.attr, Owner9)
check88("__get__ objtype from class", Owner9.attr, Owner9)

# --- property is a data descriptor ---
class Prop4:
    @property
    def x(self):
        return "property"

p4 = Prop4()
p4.__dict__["x"] = "instance"
check88("property (data desc) beats instance dict", p4.x, "property")

# --- classmethod descriptor behavior ---
class CM2:
    @classmethod
    def m(cls):
        return cls

check88("classmethod returns class", CM2.m(), CM2)

# --- Inheritance of descriptors ---
class BaseDesc:
    x = Desc6(100)

class ChildDesc(BaseDesc):
    pass

cd = ChildDesc()
check88("inherited descriptor default", cd.x, 100)
cd.x = 200
check88("inherited descriptor set", cd.x, 200)
bd = BaseDesc()
check88("base not affected by child set", bd.x, 100)

# --- Overriding descriptor in subclass ---
class BaseO:
    @property
    def val(self):
        return "base"

class ChildO(BaseO):
    @property
    def val(self):
        return "child"

check88("overridden property in child", ChildO().val, "child")
check88("original property in base", BaseO().val, "base")

# --- classmethod with args ---
class CM3:
    @classmethod
    def create(cls, val):
        obj = cls()
        obj.val = val
        return obj

cm3obj = CM3.create(42)
check88("classmethod factory", cm3obj.val, 42)
check88("classmethod factory type", isinstance(cm3obj, CM3), True)

# --- staticmethod does not receive self or cls ---
class SM2:
    @staticmethod
    def identity(x):
        return x

check88("staticmethod identity", SM2.identity("abc"), "abc")
check88("staticmethod identity int", SM2.identity(7), 7)

# --- Chained property access ---
class Inner:
    def __init__(self, v):
        self.v = v

class Outer:
    def __init__(self, v):
        self._inner = Inner(v)
    @property
    def inner_val(self):
        return self._inner.v

ov = Outer(55)
check88("chained property access", ov.inner_val, 55)

print(f"Tests: {total88} | Passed: {passed88} | Failed: {total88 - passed88}")
