"""Generic path operations used by posixpath and ntpath."""

import os
import stat as _stat

__all__ = [
    'commonprefix', 'exists', 'getatime', 'getctime', 'getmtime',
    'getsize', 'isdir', 'isfile', 'samefile', 'sameopenfile', 'samestat',
]


def exists(path):
    """Test whether a path exists. Returns False for broken symbolic links."""
    try:
        os.stat(str(path))
    except (OSError, ValueError):
        return False
    return True


def isfile(path):
    """Test whether a path is a regular file."""
    try:
        st = os.stat(str(path))
        return _stat.S_ISREG(st.st_mode)
    except (OSError, ValueError):
        return False


def isdir(path):
    """Test whether a path is a directory."""
    try:
        st = os.stat(str(path))
        return _stat.S_ISDIR(st.st_mode)
    except (OSError, ValueError):
        return False


def getsize(path):
    """Return the size of a file, reported by os.stat()."""
    return os.stat(str(path)).st_size


def getmtime(path):
    """Return the last modification time of a file, reported by os.stat()."""
    return os.stat(str(path)).st_mtime


def getatime(path):
    """Return the last access time of a file, reported by os.stat()."""
    return os.stat(str(path)).st_atime


def getctime(path):
    """Return the metadata change time of a file, reported by os.stat()."""
    return os.stat(str(path)).st_ctime


def commonprefix(m):
    """Given a list of pathnames, returns the longest common leading component."""
    if not m:
        return ''
    # Handle both strings and bytes
    if not m:
        return ''
    s1 = min(m)
    s2 = max(m)
    for i, c in enumerate(s1):
        if c != s2[i]:
            return s1[:i]
    return s1


def samefile(f1, f2):
    """Test whether two pathnames reference the same actual file or directory."""
    s1 = os.stat(str(f1))
    s2 = os.stat(str(f2))
    return samestat(s1, s2)


def sameopenfile(fp1, fp2):
    """Test whether two open file objects reference the same file."""
    s1 = os.fstat(fp1)
    s2 = os.fstat(fp2)
    return samestat(s1, s2)


def samestat(s1, s2):
    """Test whether two stat results reference the same file."""
    return (s1.st_ino == s2.st_ino and
            s1.st_dev == s2.st_dev)
