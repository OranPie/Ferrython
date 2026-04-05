"""Test phase83: New pure Python stdlib modules."""

checks = 0

# ===== posixpath (NO Rust counterpart - pure Python) =====
import posixpath

# Test join
result = posixpath.join('/home', 'user', 'file.txt')
assert result == '/home/user/file.txt', "join: %s" % result
checks += 1
print("PASS posixpath_join")

# Test join with absolute component
result = posixpath.join('/home', '/etc', 'passwd')
assert result == '/etc/passwd', "join absolute: %s" % result
checks += 1
print("PASS posixpath_join_absolute")

# Test split
head, tail = posixpath.split('/home/user/file.txt')
assert head == '/home/user', "split head: %s" % head
assert tail == 'file.txt', "split tail: %s" % tail
checks += 1
print("PASS posixpath_split")

# Test splitext
root, ext = posixpath.splitext('/home/user/file.tar.gz')
assert root == '/home/user/file.tar', "splitext root: %s" % root
assert ext == '.gz', "splitext ext: %s" % ext
checks += 1
print("PASS posixpath_splitext")

# Test splitext with dotfile
root, ext = posixpath.splitext('/home/.bashrc')
assert root == '/home/.bashrc', "splitext dotfile root: %s" % root
assert ext == '', "splitext dotfile ext: %s" % ext
checks += 1
print("PASS posixpath_splitext_dotfile")

# Test basename and dirname
assert posixpath.basename('/home/user/file.txt') == 'file.txt'
assert posixpath.dirname('/home/user/file.txt') == '/home/user'
checks += 1
print("PASS posixpath_basename_dirname")

# Test normpath
assert posixpath.normpath('/home/./user/../user/file.txt') == '/home/user/file.txt'
assert posixpath.normpath('a/b/../c') == 'a/c'
assert posixpath.normpath('') == '.'
checks += 1
print("PASS posixpath_normpath")

# Test isabs
assert posixpath.isabs('/home') == True
assert posixpath.isabs('home') == False
checks += 1
print("PASS posixpath_isabs")

# Test commonprefix
result = posixpath.commonprefix(['/usr/lib', '/usr/local', '/usr/bin'])
assert result == '/usr/', "commonprefix: %s" % result
checks += 1
print("PASS posixpath_commonprefix")

# Test expanduser
result = posixpath.expanduser('~/test')
assert '~' not in result, "expanduser failed: %s" % result
assert result.endswith('/test'), "expanduser: %s" % result
checks += 1
print("PASS posixpath_expanduser")

# Test exists, isdir, isfile
assert posixpath.exists('/') == True
assert posixpath.isdir('/') == True
assert posixpath.isfile('/') == False
assert posixpath.exists('/nonexistent_xyz_123') == False
checks += 1
print("PASS posixpath_exists_isdir_isfile")

# Test abspath
result = posixpath.abspath('test.py')
assert result.startswith('/'), "abspath: %s" % result
assert result.endswith('/test.py'), "abspath: %s" % result
checks += 1
print("PASS posixpath_abspath")

# ===== genericpath (NO Rust counterpart - pure Python) =====
import genericpath

# Test exists
assert genericpath.exists('/') == True
assert genericpath.exists('/nonexistent_xyz') == False
checks += 1
print("PASS genericpath_exists")

# Test isdir and isfile
assert genericpath.isdir('/') == True
assert genericpath.isfile('/') == False
checks += 1
print("PASS genericpath_isdir_isfile")

# Test commonprefix
result = genericpath.commonprefix(['/usr/lib', '/usr/local'])
assert result == '/usr/l', "commonprefix: %s" % result
result = genericpath.commonprefix([])
assert result == '', "commonprefix empty: %s" % result
checks += 1
print("PASS genericpath_commonprefix")

# Test getsize on a known file
size = genericpath.getsize('/')
assert isinstance(size, (int, float)), "getsize type: %s" % type(size)
checks += 1
print("PASS genericpath_getsize")

# ===== calendar (Rust counterpart exists, testing via Rust) =====
import calendar

# Test isleap
assert calendar.isleap(2024) == True, "2024 should be leap"
assert calendar.isleap(2023) == False, "2023 not leap"
assert calendar.isleap(2000) == True, "2000 should be leap"
assert calendar.isleap(1900) == False, "1900 not leap"
checks += 1
print("PASS calendar_isleap")

# Test monthrange
day1, ndays = calendar.monthrange(2024, 2)
assert ndays == 29, "Feb 2024 = 29 days, got %d" % ndays
day1, ndays = calendar.monthrange(2023, 2)
assert ndays == 28, "Feb 2023 = 28 days, got %d" % ndays
day1, ndays = calendar.monthrange(2024, 1)
assert ndays == 31, "Jan 2024 = 31, got %d" % ndays
checks += 1
print("PASS calendar_monthrange")

