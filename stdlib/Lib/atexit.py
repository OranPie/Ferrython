"""Pure-Python atexit module — register cleanup functions."""

_registered = []

def register(func, *args, **kwargs):
    _registered.append((func, args, kwargs))
    return func

def unregister(func):
    global _registered
    _registered = [(f, a, k) for f, a, k in _registered if f is not func]

def _run_exitfuncs():
    exc_info = None
    while _registered:
        func, args, kwargs = _registered.pop()
        try:
            func(*args, **kwargs)
        except Exception:
            pass
    return exc_info

def _ncallbacks():
    return len(_registered)
