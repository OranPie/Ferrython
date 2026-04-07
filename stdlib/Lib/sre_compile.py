"""Internal support module for sre — compile pattern strings to Pattern objects.

This is a compatibility shim; the actual regex engine is implemented in Rust.
"""

import re as _re

MAXREPEAT = 4294967295
MAXGROUPS = 2147483647

# Pattern flag constants
SRE_FLAG_IGNORECASE = 2
SRE_FLAG_LOCALE = 4
SRE_FLAG_MULTILINE = 8
SRE_FLAG_DOTALL = 16
SRE_FLAG_UNICODE = 32
SRE_FLAG_VERBOSE = 64
SRE_FLAG_DEBUG = 128
SRE_FLAG_ASCII = 256
SRE_FLAG_TEMPLATE = 1

def compile(p, flags=0):
    """Compile a pattern string to a Pattern object."""
    if isinstance(p, str):
        return _re.compile(p, flags)
    return p

def isstring(obj):
    return isinstance(obj, (str, bytes))
