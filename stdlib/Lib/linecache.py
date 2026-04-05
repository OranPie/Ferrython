"""Pure-Python linecache module — caches lines read from files."""

_cache = {}

def clearcache():
    _cache.clear()

def getlines(filename, module_globals=None):
    if filename in _cache:
        return _cache[filename]
    try:
        with open(filename, 'r') as f:
            lines = f.readlines()
    except (OSError, IOError):
        lines = []
    _cache[filename] = lines
    return lines

def getline(filename, lineno, module_globals=None):
    lines = getlines(filename, module_globals)
    if 1 <= lineno <= len(lines):
        return lines[lineno - 1]
    return ''

def checkcache(filename=None):
    if filename is not None:
        if filename in _cache:
            del _cache[filename]
    else:
        _cache.clear()
