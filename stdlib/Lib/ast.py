"""
ast — Abstract Syntax Trees

Wraps the Rust _ast module and adds NodeVisitor/NodeTransformer.
"""

from _ast import *
from _ast import parse, dump, literal_eval, walk, get_docstring
from _ast import fix_missing_locations, increment_lineno, iter_child_nodes
from _ast import copy_location, unparse

# Re-export PyCF_ONLY_AST
try:
    from _ast import PyCF_ONLY_AST
except ImportError:
    PyCF_ONLY_AST = 1024


class NodeVisitor:
    """
    A node visitor base class that walks the abstract syntax tree and calls a
    visitor function for every node found. This function may return a value
    which is forwarded by the `visit` method.

    Subclass this class and define visit_<NodeType> methods.
    """

    def visit(self, node):
        """Visit a node."""
        method = 'visit_' + type(node).__name__
        # Also check _type_name for AST nodes from the Rust parser
        if hasattr(node, '_type_name'):
            method = 'visit_' + node._type_name
        visitor = getattr(self, method, self.generic_visit)
        return visitor(node)

    def generic_visit(self, node):
        """Called if no explicit visitor function exists for a node."""
        for child in iter_child_nodes(node):
            self.visit(child)


class NodeTransformer(NodeVisitor):
    """
    A node visitor that transforms the tree. Subclass and define
    visit_<NodeType> methods that return the (possibly modified) node.
    If a visitor returns None, the node is removed from the tree.
    """

    def generic_visit(self, node):
        """Called if no explicit visitor function exists for a node."""
        if not hasattr(node, '_fields'):
            return node
        fields = node._fields
        if hasattr(fields, '__iter__'):
            for field_name_obj in fields:
                field_name = str(field_name_obj) if not isinstance(field_name_obj, str) else field_name_obj
                old_value = getattr(node, field_name, None)
                if old_value is None:
                    continue
                if isinstance(old_value, list):
                    new_values = []
                    for value in old_value:
                        if hasattr(value, '_fields'):
                            value = self.visit(value)
                            if value is None:
                                continue
                            elif not isinstance(value, list):
                                new_values.append(value)
                            else:
                                new_values.extend(value)
                        else:
                            new_values.append(value)
                    # Update the list in place
                    old_value.clear()
                    old_value.extend(new_values)
                elif hasattr(old_value, '_fields'):
                    new_node = self.visit(old_value)
                    if new_node is None:
                        pass  # field removed
                    elif new_node is not old_value:
                        setattr(node, field_name, new_node)
        return node
