# Architectural Probe Benchmarks
# Purpose: Isolate ROOT CAUSES of performance differences vs CPython.
# These test the interpreter's fundamental costs, NOT Python-level features.
#
# Categories:
#   1. Object allocation & refcount (Rc overhead)
#   2. Dispatch loop overhead (instruction throughput)
#   3. Frame creation/teardown (call overhead)
#   4. Collection internals (hash, resize, iteration)
#   5. String internals (allocation, interning)
#   6. Exception machinery (create, propagate, catch)

import time

def bench(name, fn, iterations=100000):
    # Run 3 times, take the best (least noisy)
    best = float('inf')
    for _ in range(3):
        start = time.time()
        fn(iterations)
        elapsed = time.time() - start
        if elapsed < best:
            best = elapsed
    ops_per_sec = iterations / best if best > 0 else 0
    print(f"  {name:40s}  {best:.4f}s  ({ops_per_sec:.0f} ops/s)")

# ============================================================
# 1. OBJECT ALLOCATION & REFCOUNT
#    Probes: Rc::new cost, Rc::clone cost, Rc::drop cost
# ============================================================

def bench_alloc_int(n):
    """Create N integer objects. Tests PyObject::int allocation."""
    for i in range(n):
        x = i

def bench_alloc_tuple_small(n):
    """Create N small tuples (2 elements). Tests tuple+Rc allocation."""
    for i in range(n):
        t = (i, i)

def bench_alloc_tuple_large(n):
    """Create N larger tuples (10 elements). Tests scaling with size."""
    for i in range(n):
        t = (i, i, i, i, i, i, i, i, i, i)

def bench_alloc_list_empty(n):
    """Create N empty lists. Tests List variant + Vec allocation."""
    for i in range(n):
        x = []

def bench_alloc_list_small(n):
    """Create N small lists. Tests list + element allocation."""
    for i in range(n):
        x = [1, 2, 3]

def bench_alloc_dict_empty(n):
    """Create N empty dicts. Tests Dict variant + FxHashMap allocation."""
    for i in range(n):
        x = {}

def bench_alloc_str_short(n):
    """Create N short strings. Tests CompactString inline (<=24 bytes)."""
    for i in range(n):
        x = "hello"

def bench_alloc_str_long(n):
    """Create N long strings. Tests CompactString heap allocation."""
    for i in range(n):
        x = "this is a string that is definitely longer than twenty four bytes"

def bench_refcount_clone(n):
    """Assign same object N times. Tests Rc::clone + Rc::drop cost."""
    obj = [1, 2, 3]
    for i in range(n):
        x = obj

def bench_refcount_deep(n):
    """Build and tear down reference chains. Tests Rc drop cascading."""
    for i in range(n):
        a = [1]
        b = [a]
        c = [b]
        d = [c]

# ============================================================
# 2. DISPATCH LOOP OVERHEAD
#    Probes: instruction decode, opcode match, PC advance
# ============================================================

def bench_nop_loop(n):
    """Tight loop with pass. Measures pure dispatch overhead per iteration."""
    for i in range(n):
        pass

def bench_load_store_local(n):
    """Load and store to local variable. Tests LOAD_FAST/STORE_FAST."""
    x = 0
    for i in range(n):
        x = x

def bench_multi_assign(n):
    """Multiple assignments per iteration. Tests instruction throughput."""
    for i in range(n):
        a = i
        b = i
        c = i
        d = i

def bench_compare_jump(n):
    """Compare + conditional jump. Tests COMPARE_OP + POP_JUMP_IF_FALSE."""
    x = 5
    s = 0
    for i in range(n):
        if x > 3:
            s = s + 1

def bench_binary_ops_chain(n):
    """Chain of binary ops. Tests BINARY_ADD throughput."""
    for i in range(n):
        x = i + i + i + i

def bench_unary_not(n):
    """Unary not in loop. Tests UNARY_NOT dispatch."""
    x = True
    for i in range(n):
        x = not x

# ============================================================
# 3. FRAME CREATION & TEARDOWN
#    Probes: call overhead, arg passing, return value
# ============================================================

def empty():
    pass

def bench_call_empty(n):
    """Call empty function. Tests frame alloc + setup + teardown."""
    for i in range(n):
        empty()

def identity(x):
    return x

def bench_call_identity(n):
    """Call with 1 arg + return. Tests arg passing + return."""
    for i in range(n):
        identity(i)

def add3(a, b, c):
    return a + b + c

def bench_call_3args(n):
    """Call with 3 args. Tests multi-arg frame setup."""
    for i in range(n):
        add3(i, i, i)

