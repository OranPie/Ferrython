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

# ── Dict operations ──

def bench_dict_setget(n):
    d = {}
    for i in range(n):
        d[i] = i
    for i in range(n):
        x = d[i]

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
print()

print("Strings:")
bench("str_concat (10K)", bench_str_concat, 10000)
bench("str_format", bench_str_format, N)
print()

print("Collections:")
bench("list_append", bench_list_append, N)
bench("list_comprehension (1K×100)", bench_list_comprehension, 1000)
bench("dict_set_get", bench_dict_setget, N)
print()

print("Calls:")
bench("function_call", bench_function_call, N)
bench("method_call", bench_method_call, N)
print()

print("Objects:")
bench("attr_access", bench_attr_access, N)
print()

print("Control flow:")
bench("try_except", bench_try_except, N)
bench("exception_raise (10K)", bench_exception_raise, 10000)
bench("fibonacci(20) ×100", bench_fib, 100)
print()

print("=" * 70)
print("Done.")
