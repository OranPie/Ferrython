"""POSIX path manipulation functions.

Common operations on POSIX pathnames. Instead of importing this module
directly, import os and refer to this module as os.path.
"""

import os

curdir = '.'
pardir = '..'
extsep = '.'
sep = '/'
pathsep = ':'
defpath = '/bin:/usr/bin'
altsep = None
devnull = '/dev/null'

__all__ = [
    'normcase', 'isabs', 'join', 'splitdrive', 'split', 'splitext',
    'basename', 'dirname', 'commonprefix', 'commonpath',
    'expanduser', 'expandvars', 'normpath', 'abspath',
    'realpath', 'relpath', 'islink', 'exists', 'lexists',
    'isfile', 'isdir',
]


def normcase(s):
    """Normalize case of pathname. Has no effect on POSIX."""
    return str(s)


def isabs(s):
    """Test whether a path is absolute."""
    s = str(s)
    return s.startswith('/')


def join(a, *p):
    """Join two or more pathname components, inserting '/' as needed."""
    a = str(a)
    path = a
    for b in p:
        b = str(b)
        if b.startswith('/'):
            path = b
        elif not path or path.endswith('/'):
            path = path + b
        else:
            path = path + '/' + b
    return path


def split(p):
    """Split a pathname. Returns tuple (head, tail) where tail is
    everything after the final slash."""
    p = str(p)
    i = p.rfind('/') + 1
    head = p[:i]
    tail = p[i:]
    if head and head != '/' * len(head):
        head = head.rstrip('/')
    return (head, tail)


def splitext(p):
    """Split the extension from a pathname.
    Returns (root, ext) where ext starts with a period or is empty."""
    p = str(p)
    # Find the rightmost dot that is after the rightmost separator
    sep_index = p.rfind('/')
    dot_index = p.rfind('.')
    if dot_index > sep_index and dot_index > 0:
        # Make sure it's not a leading dot in the filename
        fname_start = sep_index + 1
        if dot_index == fname_start:
            # File starts with dot like .bashrc - no extension
            return (p, '')
        return (p[:dot_index], p[dot_index:])
    return (p, '')


def splitdrive(p):
    """Split a pathname into drive and path. On POSIX, drive is always empty."""
    return ('', str(p))


def basename(p):
    """Returns the final component of a pathname."""
    p = str(p)
    i = p.rfind('/') + 1
    return p[i:]


def dirname(p):
    """Returns the directory component of a pathname."""
    p = str(p)
    i = p.rfind('/') + 1
    head = p[:i]
    if head and head != '/' * len(head):
        head = head.rstrip('/')
    return head


def commonprefix(m):
    """Given a list of pathnames, returns the longest common leading component."""
    if not m:
        return ''
    s1 = min(m)
    s2 = max(m)
    for i, c in enumerate(s1):
        if c != s2[i]:
            return s1[:i]
    return s1


def commonpath(paths):
    """Return the longest common sub-path of each pathname in the sequence."""
    if not paths:
        raise ValueError('commonpath() arg is an empty sequence')

    paths = [str(p) for p in paths]
    split_paths = [p.split('/') for p in paths]

    isabs_val = paths[0].startswith('/')
    for p in paths[1:]:
        if p.startswith('/') != isabs_val:
            raise ValueError("Can't mix absolute and relative paths")

    shortest = min(split_paths, key=len)
    common = []
    for i, component in enumerate(shortest):
        if all(path[i] == component for path in split_paths):
            common.append(component)
        else:
            break

    result = '/'.join(common)
    if isabs_val and not result.startswith('/'):
        result = '/' + result
    return result


def normpath(path):
    """Normalize path, eliminating double slashes and resolving . and .."""
    path = str(path)
    if not path:
        return '.'

    initial_slashes = 1 if path.startswith('/') else 0
    # POSIX allows one or two initial slashes, but treats three or more as one
    if initial_slashes and path.startswith('//') and not path.startswith('///'):
        initial_slashes = 2

    comps = path.split('/')
    new_comps = []
    for comp in comps:
        if comp in ('', '.'):
            continue
        if comp == '..':
            if new_comps and new_comps[-1] != '..':
                new_comps.pop()
            elif not initial_slashes:
                new_comps.append(comp)
        else:
            new_comps.append(comp)

    path = '/'.join(new_comps)
    if initial_slashes:
        path = '/' * initial_slashes + path
    return path or '.'


def abspath(path):
    """Return an absolute path."""
    path = str(path)
    if not isabs(path):
        cwd = os.getcwd()
        path = join(cwd, path)
    return normpath(path)


def realpath(filename):
    """Return the canonical path of the specified filename."""
    return abspath(filename)


def relpath(path, start=None):
    """Return a relative filepath to path from the start directory."""
    if start is None:
        start = curdir
    path = str(path)
    start = str(start)

    start_list = [x for x in abspath(start).split('/') if x]
    path_list = [x for x in abspath(path).split('/') if x]

    i = 0
    for s, p in zip(start_list, path_list):
        if s != p:
            break
        i += 1

    rel_list = ['..'] * (len(start_list) - i) + path_list[i:]
    if not rel_list:
        return curdir
    return join(*rel_list)


def expanduser(path):
    """Expand ~ and ~user constructions."""
    path = str(path)
    if not path.startswith('~'):
        return path

    i = path.find('/', 1)
    if i < 0:
        i = len(path)

    if i == 1:
        # ~/...
        userhome = os.environ.get('HOME', '')
        if not userhome:
            userhome = '/'
    else:
        # ~user/...
        name = path[1:i]
        userhome = '/home/' + name

    return userhome + path[i:]


def expandvars(path):
    """Expand shell variables of form $var and ${var}."""
    path = str(path)
    if '$' not in path:
        return path

    result = []
    i = 0
    while i < len(path):
        c = path[i]
        if c == '$':
            if i + 1 < len(path) and path[i + 1] == '{':
                # ${var}
                j = path.find('}', i + 2)
                if j >= 0:
                    name = path[i + 2:j]
                    value = os.environ.get(name, '${' + name + '}')
                    result.append(value)
                    i = j + 1
                    continue
            elif i + 1 < len(path) and (path[i + 1].isalpha() or path[i + 1] == '_'):
                # $var
                j = i + 1
                while j < len(path) and (path[j].isalnum() or path[j] == '_'):
                    j += 1
                name = path[i + 1:j]
                value = os.environ.get(name, '$' + name)
                result.append(value)
                i = j
                continue
        result.append(c)
        i += 1
    return ''.join(result)


def islink(path):
    """Test whether a path is a symbolic link."""
    try:
        st = os.lstat(str(path))
        return _stat_S_ISLNK(st.st_mode)
    except (OSError, ValueError, AttributeError):
        return False


def _stat_S_ISLNK(mode):
    """Check if mode is a symlink."""
    return (mode & 0o170000) == 0o120000


def exists(path):
    """Test whether a path exists."""
    try:
        os.stat(str(path))
    except (OSError, ValueError):
        return False
    return True


def lexists(path):
    """Test whether a path exists. Returns True for broken symbolic links."""
    try:
        os.lstat(str(path))
    except (OSError, ValueError):
        return False
    return True


def isfile(path):
    """Test whether a path is a regular file."""
    try:
        st = os.stat(str(path))
        return (st.st_mode & 0o170000) == 0o100000
    except (OSError, ValueError):
        return False


def isdir(path):
    """Test whether a path is a directory."""
    try:
        st = os.stat(str(path))
        return (st.st_mode & 0o170000) == 0o040000
    except (OSError, ValueError):
        return False
