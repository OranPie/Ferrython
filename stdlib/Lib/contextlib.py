"""Utilities for with-statement contexts."""


class _GeneratorContextManager:
    """Helper for @contextmanager decorator."""

    def __init__(self, func, args, kwds):
        self.gen = func(*args, **kwds)
        self.func = func
        self.args = args
        self.kwds = kwds

    def __enter__(self):
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
                return exc is not value
            except BaseException:
                raise
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
    def helper(*args, **kwds):
        return _GeneratorContextManager(func, args, kwds)
    helper.__name__ = getattr(func, '__name__', 'contextmanager')
    helper.__doc__ = getattr(func, '__doc__', None)
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

    def __exit__(self, *exc_details):
        received_exc = exc_details[0] is not None
        suppressed_exc = False
        pending_raise = False
        new_exc_details = (None, None, None)

        while self._exit_callbacks:
            cb = self._exit_callbacks.pop()
            try:
                if cb(*exc_details):
                    suppressed_exc = True
                    pending_raise = False
                    exc_details = (None, None, None)
            except Exception:
                import sys
                new_exc_details = sys.exc_info()
                exc_details = new_exc_details
                pending_raise = True

        if pending_raise:
            raise exc_details[1]
        return received_exc and suppressed_exc

    def enter_context(self, cm):
        _exit = type(cm).__exit__
        result = type(cm).__enter__(cm)
        def _exit_wrapper(exc_type, exc, tb):
            return _exit(cm, exc_type, exc, tb)
        self._exit_callbacks.append(_exit_wrapper)
        return result

    def push(self, exit_callback):
        self._exit_callbacks.append(exit_callback)
        return exit_callback

    def callback(self, callback, *args, **kwds):
        def _exit_wrapper(exc_type, exc, tb):
            callback(*args, **kwds)
            return False
        self._exit_callbacks.append(_exit_wrapper)
        return callback

    def pop_all(self):
        new_stack = ExitStack()
        new_stack._exit_callbacks = self._exit_callbacks
        self._exit_callbacks = []
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
