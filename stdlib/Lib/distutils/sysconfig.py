"""
distutils.sysconfig — Provides access to Python's configuration info.
"""

import sys
import os

PREFIX = sys.prefix
EXEC_PREFIX = getattr(sys, 'exec_prefix', sys.prefix)
BASE_PREFIX = getattr(sys, 'base_prefix', sys.prefix)

def get_python_inc(plat_specific=0, prefix=None):
    """Return the directory for Python header files."""
    if prefix is None:
        prefix = PREFIX
    return os.path.join(prefix, 'include', f'python{sys.version_info[0]}.{sys.version_info[1]}')

def get_python_lib(plat_specific=0, standard_lib=0, prefix=None):
    """Return the directory for Python library files."""
    if prefix is None:
        prefix = PREFIX
    if standard_lib:
        return os.path.join(prefix, 'lib', f'python{sys.version_info[0]}.{sys.version_info[1]}')
    return os.path.join(prefix, 'lib', f'python{sys.version_info[0]}.{sys.version_info[1]}', 'site-packages')

_config_vars = None

def get_config_vars(*args):
    """Return a dict of all config variables, or the values of requested names."""
    global _config_vars
    if _config_vars is None:
        _config_vars = {
            'prefix': PREFIX,
            'exec_prefix': EXEC_PREFIX,
            'py_version_short': f'{sys.version_info[0]}.{sys.version_info[1]}',
            'LIBDEST': get_python_lib(standard_lib=1),
            'BINDIR': os.path.join(PREFIX, 'bin'),
            'INCLUDEDIR': get_python_inc(),
            'SO': '.so',
            'EXT_SUFFIX': '.so',
            'SOABI': f'cpython-{sys.version_info[0]}{sys.version_info[1]}',
        }
    if args:
        return [_config_vars.get(name) for name in args]
    return _config_vars

def get_config_var(name):
    """Return the value of a single config variable."""
    result = get_config_vars(name)
    return result[0] if result else None

def customize_compiler(compiler):
    """Customize a compiler instance (no-op in stub)."""
    pass
