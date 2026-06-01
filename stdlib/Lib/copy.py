"""Generic shallow and deep copy operations."""

import copyreg
import types
import weakref


class Error(Exception):
    pass


error = Error
__all__ = ["Error", "copy", "deepcopy"]


_copy_dispatch = {}
_deepcopy_dispatch = {}
_nil = []


def _copy_immutable(x):
    return x


def _deepcopy_atomic(x, memo):
    return x


def _copy_bytearray(x):
    return bytearray(x)


def _deepcopy_list(x, memo):
    y = []
    memo[id(x)] = y
    for item in x:
        y.append(deepcopy(item, memo))
    return y


def _deepcopy_tuple(x, memo):
    copied = [deepcopy(item, memo) for item in x]
    existing = memo.get(id(x), _nil)
    if existing is not _nil:
        return existing
    for original, item in zip(x, copied):
        if original is not item:
            return tuple(copied)
    return x


def _deepcopy_dict(x, memo):
    y = {}
    memo[id(x)] = y
    for key, value in x.items():
        y[deepcopy(key, memo)] = deepcopy(value, memo)
    return y


def _deepcopy_set(x, memo):
    y = set()
    memo[id(x)] = y
    for item in x:
        y.add(deepcopy(item, memo))
    return y


def _deepcopy_frozenset(x, memo):
    copied = [deepcopy(item, memo) for item in x]
    for original, item in zip(x, copied):
        if original is not item:
            return frozenset(copied)
    return x


def _deepcopy_bytearray(x, memo):
    return bytearray(x)


def _deepcopy_range(x, memo):
    start = _deepcopy_range_bound(x.start, memo)
    stop = _deepcopy_range_bound(x.stop, memo)
    step = _deepcopy_range_bound(x.step, memo)
    return range(start, stop, step)


def _deepcopy_range_bound(value, memo):
    copied = deepcopy(value, memo)
    value_type = type(value)
    if copied is value and value_type is not int:
        try:
            return value_type(value)
        except Exception:
            return copied
    return copied


def _deepcopy_method(x, memo):
    self = getattr(x, "__self__", None)
    func = getattr(x, "__func__", None)
    if self is not None and func is not None:
        name = getattr(func, "__name__", None)
        copied_self = deepcopy(self, memo)
        if name is not None:
            return getattr(copied_self, name)
        return x
    return x


def _register_dispatch():
    atomic_types = [
        type(None),
        type(Ellipsis),
        type(NotImplemented),
        int,
        float,
        bool,
        complex,
        str,
        bytes,
        type,
        slice,
        property,
    ]
    for name in ("CodeType", "BuiltinFunctionType", "FunctionType"):
        t = getattr(types, name, None)
        if t is not None:
            atomic_types.append(t)

    try:
        atomic_types.append(weakref.ref(object()))
    except Exception:
        pass

    for t in atomic_types:
        _copy_dispatch[t] = _copy_immutable
        _deepcopy_dispatch[t] = _deepcopy_atomic

    _copy_dispatch[tuple] = _copy_immutable
    _copy_dispatch[frozenset] = _copy_immutable
    _copy_dispatch[list] = list.copy
    _copy_dispatch[dict] = dict.copy
    _copy_dispatch[set] = set.copy
    _copy_dispatch[bytearray] = _copy_bytearray

    _deepcopy_dispatch[list] = _deepcopy_list
    _deepcopy_dispatch[tuple] = _deepcopy_tuple
    _deepcopy_dispatch[dict] = _deepcopy_dict
    _deepcopy_dispatch[set] = _deepcopy_set
    _deepcopy_dispatch[frozenset] = _deepcopy_frozenset
    _deepcopy_dispatch[bytearray] = _deepcopy_bytearray
    range_type = type(range(0))
    _copy_dispatch[range_type] = _copy_immutable
    _deepcopy_dispatch[range_type] = _deepcopy_range

    method_type = getattr(types, "MethodType", None)
    if method_type is not None:
        _deepcopy_dispatch[method_type] = _deepcopy_method


_register_dispatch()


def _class_dict_get(cls, name, default=None):
    for base in getattr(cls, "__mro__", (cls,)):
        value = getattr(base, "__dict__", {}).get(name, _nil)
        if value is not _nil:
            return value
    return default


def _call_special(method, obj, *args):
    try:
        return method(obj, *args)
    except TypeError:
        bound = getattr(obj, getattr(method, "__name__", ""), None)
        if bound is None:
            raise
        return bound(*args)


