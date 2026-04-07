"""_compat_pickle — Compatibility mapping for pickle."""

# Mappings to handle Python 2→3 module renames for unpickling
IMPORT_MAPPING = {
    'copy_reg': 'copyreg',
    'Queue': 'queue',
    'ConfigParser': 'configparser',
    'repr': 'reprlib',
    'tkinter': 'tkinter',
}

NAME_MAPPING = {
    ('__builtin__', 'xrange'): ('builtins', 'range'),
    ('__builtin__', 'reduce'): ('functools', 'reduce'),
    ('__builtin__', 'unicode'): ('builtins', 'str'),
    ('__builtin__', 'basestring'): ('builtins', 'str'),
    ('__builtin__', 'long'): ('builtins', 'int'),
    ('__builtin__', 'raw_input'): ('builtins', 'input'),
    ('exceptions', 'StandardError'): ('builtins', 'Exception'),
}

REVERSE_IMPORT_MAPPING = {v: k for k, v in IMPORT_MAPPING.items()}
REVERSE_NAME_MAPPING = {v: k for k, v in NAME_MAPPING.items()}
