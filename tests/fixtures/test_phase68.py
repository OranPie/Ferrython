# Phase 68: __init_subclass__, __set_name__, and __format__ protocols

# ── Test 1: __init_subclass__ basic ──

class Base:
    subclasses = []

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        Base.subclasses.append(cls)
        cls.registered = True

class Child(Base):
    pass

assert Child.registered == True, "__init_subclass__ should set registered on Child"
assert len(Base.subclasses) == 1, "Base.subclasses should have one entry"

# ── Test 2: __init_subclass__ with multiple subclasses ──

class Child2(Base):
    pass

assert Child2.registered == True, "__init_subclass__ should set registered on Child2"
assert len(Base.subclasses) == 2, "Base.subclasses should have two entries"

# ── Test 3: __init_subclass__ chaining ──

class GrandChild(Child):
    pass

assert GrandChild.registered == True, "__init_subclass__ should also fire for grandchild via Child"
assert len(Base.subclasses) == 3, "Base.subclasses should have three entries"

# ── Test 4: __set_name__ basic ──

class Descriptor:
    def __set_name__(self, owner, name):
        self.owner_name = owner.__name__
        self.attr_name = name

class MyClass:
    x = Descriptor()
    y = Descriptor()

assert MyClass.x.attr_name == "x", f"Expected 'x', got {MyClass.x.attr_name!r}"
assert MyClass.y.attr_name == "y", f"Expected 'y', got {MyClass.y.attr_name!r}"
assert MyClass.x.owner_name == "MyClass", f"Expected 'MyClass', got {MyClass.x.owner_name!r}"

# ── Test 5: __set_name__ with inheritance ──

class Base2:
    pass

class Desc2:
    def __set_name__(self, owner, name):
        self.name = name

class Sub(Base2):
    d = Desc2()

assert Sub.d.name == "d", f"Expected 'd', got {Sub.d.name!r}"

# ── Test 6: __format__ with format() builtin ──

class Money:
    def __init__(self, amount):
        self.amount = amount

    def __format__(self, spec):
        if spec == ".2f":
            return "$" + format(self.amount, ".2f")
        return str(self.amount)

m = Money(42.5)
assert format(m, ".2f") == "$42.50", f"Expected '$42.50', got {format(m, '.2f')!r}"
assert format(m, "") == "42.5", f"Expected '42.5', got {format(m, '')!r}"

# ── Test 7: __format__ with f-strings ──

class Tag:
    def __init__(self, val):
        self.val = val

    def __format__(self, spec):
        if spec == "upper":
            return self.val.upper()
        return self.val

t = Tag("hello")
assert f"{t:upper}" == "HELLO", f"Expected 'HELLO', got {f'{t:upper}'!r}"
assert f"{t}" == "hello", f"Expected 'hello', got {f'{t}'!r}"

# ── Test 8: __format__ with empty spec returns __str__ equivalent ──

class Obj:
    def __format__(self, spec):
        if spec:
            return f"formatted({spec})"
        return "default"

o = Obj()
assert format(o) == "default", f"Expected 'default', got {format(o)!r}"
assert format(o, "xyz") == "formatted(xyz)", f"Expected 'formatted(xyz)', got {format(o, 'xyz')!r}"

# ── Test 9: __init_subclass__ with class attributes ──

class Registry:
    _registry = {}

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        Registry._registry[cls.__name__] = cls

class PluginA(Registry):
    pass

class PluginB(Registry):
    pass

assert "PluginA" in Registry._registry, "PluginA should be registered"
assert "PluginB" in Registry._registry, "PluginB should be registered"

print("phase68: all tests passed")
