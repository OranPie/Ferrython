# Ferrython Generic Execution Path Benchmarks
# Purpose: isolate fallback-heavy paths that are common in ordinary Python code.
#
# Run with:
#   target/release/ferrython tests/benchmarks/bench_generic_paths.py
#   python3 tests/benchmarks/bench_generic_paths.py

import time


def bench(name, fn, rounds):
    best = float("inf")
    best_result = None
    for _ in range(3):
        start = time.time()
        result = fn(rounds)
        elapsed = time.time() - start
        if elapsed < best:
            best = elapsed
            best_result = result
    ops_per_sec = rounds / best if best > 0 else 0
    print(f"  {name:42s}  {best:.4f}s  ({ops_per_sec:.0f} rounds/s)  checksum={best_result}")


class Simple:
    class_value = 17

    def __init__(self, value):
        self.value = value
        self.other = value + 1

    def method0(self):
        return self.value

    def method1(self, x):
        return self.value + x

    def method2(self, x, y):
        return self.value + x + y


class HashKey:
    def __init__(self, group, value):
        self.group = group
        self.value = value

    def __hash__(self):
        return self.group * 1000003 + self.value

    def __eq__(self, other):
        try:
            return self.group == other.group and self.value == other.value
        except AttributeError:
            return False


class Descriptor:
    def __get__(self, obj, owner):
        if obj is None:
            return self
        return obj.value + 5


class WithDescriptor:
    computed = Descriptor()

    def __init__(self, value):
        self.value = value


def free0():
    return 1


def free1(x):
    return x + 1


def free2(x, y):
    return x + y


def bench_free_function_calls(rounds):
    total = 0
    for i in range(rounds * 200):
        total += free0()
        total += free1(i)
        total += free2(i, 3)
    return total


def bench_bound_method_calls(rounds):
    total = 0
    obj = Simple(3)
    for i in range(rounds * 160):
        total += obj.method0()
        total += obj.method1(i)
        total += obj.method2(i, 5)
    return total


def bench_attr_read_write(rounds):
    total = 0
    obj = Simple(3)
    for i in range(rounds * 500):
        obj.value = i
        total += obj.value
        total += obj.other
    return total


def bench_class_attr_lookup(rounds):
    total = 0
    obj = Simple(3)
    for _ in range(rounds * 500):
        total += obj.class_value
    return total


def bench_getattr_hasattr(rounds):
    total = 0
    obj = Simple(3)
    for _ in range(rounds * 220):
        total += getattr(obj, "value")
        if hasattr(obj, "missing"):
            total += 1000
        if hasattr(obj, "method1"):
            total += 1
    return total


def bench_descriptor_get(rounds):
    total = 0
    obj = WithDescriptor(9)
    for _ in range(rounds * 300):
        total += obj.computed
    return total


def bench_hash_dunder(rounds):
    total = 0
    keys = [HashKey(i % 17, i) for i in range(160)]
    for r in range(rounds * 20):
        for key in keys:
            total += hash(key)
        total += r
    return total


def bench_eq_dunder(rounds):
    total = 0
    left = [HashKey(i % 17, i) for i in range(160)]
    right = [HashKey(i % 17, i) for i in range(160)]
    for _ in range(rounds * 20):
        for i in range(160):
            if left[i] == right[i]:
                total += i
    return total


def bench_custom_dict_lookup(rounds):
    total = 0
    keys = [HashKey(i % 31, i) for i in range(300)]
    d = {}
    for key in keys:
        d[key] = key.value
    for r in range(rounds * 16):
        base = r % 300
        for i in range(120):
            value = (base + i) % 300
            total += d.get(HashKey(value % 31, value), 0)
    return total + len(d)


def bench_custom_set_lookup(rounds):
    total = 0
    s = set(HashKey(i % 19, i) for i in range(256))
    for r in range(rounds * 20):
        base = r % 256
        for i in range(96):
            value = (base + i) % 256
            if HashKey(value % 19, value) in s:
                total += value
    return total + len(s)


print("=" * 76)
print("Ferrython Generic Execution Path Benchmarks")
print("=" * 76)
print()

bench("free function call 0/1/2 args", bench_free_function_calls, 90)
bench("bound method call 0/1/2 args", bench_bound_method_calls, 90)
bench("instance attr read/write", bench_attr_read_write, 90)
bench("class attr lookup", bench_class_attr_lookup, 90)
bench("getattr/hasattr mixed", bench_getattr_hasattr, 80)
bench("descriptor __get__", bench_descriptor_get, 80)
bench("custom __hash__ dispatch", bench_hash_dunder, 80)
bench("custom __eq__ dispatch", bench_eq_dunder, 80)
bench("custom dict lookup", bench_custom_dict_lookup, 50)
bench("custom set lookup", bench_custom_set_lookup, 50)

print()
print("=" * 76)
print("Done.")
