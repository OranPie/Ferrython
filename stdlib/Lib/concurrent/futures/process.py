"""concurrent.futures.process — ProcessPoolExecutor implementation."""

from concurrent.futures import ProcessPoolExecutor, Future

__all__ = ['ProcessPoolExecutor', 'BrokenProcessPool']


class BrokenProcessPool(RuntimeError):
    """Raised when a worker process in a ProcessPoolExecutor has failed."""
    pass


class _WorkItem:
    """A work item for the process pool."""
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
