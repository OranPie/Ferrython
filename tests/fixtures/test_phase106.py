"""Phase 106: Tests for new modules — grp, pwd, html.entities, email.parser,
email.header, importlib.resources, asyncio sub-modules, ntpath."""

results = []
def check(name, got, expected):
    status = "PASS" if got == expected else "FAIL"
    if status == "FAIL":
        print(f"FAIL: {name} got: {repr(got)} expected: {repr(expected)}")
    results.append(status)

# ── html.entities ──
import html.entities as he

check("html_entities_amp", he.name2codepoint['amp'], 0x26)
check("html_entities_lt", he.name2codepoint['lt'], 0x3c)
check("html_entities_gt", he.name2codepoint['gt'], 0x3e)
check("html_entities_nbsp", he.name2codepoint['nbsp'], 0xa0)
check("html_entities_euro", he.name2codepoint['euro'], 0x20ac)

# Reverse mapping
check("html_codepoint2name_amp", he.codepoint2name[0x26], 'amp')

# entitydefs
check("html_entitydefs_amp", he.entitydefs['amp'], '&')
check("html_entitydefs_lt", he.entitydefs['lt'], '<')

# html5
check("html_html5_amp", he.html5['amp;'], '&')

# ── email.parser ──
import email.parser as ep

parser = ep.Parser()
msg = parser.parsestr("From: test@example.com\nTo: dest@example.com\nSubject: Hello\n\nBody text here")
check("email_parser_from", msg['From'], 'test@example.com')
check("email_parser_to", msg['To'], 'dest@example.com')
check("email_parser_subject", msg['Subject'], 'Hello')
check("email_parser_body", msg.get_payload(), 'Body text here')

# HeaderParser
hp = ep.HeaderParser()
msg2 = hp.parsestr("From: a@b.com\nSubject: Test\n\nBody ignored")
check("header_parser_from", msg2['From'], 'a@b.com')
check("header_parser_no_body", msg2.get_payload() in (None, '', 'None'), True)

# BytesParser
bp = ep.BytesParser()
msg3 = bp.parsebytes(b"From: bytes@test.com\nSubject: Bytes\n\nBinary body")
check("bytes_parser_from", msg3['From'], 'bytes@test.com')

# ── email.header ──
import email.header as eh

# Basic header
h = eh.Header('Hello World')
check("email_header_str", str(h), 'Hello World')

# decode_header on plain text
decoded = eh.decode_header('Simple text')
check("email_header_decode_plain", decoded[0][0], 'Simple text')

# make_header
h2 = eh.make_header([('Hello', None), (' World', None)])
check("email_header_make", str(h2), 'Hello  World')

# ── grp module ──
import grp
# getgrgid for root group (gid 0)
try:
    g = grp.getgrgid(0)
    check("grp_gid0_name", g.gr_name in ('root', 'wheel'), True)
    check("grp_gid0_gid", g.gr_gid, 0)
    check("grp_has_mem", hasattr(g, 'gr_mem'), True)
except Exception:
    check("grp_gid0", False, True)

# getgrnam
try:
    g2 = grp.getgrnam(g.gr_name)
    check("grp_getgrnam_gid", g2.gr_gid, 0)
except Exception:
    check("grp_getgrnam", False, True)

# getgrall returns list
all_groups = grp.getgrall()
check("grp_getgrall_type", type(all_groups).__name__, "list")
check("grp_getgrall_nonempty", len(all_groups) > 0, True)

# ── pwd module ──
import pwd

# getpwuid for root (uid 0)
try:
    p = pwd.getpwuid(0)
    check("pwd_uid0_name", p.pw_name, 'root')
    check("pwd_uid0_uid", p.pw_uid, 0)
    check("pwd_has_dir", hasattr(p, 'pw_dir'), True)
    check("pwd_has_shell", hasattr(p, 'pw_shell'), True)
except Exception:
    check("pwd_uid0", False, True)

# getpwnam
try:
    p2 = pwd.getpwnam('root')
    check("pwd_getpwnam_uid", p2.pw_uid, 0)
except Exception:
    check("pwd_getpwnam", False, True)

# getpwall
all_users = pwd.getpwall()
check("pwd_getpwall_type", type(all_users).__name__, "list")
check("pwd_getpwall_nonempty", len(all_users) > 0, True)

# ── importlib.resources ──
import importlib.resources as ir

check("ir_has_files", callable(ir.files), True)
check("ir_has_read_text", callable(ir.read_text), True)
check("ir_has_read_binary", callable(ir.read_binary), True)
check("ir_has_path", callable(ir.path), True)
check("ir_has_is_resource", callable(ir.is_resource), True)
check("ir_has_contents", callable(ir.contents), True)

# files returns an object
f = ir.files("some_package")
check("ir_files_type", type(f).__name__ != 'NoneType', True)

# ── asyncio sub-modules ──
import asyncio.events
import asyncio.tasks
import asyncio.futures
import asyncio.queues

check("asyncio_events_import", True, True)
check("asyncio_tasks_import", True, True)
check("asyncio_futures_import", True, True)
check("asyncio_queues_import", True, True)

# ── multiprocessing sub-modules ──
import multiprocessing.pool
import multiprocessing.managers

check("mp_pool_import", True, True)
check("mp_managers_import", True, True)

# ── ntpath ──
import ntpath

check("ntpath_sep", ntpath.sep, '\\')
check("ntpath_join", ntpath.join('C:\\Users', 'test'), 'C:\\Users\\test')
check("ntpath_split", ntpath.split('C:\\Users\\test')[1], 'test')
check("ntpath_splitext", ntpath.splitext('file.txt'), ('file', '.txt'))
check("ntpath_basename", ntpath.basename('C:\\Users\\test.py'), 'test.py')
check("ntpath_dirname", ntpath.dirname('C:\\Users\\test.py'), 'C:\\Users')
check("ntpath_isabs_drive", ntpath.isabs('C:\\foo'), True)
check("ntpath_isabs_rel", ntpath.isabs('foo\\bar'), False)
check("ntpath_normcase", ntpath.normcase('C:/Foo/Bar'), 'c:\\foo\\bar')
check("ntpath_splitdrive", ntpath.splitdrive('C:\\foo'), ('C:', '\\foo'))

# ── Summary ──
passed = results.count("PASS")
failed = results.count("FAIL")
print(f"\n{passed} passed, {failed} failed out of {len(results)} tests")
if failed > 0:
    raise SystemExit(1)
