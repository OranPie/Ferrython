"""Pure Python implementation of the tempfile module.

Provides temporary files and directories with automatic cleanup.
"""

import os
import io
import random
import string


TMP_MAX = 10000

_name_sequence = None

tempdir = None


def _get_default_tempdir():
    """Get the default temporary directory."""
    candidates = []
    for envname in 'TMPDIR', 'TEMP', 'TMP':
        dirname = os.environ.get(envname)
        if dirname:
            candidates.append(dirname)
    candidates.extend(['/tmp', '/var/tmp', '/usr/tmp'])
    
    for d in candidates:
        if os.path.isdir(d) and os.access(d, os.W_OK):
            return d
    return '.'


def gettempdir():
    """Return the default temporary directory."""
    global tempdir
    if tempdir is None:
        tempdir = _get_default_tempdir()
    return tempdir


def _random_name(length=8):
    chars = string.ascii_lowercase + string.digits
    return ''.join(random.choice(chars) for _ in range(length))


def gettempprefix():
    """Return the default prefix for temporary files."""
    return 'tmp'


def mkstemp(suffix=None, prefix=None, dir=None, text=False):
    """Create a temporary file in the most secure manner possible.
    Returns (fd, name)."""
    if suffix is None:
        suffix = ''
    if prefix is None:
        prefix = gettempprefix()
    if dir is None:
        dir = gettempdir()
    
    for seq in range(TMP_MAX):
        name = os.path.join(dir, prefix + _random_name() + suffix)
        try:
            flags = os.O_RDWR | os.O_CREAT | os.O_EXCL
            if not text:
                if hasattr(os, 'O_BINARY'):
                    flags |= os.O_BINARY
            fd = os.open(name, flags, 0o600)
            return (fd, name)
        except FileExistsError:
            continue
    raise FileExistsError("No usable temporary file name found")


def mkdtemp(suffix=None, prefix=None, dir=None):
    """Create a temporary directory.
    Returns the directory path."""
    if suffix is None:
        suffix = ''
    if prefix is None:
        prefix = gettempprefix()
    if dir is None:
        dir = gettempdir()
    
    for seq in range(TMP_MAX):
        name = os.path.join(dir, prefix + _random_name() + suffix)
        try:
            os.mkdir(name, 0o700)
            return name
        except FileExistsError:
            continue
    raise FileExistsError("No usable temporary directory name found")


def mktemp(suffix='', prefix='tmp', dir=None):
    """Return a temporary file name (deprecated, use mkstemp instead)."""
    if dir is None:
        dir = gettempdir()
    return os.path.join(dir, prefix + _random_name() + suffix)


class _TemporaryFileCloser:
    def __init__(self, file, name, delete=True):
        self.file = file
        self.name = name
        self.delete = delete
        self.close_called = False
    
    def close(self):
        if not self.close_called:
            self.close_called = True
            try:
                self.file.close()
            finally:
                if self.delete:
                    try:
                        os.unlink(self.name)
                    except OSError:
                        pass


class NamedTemporaryFile:
    """Create a temporary file that is automatically deleted when closed."""
    
    def __init__(self, mode='w+b', buffering=-1, encoding=None,
                 newline=None, suffix=None, prefix=None,
                 dir=None, delete=True, delete_on_close=True):
        fd, name = mkstemp(suffix=suffix, prefix=prefix, dir=dir,
                           text='b' not in mode)
        try:
            self.file = os.fdopen(fd, mode) if hasattr(os, 'fdopen') else open(name, mode)
        except Exception:
            os.close(fd)
            os.unlink(name)
            raise
        self.name = name
        self.delete = delete
        self.delete_on_close = delete_on_close
        self._closer = _TemporaryFileCloser(self.file, name, delete and delete_on_close)
    
    def __enter__(self):
        return self
    
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()
        return False
    
    def close(self):
        self._closer.close()
    
    def write(self, data):
        return self.file.write(data)
    
    def read(self, size=-1):
        return self.file.read(size)
    
    def seek(self, offset, whence=0):
        return self.file.seek(offset, whence)
    
    def tell(self):
        return self.file.tell()
    
    def flush(self):
        return self.file.flush()
    
    def __iter__(self):
        return iter(self.file)
    
    @property
    def closed(self):
        return self.file.closed


class SpooledTemporaryFile:
    """Temporary file wrapper that starts in memory then rolls to disk."""
    
    _rolled = False
    
    def __init__(self, max_size=0, mode='w+b', buffering=-1,
                 encoding=None, newline=None, suffix=None, prefix=None,
                 dir=None):
        if 'b' in mode:
            self._file = io.BytesIO()
        else:
            self._file = io.StringIO()
        self._max_size = max_size
        self._mode = mode
        self._suffix = suffix
        self._prefix = prefix
        self._dir = dir
    
    def _check(self):
        if not self._rolled and self._max_size and self.tell() > self._max_size:
            self.rollover()
    
    def rollover(self):
        if self._rolled:
            return
        file = self._file
        newfile = NamedTemporaryFile(mode=self._mode, suffix=self._suffix,
                                     prefix=self._prefix, dir=self._dir)
        pos = file.tell()
        file.seek(0)
        newfile.write(file.read())
        newfile.seek(pos)
        self._file = newfile
        self._rolled = True
    
    @property
    def name(self):
        if self._rolled:
            return self._file.name
        return None
    
    def write(self, data):
        result = self._file.write(data)
        self._check()
        return result
    
    def read(self, size=-1):
        return self._file.read(size)
    
    def seek(self, offset, whence=0):
        return self._file.seek(offset, whence)
    
    def tell(self):
        return self._file.tell()
    
    def close(self):
        self._file.close()
    
    def __enter__(self):
        return self
    
    def __exit__(self, *args):
        self.close()


class TemporaryDirectory:
    """Create a temporary directory that is cleaned up on close."""
    
    def __init__(self, suffix=None, prefix=None, dir=None,
                 ignore_cleanup_errors=False):
        self.name = mkdtemp(suffix=suffix, prefix=prefix, dir=dir)
        self._ignore_cleanup_errors = ignore_cleanup_errors
    
    def __repr__(self):
        return "<{} {!r}>".format(self.__class__.__name__, self.name)
    
    def __enter__(self):
        return self.name
    
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.cleanup()
        return False
    
    def cleanup(self):
        _rmtree(self.name, self._ignore_cleanup_errors)


def _rmtree(path, ignore_errors=False):
    """Remove directory tree."""
    try:
        entries = os.listdir(path)
    except OSError:
        if not ignore_errors:
            raise
        return
    for entry in entries:
        fullpath = os.path.join(path, entry)
        try:
            if os.path.isdir(fullpath):
                _rmtree(fullpath, ignore_errors)
            else:
                os.unlink(fullpath)
        except OSError:
            if not ignore_errors:
                raise
    try:
        os.rmdir(path)
    except OSError:
        if not ignore_errors:
            raise
