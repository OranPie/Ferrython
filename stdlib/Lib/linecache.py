"""Cache lines from Python source files.

This is intended to be used by tracebacks, debuggers, and similar tools to
read lines from source files.
"""

import os
import sys
import tokenize

__all__ = ['getline', 'clearcache', 'checkcache', 'lazycache']

# {filename: (size, mtime, lines, fullname)}
_cache = {}


def clearcache():
    """Clear the cache entirely."""
    _cache.clear()


def getline(filename, lineno, module_globals=None):
    """Get a line from the cache or file.

    If lineno is out of range, return empty string.
    """
    lines = getlines(filename, module_globals)
    if 1 <= lineno <= len(lines):
        return lines[lineno - 1]
    return ''


def getlines(filename, module_globals=None):
    """Get the lines for a named module, reading from file if needed.

    Returns the lines as a list of strings with trailing newlines.
    If the file cannot be read, returns an empty list.
    """
    if filename in _cache:
        entry = _cache[filename]
        if len(entry) >= 4:
            return entry[2]
        return entry

    return updatecache(filename, module_globals)


def checkcache(filename=None):
    """Discard cache entries that are out of date.

    If filename is None, check all entries.
    """
    if filename is not None:
        if filename in _cache:
            entry = _cache[filename]
            if len(entry) >= 4 and entry[3]:
                fullname = entry[3]
            else:
                fullname = filename
            try:
                stat = os.stat(fullname)
            except OSError:
                del _cache[filename]
                return
            if len(entry) >= 4:
                size, mtime = entry[0], entry[1]
                if size != stat.st_size or mtime != stat.st_mtime:
                    del _cache[filename]
        return

    filenames = list(_cache.keys())
    for filename in filenames:
        checkcache(filename)


def updatecache(filename, module_globals=None):
    """Update a cache entry and return its list of lines.

    If something's wrong, print a message, discard the cache entry,
    and return an empty list.
    """
    if filename in _cache:
        if len(_cache[filename]) != 1:
            del _cache[filename]

    fullname = filename
    try:
        stat = os.stat(fullname)
    except OSError:
        # Try module-related paths
        if module_globals and '__loader__' in module_globals:
            name = module_globals.get('__name__')
            loader = module_globals.get('__loader__')
            if name and loader:
                try:
                    source = loader.get_source(name)
                except (ImportError, AttributeError):
                    source = None
                if source is not None:
                    lines = source.splitlines(True)
                    _cache[filename] = (len(source), None, lines, fullname)
                    return lines

        # Try along sys.path
        basename = os.path.basename(filename)
        for dirname in sys.path:
            fullname = os.path.join(dirname, basename)
            try:
                stat = os.stat(fullname)
                break
            except OSError:
                pass
        else:
            return []

    try:
        with open(fullname, 'rb') as fp:
            raw = fp.read()
    except OSError:
        return []

    # Detect encoding, default to utf-8
    encoding = 'utf-8'
    try:
        encoding = tokenize.detect_encoding(raw.__class__(raw[:512].split(b'\n', 2)[:2]))[0]
    except Exception:
        pass

    try:
        lines = raw.decode(encoding).splitlines(True)
    except (UnicodeDecodeError, LookupError):
        try:
            lines = raw.decode('utf-8', errors='replace').splitlines(True)
        except Exception:
            return []

    size = stat.st_size
    mtime = stat.st_mtime
    _cache[filename] = (size, mtime, lines, fullname)
    return lines


def lazycache(filename, module_globals):
    """Seed the cache for filename with module_globals.

    The module loader will be asked for the source only when getlines is
    called, not immediately. If there is an entry in the cache already, it is
    not altered.

    Returns True if a lazy cache entry can be created, False otherwise.
    """
    if filename in _cache:
        if len(_cache[filename]) == 1:
            return True
        return False

    if not module_globals:
        return False

    # Only cache if we have a loader with get_source
    loader = module_globals.get('__loader__')
    name = module_globals.get('__name__')
    if loader is None or name is None:
        return False
    if not hasattr(loader, 'get_source'):
        return False

    # Store a lazy entry: (module_globals,)
    _cache[filename] = (module_globals,)
    return True