# Test weekday
wd = calendar.weekday(2024, 1, 1)
assert wd == 0, "2024-01-01 is Monday(0), got %d" % wd
wd = calendar.weekday(2024, 7, 4)
assert wd == 3, "2024-07-04 is Thursday(3), got %d" % wd
checks += 1
print("PASS calendar_weekday")

# Test leapdays
ld = calendar.leapdays(2000, 2024)
assert ld == 6, "leapdays(2000,2024) = 6, got %d" % ld
checks += 1
print("PASS calendar_leapdays")

# Test month_name and day_name
assert calendar.month_name[1] == 'January', "month_name[1]: %s" % calendar.month_name[1]
assert calendar.month_name[12] == 'December', "month_name[12]: %s" % calendar.month_name[12]
assert calendar.day_name[0] == 'Monday', "day_name[0]: %s" % calendar.day_name[0]
assert calendar.day_name[6] == 'Sunday', "day_name[6]: %s" % calendar.day_name[6]
checks += 1
print("PASS calendar_names")

# Test monthcalendar
mc = calendar.monthcalendar(2024, 1)
assert len(mc) == 5, "Jan 2024 has 5 weeks, got %d" % len(mc)
assert mc[0][0] == 1, "Jan 1 2024 is Monday, got %d" % mc[0][0]
assert mc[4][2] == 31, "Jan 31 2024, got %d" % mc[4][2]
checks += 1
print("PASS calendar_monthcalendar")

# ===== fractions (Rust counterpart exists, testing via Rust) =====
from fractions import Fraction

# Test basic creation
f = Fraction(3, 4)
assert str(f) == '3/4', "Fraction(3,4): %s" % str(f)
assert f.numerator == 3, "numerator: %d" % f.numerator
assert f.denominator == 4, "denominator: %d" % f.denominator
checks += 1
print("PASS fraction_create")

# Test auto-simplification
f = Fraction(6, 8)
assert str(f) == '3/4', "Fraction(6,8) should simplify to 3/4: %s" % str(f)
checks += 1
print("PASS fraction_simplify")

# Test addition
f1 = Fraction(1, 3)
f2 = Fraction(1, 6)
result = f1 + f2
assert result == Fraction(1, 2), "1/3 + 1/6 = 1/2, got %s" % str(result)
checks += 1
print("PASS fraction_add")

# Test subtraction
result = Fraction(3, 4) - Fraction(1, 4)
assert result == Fraction(1, 2), "3/4 - 1/4 = 1/2, got %s" % str(result)
checks += 1
print("PASS fraction_sub")

# Test multiplication
result = Fraction(2, 3) * Fraction(3, 4)
assert result == Fraction(1, 2), "2/3 * 3/4 = 1/2, got %s" % str(result)
checks += 1
print("PASS fraction_mul")

# Test division
result = Fraction(1, 2) / Fraction(3, 4)
assert result == Fraction(2, 3), "1/2 / 3/4 = 2/3, got %s" % str(result)
checks += 1
print("PASS fraction_div")

# Test comparison
assert Fraction(1, 3) < Fraction(1, 2)
assert Fraction(2, 4) == Fraction(1, 2)
assert Fraction(3, 4) > Fraction(1, 2)
checks += 1
print("PASS fraction_compare")

# Test float conversion
assert float(Fraction(1, 4)) == 0.25, "float(1/4) = 0.25"
checks += 1
print("PASS fraction_float")

# Test negation
f = -Fraction(3, 4)
assert f.numerator == -3, "neg numerator: %d" % f.numerator
assert f.denominator == 4, "neg denominator: %d" % f.denominator
checks += 1
print("PASS fraction_neg")

# Test floor division
result = Fraction(7, 4) // Fraction(1, 2)
assert result == 3, "7/4 // 1/2 = 3, got %s" % str(result)
checks += 1
print("PASS fraction_floordiv")

# Test limit_denominator
f = Fraction(3141592, 1000000)
limited = f.limit_denominator(100)
assert limited.denominator <= 100, "limit_denominator: %s" % str(limited)
checks += 1
print("PASS fraction_limit_denominator")

# ===== decimal_ (NO Rust counterpart - pure Python) =====
from decimal_ import Decimal

# Test basic creation from string
d = Decimal('3.14')
assert str(d) == '3.14', "Decimal('3.14'): %s" % str(d)
checks += 1
print("PASS decimal_create_string")

# Test creation from int
d = Decimal(42)
assert str(d) == '42', "Decimal(42): %s" % str(d)
checks += 1
print("PASS decimal_create_int")

# Test addition
d1 = Decimal('1.5')
d2 = Decimal('2.5')
result = d1 + d2
assert str(result) == '4.0' or str(result) == '4.00', "1.5 + 2.5: %s" % str(result)
checks += 1
print("PASS decimal_add")