def bench_call_nested(n):
    """Nested calls (2 deep). Tests frame stack depth."""
    def outer(x):
        return inner(x)
    def inner(x):
        return x
    for i in range(n):
        outer(i)

def bench_call_recursive_5(n):
    """Recursive to depth 5. Tests repeated frame alloc/dealloc."""
    def rec(depth):
        if depth <= 0:
            return 0
        return rec(depth - 1)
    for i in range(n):
        rec(5)

def bench_closure_capture(n):
    """Closure capturing 1 variable. Tests Cell/freevar access."""
    x = 42
    def get_x():
        return x
    for i in range(n):
        get_x()

def bench_closure_capture_many(n):
    """Closure capturing 5 variables. Tests multiple Cell lookups."""
    a, b, c, d, e = 1, 2, 3, 4, 5
    def get_all():
        return a + b + c + d + e
    for i in range(n):
        get_all()

# ============================================================
# 4. COLLECTION INTERNALS
#    Probes: hash cost, resize, iteration, indexing
# ============================================================

def bench_list_grow(n):
    """Append to list from empty. Tests Vec reallocation strategy."""
    lst = []
    for i in range(n):
        lst.append(i)

def bench_list_pop(n):
    """Pop from end of list. Tests Vec::pop."""
    lst = list(range(n))
    for i in range(n):
        lst.pop()

