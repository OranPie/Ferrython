# test_phase159.py - dataclass recursive repr guard

from dataclasses import dataclass


@dataclass
class Node:
    child: "Node"


node = Node(None)
node.child = node
assert repr(node) == "Node(child=...)"


@dataclass
class Wrapper:
    node: Node


wrapper = Wrapper(node)
assert repr(wrapper) == "Wrapper(node=Node(child=...))"

print("test_phase159 passed")