# Test subtraction
result = Decimal('10.5') - Decimal('3.3')
r_float = float(result)
assert abs(r_float - 7.2) < 0.001, "10.5 - 3.3 = 7.2, got %s (float: %f)" % (str(result), r_float)
checks += 1
print("PASS decimal_sub")

# Test multiplication
result = Decimal('2.5') * Decimal('4')
r_float = float(result)
assert abs(r_float - 10.0) < 0.001, "2.5 * 4 = 10, got %s" % str(result)
checks += 1
print("PASS decimal_mul")

# Test comparison
assert Decimal('3.14') > Decimal('2.71')
assert Decimal('1.0') == Decimal('1.0')
assert Decimal('2.5') < Decimal('3.0')
checks += 1
print("PASS decimal_compare")

# Test negation
d = -Decimal('5.5')
assert float(d) == -5.5, "neg: %s" % str(d)
checks += 1
print("PASS decimal_neg")

# Test int conversion
d = Decimal('42.9')
assert int(d) == 42, "int(42.9) = 42, got %d" % int(d)
checks += 1
print("PASS decimal_int")

# Test quantize
d = Decimal('3.14159')
q = d.quantize(Decimal('0.01'))
assert str(q) == '3.14', "quantize to 0.01: %s" % str(q)
checks += 1
print("PASS decimal_quantize")

# Test zero
assert Decimal(0) == Decimal('0')
assert not bool(Decimal(0))
assert bool(Decimal('1.5'))
checks += 1
print("PASS decimal_zero")

# ===== getopt (NO Rust counterpart - pure Python) =====
import getopt

# Test basic short options
opts, args = getopt.getopt(['-v', '-o', 'outfile', 'infile'], 'vo:')
assert ('-v', '') in opts, "getopt -v"
assert ('-o', 'outfile') in opts, "getopt -o"
assert args == ['infile'], "getopt remaining args: %s" % str(args)
checks += 1
print("PASS getopt_basic")

# Test no options
opts2, args2 = getopt.getopt(['file1', 'file2'], 'v')
assert opts2 == [], "no opts expected"
assert args2 == ['file1', 'file2']
checks += 1
print("PASS getopt_no_opts")

# Test -- stops processing
opts3, args3 = getopt.getopt(['-v', '--', '-o', 'file'], 'vo:')
assert len(opts3) == 1, "only -v before --"
assert args3 == ['-o', 'file'], "args after --: %s" % str(args3)
checks += 1
print("PASS getopt_dashdash")

# Test long options
opts4, args4 = getopt.getopt(['--verbose', '--output=test.txt', 'in.txt'],
                              'vo:', ['verbose', 'output='])
assert ('--verbose', '') in opts4, "long --verbose"
assert ('--output', 'test.txt') in opts4, "long --output"
assert args4 == ['in.txt']
checks += 1
print("PASS getopt_long")

# Test error on unknown option
try:
    getopt.getopt(['-x'], 'vo:')
    assert False, "should have raised GetoptError"
except getopt.GetoptError as e:
    assert 'x' in str(e)
    checks += 1
print("PASS getopt_error")

# Test gnu_getopt
opts5, args5 = getopt.gnu_getopt(['-v', 'file1', '-o', 'out', 'file2'], 'vo:')
assert ('-v', '') in opts5, "gnu -v"
assert ('-o', 'out') in opts5, "gnu -o"
assert 'file1' in args5 and 'file2' in args5
checks += 1
print("PASS getopt_gnu")

# Test combined short options
opts6, args6 = getopt.getopt(['-abc'], 'abc')
assert len(opts6) == 3, "combined -abc should give 3 opts, got %d" % len(opts6)
checks += 1
print("PASS getopt_combined")

# Test short option with attached value
opts7, args7 = getopt.getopt(['-ovalue'], 'o:')
assert opts7 == [('-o', 'value')], "attached value: %s" % str(opts7)
checks += 1
print("PASS getopt_attached_value")

# ===== codeop (NO Rust counterpart - pure Python) =====
import codeop

cc = codeop.CommandCompiler()
result = cc("1 + 2", "<test>", "eval")
assert result is not None, "complete expression should compile"
checks += 1
print("PASS codeop_compile")

comp = codeop.Compile()
code_obj = comp("x = 42", "<test>", "exec")
assert code_obj is not None
checks += 1
print("PASS codeop_Compile")

# ===== code (NO Rust counterpart - pure Python) =====
import code

interp = code.InteractiveInterpreter()
assert isinstance(interp.locals, dict)
assert '__name__' in interp.locals
checks += 1
print("PASS code_interpreter")

console = code.InteractiveConsole()
assert isinstance(console.locals, dict)
assert console.buffer == []
checks += 1
print("PASS code_console")

result2 = code.compile_command("x = 1", "<test>", "exec")
assert result2 is not None, "compile_command should return code object"
checks += 1
print("PASS code_compile_command")

# ===== Summary =====
print()
print("%d/%d checks passed" % (checks, checks))
