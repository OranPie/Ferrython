"""Pure Python implementation of the shutil module.

High-level file operations: copy, move, remove, archive.
"""

import os
import stat


def _fnmatch(name, pattern):
    """Simple fnmatch replacement that handles *, ?, and literal matches."""
    import re
    # Convert glob pattern to regex
    i, n = 0, len(pattern)
    res = ''
    while i < n:
        c = pattern[i]
        i += 1
        if c == '*':
            res += '.*'
        elif c == '?':
            res += '.'
        elif c == '[':
            j = i
            if j < n and pattern[j] == '!':
                j += 1
            if j < n and pattern[j] == ']':
                j += 1
            while j < n and pattern[j] != ']':
                j += 1
            if j >= n:
                res += '\\['
            else:
                stuff = pattern[i:j].replace('\\', '\\\\')
                i = j + 1
                if stuff[0] == '!':
                    stuff = '^' + stuff[1:]
                res += '[' + stuff + ']'
        else:
            res += re.escape(c)
    return bool(re.fullmatch(res, name, re.IGNORECASE if os.name == 'nt' else 0))


def copyfileobj(fsrc, fdst, length=16*1024):
    """Copy data from file-like object fsrc to file-like object fdst."""
    while True:
        buf = fsrc.read(length)
        if not buf:
            break
        fdst.write(buf)


def copyfile(src, dst, follow_symlinks=True):
    """Copy data from src to dst. dst must be a complete target filename."""
    if os.path.isdir(dst):
        raise IsADirectoryError("Is a directory: '{}'".format(dst))
    with open(src, 'rb') as fsrc:
        with open(dst, 'wb') as fdst:
            copyfileobj(fsrc, fdst)
    return dst


def copymode(src, dst, follow_symlinks=True):
    """Copy mode bits from src to dst."""
    st = os.stat(src)
    os.chmod(dst, stat.S_IMODE(st.st_mode))


def copystat(src, dst, follow_symlinks=True):
    """Copy file metadata (mode bits, atime, mtime, flags) from src to dst."""
    st = os.stat(src)
    mode = stat.S_IMODE(st.st_mode)
    os.chmod(dst, mode)


def copy(src, dst, follow_symlinks=True):
    """Copy file and permissions."""
    if os.path.isdir(dst):
        dst = os.path.join(dst, os.path.basename(src))
    copyfile(src, dst, follow_symlinks=follow_symlinks)
    copymode(src, dst, follow_symlinks=follow_symlinks)
    return dst


def copy2(src, dst, follow_symlinks=True):
    """Copy file and metadata."""
    if os.path.isdir(dst):
        dst = os.path.join(dst, os.path.basename(src))
    copyfile(src, dst, follow_symlinks=follow_symlinks)
    copystat(src, dst, follow_symlinks=follow_symlinks)
    return dst


def copytree(src, dst, symlinks=False, ignore=None, copy_function=None,
             ignore_dangling_symlinks=False, dirs_exist_ok=False):
    """Recursively copy a directory tree."""
    if copy_function is None:
        copy_function = copy2
    names = os.listdir(src)
    if ignore is not None:
        ignored_names = ignore(src, names)
    else:
        ignored_names = set()
    
    if not os.path.exists(dst):
        os.makedirs(dst)
    elif not dirs_exist_ok:
        raise FileExistsError("Directory already exists: '{}'".format(dst))
    
    errors = []
    for name in names:
        if name in ignored_names:
            continue
        srcname = os.path.join(src, name)
        dstname = os.path.join(dst, name)
        try:
            if os.path.isdir(srcname):
                copytree(srcname, dstname, symlinks, ignore, copy_function,
                         ignore_dangling_symlinks, dirs_exist_ok)
            else:
                copy_function(srcname, dstname)
        except Exception as why:
            errors.append((srcname, dstname, str(why)))
    if errors:
        raise Exception("copytree errors: {}".format(errors))
    return dst


