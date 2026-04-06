"""ntpath — Common operations on Windows pathnames.

On Unix, this module provides Windows path operations for cross-platform code.
"""
import os

curdir = '.'
pardir = '..'
extsep = '.'
sep = '\\'
pathsep = ';'
altsep = '/'
defpath = '.;C:\\bin'
devnull = 'nul'


def normcase(s):
    """Normalize case of pathname. On Windows, lowercases and converts / to \\."""
    return s.replace('/', '\\').lower()


def isabs(s):
    """Test whether a path is absolute."""
    s = s.replace('/', '\\')
    # UNC paths
    if s[:2] == '\\\\':
        return True
    # Drive letter
    if len(s) >= 3 and s[1] == ':' and s[2] == '\\':
        return True
    return False


def join(path, *paths):
    """Join two or more pathname components."""
    result = path
    for p in paths:
        if isabs(p) or (len(p) >= 2 and p[1] == ':'):
            result = p
        elif result.endswith(('\\', '/')):
            result = result + p
        else:
            result = result + '\\' + p
    return result


def split(p):
    """Split a pathname into (head, tail)."""
    p = p.replace('/', '\\')
    # Handle drive
    d = splitdrive(p)[0]
    i = len(p)
    while i > len(d) and p[i-1] not in '\\':
        i -= 1
    head = p[:i]
    tail = p[i:]
    head = head.rstrip('\\') or head
    return head, tail


def splitext(p):
    """Split the extension from a pathname."""
    dot = p.rfind('.')
    sep_idx = max(p.rfind('\\'), p.rfind('/'))
    if dot > sep_idx:
        return p[:dot], p[dot:]
    return p, ''


def splitdrive(p):
    """Split a pathname into drive/UNC sharepoint and relative path."""
    if len(p) >= 2:
        if p[0:2] == '\\\\' or p[0:2] == '//':
            # UNC path
            idx = p.find('\\', 2)
            if idx == -1:
                idx = p.find('/', 2)
            if idx == -1:
                return p, ''
            idx2 = p.find('\\', idx + 1)
            if idx2 == -1:
                idx2 = p.find('/', idx + 1)
            if idx2 == -1:
                return p, ''
            return p[:idx2], p[idx2:]
        if p[1] == ':':
            return p[:2], p[2:]
    return '', p


def basename(p):
    """Return the base name of pathname p."""
    return split(p)[1]


def dirname(p):
    """Return the directory name of pathname p."""
    return split(p)[0]


def normpath(path):
    """Normalize path, eliminating double slashes, etc."""
    path = path.replace('/', '\\')
    prefix, path = splitdrive(path)
    while path[:1] == '\\':
        prefix = prefix + '\\'
        path = path[1:]
    comps = path.split('\\')
    i = 0
    while i < len(comps):
        if comps[i] == '.':
            del comps[i]
        elif comps[i] == '..' and i > 0 and comps[i-1] != '..':
            del comps[i-1:i+1]
            i -= 1
        elif comps[i] == '' and i > 0:
            del comps[i]
        else:
            i += 1
    if not prefix and not comps:
        comps.append('.')
    return prefix + '\\'.join(comps)


def exists(path):
    """Test whether a path exists."""
    try:
        os.stat(path)
        return True
    except OSError:
        return False


def isfile(path):
    return os.path.isfile(path)


def isdir(path):
    return os.path.isdir(path)


def abspath(path):
    """Return an absolute path."""
    if not isabs(path):
        cwd = os.getcwd()
        path = join(cwd, path)
    return normpath(path)


def expanduser(path):
    """Expand ~ in path."""
    if not path.startswith('~'):
        return path
    home = os.environ.get('USERPROFILE', os.environ.get('HOME', ''))
    if path == '~':
        return home
    if path.startswith('~\\') or path.startswith('~/'):
        return home + path[1:]
    return path


def relpath(path, start=None):
    """Return a relative version of a path."""
    if start is None:
        start = curdir
    return path  # simplified
