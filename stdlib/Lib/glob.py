"""Pure Python implementation of the glob module.

Filename globbing utility using fnmatch.
"""

import os
import re
import fnmatch


def glob(pathname, *, root_dir=None, dir_fd=None, recursive=False,
         include_hidden=False):
    """Return a list of paths matching a pathname pattern."""
    return list(iglob(pathname, root_dir=root_dir, dir_fd=dir_fd,
                      recursive=recursive, include_hidden=include_hidden))


def iglob(pathname, *, root_dir=None, dir_fd=None, recursive=False,
          include_hidden=False):
    """Return an iterator of paths matching a pathname pattern."""
    sys_root = root_dir or ''
    
    dirname, basename = os.path.split(pathname)
    if not has_magic(pathname):
        full = os.path.join(sys_root, pathname) if sys_root else pathname
        if basename:
            if os.path.lexists(full):
                yield pathname
        else:
            if os.path.isdir(full):
                yield pathname
        return
    
    if not dirname:
        if recursive and _isrecursive(basename):
            yield from _glob2(sys_root, basename, '.', include_hidden)
        else:
            yield from _glob1(sys_root, basename, '.', include_hidden)
        return
    
    if dirname != pathname and has_magic(dirname):
        dirs = iglob(dirname, root_dir=root_dir, recursive=recursive,
                     include_hidden=include_hidden)
    else:
        dirs = [dirname]
    
    if has_magic(basename):
        if recursive and _isrecursive(basename):
            glob_in_dir = _glob2
        else:
            glob_in_dir = _glob1
    else:
        glob_in_dir = _glob0
    
    for d in dirs:
        yield from glob_in_dir(sys_root, basename, d, include_hidden)


def _glob1(sys_root, pattern, dirname, include_hidden):
    full = os.path.join(sys_root, dirname) if sys_root else dirname
    try:
        names = os.listdir(full if full else '.')
    except (OSError, IOError):
        return
    if not include_hidden and not _ishidden(pattern):
        names = [x for x in names if not _ishidden(x)]
    for name in names:
        if fnmatch.fnmatch(name, pattern):
            yield os.path.join(dirname, name) if dirname != '.' else name


def _glob0(sys_root, basename, dirname, include_hidden):
    full = os.path.join(sys_root, dirname) if sys_root else dirname
    fullpath = os.path.join(full, basename) if full else basename
    if os.path.lexists(fullpath):
        yield os.path.join(dirname, basename) if dirname != '.' else basename


def _glob2(sys_root, pattern, dirname, include_hidden):
    """Recursive globbing with **."""
    yield from _rlistdir(sys_root, dirname, include_hidden)


def _rlistdir(sys_root, dirname, include_hidden):
    full = os.path.join(sys_root, dirname) if sys_root else dirname
    try:
        names = os.listdir(full if full else '.')
    except (OSError, IOError):
        return
    yield dirname if dirname != '.' else ''
    for name in names:
        if not include_hidden and _ishidden(name):
            continue
        path = os.path.join(dirname, name) if dirname != '.' else name
        fullpath = os.path.join(sys_root, path) if sys_root else path
        if os.path.isdir(fullpath):
            yield from _rlistdir(sys_root, path, include_hidden)
        else:
            yield path


def has_magic(s):
    """Check whether a path contains any glob wildcards."""
    return bool(re.search(r'[*?\[\]]', s))


def _ishidden(path):
    return path[0:1] == '.'


def _isrecursive(pattern):
    return pattern == '**'


def escape(pathname):
    """Escape all special characters."""
    special_chars = ('?', '*', '[')
    pathname = pathname.replace('[', '[[]')
    for char in ('?', '*'):
        pathname = pathname.replace(char, '[' + char + ']')
    return pathname
