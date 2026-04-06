"""Phase 105: Tests for deepened mmap, resource, numbers, typing additions."""

results = []
def check(name, got, expected):
    status = "PASS" if got == expected else "FAIL"
    if status == "FAIL":
        print(f"FAIL: {name} got: {repr(got)} expected: {repr(expected)}")
    results.append(status)

# ── mmap module ──
import mmap

# Constants
check("mmap_access_read", mmap.ACCESS_READ, 1)
check("mmap_access_write", mmap.ACCESS_WRITE, 2)
check("mmap_access_copy", mmap.ACCESS_COPY, 3)
check("mmap_pagesize", mmap.PAGESIZE > 0, True)
check("mmap_prot_read", mmap.PROT_READ, 1)
check("mmap_prot_write", mmap.PROT_WRITE, 2)

# Create anonymous mmap
m = mmap.mmap(-1, 100)
check("mmap_size", m.size(), 100)
check("mmap_len", len(m), 100)
check("mmap_tell_init", m.tell(), 0)

# write/read/tell
m.write(b"hello world")
check("mmap_tell_after_write", m.tell(), 11)
m.seek(0)
check("mmap_tell_after_seek", m.tell(), 0)
data = m.read(5)
check("mmap_read", data, b"hello")
check("mmap_tell_after_read", m.tell(), 5)

# read_byte
m.seek(0)
b = m.read_byte()
check("mmap_read_byte", b, ord('h'))

# write_byte
m.seek(0)
m.write_byte(72)  # 'H'
m.seek(0)
check("mmap_write_byte", m.read(1), b"H")

# find/rfind
m.seek(0)
m.write(b"hello world hello")
check("mmap_find", m.find(b"world"), 6)
check("mmap_find_not", m.find(b"xyz"), -1)
check("mmap_rfind", m.rfind(b"hello"), 12)
check("mmap_find_start", m.find(b"hello", 1), 12)

# readline
m.seek(0)
m.write(b"line1\nline2\n")
m.seek(0)
line = m.readline()
check("mmap_readline", line, b"line1\n")

# __getitem__ / __setitem__
m.seek(0)
m.write(b"ABCDEF")
check("mmap_getitem", m[0], 65)  # 'A'
check("mmap_getitem_neg", m[-1] >= 0, True)  # negative indexing

# resize
m.resize(200)
check("mmap_resize", m.size(), 200)

# move
m.seek(0)
m.write(b"ABCDEFGHIJ")
m.move(5, 0, 5)  # copy "ABCDE" to offset 5
m.seek(5)
check("mmap_move", m.read(5), b"ABCDE")

# context manager
with mmap.mmap(-1, 10) as mm:
    mm.write(b"test")
    mm.seek(0)
    check("mmap_context", mm.read(4), b"test")

# repr
check("mmap_repr", "mmap" in repr(m), True)

# ── resource module ──
import resource

# getrlimit returns (soft, hard) tuple
limits = resource.getrlimit(resource.RLIMIT_NOFILE)
check("resource_getrlimit_type", type(limits).__name__, "tuple")
check("resource_getrlimit_len", len(limits), 2)
check("resource_soft_positive", limits[0] != 0, True)

# Constants
check("resource_rlimit_cpu", resource.RLIMIT_CPU, 0)
check("resource_rlimit_stack", resource.RLIMIT_STACK, 3)
check("resource_rlimit_nproc", resource.RLIMIT_NPROC, 6)
check("resource_rusage_self", resource.RUSAGE_SELF, 0)
check("resource_rusage_children", resource.RUSAGE_CHILDREN, -1)

# getrusage — real data
usage = resource.getrusage(resource.RUSAGE_SELF)
check("resource_utime_type", type(usage.ru_utime).__name__, "float")
check("resource_stime_type", type(usage.ru_stime).__name__, "float")
check("resource_maxrss_positive", usage.ru_maxrss > 0, True)
check("resource_has_minflt", hasattr(usage, 'ru_minflt'), True)
check("resource_has_nvcsw", hasattr(usage, 'ru_nvcsw'), True)

# getpagesize
check("resource_pagesize", resource.getpagesize() > 0, True)

# ── numbers module ──
import numbers

# Hierarchy check
check("numbers_number_exists", hasattr(numbers, 'Number'), True)
check("numbers_complex_exists", hasattr(numbers, 'Complex'), True)
check("numbers_real_exists", hasattr(numbers, 'Real'), True)
check("numbers_rational_exists", hasattr(numbers, 'Rational'), True)
check("numbers_integral_exists", hasattr(numbers, 'Integral'), True)

# MRO: Integral -> Rational -> Real -> Complex -> Number
integral = numbers.Integral
check("numbers_integral_is_class", type(integral).__name__, "type")

# Abstract methods should exist on Complex
complex_cls = numbers.Complex
check("numbers_complex_has_add", hasattr(complex_cls, '__add__'), True)
check("numbers_complex_has_mul", hasattr(complex_cls, '__mul__'), True)
check("numbers_complex_has_abs", hasattr(complex_cls, '__abs__'), True)
check("numbers_complex_has_eq", hasattr(complex_cls, '__eq__'), True)

# Real should have float/trunc/floor/ceil
real_cls = numbers.Real
check("numbers_real_has_float", hasattr(real_cls, '__float__'), True)
check("numbers_real_has_floor", hasattr(real_cls, '__floor__'), True)
check("numbers_real_has_mod", hasattr(real_cls, '__mod__'), True)

# Integral should have bitwise ops
check("numbers_integral_has_and", hasattr(integral, '__and__'), True)
check("numbers_integral_has_or", hasattr(integral, '__or__'), True)
check("numbers_integral_has_xor", hasattr(integral, '__xor__'), True)
check("numbers_integral_has_invert", hasattr(integral, '__invert__'), True)

# ── typing additions ──
import typing

# New additions
check("typing_required", hasattr(typing, 'Required'), True)
check("typing_not_required", hasattr(typing, 'NotRequired'), True)
check("typing_readonly", hasattr(typing, 'ReadOnly'), True)
check("typing_buffer", hasattr(typing, 'Buffer'), True)
check("typing_dataclass_transform", hasattr(typing, 'dataclass_transform'), True)
check("typing_get_overloads", callable(typing.get_overloads), True)
check("typing_clear_overloads", callable(typing.clear_overloads), True)
check("typing_is_typeddict", callable(typing.is_typeddict), True)

# get_overloads returns list
check("typing_get_overloads_empty", typing.get_overloads(lambda: None), [])

# clear_overloads returns None
check("typing_clear_overloads_none", typing.clear_overloads(), None)

# is_typeddict for regular dict
check("typing_is_typeddict_false", typing.is_typeddict(dict), False)

# ── Summary ──
passed = results.count("PASS")
failed = results.count("FAIL")
print(f"\n{passed} passed, {failed} failed out of {len(results)} tests")
if failed > 0:
    raise SystemExit(1)