def bench_list_iterate(n):
    """Iterate over 1000-element list N/1000 times. Tests ForIter cost."""
    lst = list(range(1000))
    for _ in range(n // 1000):
        for x in lst:
            pass

def bench_list_setitem(n):
    """Set list item by index. Tests STORE_SUBSCR for list."""
    lst = list(range(100))
    for i in range(n):
        lst[i % 100] = i

def bench_dict_insert_int(n):
    """Insert int keys. Tests hash(int) + FxHashMap insert."""
    d = {}
    for i in range(n):
        d[i] = i

def bench_dict_insert_str(n):
    """Insert string keys. Tests hash(str) + FxHashMap insert."""
    d = {}
    keys = ["key_" + str(i) for i in range(min(n, 10000))]
    for k in keys:
        d[k] = 1

def bench_dict_lookup_hit(n):
    """Dict lookup (always hits). Tests hash + probe + compare."""
    d = {i: i for i in range(1000)}
    for i in range(n):
        x = d[i % 1000]

def bench_dict_lookup_miss(n):
    """Dict lookup (always misses). Tests hash + probe for miss."""
    d = {i: i for i in range(1000)}
    for i in range(n):
        x = d.get(i + 10000)

def bench_dict_iterate(n):
    """Iterate dict keys. Tests dict iterator machinery."""
    d = {i: i for i in range(1000)}
    for _ in range(n // 1000):
        for k in d:
            pass

def bench_set_add(n):
    """Add to set. Tests hash + set insert."""
    s = set()
    for i in range(n):
        s.add(i)

def bench_set_lookup(n):
    """Set membership test. Tests hash + probe."""
    s = set(range(1000))
    for i in range(n):
        x = i % 1000 in s

def bench_tuple_unpack(n):
    """Unpack 3-tuple. Tests UNPACK_SEQUENCE."""
    t = (1, 2, 3)
    for i in range(n):
        a, b, c = t

def bench_range_iterate(n):
    """Iterate over range(1000). Tests RangeIter vs Iterator path."""
    for _ in range(n // 1000):
        for i in range(1000):
            pass

# ============================================================
# 5. STRING INTERNALS
#    Probes: creation, comparison, hashing, concatenation
# ============================================================

def bench_str_compare_eq(n):
    """Compare equal strings. Tests str equality (pointer or content)."""
    a = "hello world"
    b = "hello world"
    for i in range(n):
        x = a == b

def bench_str_compare_neq(n):
    """Compare unequal strings. Tests early-exit on mismatch."""
    a = "hello world"
    b = "hello xorld"
    for i in range(n):
        x = a != b

def bench_str_hash(n):
    """Hash strings via dict key use. Tests hash(str) cost."""
    d = {}
    keys = ["key_" + str(i % 100) for i in range(n)]
    for k in keys:
        d[k] = 1

def bench_str_len(n):
    """Get string length. Tests len(str)."""
    s = "hello world test string"
    for i in range(n):
        x = len(s)

def bench_str_slice(n):
    """Slice string. Tests str.__getitem__."""
    s = "hello world test string"
    for i in range(n):
        x = s[3:10]

def bench_str_join(n):
    """Join list of strings. Tests str.join."""
    parts = ["hello", "world", "test"]
    for i in range(n):
        x = " ".join(parts)

def bench_str_split(n):
    """Split string. Tests str.split."""
    s = "hello world test string"
    for i in range(n):
        x = s.split()

def bench_int_to_str(n):
    """Convert int to str. Tests str(int) / PyObject allocation."""
    for i in range(n):
        x = str(i)

def bench_str_startswith(n):
    """str.startswith. Tests method dispatch + string comparison."""
    s = "hello world"
    for i in range(n):
        x = s.startswith("hello")

# ============================================================
# 6. EXCEPTION MACHINERY
#    Probes: exception object creation, traceback, propagation
# ============================================================

def bench_exc_create(n):
    """Create exception objects (no raise). Tests PyException alloc."""
    for i in range(n):
        e = ValueError("test")

def bench_exc_raise_catch(n):
    """Raise and catch. Tests full raise + unwind + except."""
    for i in range(min(n, 10000)):
        try:
            raise ValueError("test")
        except ValueError:
            pass

def bench_exc_nested_try(n):
    """Nested try/except blocks. Tests block stack setup/teardown."""
    for i in range(n):
        try:
            try:
                try:
                    x = 1
                except:
                    pass
            except:
                pass
        except:
            pass

def bench_exc_type_match(n):
    """Exception type matching. Tests isinstance in except clause."""
    for i in range(min(n, 10000)):
        try:
            raise ValueError("test")
        except TypeError:
            pass
        except ValueError:
            pass

# ============================================================
# 7. ATTRIBUTE & GLOBAL ACCESS PATTERNS
#    Probes: attr lookup, namespace search, global dict
# ============================================================

class Simple:
    def __init__(self):
        self.x = 1

class Deep:
    def __init__(self):
        self.a = 1
        self.b = 2
        self.c = 3
        self.d = 4
        self.e = 5
        self.f = 6
        self.g = 7
        self.h = 8

def bench_attr_read_simple(n):
    """Read 1 attr. Tests single attr lookup."""
    obj = Simple()
    for i in range(n):
        x = obj.x

def bench_attr_read_deep(n):
    """Read 8th attr of object with 8 attrs. Tests lookup in larger namespace."""
    obj = Deep()
    for i in range(n):
        x = obj.h

def bench_attr_write(n):
    """Write attr. Tests STORE_ATTR dispatch."""
    obj = Simple()
    for i in range(n):
        obj.x = i

def bench_global_read_tight(n):
    """Read global in tight loop. Tests LOAD_GLOBAL cost."""
    global GLOBAL_VAR
    for i in range(n):
        x = GLOBAL_VAR

GLOBAL_VAR = 42

def bench_hasattr(n):
    """hasattr check. Tests attr lookup + exception handling for miss."""
    obj = Simple()
    for i in range(n):
        x = hasattr(obj, "x")

def bench_getattr(n):
    """getattr. Tests dynamic attr lookup."""
    obj = Simple()
    for i in range(n):
        x = getattr(obj, "x")

# ============================================================
# 8. MIXED / REALISTIC PATTERNS
#    Probes: combined costs in realistic patterns
# ============================================================

def bench_sum_list(n):
    """sum() builtin on list. Tests builtin + iteration."""
    lst = list(range(100))
    for i in range(n // 100):
        x = sum(lst)

def bench_enumerate_loop(n):
    """for i, x in enumerate(list). Tests enumerate iterator."""
    lst = list(range(100))
    for _ in range(n // 100):
        for i, x in enumerate(lst):
            pass

def bench_zip_loop(n):
    """for a, b in zip(l1, l2). Tests zip iterator."""
    l1 = list(range(100))
    l2 = list(range(100))
    for _ in range(n // 100):
        for a, b in zip(l1, l2):
            pass

def bench_map_consume(n):
    """Consume map() iterator. Tests map + lambda dispatch."""
    lst = list(range(100))
    for _ in range(n // 100):
        x = list(map(lambda x: x + 1, lst))

def bench_sorted_small(n):
    """Sort small list. Tests sorted() builtin."""
    for i in range(n // 10):
        x = sorted([5, 3, 8, 1, 9, 2, 7, 4, 6, 0])

def bench_type_call(n):
    """type(obj). Tests type() builtin dispatch."""
    x = 42
    for i in range(n):
        t = type(x)

def bench_len_call(n):
    """len(list). Tests len() builtin dispatch."""
    lst = [1, 2, 3]
    for i in range(n):
        x = len(lst)

def bench_bool_conversion(n):
    """bool(x) implicit. Tests truthiness evaluation."""
    x = [1]
    for i in range(n):
        if x:
            pass

# ============================================================
# RUN ALL
# ============================================================

print("=" * 70)
print("Architectural Probe Benchmarks")
print("=" * 70)
print()

N = 100000

print("1. OBJECT ALLOCATION & REFCOUNT:")
bench("alloc_int", bench_alloc_int, N)
bench("alloc_tuple_small (2)", bench_alloc_tuple_small, N)
bench("alloc_tuple_large (10)", bench_alloc_tuple_large, N)
bench("alloc_list_empty", bench_alloc_list_empty, N)
bench("alloc_list_small (3)", bench_alloc_list_small, N)
bench("alloc_dict_empty", bench_alloc_dict_empty, N)
bench("alloc_str_short", bench_alloc_str_short, N)
bench("alloc_str_long", bench_alloc_str_long, N)
bench("refcount_clone", bench_refcount_clone, N)
bench("refcount_deep", bench_refcount_deep, N)
print()

print("2. DISPATCH LOOP OVERHEAD:")
bench("nop_loop (for/pass)", bench_nop_loop, N)
bench("load_store_local", bench_load_store_local, N)
bench("multi_assign (4 stores)", bench_multi_assign, N)
bench("compare_jump", bench_compare_jump, N)
bench("binary_ops_chain (4 adds)", bench_binary_ops_chain, N)
bench("unary_not", bench_unary_not, N)
print()

print("3. FRAME CREATION & TEARDOWN:")
bench("call_empty", bench_call_empty, N)
bench("call_identity (1 arg)", bench_call_identity, N)
bench("call_3args", bench_call_3args, N)
bench("call_nested (2 deep)", bench_call_nested, N)
bench("call_recursive_5", bench_call_recursive_5, N)
bench("closure_capture (1 var)", bench_closure_capture, N)
bench("closure_capture_many (5 vars)", bench_closure_capture_many, N)
print()

print("4. COLLECTION INTERNALS:")
bench("list_grow (append)", bench_list_grow, N)
bench("list_pop", bench_list_pop, N)
bench("list_iterate (1000×N/1000)", bench_list_iterate, N)
bench("list_setitem", bench_list_setitem, N)
bench("dict_insert_int", bench_dict_insert_int, N)
bench("dict_insert_str", bench_dict_insert_str, N)
bench("dict_lookup_hit", bench_dict_lookup_hit, N)
bench("dict_lookup_miss", bench_dict_lookup_miss, N)
bench("dict_iterate", bench_dict_iterate, N)
bench("set_add", bench_set_add, N)
bench("set_lookup", bench_set_lookup, N)
bench("tuple_unpack (3)", bench_tuple_unpack, N)
bench("range_iterate (1000×N/1000)", bench_range_iterate, N)
print()

print("5. STRING INTERNALS:")
bench("str_compare_eq", bench_str_compare_eq, N)
bench("str_compare_neq", bench_str_compare_neq, N)
bench("str_hash (via dict)", bench_str_hash, N)
bench("str_len", bench_str_len, N)
bench("str_slice", bench_str_slice, N)
bench("str_join", bench_str_join, N)
bench("str_split", bench_str_split, N)
bench("int_to_str", bench_int_to_str, N)
bench("str_startswith", bench_str_startswith, N)
print()

print("6. EXCEPTION MACHINERY:")
bench("exc_create (no raise)", bench_exc_create, N)
bench("exc_raise_catch (10K)", bench_exc_raise_catch, 10000)
bench("exc_nested_try (3 deep)", bench_exc_nested_try, N)
bench("exc_type_match (10K)", bench_exc_type_match, 10000)
print()

print("7. ATTRIBUTE & GLOBAL ACCESS:")
bench("attr_read_simple", bench_attr_read_simple, N)
bench("attr_read_deep (8th of 8)", bench_attr_read_deep, N)
bench("attr_write", bench_attr_write, N)
bench("global_read_tight", bench_global_read_tight, N)
bench("hasattr", bench_hasattr, N)
bench("getattr", bench_getattr, N)
print()

print("8. MIXED / REALISTIC PATTERNS:")
bench("sum_list (100 elems)", bench_sum_list, N)
bench("enumerate_loop", bench_enumerate_loop, N)
bench("zip_loop", bench_zip_loop, N)
bench("map_consume (100 elems)", bench_map_consume, N)
bench("sorted_small (10 elems)", bench_sorted_small, N)
bench("type_call", bench_type_call, N)
bench("len_call", bench_len_call, N)
bench("bool_conversion", bench_bool_conversion, N)
print()

print("=" * 70)
print("Done.")
