# Test 85: Scope and namespace tests

passed85 = 0
total85 = 0

def check85(desc, got, expected):
    global passed85, total85
    total85 += 1
    if got == expected:
        passed85 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- Local variable scope in function ---
def local_scope85_1():
    x85_1 = 42
    return x85_1

check85("local variable scope", local_scope85_1(), 42)

# --- Global variable access from function ---
gvar85_2 = 100

def read_global85_2():
    return gvar85_2

check85("global variable access", read_global85_2(), 100)

# --- global keyword ---
gvar85_3 = 10

def modify_global85_3():
    global gvar85_3
    gvar85_3 = 20

modify_global85_3()
check85("global keyword modifies", gvar85_3, 20)

# --- nonlocal keyword in nested functions ---
def outer85_4():
    x85_4 = 1
    def inner85_4():
        nonlocal x85_4
        x85_4 = 2
    inner85_4()
    return x85_4

check85("nonlocal keyword", outer85_4(), 2)

def outer85_4b():
    x85_4b = 10
    def middle85_4b():
        nonlocal x85_4b
        x85_4b += 5
        def inner85_4b():
            nonlocal x85_4b
            x85_4b += 3
        inner85_4b()
    middle85_4b()
    return x85_4b

check85("nonlocal nested two levels", outer85_4b(), 18)

# --- Comprehension variable scope (doesn't leak in Python 3) ---
x85_5 = "before"
result85_5 = [x85_5_inner for x85_5_inner in range(3)]
check85("comprehension var no leak", x85_5, "before")
check85("comprehension result", result85_5, [0, 1, 2])

# For loop DOES leak in Python 3
for leak85_5 in range(5):
    pass
check85("for loop var leaks", leak85_5, 4)

# --- Function default args evaluated once (mutable default) ---
def append_to85_6(val, lst=[]):
    lst.append(val)
    return lst

r85_6a = append_to85_6(1)
r85_6b = append_to85_6(2)
check85("mutable default arg shared call 1", r85_6a, [1, 2])
check85("mutable default arg shared call 2", r85_6b, [1, 2])
check85("mutable default same object", r85_6a is r85_6b, True)

# --- LEGB rule: local, enclosing, global, builtin ---
legb85_7 = "global"

def outer85_7():
    legb85_7_enc = "enclosing"
    def inner85_7():
        legb85_7_loc = "local"
        return legb85_7_loc
    return inner85_7()

check85("LEGB local", outer85_7(), "local")

def outer85_7b():
    legb85_7b_val = "enclosing"
    def inner85_7b():
        return legb85_7b_val
    return inner85_7b()

check85("LEGB enclosing", outer85_7b(), "enclosing")

def read_global85_7c():
    return legb85_7

check85("LEGB global", read_global85_7c(), "global")

def use_builtin85_7d():
    return len([1, 2, 3])

check85("LEGB builtin", use_builtin85_7d(), 3)

# --- Name shadowing ---
shadow85_8 = "outer"

def shadow_test85_8():
    shadow85_8 = "inner"
    return shadow85_8

check85("name shadowing inside func", shadow_test85_8(), "inner")
check85("name shadowing outer unchanged", shadow85_8, "outer")

def shadow_builtin85_8b():
    len = lambda x: 999
    return len([1, 2, 3])

check85("shadow builtin", shadow_builtin85_8b(), 999)
check85("builtin restored outside", len([1, 2, 3]), 3)

# --- del variable ---
del_var85_9 = 42
check85("var before del", del_var85_9, 42)
del del_var85_9
try:
    _ = del_var85_9
    check85("del variable raises NameError", False, True)
except NameError:
    check85("del variable raises NameError", True, True)

# --- locals() and globals() availability ---
def locals_test85_10():
    a85_10 = 1
    b85_10 = 2
    loc = locals()
    return "a85_10" in loc and "b85_10" in loc

check85("locals() has local vars", locals_test85_10(), True)
check85("globals() has check85", "check85" in globals(), True)
check85("globals() type", type(globals()), dict)

# --- vars() on object ---
class Obj85_11:
    def __init__(self):
        self.x85_11 = 1
        self.y85_11 = 2

obj85_11 = Obj85_11()
v85_11 = vars(obj85_11)
check85("vars() x attr", v85_11["x85_11"], 1)
check85("vars() y attr", v85_11["y85_11"], 2)
check85("vars() type is dict", type(v85_11), dict)

# --- __name__ check ---
check85("__name__ is __main__", __name__, "__main__")

# --- Module-level code execution order ---
order85_12 = []
order85_12.append("first")
order85_12.append("second")
order85_12.append("third")
check85("module-level exec order", order85_12, ["first", "second", "third"])

# --- Nested scope edge cases ---
def make_counter85_13():
    count = 0
    def increment():
        nonlocal count
        count += 1
        return count
    def get():
        return count
    return increment, get

inc85_13, get85_13 = make_counter85_13()
check85("counter initial", get85_13(), 0)
inc85_13()
inc85_13()
check85("counter after 2 increments", get85_13(), 2)

# --- Class scope does not extend to methods ---
class ClassScope85_14:
    x85_14 = 10
    def get_x(self):
        return self.x85_14

cs85_14 = ClassScope85_14()
check85("class attr via self", cs85_14.get_x(), 10)

print(f"Tests: {total85} | Passed: {passed85} | Failed: {total85 - passed85}")
