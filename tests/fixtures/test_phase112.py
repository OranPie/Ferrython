# test_phase112.py — ctypes stub, module completeness checks

# ── ctypes ──
import ctypes

# Type classes exist
assert hasattr(ctypes, 'c_int')
assert hasattr(ctypes, 'c_long')
assert hasattr(ctypes, 'c_char')
assert hasattr(ctypes, 'c_char_p')
assert hasattr(ctypes, 'c_wchar_p')
assert hasattr(ctypes, 'c_void_p')
assert hasattr(ctypes, 'c_double')
assert hasattr(ctypes, 'c_float')
assert hasattr(ctypes, 'c_bool')
assert hasattr(ctypes, 'c_size_t')
assert hasattr(ctypes, 'c_ssize_t')
assert hasattr(ctypes, 'c_longlong')
assert hasattr(ctypes, 'c_ulonglong')

# Fixed-width types
assert hasattr(ctypes, 'c_int8')
assert hasattr(ctypes, 'c_int16')
assert hasattr(ctypes, 'c_int32')
assert hasattr(ctypes, 'c_int64')
assert hasattr(ctypes, 'c_uint8')
assert hasattr(ctypes, 'c_uint16')
assert hasattr(ctypes, 'c_uint32')
assert hasattr(ctypes, 'c_uint64')

# Structure/Union/Array
assert hasattr(ctypes, 'Structure')
assert hasattr(ctypes, 'Union')
assert hasattr(ctypes, 'Array')

# CDLL loader
assert hasattr(ctypes, 'CDLL')

# Utility functions
assert hasattr(ctypes, 'POINTER')
assert hasattr(ctypes, 'pointer')
assert hasattr(ctypes, 'cast')
assert hasattr(ctypes, 'byref')
assert hasattr(ctypes, 'sizeof')
assert hasattr(ctypes, 'addressof')
assert hasattr(ctypes, 'create_string_buffer')
assert hasattr(ctypes, 'create_unicode_buffer')

# sizeof returns a number
s = ctypes.sizeof(ctypes.c_int)
assert isinstance(s, int)

# create_string_buffer
buf = ctypes.create_string_buffer(10)
assert len(buf) == 10

# ctypes.util
assert hasattr(ctypes, 'util')
assert hasattr(ctypes.util, 'find_library')

# find_library returns None for nonexistent
result = ctypes.util.find_library("nonexistent_lib_xyz_123")
assert result is None

# ── Verify collections.abc ABCs ──
import collections.abc

assert hasattr(collections.abc, 'Hashable')
assert hasattr(collections.abc, 'Iterable')
assert hasattr(collections.abc, 'Iterator')
assert hasattr(collections.abc, 'Sequence')
assert hasattr(collections.abc, 'MutableSequence')
assert hasattr(collections.abc, 'Mapping')
assert hasattr(collections.abc, 'MutableMapping')
assert hasattr(collections.abc, 'Set')
assert hasattr(collections.abc, 'MutableSet')
assert hasattr(collections.abc, 'Callable')
assert hasattr(collections.abc, 'Awaitable')
assert hasattr(collections.abc, 'Coroutine')
assert hasattr(collections.abc, 'AsyncIterable')
assert hasattr(collections.abc, 'AsyncIterator')

# ── Verify typing module completeness ──
import typing
assert hasattr(typing, 'Optional')
assert hasattr(typing, 'Union')
assert hasattr(typing, 'List')
assert hasattr(typing, 'Dict')
assert hasattr(typing, 'Tuple')
assert hasattr(typing, 'Set')
assert hasattr(typing, 'Any')
assert hasattr(typing, 'TypeVar')
assert hasattr(typing, 'Generic')

print("phase112: all tests passed")
