"""Utilities for with-statement contexts."""


class ContextDecorator:
    """A base class that enables context managers to work as decorators."""

    def _recreate_cm(self):
        return self

    def __call__(self, func):
        from functools import wraps
        @wraps(func)
        def inner(*args, _func=func, _self=self, **kwds):
            with _self._recreate_cm():
                return _func(*args, **kwds)
        return inner


class _GeneratorContextManager(ContextDecorator):
    """Helper for @contextmanager decorator."""

    def __init__(self, func, args, kwds):
        self.gen = func(*args, **kwds)
        self.func = func
        self.args = args
        self.kwds = kwds
        self.__doc__ = getattr(func, "__doc__", None)

    def _recreate_cm(self):
        return self.__class__(self.func, self.args, self.kwds)

    def __enter__(self):
        del self.args, self.kwds
        try:
            return next(self.gen)
        except StopIteration:
            raise RuntimeError("generator didn't yield")

    def __exit__(self, typ, value, traceback):
        if typ is None:
            try:
                next(self.gen)
            except StopIteration:
                return False
            else:
                raise RuntimeError("generator didn't stop")
        else:
            if value is None:
                value = typ()
            try:
                self.gen.throw(typ, value, traceback)
            except StopIteration as exc:
                return exc is not value
            except RuntimeError as exc:
                if exc is value:
                    exc.__traceback__ = traceback
                    return False
                if isinstance(value, StopIteration) and exc.__cause__ is value:
                    value.__traceback__ = traceback
                    return False
                raise
            except BaseException as exc:
                if exc is not value:
                    raise
                exc.__traceback__ = traceback
                return False
            else:
                raise RuntimeError("generator didn't stop after throw()")


def contextmanager(func):
    """@contextmanager decorator.

    Typical usage:

        @contextmanager
        def some_generator(<arguments>):
            <setup>
            try:
                yield <value>
            finally:
                <cleanup>
    """
    from functools import wraps
    @wraps(func)
    def helper(*args, _func=func, **kwds):
        return _GeneratorContextManager(_func, args, kwds)
    return helper


class closing:
    """Context manager for safely finalizing an object with close()."""

    def __init__(self, thing):
        self.thing = thing

    def __enter__(self):
        return self.thing

    def __exit__(self, *exc_info):
        self.thing.close()


class suppress:
    """Context manager to suppress specified exceptions."""

    def __init__(self, *exceptions):
        self._exceptions = exceptions

    def __enter__(self):
        pass

    def __exit__(self, exctype, excinst, exctb):
        return exctype is not None and issubclass(exctype, self._exceptions)


class redirect_stdout:
    """Context manager for temporarily redirecting stdout."""

    _stream = 'stdout'

    def __init__(self, new_target):
        self._new_target = new_target
        self._old_targets = []

    def __enter__(self):
        import sys
        self._old_targets.append(getattr(sys, self._stream))
        setattr(sys, self._stream, self._new_target)
        return self._new_target

    def __exit__(self, exctype, excinst, exctb):
        import sys
        setattr(sys, self._stream, self._old_targets.pop())


class redirect_stderr(redirect_stdout):
    """Context manager for temporarily redirecting stderr."""
    _stream = 'stderr'


class ExitStack:
    """Context manager for dynamic management of a stack of exit callbacks."""

    def __init__(self):
        self._exit_callbacks = []

    def __enter__(self):
        return self

    def __exit__(_self, *exc_details):
        received_exc = exc_details[0] is not None
        suppressed_exc = False
        pending_raise = False

        while _self._exit_callbacks:
            _, cb = _self._exit_callbacks.pop()
            try:
                if cb(*exc_details):
                    suppressed_exc = True
                    pending_raise = False
                    exc_details = (None, None, None)
            except Exception:
                import sys
                exc_details = sys.exc_info()
                pending_raise = True

        if pending_raise:
            raise exc_details[1]
        return received_exc and suppressed_exc

    def enter_context(_self, cm):
        _exit = type(cm).__exit__
        result = type(cm).__enter__(cm)
        _exit_wrapper = _exit.__get__(cm, type(cm))
        _self._exit_callbacks.append((True, _exit_wrapper))
        return result

    def push(_self, exit):
        _exit = type(exit).__dict__.get("__exit__")
        if _exit is None or not callable(_exit):
            _self._exit_callbacks.append((True, exit))
        else:
            _exit_wrapper = _exit.__get__(exit, type(exit))
            _self._exit_callbacks.append((True, _exit_wrapper))
        return exit

    def callback(_self, _callback=None, *args, **kwds):
        if _callback is None:
            if "callback" in kwds:
                import warnings
                warnings.warn("callback as a keyword argument is deprecated",
                              DeprecationWarning, stacklevel=2)
                _callback = kwds.pop("callback")
            else:
                raise TypeError("callback() missing 1 required positional argument: 'callback'")
        def _exit_wrapper(exc_type, exc, tb, _callback=_callback, _args=args, _kwds=kwds):
            _callback(*_args, **_kwds)
            return False
        _exit_wrapper.__wrapped__ = _callback
        _self._exit_callbacks.append((True, _exit_wrapper))
        return _callback

    def close(_self):
        """Immediately unwind the callback stack."""
        _self.__exit__(None, None, None)

    def pop_all(_self):
        new_stack = ExitStack()
        new_stack._exit_callbacks = _self._exit_callbacks
        _self._exit_callbacks = []
        return new_stack