def rmtree(path, ignore_errors=False, onerror=None):
    """Recursively delete a directory tree."""
    if ignore_errors:
        def onerror_fn(*args):
            pass
    elif onerror is None:
        def onerror_fn(*args):
            raise
    else:
        onerror_fn = onerror
    
    try:
        entries = os.listdir(path)
    except Exception as err:
        onerror_fn(os.listdir, path, err)
        return
    
    for entry in entries:
        fullpath = os.path.join(path, entry)
        try:
            if os.path.isdir(fullpath):
                rmtree(fullpath, ignore_errors, onerror)
            else:
                os.remove(fullpath)
        except Exception as err:
            onerror_fn(os.remove, fullpath, err)
    
    try:
        os.rmdir(path)
    except Exception as err:
        onerror_fn(os.rmdir, path, err)


def move(src, dst, copy_function=None):
    """Recursively move a file or directory to another location."""
    if copy_function is None:
        copy_function = copy2
    
    real_dst = dst
    if os.path.isdir(dst):
        real_dst = os.path.join(dst, os.path.basename(src))
        if os.path.exists(real_dst):
            raise Exception("Destination path '{}' already exists".format(real_dst))
    
    try:
        os.rename(src, real_dst)
    except OSError:
        if os.path.isdir(src):
            copytree(src, real_dst, copy_function=copy_function)
            rmtree(src)
        else:
            copy_function(src, real_dst)
            os.remove(src)
    return real_dst


def disk_usage(path):
    """Return disk usage statistics about the given path."""
    st = os.statvfs(path) if hasattr(os, 'statvfs') else None
    if st is not None:
        free = st.f_bavail * st.f_frsize
        total = st.f_blocks * st.f_frsize
        used = (st.f_blocks - st.f_bfree) * st.f_frsize
        return _ntuple_diskusage(total, used, free)
    return _ntuple_diskusage(0, 0, 0)


class _ntuple_diskusage:
    __slots__ = ('total', 'used', 'free')
    def __init__(self, total, used, free):
        self.total = total
        self.used = used
        self.free = free
    def __repr__(self):
        return 'usage(total={}, used={}, free={})'.format(self.total, self.used, self.free)


def which(name, mode=os.F_OK | os.X_OK, path=None):
    """Given a command, mode, and a PATH string, return the path which
    conforms to the given mode on the PATH, or None if there is no such file."""
    if os.path.dirname(name):
        if os.path.isfile(name) and os.access(name, mode):
            return name
        return None
    
    if path is None:
        path = os.environ.get("PATH", os.defpath)
    if not path:
        return None
    path_list = path.split(os.pathsep)
    
    for dir_path in path_list:
        name_path = os.path.join(dir_path, name)
        if os.path.isfile(name_path) and os.access(name_path, mode):
            return name_path
    return None


def make_archive(base_name, format, root_dir=None, base_dir=None):
    """Create an archive file."""
    raise NotImplementedError("make_archive not yet implemented")


def unpack_archive(filename, extract_dir=None, format=None):
    """Unpack an archive."""
    raise NotImplementedError("unpack_archive not yet implemented")


def get_terminal_size(fallback=(80, 24)):
    """Get terminal size."""
    try:
        columns = int(os.environ.get('COLUMNS', 0))
        lines = int(os.environ.get('LINES', 0))
        if columns > 0 and lines > 0:
            return os.terminal_size((columns, lines))
    except (ValueError, TypeError):
        pass
    return os.terminal_size(fallback) if hasattr(os, 'terminal_size') else fallback


def ignore_patterns(*patterns):
    """Function that can be used as copytree() ignore parameter."""
    pattern_list = list(patterns)
    def _ignore_patterns(path, names):
        ignored_names = set()
        for pattern in pattern_list:
            for name in names:
                if _fnmatch(name, pattern):
                    ignored_names.add(name)
        return ignored_names
    return _ignore_patterns


# Error class
class Error(OSError):
    pass

class SameFileError(Error):
    pass
