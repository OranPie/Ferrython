"""Pure Python implementation of the sched module.

A generally useful event scheduler class.
"""

import heapq
import time as _time


class Event:
    """Event object for the scheduler."""
    __slots__ = ('time', 'priority', 'sequence', 'action', 'argument', 'kwargs')
    
    def __init__(self, time, priority, sequence, action, argument=(), kwargs=None):
        self.time = time
        self.priority = priority
        self.sequence = sequence
        self.action = action
        self.argument = argument
        self.kwargs = kwargs if kwargs is not None else {}
    
    @property
    def _key(self):
        return (self.time, self.priority, self.sequence)
    
    def __eq__(self, other):
        return self._key == other._key
    
    def __lt__(self, other):
        return self._key < other._key
    
    def __le__(self, other):
        return self._key <= other._key
    
    def __gt__(self, other):
        return self._key > other._key
    
    def __ge__(self, other):
        return self._key >= other._key
    
    def __repr__(self):
        return 'Event(time={}, priority={}, action={})'.format(
            self.time, self.priority, self.action)


class scheduler:
    """General purpose event scheduler."""
    
    def __init__(self, timefunc=None, delayfunc=None):
        self._queue = []
        self._sequence_generator = 0
        self.timefunc = timefunc or _time.monotonic
        self.delayfunc = delayfunc or _time.sleep
    
    def enterabs(self, time, priority, action, argument=(), kwargs=None):
        """Enter a new event in the queue at an absolute time."""
        if kwargs is None:
            kwargs = {}
        self._sequence_generator += 1
        event = Event(time, priority, self._sequence_generator, action, argument, kwargs)
        # Use tuple keys for heap ordering since heapq may not use __lt__ on instances
        heapq.heappush(self._queue, ((time, priority, self._sequence_generator), event))
        return event
    
    def enter(self, delay, priority, action, argument=(), kwargs=None):
        """Enter a new event with a relative time delay."""
        time = self.timefunc() + delay
        return self.enterabs(time, priority, action, argument, kwargs)
    
    def cancel(self, event):
        """Remove an event from the queue."""
        self._queue = [(k, e) for k, e in self._queue if e is not event]
        heapq.heapify(self._queue)
    
    def empty(self):
        """Check whether the queue is empty."""
        return not self._queue
    
    @property
    def queue(self):
        """Return an ordered list of upcoming events."""
        events = [(k, e) for k, e in self._queue]
        events.sort(key=lambda x: x[0])
        return [e for _, e in events]
    
    def run(self, blocking=True):
        """Execute events until the queue is empty."""
        q = self._queue
        delayfunc = self.delayfunc
        timefunc = self.timefunc
        pop = heapq.heappop
        
        while q:
            key, event = q[0]
            now = timefunc()
            if event.time > now:
                if not blocking:
                    return event.time - now
                delayfunc(event.time - now)
            else:
                pop(q)
                event.action(*event.argument, **event.kwargs)
                delayfunc(0)
        
        return None
