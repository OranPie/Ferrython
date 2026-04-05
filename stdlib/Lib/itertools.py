"""Utilities for working with iterators and generators.

Extends the Rust itertools module with additional pure Python utilities.
"""


def pairwise(iterable):
    """Return successive overlapping pairs taken from the input iterable.
    s -> (s0,s1), (s1,s2), (s2, s3), ...
    """
    a = iter(iterable)
    try:
        prev = next(a)
    except StopIteration:
        return
    for item in a:
        yield prev, item
        prev = item


def batched(iterable, n):
    """Batch data into tuples of length n. The last batch may be shorter.
    batched('ABCDEFG', 3) --> ABC DEF G
    """
    if n < 1:
        raise ValueError('n must be at least one')
    it = iter(iterable)
    while True:
        batch = []
        try:
            for _ in range(n):
                batch.append(next(it))
        except StopIteration:
            if batch:
                yield tuple(batch)
            return
        yield tuple(batch)


def groupby(iterable, key=None):
    """Make an iterator that returns consecutive keys and groups.
    
    The key is a function computing a key value for each element.
    """
    if key is None:
        key = lambda x: x
    
    iterator = iter(iterable)
    exhausted = False
    
    try:
        current_value = next(iterator)
    except StopIteration:
        return
    
    current_key = key(current_value)
    
    while not exhausted:
        target_key = current_key
        values = [current_value]
        
        while True:
            try:
                current_value = next(iterator)
            except StopIteration:
                exhausted = True
                break
            current_key = key(current_value)
            if current_key != target_key:
                break
            values.append(current_value)
        
        yield target_key, iter(values)


def accumulate(iterable, func=None, initial=None):
    """Return running totals (or accumulated results of other binary functions).
    
    accumulate([1,2,3,4,5]) --> 1 3 6 10 15
    accumulate([1,2,3,4,5], operator.mul) --> 1 2 6 24 120
    """
    it = iter(iterable)
    total = initial
    if total is None:
        try:
            total = next(it)
        except StopIteration:
            return
    
    yield total
    if func is None:
        for element in it:
            total = total + element
            yield total
    else:
        for element in it:
            total = func(total, element)
            yield total


def product(*iterables, repeat=1):
    """Cartesian product of input iterables.
    
    product('ABCD', 'xy') --> Ax Ay Bx By Cx Cy Dx Dy
    """
    pools = [list(pool) for pool in iterables] * repeat
    result = [[]]
    for pool in pools:
        result = [x + [y] for x in result for y in pool]
    for prod in result:
        yield tuple(prod)


def combinations(iterable, r):
    """Return r length subsequences of elements from the input iterable."""
    pool = list(iterable)
    n = len(pool)
    if r > n:
        return
    indices = list(range(r))
    yield tuple(pool[i] for i in indices)
    while True:
        found = False
        for i in reversed(range(r)):
            if indices[i] != i + n - r:
                found = True
                break
        if not found:
            return
        indices[i] += 1
        for j in range(i + 1, r):
            indices[j] = indices[j - 1] + 1
        yield tuple(pool[i] for i in indices)


def permutations(iterable, r=None):
    """Return successive r length permutations of elements in the iterable."""
    pool = list(iterable)
    n = len(pool)
    r = n if r is None else r
    if r > n:
        return
    indices = list(range(n))
    cycles = list(range(n, n - r, -1))
    yield tuple(pool[i] for i in indices[:r])
    while n:
        found = False
        for i in reversed(range(r)):
            cycles[i] -= 1
            if cycles[i] == 0:
                indices[i:] = indices[i + 1:] + indices[i:i + 1]
                cycles[i] = n - i
            else:
                j = cycles[i]
                indices[i], indices[-j] = indices[-j], indices[i]
                yield tuple(pool[i] for i in indices[:r])
                found = True
                break
        if not found:
            return


def combinations_with_replacement(iterable, r):
    """Return successive r-length combinations with replacement."""
    pool = list(iterable)
    n = len(pool)
    if not n and r:
        return
    indices = [0] * r
    yield tuple(pool[i] for i in indices)
    while True:
        found = False
        for i in reversed(range(r)):
            if indices[i] != n - 1:
                found = True
                break
        if not found:
            return
        indices[i:] = [indices[i] + 1] * (r - i)
        yield tuple(pool[i] for i in indices)


def takewhile(predicate, iterable):
    """Return successive entries from an iterable as long as the predicate is true."""
    for x in iterable:
        if predicate(x):
            yield x
        else:
            break


def dropwhile(predicate, iterable):
    """Drop items from iterable while predicate is true; yield remaining items."""
    dropping = True
    for x in iterable:
        if dropping:
            if predicate(x):
                continue
            dropping = False
        yield x


def starmap(function, iterable):
    """Return an iterator whose values are computed by calling function(*args)."""
    for args in iterable:
        yield function(*args)


def tee(iterable, n=2):
    """Return n independent iterators from a single iterable."""
    it = iter(iterable)
    deques = [[] for _ in range(n)]
    
    def gen(mydeque):
        while True:
            if not mydeque:
                try:
                    newval = next(it)
                except StopIteration:
                    return
                for d in deques:
                    d.append(newval)
            yield mydeque.pop(0)
    
    return tuple(gen(d) for d in deques)


def islice(iterable, *args):
    """Make an iterator that returns selected elements from the iterable."""
    s = slice(*args)
    start, stop, step = s.start or 0, s.stop, s.step or 1
    
    if stop is None:
        # islice(it, start)
        it = iter(iterable)
        for i, item in enumerate(it):
            if i >= start:
                if (i - start) % step == 0:
                    yield item
    else:
        it = iter(iterable)
        for i, item in enumerate(it):
            if i >= stop:
                break
            if i >= start and (i - start) % step == 0:
                yield item


def compress(data, selectors):
    """Make an iterator that filters elements from data returning only those
    that have a corresponding element in selectors that evaluates to True."""
    return (d for d, s in zip(data, selectors) if s)


def filterfalse(predicate, iterable):
    """Make an iterator that filters elements from iterable returning only
    those for which the predicate is False."""
    if predicate is None:
        predicate = bool
    for x in iterable:
        if not predicate(x):
            yield x


def repeat(obj, times=None):
    """Make an iterator that returns object over and over again."""
    if times is None:
        while True:
            yield obj
    else:
        for _ in range(times):
            yield obj


def count(start=0, step=1):
    """Make an iterator that returns evenly spaced values starting with start."""
    n = start
    while True:
        yield n
        n += step


def cycle(iterable):
    """Make an iterator returning elements from the iterable and saving a copy of each."""
    saved = []
    for element in iterable:
        yield element
        saved.append(element)
    while saved:
        for element in saved:
            yield element
