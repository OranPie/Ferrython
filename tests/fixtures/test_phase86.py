"""Test pure Python stdlib modules: optparse, ipaddress, webbrowser, trace."""
checks_passed = 0

# =====================================================
# Test optparse module
# =====================================================
from optparse import OptionParser, Values, Option, OptionGroup
from optparse import OptionError, BadOptionError, OptionValueError

# 1. Basic parser creation
parser = OptionParser(usage="test prog", description="A test")
assert parser.usage == "test prog"
assert parser.description == "A test"
checks_passed += 1

# 2. Add options and parse with store
parser = OptionParser(add_help_option=False)
parser.add_option('-f', '--file', dest='filename', default='out.txt',
                  help='output file')
parser.add_option('-v', '--verbose', action='store_true', dest='verbose',
                  default=False)
opts, args = parser.parse_args(['-f', 'data.csv', '--verbose', 'extra'])
assert opts.filename == 'data.csv'
assert opts.verbose == True
assert args == ['extra']
checks_passed += 1

# 3. Long option with =
parser2 = OptionParser(add_help_option=False)
parser2.add_option('--output', dest='output', default='default.txt')
opts2, args2 = parser2.parse_args(['--output=result.json'])
assert opts2.output == 'result.json'
assert args2 == []
checks_passed += 1

# 4. Integer type conversion
parser3 = OptionParser(add_help_option=False)
parser3.add_option('-n', '--count', type='int', dest='count', default=0)
opts3, _ = parser3.parse_args(['-n', '42'])
assert opts3.count == 42
checks_passed += 1

# 5. Float type conversion
parser4 = OptionParser(add_help_option=False)
parser4.add_option('-r', '--rate', type='float', dest='rate', default=1.0)
opts4, _ = parser4.parse_args(['--rate', '3.14'])
assert abs(opts4.rate - 3.14) < 0.001
checks_passed += 1

# 6. store_false action
parser5 = OptionParser(add_help_option=False)
parser5.add_option('--no-color', action='store_false', dest='color',
                   default=True)
opts5, _ = parser5.parse_args(['--no-color'])
assert opts5.color == False
checks_passed += 1

# 7. count action
parser6 = OptionParser(add_help_option=False)
parser6.add_option('-v', action='count', dest='verbosity', default=0)
opts6, _ = parser6.parse_args(['-v', '-v', '-v'])
assert opts6.verbosity == 3
checks_passed += 1

# 8. Double dash stops processing
parser7 = OptionParser(add_help_option=False)
parser7.add_option('-x', action='store_true', dest='x', default=False)
opts7, args7 = parser7.parse_args(['-x', '--', '-y', '-z'])
assert opts7.x == True
assert args7 == ['-y', '-z']
checks_passed += 1

# 9. Defaults
parser8 = OptionParser(add_help_option=False)
parser8.add_option('--name', dest='name', default='world')
opts8, _ = parser8.parse_args([])
assert opts8.name == 'world'
checks_passed += 1

# 10. BadOptionError
parser9 = OptionParser(add_help_option=False)
try:
    parser9.parse_args(['--nonexistent'])
    assert False, "Should have raised"
except BadOptionError:
    checks_passed += 1

# =====================================================
# Test ipaddress module
# =====================================================
from ipaddress import IPv4Address, IPv4Network, IPv4Interface
from ipaddress import ip_address, ip_network, ip_interface

# 11. Basic IPv4Address creation from string
addr = IPv4Address('192.168.1.1')
assert str(addr) == '192.168.1.1'
checks_passed += 1

# 12. IPv4Address from integer
addr2 = IPv4Address(3232235777)  # 192.168.1.1
assert str(addr2) == '192.168.1.1'
assert int(addr2) == 3232235777
checks_passed += 1

# 13. IPv4Address equality and hashing
a = IPv4Address('10.0.0.1')
b = IPv4Address('10.0.0.1')
c = IPv4Address('10.0.0.2')
assert a == b
assert a != c
assert hash(a) == hash(b)
checks_passed += 1

# 14. IPv4Address ordering
assert IPv4Address('1.0.0.0') < IPv4Address('2.0.0.0')
assert IPv4Address('10.0.0.5') > IPv4Address('10.0.0.3')
assert IPv4Address('1.1.1.1') <= IPv4Address('1.1.1.1')
checks_passed += 1