class nullcontext:
    """Context manager that does no additional processing.

    Used as a stand-in for an optional context manager.
    """

    def __init__(self, enter_result=None):
        self.enter_result = enter_result

    def __enter__(self):
        return self.enter_result

    def __exit__(self, *excinfo):
        pass


class AbstractContextManager:
    """An abstract base class for context managers."""

    def __enter__(self):
        return self

    @classmethod
    def __subclasshook__(cls, C):
        if cls is AbstractContextManager:
            return _check_methods(C, "__enter__", "__exit__")
        return NotImplemented

    from abc import abstractmethod
    @abstractmethod
    def __exit__(self, exc_type, exc_value, traceback):
        return None


def _check_methods(C, *methods):
    for method in methods:
        for B in C.__mro__:
            if method in B.__dict__:
                if B.__dict__[method] is None:
                    return NotImplemented
                break
        else:
            return NotImplemented
    return True


class AbstractAsyncContextManager:
    """An abstract base class for async context managers."""

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc_value, traceback):
        return None


class _AsyncGeneratorContextManager:
    """Helper for @asynccontextmanager decorator."""

    def __init__(self, func, args, kwds):
        self.gen = func(*args, **kwds)
        self.func = func
        self.args = args
        self.kwds = kwds

    async def __aenter__(self):
        try:
            return await self.gen.__anext__()
        except StopAsyncIteration:
            raise RuntimeError("async generator didn't yield")

    async def __aexit__(self, typ, value, traceback):
        if typ is None:
            try:
                await self.gen.__anext__()
            except StopAsyncIteration:
                return False
            else:
                raise RuntimeError("async generator didn't stop")
        else:
            try:
                await self.gen.athrow(typ, value, traceback)
            except StopAsyncIteration as exc:
                return exc is not value
            except RuntimeError as exc:
                if exc is value:
                    return False
                if exc.__cause__ is value:
                    return False
                raise
            except BaseException:
                raise
            else:
                raise RuntimeError("async generator didn't stop after athrow()")


def asynccontextmanager(func):
    """@asynccontextmanager decorator for async generators."""
    def helper(*args, **kwds):
        return _AsyncGeneratorContextManager(func, args, kwds)
    return helper


class AsyncExitStack:
    """Async context manager for dynamic management of a stack of exit callbacks."""

    def __init__(self):
        self._exit_callbacks = []

    async def __aenter__(self):
        return self

    async def __aexit__(self, *exc_details):
        received_exc = exc_details[0] is not None
        suppressed_exc = False
        pending_raise = False
        new_exc_details = (None, None, None)

        while self._exit_callbacks:
            is_sync, cb = self._exit_callbacks.pop()
            try:
                if is_sync:
                    cb_result = cb(*exc_details)
                else:
                    cb_result = await cb(*exc_details)
                if cb_result:
                    suppressed_exc = True
                    pending_raise = False
                    exc_details = (None, None, None)
            except Exception:
                pending_raise = True
                import sys
                exc_details = sys.exc_info()

        return received_exc and suppressed_exc and not pending_raise

    async def enter_async_context(self, cm):
        result = await cm.__aenter__()
        self._exit_callbacks.append((False, cm.__aexit__))
        return result

    def push_async_exit(self, exit_method):
        self._exit_callbacks.append((False, exit_method))

    def push_async_callback(self, callback, *args, **kwds):
        async def _exit_wrapper(exc_type, exc, tb):
            await callback(*args, **kwds)
        self._exit_callbacks.append((False, _exit_wrapper))

    def callback(self, callback, *args, **kwds):
        def _exit_wrapper(exc_type, exc, tb):
            callback(*args, **kwds)
        self._exit_callbacks.append((True, _exit_wrapper))
