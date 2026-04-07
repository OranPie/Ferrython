# test_phase115.py — uuid deepening, decimal localcontext, shutil, new modules

# ── UUID module deepening ──
import uuid

# uuid4 with proper attributes
u = uuid.uuid4()
assert hasattr(u, 'hex')
assert hasattr(u, 'version')
assert u.version == 4
assert hasattr(u, 'variant')
assert hasattr(u, 'bytes')
assert len(u.bytes) == 16
assert hasattr(u, 'bytes_le')
assert len(u.bytes_le) == 16
assert hasattr(u, 'urn')
assert u.urn.startswith('urn:uuid:')
assert hasattr(u, 'fields')
assert len(u.fields) == 6
assert hasattr(u, 'node')
assert hasattr(u, 'time_low')
assert hasattr(u, 'time_mid')

# UUID constructor
u2 = uuid.UUID('12345678-1234-5678-1234-567812345678')
assert u2.hex == '12345678123456781234567812345678'
assert u2.version == 5

# uuid1
u1 = uuid.uuid1()
assert u1.version == 1

# uuid3 (MD5-based)
u3 = uuid.uuid3(uuid.NAMESPACE_DNS, 'python.org')
assert u3.version == 3
assert len(u3.hex) == 32

# uuid5 (SHA1-based)
u5 = uuid.uuid5(uuid.NAMESPACE_DNS, 'python.org')
assert u5.version == 5
assert len(u5.hex) == 32

# NAMESPACE constants are UUID objects
assert hasattr(uuid.NAMESPACE_DNS, 'hex')
assert hasattr(uuid.NAMESPACE_URL, 'hex')
assert hasattr(uuid.NAMESPACE_OID, 'hex')
assert hasattr(uuid.NAMESPACE_X500, 'hex')

# ── Decimal localcontext ──
from decimal import Decimal, getcontext, localcontext, ROUND_HALF_UP

ctx = getcontext()
assert hasattr(ctx, 'prec')
assert ctx.prec == 28  # default

# localcontext as context manager
# Note: we modify prec inside and it should restore after
original_prec = getcontext().prec
with localcontext() as ctx:
    ctx.prec = 50
    assert getcontext().prec == 50
assert getcontext().prec == original_prec

# Decimal exception types
from decimal import InvalidOperation, DivisionByZero, Overflow
from decimal import BasicContext, ExtendedContext
assert BasicContext.prec == 9
assert ExtendedContext.prec == 9

# ── netrc module ──
import netrc as netrc_mod
# Just verify import works
assert hasattr(netrc_mod, 'netrc')
assert hasattr(netrc_mod, 'NetrcParseError')

# ── bdb module ──
import bdb
assert hasattr(bdb, 'Bdb')
assert hasattr(bdb, 'Breakpoint')
assert hasattr(bdb, 'BdbQuit')
bp_class = bdb.Breakpoint
assert hasattr(bp_class, 'bplist')

# ── sre_constants ──
import sre_constants
assert sre_constants.MAXREPEAT == 4294967295
assert sre_constants.SRE_FLAG_IGNORECASE == 2

# ── sre_parse ──
import sre_parse
assert hasattr(sre_parse, 'parse')
assert hasattr(sre_parse, 'SubPattern')

# ── sre_compile ──
import sre_compile
assert hasattr(sre_compile, 'compile')
assert sre_compile.MAXREPEAT == 4294967295

# ── shutil archive functions ──
import shutil
assert hasattr(shutil, 'make_archive')
assert hasattr(shutil, 'unpack_archive')
assert hasattr(shutil, 'get_archive_formats')
formats = shutil.get_archive_formats()
assert len(formats) >= 3

print("phase115: all tests passed")
