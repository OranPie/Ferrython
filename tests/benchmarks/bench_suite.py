# Ferrython Microbenchmark Suite
# Run with: ferrython --bench

import time

def bench(name, fn, iterations=100000):
    start = time.time()
    fn(iterations)
    elapsed = time.time() - start
    ops_per_sec = iterations / elapsed if elapsed > 0 else 0
    print(f"  {name:30s}  {elapsed:.4f}s  ({ops_per_sec:.0f} ops/s)")

# ── Arithmetic ──

def bench_int_add(n):
    x = 0
    for i in range(n):
        x = x + 1

def bench_float_add(n):
    x = 0.0
    for i in range(n):
        x = x + 1.0

def bench_int_mul(n):
    x = 1
    for i in range(n):
        x = (x * 3) % 1000000

def bench_int_sub(n):
    x = n
    for i in range(n):
        x = x - 1

def bench_float_mul(n):
    x = 1.0
    for i in range(n):
        x = x * 1.000001

# ── Loops ──

def bench_while_loop(n):
    i = 0
    while i < n:
        i = i + 1

def bench_nested_loop(n):
    s = 0
    for i in range(100):
        for j in range(n // 100):
            s = s + 1

# ── String operations ──

def bench_str_concat(n):
    s = ""
    for i in range(min(n, 10000)):
        s = s + "a"

def bench_str_format(n):
    for i in range(n):
        s = f"hello {i} world"

# ── List operations ──

def bench_list_append(n):
    lst = []
    for i in range(n):
        lst.append(i)

def bench_list_comprehension(n):
    for i in range(min(n, 1000)):
        lst = [x * 2 for x in range(100)]

def bench_list_index(n):
    lst = list(range(100))
    for i in range(n):
        x = lst[i % 100]

# ── Dict operations ──

def bench_dict_setget(n):
    d = {}
    for i in range(n):
        d[i] = i
    for i in range(n):
        x = d[i]

def bench_dict_in(n):
    d = {i: i for i in range(1000)}
    for i in range(n):
        x = i in d

# ── Function calls ──

def noop():
    pass

def bench_function_call(n):
    for i in range(n):
        noop()

def bench_method_call(n):
    lst = [1, 2, 3]
    for i in range(n):
        lst.append(i)
        lst.pop()

def add(a, b):
    return a + b

def bench_function_call_args(n):
    for i in range(n):
        add(i, 1)

def bench_closure_call(n):
    x = 10
    def inner():
        return x
    for i in range(n):
        inner()

# ── Attribute access ──

class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y

def bench_attr_access(n):
    p = Point(1, 2)
    for i in range(n):
        x = p.x
        y = p.y

def bench_attr_set(n):
    p = Point(1, 2)
    for i in range(n):
        p.x = i

# ── Global access ──

GLOBAL_VAR = 42

def bench_global_read(n):
    for i in range(n):
        x = GLOBAL_VAR

# ── Type checks ──

def bench_isinstance(n):
    x = 42
    for i in range(n):
        isinstance(x, int)

# ── Exception handling ──

def bench_try_except(n):
    for i in range(n):
        try:
            x = 1
        except:
            pass

def bench_exception_raise(n):
    for i in range(min(n, 10000)):
        try:
            raise ValueError("test")
        except ValueError:
            pass

# ── Fibonacci (recursive) ──

def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

def bench_fib(n):
    for i in range(min(n, 100)):
        fib(20)

# ── New: String methods ──

def bench_str_split(n):
    s = "hello world foo bar baz qux"
    for i in range(n):
        parts = s.split(" ")

def bench_str_join(n):
    parts = ["hello", "world", "foo", "bar", "baz"]
    for i in range(n):
        s = " ".join(parts)

def bench_str_replace(n):
    s = "hello world hello world hello"
    for i in range(n):
        r = s.replace("hello", "hi")

def bench_str_startswith(n):
    s = "hello world"
    for i in range(n):
        x = s.startswith("hello")

# ── New: Dict comprehension & methods ──

def bench_dict_comp(n):
    for i in range(min(n, 1000)):
        d = {k: k * 2 for k in range(100)}

def bench_dict_keys_iter(n):
    d = {i: i for i in range(100)}
    for i in range(n):
        for k in d:
            pass

def bench_dict_items(n):
    d = {i: i for i in range(100)}
    for i in range(min(n, 1000)):
        for k, v in d.items():
            pass

# ── New: Tuple operations ──

def bench_tuple_create(n):
    for i in range(n):
        t = (i, i + 1, i + 2)

def bench_tuple_unpack(n):
    t = (1, 2, 3)
    for i in range(n):
        a, b, c = t

# ── New: Set operations ──

def bench_set_add(n):
    s = set()
    for i in range(n):
        s.add(i % 1000)

def bench_set_in(n):
    s = set(range(1000))
    for i in range(n):
        x = i in s

# ── New: Generator / iterator ──

def bench_generator(n):
    def gen(limit):
        i = 0
        while i < limit:
            yield i
            i += 1
    total = 0
    for x in gen(n):
        total += x

def bench_enumerate_loop(n):
    lst = list(range(min(n, 1000)))
    for _ in range(n // 1000):
        for i, v in enumerate(lst):
            pass

def bench_zip_loop(n):
    a = list(range(min(n, 1000)))
    b = list(range(min(n, 1000)))
    for _ in range(n // 1000):
        for x, y in zip(a, b):
            pass

# ── New: Builtin functions ──

def bench_sum_list(n):
    lst = list(range(100))
    for i in range(min(n, 10000)):
        s = sum(lst)

def bench_len_call(n):
    lst = [1, 2, 3, 4, 5]
    for i in range(n):
        x = len(lst)

def bench_sorted_small(n):
    for i in range(min(n, 10000)):
        s = sorted([5, 3, 1, 4, 2])

def bench_min_max(n):
    lst = [5, 3, 8, 1, 9, 2, 7]
    for i in range(n):
        x = min(lst)
        y = max(lst)

# ── New: Class instantiation ──

class SimpleObj:
    def __init__(self, x):
        self.x = x

def bench_class_create(n):
    for i in range(n):
        o = SimpleObj(i)

def bench_class_method(n):
    class Adder:
        def __init__(self, val):
            self.val = val
        def add(self, x):
            return self.val + x
    a = Adder(10)
    for i in range(n):
        a.add(i)

# ── New: Polymorphic / dynamic ──

def bench_getattr(n):
    p = Point(1, 2)
    for i in range(n):
        x = getattr(p, 'x')

def bench_hasattr(n):
    p = Point(1, 2)
    for i in range(n):
        x = hasattr(p, 'x')
        y = hasattr(p, 'z')

# ── Run all benchmarks ──

print("=" * 70)
print("Ferrython Microbenchmark Suite")
print("=" * 70)
print()

N = 100000

print("Arithmetic:")
bench("int_add", bench_int_add, N)
bench("float_add", bench_float_add, N)
bench("int_mul_mod", bench_int_mul, N)
bench("int_sub", bench_int_sub, N)
bench("float_mul", bench_float_mul, N)
print()

print("Loops:")
bench("while_loop", bench_while_loop, N)
bench("nested_loop", bench_nested_loop, N)
print()

print("Strings:")
bench("str_concat (10K)", bench_str_concat, 10000)
bench("str_format", bench_str_format, N)
print()

print("Collections:")
bench("list_append", bench_list_append, N)
bench("list_comprehension (1K×100)", bench_list_comprehension, 1000)
bench("list_index", bench_list_index, N)
bench("dict_set_get", bench_dict_setget, N)
bench("dict_in", bench_dict_in, N)
print()

print("Calls:")
bench("function_call", bench_function_call, N)
bench("function_call_args", bench_function_call_args, N)
bench("method_call", bench_method_call, N)
bench("closure_call", bench_closure_call, N)
print()

print("Objects:")
bench("attr_access", bench_attr_access, N)
bench("attr_set", bench_attr_set, N)
bench("global_read", bench_global_read, N)
bench("isinstance", bench_isinstance, N)
print()

print("Control flow:")
bench("try_except", bench_try_except, N)
bench("exception_raise (10K)", bench_exception_raise, 10000)
bench("fibonacci(20) ×100", bench_fib, 100)
print()

print("Strings (methods):")
bench("str_split", bench_str_split, N)
bench("str_join", bench_str_join, N)
bench("str_replace", bench_str_replace, N)
bench("str_startswith", bench_str_startswith, N)
print()

print("Dict (extra):")
bench("dict_comp (1K×100)", bench_dict_comp, 1000)
bench("dict_keys_iter", bench_dict_keys_iter, N)
bench("dict_items (1K×100)", bench_dict_items, 1000)
print()

print("Tuples:")
bench("tuple_create", bench_tuple_create, N)
bench("tuple_unpack", bench_tuple_unpack, N)
print()

print("Sets:")
bench("set_add", bench_set_add, N)
bench("set_in", bench_set_in, N)
print()

print("Generators/Iterators:")
bench("generator", bench_generator, N)
bench("enumerate_loop", bench_enumerate_loop, N)
bench("zip_loop", bench_zip_loop, N)
print()

print("Builtins:")
bench("sum_list (10K×100)", bench_sum_list, 10000)
bench("len_call", bench_len_call, N)
bench("sorted_small (10K)", bench_sorted_small, 10000)
bench("min_max", bench_min_max, N)
print()

print("Classes:")
bench("class_create", bench_class_create, N)
bench("class_method", bench_class_method, N)
print()

print("Dynamic:")
bench("getattr", bench_getattr, N)
bench("hasattr", bench_hasattr, N)
print()

print("=" * 70)
print("Done.")
