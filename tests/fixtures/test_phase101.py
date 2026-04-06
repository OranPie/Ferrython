checks = 0
def check(name, got, expected):
    global checks
    checks += 1
    if got != expected:
        print(f"FAIL {name}: got {got!r}, expected {expected!r}")
        raise SystemExit(1)

# Name mangling: __name (2+ leading underscores, no trailing __) → _ClassName__name

class Foo:
    __class_var = 42

    def __init__(self):
        self.__x = 10
        self.__y = 20

    def get_x(self):
        return self.__x

    def get_y(self):
        return self.__y

    def __secret(self):
        return "hidden"

    def call_secret(self):
        return self.__secret()

f = Foo()

# Class body mangling
check("class_var_mangled", hasattr(Foo, '_Foo__class_var'), True)
check("class_var_value", Foo._Foo__class_var, 42)
check("class_var_not_raw", hasattr(Foo, '__class_var'), False)

# Instance attribute mangling
check("inst_x_mangled", hasattr(f, '_Foo__x'), True)
check("inst_x_value", f._Foo__x, 10)
check("inst_y_mangled", hasattr(f, '_Foo__y'), True)
check("inst_y_value", f._Foo__y, 20)

# Method access through self
check("get_x", f.get_x(), 10)
check("get_y", f.get_y(), 20)

# Method mangling
check("method_mangled", hasattr(Foo, '_Foo__secret'), True)
check("call_secret", f.call_secret(), "hidden")

# Dunder names should NOT be mangled
class Bar:
    __init__ = None  # should stay as __init__, not _Bar__init__

check("dunder_not_mangled", hasattr(Bar, '__init__'), True)

# Single underscore should NOT be mangled
class Baz:
    _x = 5
    def __init__(self):
        self._y = 10

check("single_underscore_cls", Baz._x, 5)
b = Baz()
check("single_underscore_inst", b._y, 10)

# Inheritance: mangling uses the defining class name
class Parent:
    def __init__(self):
        self.__val = "parent"

    def get_val(self):
        return self.__val

class Child(Parent):
    def __init__(self):
        super().__init__()
        self.__val = "child"  # this becomes _Child__val

    def get_child_val(self):
        return self.__val  # reads _Child__val

c = Child()
check("parent_val", c._Parent__val, "parent")
check("child_val", c._Child__val, "child")
check("parent_method", c.get_val(), "parent")  # reads _Parent__val
check("child_method", c.get_child_val(), "child")

# Triple underscore should be mangled (only trailing __ is excluded)
class Triple:
    def __init__(self):
        self.___x = 99  # ___x has 3 underscores, mangled to _Triple___x

t = Triple()
check("triple_underscore", hasattr(t, '_Triple___x'), True)

print(f"{checks}/{checks} passed")
