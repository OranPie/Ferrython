"""_weakrefset — WeakSet implementation."""

import weakref

class WeakSet:
    def __init__(self, data=None):
        self.data = set()
        self._pending_removals = []
        if data is not None:
            self.update(data)
    
    def __iter__(self):
        return iter(list(self.data))
    
    def __len__(self):
        return len(self.data)
    
    def __contains__(self, item):
        return id(item) in {id(x) for x in self.data}
    
    def add(self, item):
        self.data.add(item)
    
    def discard(self, item):
        self.data.discard(item)
    
    def remove(self, item):
        self.data.remove(item)
    
    def pop(self):
        return self.data.pop()
    
    def clear(self):
        self.data.clear()
    
    def update(self, other):
        for item in other:
            self.add(item)
    
    def __ior__(self, other):
        self.update(other)
        return self
    
    def copy(self):
        new = WeakSet()
        new.data = self.data.copy()
        return new
    
    def issubset(self, other):
        return self.data.issubset(set(other))
    
    def issuperset(self, other):
        return self.data.issuperset(set(other))
    
    def union(self, other):
        result = self.copy()
        result.update(other)
        return result
    
    def intersection(self, other):
        result = WeakSet()
        other_set = set(other)
        for item in self.data:
            if item in other_set:
                result.add(item)
        return result
    
    def difference(self, other):
        result = WeakSet()
        other_set = set(other)
        for item in self.data:
            if item not in other_set:
                result.add(item)
        return result
    
    def __repr__(self):
        return f'WeakSet({list(self.data)!r})'
