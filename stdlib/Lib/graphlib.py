"""Topological sort functionality."""


class CycleError(ValueError):
    """Subclass of ValueError raised by TopologicalSorter when cycles exist."""
    pass


class TopologicalSorter:
    """Provides functionality to topologically sort a graph of hashable nodes."""

    def __init__(self, graph=None):
        self._node2info = {}  # node -> [predecessors_set, npredecessors_remaining]
        self._ready_nodes = None
        self._npassedout = 0
        self._nfinished = 0
        self._prepared = False

        if graph is not None:
            for node, preds in graph.items():
                self.add(node, *preds)

    def add(self, node, *predecessors):
        if self._prepared:
            raise ValueError("Nodes cannot be added after a call to prepare()")
        if node not in self._node2info:
            self._node2info[node] = [set(), 0]
        info = self._node2info[node]
        for pred in predecessors:
            if pred not in self._node2info:
                self._node2info[pred] = [set(), 0]
            if pred not in info[0]:
                info[0].add(pred)
                info[1] += 1

    def prepare(self):
        if self._prepared:
            raise ValueError("cannot prepare() more than once")
        # detect cycles using DFS
        GRAY, BLACK = 0, 1
        color = {}
        for node in self._node2info:
            stack = [(node, False)]
            while stack:
                n, processed = stack.pop()
                if processed:
                    color[n] = BLACK
                    continue
                if n in color:
                    if color[n] == GRAY:
                        raise CycleError("nodes are in a cycle")
                    continue
                color[n] = GRAY
                stack.append((n, True))
                if n in self._node2info:
                    for pred in self._node2info[n][0]:
                        if pred in color:
                            if color[pred] == GRAY:
                                raise CycleError("nodes are in a cycle")
                        else:
                            stack.append((pred, False))
        self._prepared = True
        self._ready_nodes = []
        for node, info in self._node2info.items():
            if info[1] == 0:
                self._ready_nodes.append(node)

    def is_active(self):
        if not self._prepared:
            raise ValueError("prepare() must be called first")
        return self._nfinished < len(self._node2info)

    def get_ready(self):
        if not self._prepared:
            raise ValueError("prepare() must be called first")
        result = tuple(self._ready_nodes)
        self._npassedout += len(result)
        self._ready_nodes = []
        return result

    def done(self, *nodes):
        if not self._prepared:
            raise ValueError("prepare() must be called first")
        for node in nodes:
            if node not in self._node2info:
                raise ValueError(f"node {node!r} was not added using add()")
            self._nfinished += 1
            # update dependents
            for other_node, info in self._node2info.items():
                if node in info[0]:
                    info[1] -= 1
                    if info[1] == 0:
                        self._ready_nodes.append(other_node)

    def static_order(self):
        self.prepare()
        result = []
        while self.is_active():
            ready = self.get_ready()
            result.extend(ready)
            self.done(*ready)
        return result
