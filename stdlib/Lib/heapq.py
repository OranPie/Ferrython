"""heapq module — Heap queue algorithm (a.k.a. priority queue)."""

def heappush(heap, item):
    """Push item onto heap, maintaining the heap invariant."""
    heap.append(item)
    _siftdown(heap, 0, len(heap) - 1)

def heappop(heap):
    """Pop the smallest item off the heap, maintaining the heap invariant."""
    lastelt = heap.pop()
    if heap:
        returnitem = heap[0]
        heap[0] = lastelt
        _siftup(heap, 0)
        return returnitem
    return lastelt

def heappushpop(heap, item):
    """Push item on the heap, then pop and return the smallest item."""
    if heap and heap[0] < item:
        item, heap[0] = heap[0], item
        _siftup(heap, 0)
    return item

def heapreplace(heap, item):
    """Pop and return the smallest item, then push item. Raises IndexError if empty."""
    returnitem = heap[0]
    heap[0] = item
    _siftup(heap, 0)
    return returnitem

def heapify(x):
    """Transform list into a heap, in-place, in O(len(x)) time."""
    n = len(x)
    for i in range(n // 2 - 1, -1, -1):
        _siftup(x, i)

def nlargest(n, iterable):
    """Find the n largest elements in a dataset."""
    if hasattr(iterable, '__len__') and n >= len(iterable):
        return sorted(iterable, reverse=True)[:n]
    result = sorted(iterable, reverse=True)
    return result[:n]

def nsmallest(n, iterable):
    """Find the n smallest elements in a dataset."""
    if hasattr(iterable, '__len__') and n >= len(iterable):
        return sorted(iterable)[:n]
    result = sorted(iterable)
    return result[:n]

def merge(*iterables):
    """Merge multiple sorted inputs into a single sorted output."""
    result = []
    for it in iterables:
        result.extend(it)
    result.sort()
    return iter(result)

def _siftdown(heap, startpos, pos):
    """Move item at pos up to its correct location in the heap."""
    newitem = heap[pos]
    while pos > startpos:
        parentpos = (pos - 1) >> 1
        parent = heap[parentpos]
        if newitem < parent:
            heap[pos] = parent
            pos = parentpos
        else:
            break
    heap[pos] = newitem

def _siftup(heap, pos):
    """Move item at pos down to its correct location in the heap."""
    endpos = len(heap)
    startpos = pos
    newitem = heap[pos]
    childpos = 2 * pos + 1
    while childpos < endpos:
        rightpos = childpos + 1
        if rightpos < endpos and not heap[childpos] < heap[rightpos]:
            childpos = rightpos
        heap[pos] = heap[childpos]
        pos = childpos
        childpos = 2 * pos + 1
    heap[pos] = newitem
    _siftdown(heap, startpos, pos)