# 15. IPv4Address properties
lo = IPv4Address('127.0.0.1')
assert lo.is_loopback == True
priv = IPv4Address('192.168.0.1')
assert priv.is_private == True
multi = IPv4Address('224.0.0.1')
assert multi.is_multicast == True
unspec = IPv4Address('0.0.0.0')
assert unspec.is_unspecified == True
checks_passed += 1

# 16. IPv4Address packed
addr3 = IPv4Address('10.20.30.40')
assert addr3.packed == bytes([10, 20, 30, 40])
checks_passed += 1

# 17. IPv4Address arithmetic
addr4 = IPv4Address('10.0.0.1')
addr5 = addr4 + 1
assert str(addr5) == '10.0.0.2'
diff = IPv4Address('10.0.0.5') - IPv4Address('10.0.0.1')
assert diff == 4
checks_passed += 1

# 18. IPv4Network basic
net = IPv4Network('192.168.1.0/24')
assert str(net) == '192.168.1.0/24'
assert str(net.network_address) == '192.168.1.0'
assert str(net.broadcast_address) == '192.168.1.255'
assert net.prefixlen == 24
assert net.num_addresses == 256
checks_passed += 1

# 19. IPv4Network containment
net2 = IPv4Network('10.0.0.0/8')
assert IPv4Address('10.1.2.3') in net2
assert IPv4Address('11.0.0.0') not in net2
checks_passed += 1

# 20. IPv4Network strict mode
try:
    IPv4Network('192.168.1.1/24', strict=True)
    assert False, "Should have raised"
except ValueError:
    checks_passed += 1

# 21. ip_address convenience function
a2 = ip_address('172.16.0.1')
assert isinstance(a2, IPv4Address)
assert a2.is_private == True
checks_passed += 1

# 22. IPv4Network netmask
net3 = IPv4Network('10.0.0.0/16')
assert str(net3.netmask) == '255.255.0.0'
checks_passed += 1

# 23. IPv4Interface
iface = IPv4Interface('192.168.1.100/24')
assert str(iface) == '192.168.1.100/24'
net_from_iface = iface.network
assert str(net_from_iface) == '192.168.1.0/24'
checks_passed += 1

# 24. Invalid address raises error
try:
    IPv4Address('256.0.0.1')
    assert False, "Should have raised"
except Exception:
    checks_passed += 1

# =====================================================
# Test webbrowser module (structural only, no actual browser launch)
# =====================================================
import webbrowser

# 25. Module has expected functions
assert hasattr(webbrowser, 'open')
assert hasattr(webbrowser, 'open_new')
assert hasattr(webbrowser, 'open_new_tab')
assert hasattr(webbrowser, 'get')
assert hasattr(webbrowser, 'register')
checks_passed += 1

# 26. BaseBrowser and GenericBrowser classes exist
assert hasattr(webbrowser, 'BaseBrowser')
assert hasattr(webbrowser, 'GenericBrowser')
checks_passed += 1

# 27. register and get work
webbrowser.register('test-browser', webbrowser.GenericBrowser)
browser = webbrowser.get('test-browser')
assert isinstance(browser, webbrowser.GenericBrowser)
checks_passed += 1

# 28. Error on unknown browser
try:
    webbrowser.get('nonexistent-browser-xyz')
    assert False, "Should have raised"
except webbrowser.Error:
    checks_passed += 1

# =====================================================
# Test trace module (structural)
# =====================================================
from trace import Trace, CoverageResults

# 29. CoverageResults creation and update
cr1 = CoverageResults(counts={('file.py', 1): 5, ('file.py', 2): 3})
cr2 = CoverageResults(counts={('file.py', 1): 2, ('file.py', 3): 1})
cr1.update(cr2)
assert cr1.counts[('file.py', 1)] == 7
assert cr1.counts[('file.py', 2)] == 3
assert cr1.counts[('file.py', 3)] == 1
checks_passed += 1

# 30. Trace creation
t = Trace(count=1, trace=0)
assert t.count == 1
assert t.trace == 0
r = t.results()
assert isinstance(r, CoverageResults)
assert len(r.counts) == 0
checks_passed += 1

print("test_phase86: {}/30 checks passed".format(checks_passed))
assert checks_passed == 30
