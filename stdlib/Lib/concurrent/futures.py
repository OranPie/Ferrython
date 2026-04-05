"""concurrent.futures — high-level interface for asynchronously executing callables."""

# Constants
FIRST_COMPLETED = 'FIRST_COMPLETED'
FIRST_EXCEPTION = 'FIRST_EXCEPTION'
ALL_COMPLETED = 'ALL_COMPLETED'


class CancelledError(Exception):
    pass


class TimeoutError(Exception):
    pass


class Future:
    """Represents the result of an asynchronous computation."""

    def __init__(self):
        self._result = None
        self._exception = None
        self._done = False
        self._cancelled = False
        self._callbacks = []

    def result(self, timeout=None):
        if self._exception is not None:
            raise self._exception
        return self._result

    def exception(self, timeout=None):
        return self._exception

    def done(self):
        return self._done

    def cancelled(self):
        return self._cancelled

    def running(self):
        return not self._done and not self._cancelled

    def cancel(self):
        if self._done:
            return False
        self._cancelled = True
        return True

    def set_result(self, result):
        self._result = result
        self._done = True
        for cb in self._callbacks:
            cb(self)

    def set_exception(self, exception):
        self._exception = exception
        self._done = True
        for cb in self._callbacks:
            cb(self)

    def add_done_callback(self, fn):
        self._callbacks.append(fn)
        if self._done:
            fn(self)

    def __repr__(self):
        if self._cancelled:
            return '<Future: cancelled>'
        if self._done:
            return '<Future: finished>'
        return '<Future: pending>'


class _Executor:
    """Base class for executors."""

    def __init__(self, max_workers=None):
        self._max_workers = max_workers or 4
        self._shutdown = False

    def submit(self, fn, *args, **kwargs):
        """Submit a callable for execution and return a Future."""
        future = Future()
        try:
            result = fn(*args, **kwargs)
            future.set_result(result)
        except Exception as e:
            future.set_exception(e)
        return future

    def map(self, fn, *iterables, timeout=None, chunksize=1):
        """Map fn across iterables, yielding results."""
        # Simple sequential implementation
        if len(iterables) == 1:
            return [fn(item) for item in iterables[0]]
        else:
            return [fn(*args) for args in zip(*iterables)]

    def shutdown(self, wait=True):
        self._shutdown = True

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.shutdown(wait=True)
        return False


class ThreadPoolExecutor(_Executor):
    """Executor that uses a pool of threads (sequential in Ferrython)."""
    pass


class ProcessPoolExecutor(_Executor):
    """Executor that uses a pool of processes (sequential in Ferrython)."""
    pass


def wait(fs, timeout=None, return_when=ALL_COMPLETED):
    """Wait for futures to complete."""
    done = set()
    not_done = set()
    for f in fs:
        if f.done() or f.cancelled():
            done.add(f)
        else:
            not_done.add(f)

    class WaitResult:
        def __init__(self, done, not_done):
            self.done = done
            self.not_done = not_done
    return WaitResult(done, not_done)


def as_completed(fs, timeout=None):
    """Yield futures as they complete."""
    for f in fs:
        yield f
