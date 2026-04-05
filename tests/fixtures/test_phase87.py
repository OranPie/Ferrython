"""Test Counter helpers, namedtuple methods, deque indexing."""
checks_passed = 0

# Test Counter helper functions
from collections import Counter, most_common, counter_elements, counter_update, counter_subtract, counter_total

# Create counter
c = Counter("abracadabra")
checks_passed += 1

# most_common
mc = most_common(c, 3)
assert mc[0][0] == 'a'
assert mc[0][1] == 5
checks_passed += 1

# counter_elements - returns list of elements repeated by count
elems = counter_elements(c)
assert len(elems) == 11  # "abracadabra" has 11 chars
checks_passed += 1

# counter_total - sum of counts
total = counter_total(c)
assert total == 11
checks_passed += 1

# counter_update - add counts from iterable
c2 = Counter("aab")
counter_update(c2, "bcc")
t2 = counter_total(c2)
assert t2 == 6  # a:2, b:2, c:2
checks_passed += 1

# counter_subtract
c3 = Counter("aaabbb")
counter_subtract(c3, "ab")
t3 = counter_total(c3)
assert t3 == 4  # a:2, b:2
checks_passed += 1

# Test namedtuple with fields
from collections import namedtuple

Point = namedtuple('Point', ['x', 'y'])
p = Point(1, 2)
assert p.x == 1
assert p.y == 2
checks_passed += 1

# _fields
assert len(Point._fields) == 2
checks_passed += 1

# _make
p2 = Point._make([10, 20])
assert p2.x == 10
assert p2.y == 20
checks_passed += 1

# String field spec
Color = namedtuple('Color', 'r g b')
c = Color(255, 128, 0)
assert c.r == 255
assert c.g == 128
assert c.b == 0
checks_passed += 1

# Test deque __getitem__
from collections import deque
d = deque([10, 20, 30, 40, 50])
assert d[0] == 10
assert d[-1] == 50
assert d[2] == 30
checks_passed += 1

# deque operations chain
d2 = deque()
d2.append(1)
d2.append(2)
d2.appendleft(0)
assert d2.popleft() == 0
assert d2.pop() == 2
checks_passed += 1

# deque rotate and contains
d3 = deque([1, 2, 3, 4, 5])
d3.rotate(2)
assert 1 in d3
assert 5 in d3
checks_passed += 1

# deque reverse
d4 = deque([1, 2, 3])
d4.reverse()
assert d4[0] == 3
assert d4[2] == 1
checks_passed += 1

# datetime isoformat with separator
import datetime
dt = datetime.datetime(2024, 3, 15, 10, 30, 0)
iso_space = dt.isoformat(' ')
assert iso_space == '2024-03-15 10:30:00'
checks_passed += 1

# strftime various codes
assert dt.strftime('%Y%m%d') == '20240315'
checks_passed += 1

# timestamp
ts = dt.timestamp()
assert ts > 1700000000
checks_passed += 1

# date toordinal
d = datetime.date(2024, 1, 1)
ord_val = d.toordinal()
assert ord_val > 738000
checks_passed += 1

print(f"test_phase87: {checks_passed}/18 checks passed")
assert checks_passed == 18
