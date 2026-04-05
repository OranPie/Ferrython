"""Queue module — FIFO, LIFO, and priority queues.

Since ferrython is single-threaded, these are simple non-locking
implementations backed by lists and heapq.
"""

import heapq


class Empty(Exception):
    """Raised by Queue.get(block=False) when the queue is empty."""
    pass


class Full(Exception):
    """Raised by Queue.put(block=False) when the queue is full."""
    pass


class Queue:
    """FIFO queue."""

    def __init__(self, maxsize=0):
        self.maxsize = maxsize
        self._queue = []
        self._unfinished = 0

    def qsize(self):
        return len(self._queue)

    def empty(self):
        return len(self._queue) == 0

    def full(self):
        return 0 < self.maxsize <= len(self._queue)

    def put(self, item, block=True, timeout=None):
        if self.full():
            raise Full("Queue is full")
        self._queue.append(item)
        self._unfinished += 1

    def get(self, block=True, timeout=None):
        if self.empty():
            raise Empty("Queue is empty")
        self._unfinished -= 1
        return self._queue.pop(0)

    def put_nowait(self, item):
        return self.put(item, block=False)

    def get_nowait(self):
        return self.get(block=False)

    def task_done(self):
        if self._unfinished <= 0:
            raise ValueError("task_done() called too many times")
        self._unfinished -= 1

    def join(self):
        pass  # single-threaded — nothing to wait for


class LifoQueue(Queue):
    """LIFO (stack) queue."""

    def get(self, block=True, timeout=None):
        if self.empty():
            raise Empty("Queue is empty")
        self._unfinished -= 1
        return self._queue.pop()


class PriorityQueue(Queue):
    """Priority queue backed by a heap."""

    def put(self, item, block=True, timeout=None):
        if self.full():
            raise Full("Queue is full")
        heapq.heappush(self._queue, item)
        self._unfinished += 1

    def get(self, block=True, timeout=None):
        if self.empty():
            raise Empty("Queue is empty")
        self._unfinished -= 1
        return heapq.heappop(self._queue)
