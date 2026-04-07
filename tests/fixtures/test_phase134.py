"""Phase 134: ZipFile.getinfo, XML findall descendant, re.sub callable with groups,
   plus broad stdlib verification."""

checks_passed = 0
checks_total = 0

def check(name, cond):
    global checks_passed, checks_total
    checks_total += 1
    if cond:
        checks_passed += 1
    else:
        print(f"  FAIL: {name}")

# ── ZipFile.getinfo ──
import zipfile, io

buf = io.BytesIO()
with zipfile.ZipFile(buf, 'w', zipfile.ZIP_DEFLATED) as zf:
    zf.writestr("hello.txt", "Hello World!")
    zf.writestr("data/test.txt", "Some data")
buf.seek(0)
with zipfile.ZipFile(buf, 'r') as zf:
    info = zf.getinfo("hello.txt")
    check("zipinfo_filename", info.filename == "hello.txt")
    check("zipinfo_size", info.file_size == 12)
    info2 = zf.getinfo("data/test.txt")
    check("zipinfo_nested", info2.filename == "data/test.txt")

# ── XML findall descendant (.//tag) ──
import xml.etree.ElementTree as ET

root = ET.fromstring('<a><b><c>1</c></b><b><c>2</c></b></a>')
cs = root.findall('.//c')
check("xml_findall_desc_len", len(cs) == 2)
check("xml_findall_desc_text0", cs[0].text == '1')
check("xml_findall_desc_text1", cs[1].text == '2')

# find descendant
found = root.find('.//c')
check("xml_find_desc", found is not None and found.text == '1')

# wildcard descendant
all_elems = root.findall('.//*')
check("xml_findall_wildcard", len(all_elems) >= 4)  # b, c, b, c

# ── re.sub with callable ──
import re

result = re.sub(r'(\w+)', lambda m: m.group(1).upper(), 'hello world')
check("re_sub_callable_group1", result == 'HELLO WORLD')

result2 = re.sub(r'\w+', lambda m: m.group(0)[::-1], 'hello world')
check("re_sub_callable_reverse", result2 == 'olleh dlrow')

result3 = re.sub(r'(?P<word>\w+)', lambda m: m.group('word').title(), 'hello world')
check("re_sub_callable_named", result3 == 'Hello World')

result4 = re.sub(r'(\w+)\s+(\w+)', lambda m: ' '.join(reversed(m.groups())), 'hello world foo bar')
check("re_sub_callable_groups", result4 == 'world hello bar foo')

# re.sub with count
result5 = re.sub(r'\w+', lambda m: m.group(0).upper(), 'hello world foo', count=2)
check("re_sub_callable_count", result5 == 'HELLO WORLD foo')

# ── Broad stdlib verification ──

# copy module
import copy
original = {'a': [1, 2, {'b': [3, 4]}]}
dc = copy.deepcopy(original)
dc['a'][2]['b'].append(5)
check("deepcopy_isolated", original['a'][2]['b'] == [3, 4])

# operator module
import operator
check("operator_attrgetter_dotted", operator.attrgetter('__name__')(int) == 'int')
check("operator_methodcaller", operator.methodcaller('upper')("hello") == "HELLO")
check("operator_itemgetter_multi", operator.itemgetter(0, 2)([10, 20, 30]) == (10, 30))

# textwrap
import textwrap
check("textwrap_dedent", textwrap.dedent("  hello\n  world") == "hello\nworld")
check("textwrap_indent", textwrap.indent("hello\nworld", ">> ") == ">> hello\n>> world")

# shlex
import shlex
check("shlex_split", shlex.split("echo 'hello world' -n") == ["echo", "hello world", "-n"])
check("shlex_quote", shlex.quote("hello world") == "'hello world'")

# difflib
import difflib
check("difflib_seqmatch", difflib.SequenceMatcher(None, "abcde", "ace").ratio() > 0)
check("difflib_close_matches", "apple" in difflib.get_close_matches("appel", ["ape", "apple", "peach"]))

# statistics
import statistics
check("statistics_mean", statistics.mean([1, 2, 3, 4, 5]) == 3)
check("statistics_median", statistics.median([1, 3, 5]) == 3)

# decimal
import decimal
d1 = decimal.Decimal("10.5")
d2 = decimal.Decimal("3.2")
check("decimal_arith", str(d1 + d2) == "13.7")

# heapq
import heapq
h = []
heapq.heappush(h, 5)
heapq.heappush(h, 1)
heapq.heappush(h, 3)
check("heapq_pop", heapq.heappop(h) == 1)

# bisect
import bisect
a = [1, 3, 5, 7, 9]
check("bisect_left", bisect.bisect_left(a, 5) == 2)

# functools
import functools
check("functools_reduce", functools.reduce(lambda a, b: a + b, [1, 2, 3, 4, 5]) == 15)

# configparser
import configparser
cp = configparser.ConfigParser()
cp.read_string("[s1]\nkey = value\n")
check("configparser", cp.get('s1', 'key') == 'value')

# argparse
import argparse
parser = argparse.ArgumentParser()
parser.add_argument('--name', default='world')
args = parser.parse_args(['--name', 'Alice'])
check("argparse", args.name == 'Alice')

# subprocess
import subprocess
result = subprocess.run(["echo", "test"], capture_output=True, text=True)
check("subprocess_run", result.stdout.strip() == "test")

# datetime deeper
import datetime
dt = datetime.datetime(2024, 1, 15, 10, 30)
check("datetime_strftime", dt.strftime("%Y-%m-%d") == "2024-01-15")
td = datetime.timedelta(days=1, hours=2, minutes=30)
check("timedelta_total_seconds", td.total_seconds() == 95400.0)

# collections.namedtuple
from collections import namedtuple
Point = namedtuple('Point', ['x', 'y'])
p = Point(1, 2)
check("namedtuple_access", p.x == 1 and p[0] == 1)
check("namedtuple_asdict", p._asdict() == {'x': 1, 'y': 2})

# math
import math
check("math_factorial", math.factorial(10) == 3628800)
check("math_comb", math.comb(10, 3) == 120)
check("math_prod", math.prod([1, 2, 3, 4]) == 24)

# complex
c = complex(3, 4)
check("complex_abs", abs(c) == 5.0)
check("complex_conjugate", c.conjugate() == complex(3, -4))

# sqlite3
import sqlite3
conn = sqlite3.connect(":memory:")
conn.execute("CREATE TABLE t (id INTEGER, name TEXT)")
conn.execute("INSERT INTO t VALUES (1, 'test')")
conn.commit()
conn.row_factory = sqlite3.Row
row = conn.execute("SELECT * FROM t").fetchone()
check("sqlite3_row", row['name'] == 'test')
conn.close()

print(f"test_phase134: {checks_passed}/{checks_total} checks passed")
