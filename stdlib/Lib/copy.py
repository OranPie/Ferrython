"""Generic shallow and deep copy operations."""


class Error(Exception):
    pass


error = Error


def copy(x):
    """Shallow copy operation on arbitrary Python objects."""
    cls = type(x)

    # Primitives are immutable, return as-is
    if isinstance(x, (int, float, str, bool, type(None), bytes, complex, tuple, frozenset)):
        return x

    # Check for __copy__ method
    copier = getattr(cls, '__copy__', None)
    if copier is not None:
        return copier(x)

    # List
    if isinstance(x, list):
        return list(x)

    # Dict
    if isinstance(x, dict):
        return dict(x)

    # Set
    if isinstance(x, set):
        return set(x)

    # Bytearray
    if isinstance(x, bytearray):
        return bytearray(x)

    # Generic: try to reconstruct
    reductor = getattr(x, '__reduce_ex__', None)
    if reductor is not None:
        try:
            rv = reductor(4)
            return _reconstruct(x, rv)
        except (TypeError, AttributeError):
            pass

    # Last resort: just return the object
    return x


def deepcopy(x, memo=None):
    """Deep copy operation on arbitrary Python objects."""
    if memo is None:
        memo = {}

    d = id(x)
    if d in memo:
        return memo[d]

    cls = type(x)

    # Primitives are immutable
    if isinstance(x, (int, float, str, bool, type(None), bytes, complex)):
        return x

    # Check for __deepcopy__ method
    copier = getattr(cls, '__deepcopy__', None)
    if copier is not None:
        result = copier(x, memo)
        memo[d] = result
        return result

    # Tuple (need to deepcopy elements, but only if mutable contents)
    if isinstance(x, tuple):
        result = tuple(deepcopy(item, memo) for item in x)
        memo[d] = result
        return result

    # Frozenset
    if isinstance(x, frozenset):
        result = frozenset(deepcopy(item, memo) for item in x)
        memo[d] = result
        return result

    # List
    if isinstance(x, list):
        result = []
        memo[d] = result
        for item in x:
            result.append(deepcopy(item, memo))
        return result

    # Dict
    if isinstance(x, dict):
        result = {}
        memo[d] = result
        for key, value in x.items():
            result[deepcopy(key, memo)] = deepcopy(value, memo)
        return result

    # Set
    if isinstance(x, set):
        result = set()
        memo[d] = result
        for item in x:
            result.add(deepcopy(item, memo))
        return result

    # Bytearray
    if isinstance(x, bytearray):
        result = bytearray(x)
        memo[d] = result
        return result

    # Generic object with __dict__
    try:
        result = cls.__new__(cls)
        memo[d] = result
        state = getattr(x, '__dict__', None)
        if state is not None:
            for key, value in state.items():
                setattr(result, key, deepcopy(value, memo))
        return result
    except (TypeError, AttributeError):
        pass

    # Can't copy, return as-is
    return x


def _reconstruct(x, info):
    """Helper to reconstruct from __reduce_ex__ output."""
    if isinstance(info, str):
        return x
    if not isinstance(info, tuple) or len(info) < 2:
        return x

    callable_obj = info[0]
    args = info[1] if len(info) > 1 else ()

    if callable_obj is None:
        return x

    try:
        result = callable_obj(*args)
    except (TypeError, AttributeError):
        return x

    # Apply state if present
    if len(info) > 2 and info[2] is not None:
        state = info[2]
        if isinstance(state, dict):
            for k, v in state.items():
                setattr(result, k, v)

    return result
