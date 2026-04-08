"""
distutils.util — Miscellaneous utility functions.
"""

import os
import sys

def get_platform():
    """Return a string that identifies the current platform."""
    if os.name == 'nt':
        return 'win-amd64' if sys.maxsize > 2**32 else 'win32'
    import platform
    return f'{platform.system().lower()}-{platform.machine()}'

def convert_path(pathname):
    """Convert a /-separated pathname to one using the current OS separator."""
    if os.sep == '/':
        return pathname
    return pathname.replace('/', os.sep)

def strtobool(val):
    """Convert a string representation of truth to true (1) or false (0)."""
    val = val.lower()
    if val in ('y', 'yes', 't', 'true', 'on', '1'):
        return 1
    elif val in ('n', 'no', 'f', 'false', 'off', '0'):
        return 0
    else:
        raise ValueError(f"invalid truth value {val!r}")

def byte_compile(py_files, optimize=0, force=0, prefix=None, base_dir=None, verbose=1, dry_run=0, direct=None):
    """Byte-compile Python source files (stub)."""
    pass

def split_quoted(s):
    """Split a string up according to Unix shell-like quoting rules."""
    import shlex
    return shlex.split(s)