def _safe_getattr(obj, name, default=None):
    try:
        return getattr(obj, name)
    except AttributeError:
        return default


def copy(x):
    """Shallow copy operation on arbitrary Python objects."""
    cls = type(x)

    if _is_deque(x):
        return x.copy()
    if _is_weakref_ref(x):
        return x
    if _is_weak_key_dict(x):
        return _copy_weak_key_dict(x)
    if _is_weak_value_dict(x):
        return _copy_weak_value_dict(x)

    copier = _copy_dispatch.get(cls)
    if copier is not None:
        return copier(x)

    if issubclass(cls, type):
        return x

    copier = _class_dict_get(cls, "__copy__")
    if copier is not None:
        return _call_special(copier, x)

    reductor = copyreg.dispatch_table.get(cls)
    if reductor is not None:
        rv = reductor(x)
    else:
        reductor = _safe_getattr(x, "__reduce_ex__")
        if reductor is not None:
            rv = reductor(4)
        else:
            reductor = _safe_getattr(x, "__reduce__")
            if reductor is not None:
                rv = reductor()
            else:
                if _can_copy_instance(x):
                    if _has_custom_getattribute(cls):
                        raise Error("un(shallow)copyable object of type %s" % cls)
                    return _copy_instance(x)
                raise Error("un(shallow)copyable object of type %s" % cls)

    if isinstance(rv, str):
        return x
    return _reconstruct(x, None, *rv)


def deepcopy(x, memo=None):
    """Deep copy operation on arbitrary Python objects."""
    if memo is None:
        memo = {}

    d = id(x)
    y = memo.get(d, _nil)
    if y is not _nil:
        return y

    cls = type(x)
    if _is_deque(x):
        y = x.copy()
        memo[d] = y
        y.clear()
        y.extend(deepcopy(list(x), memo))
    elif _is_weakref_ref(x):
        y = x
    elif _is_weak_key_dict(x):
        y = _deepcopy_weak_key_dict(x, memo)
    elif _is_weak_value_dict(x):
        y = _deepcopy_weak_value_dict(x, memo)
    else:
        copier = _deepcopy_dispatch.get(cls)
        if copier is not None:
            y = copier(x, memo)
        elif issubclass(cls, type):
            y = x
        else:
            y = _deepcopy_object(x, memo, cls)

    if y is not x:
        memo[d] = y
        _keep_alive(x, memo)
    return y


def _deepcopy_object(x, memo, cls):
    copier = _class_dict_get(cls, "__deepcopy__")
    if copier is not None:
        return _call_special(copier, x, memo)

    reductor = copyreg.dispatch_table.get(cls)
    if reductor is not None:
        rv = reductor(x)
    else:
        reductor = _safe_getattr(x, "__reduce_ex__")
        if reductor is not None:
            rv = reductor(4)
        else:
            reductor = _safe_getattr(x, "__reduce__")
            if reductor is not None:
                rv = reductor()
            else:
                if _can_copy_instance(x):
                    if _has_custom_getattribute(cls):
                        raise Error("un(deep)copyable object of type %s" % cls)
                    return _copy_instance(x, memo)
                raise Error("un(deep)copyable object of type %s" % cls)

    if isinstance(rv, str):
        return x
    return _reconstruct(x, memo, *rv)


def _is_weakref_ref(x):
    return getattr(x, "__weakref_ref__", False)


def _is_deque(x):
    return getattr(x, "__deque__", False)


def _is_weak_key_dict(x):
    return getattr(x, "__weakkeydict__", False)


def _is_weak_value_dict(x):
    return getattr(x, "__weakvaluedict__", False)


def _copy_weak_key_dict(x):
    y = weakref.WeakKeyDictionary()
    for key, value in x.items():
        y[key] = value
    return y


def _copy_weak_value_dict(x):
    y = weakref.WeakValueDictionary()
    for key, value in x.items():
        y[key] = value
    return y


def _deepcopy_weak_key_dict(x, memo):
    y = weakref.WeakKeyDictionary()
    memo[id(x)] = y
    for key, value in x.items():
        y[key] = deepcopy(value, memo)
    return y


def _deepcopy_weak_value_dict(x, memo):
    y = weakref.WeakValueDictionary()
    memo[id(x)] = y
    for key, value in x.items():
        y[deepcopy(key, memo)] = value
    return y


def _can_copy_instance(x):
    return _safe_getattr(x, "__dict__") is not None or _slotnames(type(x))


