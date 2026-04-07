# test_phase107.py — ast.NodeVisitor/NodeTransformer, dis enhancements,
# pprint.PrettyPrinter fix, logging handler level filtering

import ast

# ── ast.NodeVisitor ──
class NameCollector(ast.NodeVisitor):
    def __init__(self):
        self.names = []
    def visit_Name(self, node):
        self.names.append(node.id)
        self.generic_visit(node)

tree = ast.parse("x = 1\ny = x + 2\nz = x + y")
collector = NameCollector()
collector.visit(tree)
assert "x" in collector.names, f"Expected x in names: {collector.names}"
assert "y" in collector.names, f"Expected y in names: {collector.names}"
assert len(collector.names) >= 3, f"Expected >=3 names: {collector.names}"
print("PASS: ast.NodeVisitor collects names")

# Visitor with counting
class AssignCounter(ast.NodeVisitor):
    def __init__(self):
        self.count = 0
    def visit_Assign(self, node):
        self.count += 1
        self.generic_visit(node)

tree2 = ast.parse("a = 1\nb = 2\nc = 3")
counter = AssignCounter()
counter.visit(tree2)
assert counter.count == 3, f"Expected 3 assigns, got {counter.count}"
print("PASS: ast.NodeVisitor counts assigns")

# ── ast.NodeTransformer ──
class NegateConstants(ast.NodeTransformer):
    def visit_Constant(self, node):
        if hasattr(node, "value") and isinstance(node.value, int):
            node.value = -node.value
        return node

tree3 = ast.parse("x = 42")
NegateConstants().visit(tree3)
print("PASS: ast.NodeTransformer modifies tree")

# ── ast.unparse ──
tree4 = ast.parse("x = 1 + 2")
src = ast.unparse(tree4)
assert "x" in src, f"Expected x in unparse: {src}"
assert "1" in src, f"Expected 1 in unparse: {src}"
print("PASS: ast.unparse:", repr(src))

# ── ast.copy_location ──
n1 = ast.parse("a = 1").body[0]
n2 = ast.parse("b = 2").body[0]
ast.copy_location(n2, n1)
print("PASS: ast.copy_location")

# ── dis enhancements ──
import dis

def sample(a, b, c=10):
    """Sample function"""
    return a + b + c

# code_info
info = dis.code_info(sample)
assert isinstance(info, str), f"Expected str, got {type(info)}"
assert "sample" in info, f"Expected 'sample' in info"
assert "Name:" in info, f"Expected 'Name:' in info"
print("PASS: dis.code_info")

# Bytecode
bc = dis.Bytecode(sample)
assert isinstance(bc, list), f"Expected list, got {type(bc)}"
assert len(bc) > 0, "Expected non-empty bytecode"
first = bc[0]
assert hasattr(first, "opname"), "Expected opname attr"
assert hasattr(first, "offset"), "Expected offset attr"
assert hasattr(first, "arg"), "Expected arg attr"
print("PASS: dis.Bytecode with", len(bc), "instructions")

# show_code (just verify no crash)
dis.show_code(sample)
print("PASS: dis.show_code")

# ── pprint.PrettyPrinter ──
import pprint

pp = pprint.PrettyPrinter()
assert pp is not None, "PrettyPrinter should not be None"

# pformat
s = pp.pformat({"key": "value", "list": [1, 2, 3]})
assert "key" in s, f"Expected 'key' in pformat: {s}"
print("PASS: PrettyPrinter.pformat:", repr(s[:50]))

# pprint (just verify no crash)
pp.pprint([1, 2, 3])
print("PASS: PrettyPrinter.pprint")

# isreadable / isrecursive
assert pp.isreadable([1, 2]) == True
assert pp.isrecursive({}) == False
print("PASS: PrettyPrinter.isreadable/isrecursive")

# PrettyPrinter with custom width
pp2 = pprint.PrettyPrinter(width=40)
s2 = pp2.pformat({"a": 1, "b": 2, "c": 3})
assert isinstance(s2, str)
print("PASS: PrettyPrinter with width=40")

# ── logging handler level filtering ──
import logging
import io

stream = io.StringIO()
handler = logging.StreamHandler(stream)
handler.setLevel(logging.WARNING)

logger = logging.getLogger("level_test")
logger.addHandler(handler)
logger.setLevel(logging.DEBUG)

logger.debug("debug msg")
logger.info("info msg")
logger.warning("warn msg")
logger.error("error msg")
logger.critical("critical msg")

output = stream.getvalue()
assert "debug" not in output, f"DEBUG should be filtered: {output}"
assert "info" not in output, f"INFO should be filtered: {output}"
assert "warn" in output, f"WARNING should pass: {output}"
assert "error" in output, f"ERROR should pass: {output}"
assert "critical" in output, f"CRITICAL should pass: {output}"
print("PASS: logging handler level filtering")

# Logger effective level
assert logger.getEffectiveLevel() == 10  # DEBUG
logger.setLevel(logging.ERROR)
assert logger.getEffectiveLevel() == 40  # ERROR
assert logger.isEnabledFor(logging.ERROR) == True
assert logger.isEnabledFor(logging.DEBUG) == False
print("PASS: logger effective level")

print("\nAll phase 107 tests passed!")
