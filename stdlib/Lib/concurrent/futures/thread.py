"""concurrent.futures.thread — ThreadPoolExecutor implementation."""

from concurrent.futures import ThreadPoolExecutor, Future

__all__ = ['ThreadPoolExecutor', 'BrokenThreadPool']


class BrokenThreadPool(RuntimeError):
    """Raised when a worker thread in a ThreadPoolExecutor has failed."""
    pass


class _WorkItem:
    """A work item for the thread pool."""
    def __init__(self, future, fn, args, kwargs):
        self.future = future
        self.fn = fn
        self.args = args
        self.kwargs = kwargs
    
    def run(self):
        try:
            result = self.fn(*self.args, **self.kwargs)
            self.future.set_result(result)
        except Exception as exc:
            self.future.set_exception(exc)
