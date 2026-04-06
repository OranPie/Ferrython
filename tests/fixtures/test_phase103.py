# Phase 103: Toolchain, deepened gettext, linecache, codeop, sched, pdb
passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name, "got:", repr(got), "expected:", repr(expected))

# ── gettext deepened ──
import gettext

# Basic functions
check("gettext_identity", gettext.gettext("Hello"), "Hello")
check("ngettext_sing", gettext.ngettext("cat", "cats", 1), "cat")
check("ngettext_plur", gettext.ngettext("cat", "cats", 5), "cats")
check("pgettext", gettext.pgettext("menu", "Open"), "Open")
check("npgettext_sing", gettext.npgettext("menu", "file", "files", 1), "file")
check("npgettext_plur", gettext.npgettext("menu", "file", "files", 3), "files")

# Domain functions
check("dgettext", gettext.dgettext("test", "hello"), "hello")
check("dngettext", gettext.dngettext("test", "a", "b", 2), "b")

# NullTranslations class
t = gettext.NullTranslations()
check("null_gettext", t.gettext("test"), "test")
check("null_ngettext", t.ngettext("item", "items", 2), "items")
check("null_pgettext", t.pgettext("ctx", "msg"), "msg")
check("null_npgettext", t.npgettext("ctx", "a", "b", 1), "a")
check("null_info", t.info(), {})
check("null_charset", t.charset(), None)

# Fallback
t2 = gettext.NullTranslations()
t.add_fallback(t2)
check("null_fallback", t.gettext("x"), "x")

# translation() with fallback
t3 = gettext.translation("nonexistent", fallback=True)
check("trans_fallback_type", isinstance(t3, gettext.NullTranslations), True)
check("trans_fallback_gettext", t3.gettext("hi"), "hi")

# textdomain
old = gettext.textdomain()
check("textdomain_default", old, "messages")
gettext.textdomain("myapp")
check("textdomain_set", gettext.textdomain(), "myapp")
gettext.textdomain("messages")  # restore

# bindtextdomain
path = gettext.bindtextdomain("myapp", "/tmp/locale")
check("bindtextdomain", path, "/tmp/locale")

# _ alias
check("underscore_alias", gettext._("test"), "test")

# ── linecache deepened ──
import linecache
import os
import tempfile

# Create a temp file to test with
tmpdir = tempfile.gettempdir()
testfile = os.path.join(tmpdir, "_ferrython_linecache_test.py")
with open(testfile, 'w') as f:
    f.write("line one\nline two\nline three\n")

lines = linecache.getlines(testfile)
check("linecache_len", len(lines), 3)
check("linecache_line1", linecache.getline(testfile, 1).strip(), "line one")
check("linecache_line2", linecache.getline(testfile, 2).strip(), "line two")
check("linecache_line3", linecache.getline(testfile, 3).strip(), "line three")
check("linecache_oob", linecache.getline(testfile, 99), "")

# checkcache
linecache.checkcache(testfile)
# After check, may reload — should still work
lines2 = linecache.getlines(testfile)
check("linecache_after_check", len(lines2), 3)

# clearcache
linecache.clearcache()

# cleanup
os.remove(testfile)

# ── codeop deepened ──
import codeop

# Complete statements should return code object
code = codeop.compile_command("x = 1")
check("codeop_complete", code is not None, True)

code2 = codeop.compile_command("print('hello')")
check("codeop_print", code2 is not None, True)

# Compile class
compiler = codeop.Compile()
check("compile_class_type", isinstance(compiler, codeop.Compile), True)
code3 = compiler("1+1", "<test>", "eval")
check("compile_class_eval", code3 is not None, True)

# CommandCompiler class
cc = codeop.CommandCompiler()
check("cmd_compiler_type", isinstance(cc, codeop.CommandCompiler), True)
code4 = cc("x = 42")
check("cmd_compiler_complete", code4 is not None, True)

# ── pdb basic structure ──
import pdb

# Module has key functions
check("pdb_has_set_trace", callable(pdb.set_trace), True)
check("pdb_has_run", callable(pdb.run), True)
check("pdb_has_runeval", callable(pdb.runeval), True)
check("pdb_has_runcall", callable(pdb.runcall), True)
check("pdb_has_post_mortem", callable(pdb.post_mortem), True)
check("pdb_has_pm", callable(pdb.pm), True)

# Pdb class
p = pdb.Pdb()
check("pdb_instance", isinstance(p, pdb.Pdb), True)
check("pdb_prompt", pdb.Pdb.prompt, "(Pdb) ")

# Bdb class
check("pdb_has_bdb", hasattr(pdb, 'Bdb'), True)
check("pdb_has_breakpoint", hasattr(pdb, 'Breakpoint'), True)

# Breakpoint class
check("breakpoint_class_exists", callable(pdb.Breakpoint), True)

# ── sched module ──
import sched

s = sched.scheduler()
check("sched_empty", s.empty(), True)

import time

# Test enter and queue
e1 = s.enterabs(time.time() + 100, 1, lambda: None, ())
check("sched_not_empty", s.empty(), False)

# Cancel
s.cancel(e1)
check("sched_after_cancel", s.empty(), True)

# Queue property
s3 = sched.scheduler()
s3.enterabs(time.time() + 100, 2, lambda: None, ())
s3.enterabs(time.time() + 50, 1, lambda: None, ())
q = s3.queue()
check("sched_queue_len", len(q), 2)
# Earlier time should be first
check("sched_queue_ordered", q[0].priority == 1 or q[0].time < q[1].time, True)

# Run non-blocking (should not block since events are in the future)
s3.run(False)

# enter with delay
s4 = sched.scheduler()
e2 = s4.enter(1000, 1, lambda: None, ())
check("sched_enter_delay", s4.empty(), False)

# Event attributes
check("sched_event_has_time", hasattr(e2, 'time'), True)
check("sched_event_has_priority", hasattr(e2, 'priority'), True)
check("sched_event_has_action", hasattr(e2, 'action'), True)

# Event class
check("sched_has_event", hasattr(sched, 'Event'), True)

# ── Report ──
print("Phase 103 Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed > 0:
    raise Exception("TESTS FAILED: " + str(failed))
print("ALL PHASE 103 TESTS PASSED!")