def _has_custom_getattribute(cls):
    return "__getattribute__" in getattr(cls, "__dict__", {})


def _copy_instance(x, memo=None):
    cls = type(x)
    deep = memo is not None

    if isinstance(x, list):
        y = cls()
        if deep:
            memo[id(x)] = y
        for item in x:
            y.append(deepcopy(item, memo) if deep else item)
    elif isinstance(x, dict):
        y = cls()
        if deep:
            memo[id(x)] = y
        for key, value in x.items():
            if deep:
                key = deepcopy(key, memo)
                value = deepcopy(value, memo)
            y[key] = value
    elif isinstance(x, tuple):
        items = tuple(deepcopy(item, memo) if deep else item for item in tuple(x))
        y = cls(items)
        if deep:
            memo[id(x)] = y
    else:
        new = getattr(cls, "__new__", None)
        if new is None:
            raise Error("un(shallow)copyable object of type %s" % cls)
        getnewargs_ex = _safe_getattr(x, "__getnewargs_ex__")
        getnewargs = _safe_getattr(x, "__getnewargs__")
        if getnewargs_ex is not None:
            args, kwargs = getnewargs_ex()
            if deep:
                args = deepcopy(args, memo)
                kwargs = deepcopy(kwargs, memo)
            y = new(cls, *args, **kwargs)
        elif getnewargs is not None:
            args = getnewargs()
            if deep:
                args = deepcopy(args, memo)
            y = new(cls, *args)
        elif "__builtin_value__" in getattr(x, "__dict__", {}):
            value = x.__dict__["__builtin_value__"]
            if deep:
                value = deepcopy(value, memo)
            y = cls(value)
        else:
            y = new(cls)
        if deep:
            memo[id(x)] = y

    state = _instance_state(x)
    if deep:
        state = deepcopy(state, memo)
    _apply_state(y, state)
    return y


def _instance_state(x):
    getstate = _class_dict_get(type(x), "__getstate__")
    if getstate is not None:
        return _call_special(getstate, x)

    state = None
    d = _safe_getattr(x, "__dict__")
    if d is not None:
        state = {}
        for key, value in d.items():
            if key != "__builtin_value__":
                state[key] = value

    slotstate = {}
    for name in _slotnames(type(x)):
        sentinel = object()
        value = _safe_getattr(x, name, sentinel)
        if value is not sentinel:
            slotstate[name] = value

    if slotstate:
        return (state, slotstate)
    return state


def _slotnames(cls):
    names = []
    for c in getattr(cls, "__mro__", (cls,)):
        slots = getattr(c, "__dict__", {}).get("__slots__")
        if slots is None:
            continue
        if isinstance(slots, str):
            slots = (slots,)
        for name in slots:
            if name in ("__dict__", "__weakref__"):
                continue
            if name.startswith("__") and not name.endswith("__"):
                stripped = c.__name__.lstrip("_")
                if stripped:
                    name = "_%s%s" % (stripped, name)
            names.append(name)
    return names


def _keep_alive(x, memo):
    try:
        memo[id(memo)].append(x)
    except KeyError:
        memo[id(memo)] = [x]


def _reconstruct(x, memo, func, args, state=None, listiter=None, dictiter=None):
    deep = memo is not None
    if deep:
        args = tuple(deepcopy(arg, memo) for arg in args)
    y = func(*args)
    if deep:
        memo[id(x)] = y

    if state is not None:
        if deep:
            state = deepcopy(state, memo)
        _apply_state(y, state)

    if listiter is not None:
        for item in listiter:
            y.append(deepcopy(item, memo) if deep else item)

    if dictiter is not None:
        for key, value in dictiter:
            if deep:
                key = deepcopy(key, memo)
                value = deepcopy(value, memo)
            y[key] = value

    return y


def _apply_state(y, state):
    setstate = _class_dict_get(type(y), "__setstate__")
    if setstate is not None:
        _call_special(setstate, y, state)
        return

    if isinstance(state, tuple) and len(state) == 2:
        state, slotstate = state
    else:
        slotstate = None

    if state is not None:
        d = _safe_getattr(y, "__dict__")
        if d is not None:
            for key, value in state.items():
                if key != "__builtin_value__":
                    d[key] = value
        else:
            for key, value in state.items():
                if key != "__builtin_value__":
                    setattr(y, key, value)

    if slotstate is not None:
        for key, value in slotstate.items():
            setattr(y, key, value)
