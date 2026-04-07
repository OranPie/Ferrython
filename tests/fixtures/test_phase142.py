# test_phase142.py — HTML parser entities, dbm persistence, ast.unparse, compileall, signal
import sys, os, tempfile

# 1. HTML parser: self-closing, entities, char refs
from html.parser import HTMLParser
class P(HTMLParser):
    def __init__(self):
        super().__init__()
        self.ev = []
    def handle_starttag(self, tag, attrs): self.ev.append(('S', tag))
    def handle_endtag(self, tag): self.ev.append(('E', tag))
    def handle_data(self, data):
        d = data.strip()
        if d: self.ev.append(('D', d))
    def handle_entityref(self, name): self.ev.append(('&', name))
    def handle_charref(self, name): self.ev.append(('#', name))

p = P()
p.feed('<br/><b>A&amp;B&#65;</b>')
assert ('S', 'br') in p.ev, f"missing br start: {p.ev}"
assert ('E', 'br') in p.ev, f"missing br end: {p.ev}"
assert ('&', 'amp') in p.ev, f"missing entity: {p.ev}"
assert ('#', '65') in p.ev, f"missing charref: {p.ev}"

# 2. dbm persistence
import dbm
td = tempfile.mkdtemp()
db_path = os.path.join(td, 'test')
db = dbm.open(db_path, 'c')
db['hello'] = b'world'
db['num'] = b'42'
db.close()
db2 = dbm.open(db_path, 'r')
assert db2['hello'] == b'world', f"dbm read: {db2['hello']}"
assert db2['num'] == b'42'
assert len(db2) == 2
db2.close()
import shutil
shutil.rmtree(td)

# 3. ast.unparse covers comprehensions, functions, imports
import ast
src1 = ast.unparse(ast.parse('x = [i**2 for i in range(10)]'))
assert 'for i in range(10)' in src1, f"unparse listcomp: {src1}"
src2 = ast.unparse(ast.parse('def foo(a, b): return a + b'))
assert 'def foo' in src2, f"unparse funcdef: {src2}"
src3 = ast.unparse(ast.parse('from os import path'))
assert 'from os import path' in src3, f"unparse import: {src3}"
src4 = ast.unparse(ast.parse('x = -y'))
assert '-y' in src4, f"unparse unary: {src4}"

# 4. compileall
import compileall
with tempfile.NamedTemporaryFile(mode='w', suffix='.py', delete=False) as f:
    f.write("x = 1 + 2\n")
    tmp = f.name
assert compileall.compile_file(tmp) == True
os.unlink(tmp)
with tempfile.NamedTemporaryFile(mode='w', suffix='.py', delete=False) as f:
    f.write("def bad(\n")
    tmp2 = f.name
assert compileall.compile_file(tmp2) == False
os.unlink(tmp2)

# 5. signal handling
import signal
caught = []
def handler(sig, frame): caught.append(sig)
signal.signal(signal.SIGUSR1, handler)
signal.raise_signal(signal.SIGUSR1)
assert len(caught) == 1 and caught[0] == signal.SIGUSR1

# 6. ast.unparse more node types
src5 = ast.unparse(ast.parse('x = a < b <= c'))
assert '<' in src5 and '<=' in src5, f"unparse compare: {src5}"
src6 = ast.unparse(ast.parse('f = lambda x: x + 1'))
assert 'lambda' in src6, f"unparse lambda: {src6}"

print("phase142: all 6 checks passed")
