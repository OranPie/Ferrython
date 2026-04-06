# Phase 104: typing_extensions, cmd module, secrets deepened
passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name, "got:", repr(got), "expected:", repr(expected))

# ── typing_extensions ──
import typing_extensions as te

# Basic re-exports from typing
check("te_any", te.Any is not None, True)
check("te_union", te.Union is not None, True)
check("te_optional", te.Optional is not None, True)
check("te_typevar", callable(te.TypeVar), True)

# Protocol
check("te_protocol", hasattr(te, 'Protocol'), True)

# Literal  
check("te_literal", hasattr(te, 'Literal'), True)

# Final
check("te_final", hasattr(te, 'Final'), True)

# ClassVar
check("te_classvar", hasattr(te, 'ClassVar'), True)

# get_type_hints, get_args, get_origin
check("te_get_type_hints", hasattr(te, 'get_type_hints'), True)
check("te_get_args", hasattr(te, 'get_args'), True)
check("te_get_origin", hasattr(te, 'get_origin'), True)

# cast
check("te_cast", callable(te.cast), True)
check("te_cast_val", te.cast(int, "42"), "42")

# overload
check("te_overload", callable(te.overload), True)

# runtime_checkable
check("te_runtime_checkable", callable(te.runtime_checkable), True)

# TYPE_CHECKING
check("te_type_checking", te.TYPE_CHECKING == False, True)

# TypeVar
T = te.TypeVar("T")
check("te_typevar_name", T.__name__, "T")

# Generic
check("te_generic", hasattr(te, 'Generic'), True)

# Named tuple
check("te_namedtuple", hasattr(te, 'NamedTuple'), True)

# ── cmd module ──
import cmd

# Cmd class structure
c = cmd.Cmd()
check("cmd_instance", isinstance(c, cmd.Cmd), True)
check("cmd_prompt", cmd.Cmd.prompt, "(Cmd) ")
check("cmd_has_parseline", hasattr(c, "parseline"), True)
check("cmd_has_precmd", hasattr(c, "precmd"), True)
check("cmd_has_postcmd", hasattr(c, "postcmd"), True)
check("cmd_has_onecmd", hasattr(c, "onecmd"), True)
check("cmd_has_emptyline", hasattr(c, "emptyline"), True)
check("cmd_has_default", hasattr(c, "default"), True)
check("cmd_has_preloop", hasattr(c, "preloop"), True)
check("cmd_has_postloop", hasattr(c, "postloop"), True)
check("cmd_has_columnize", hasattr(c, "columnize"), True)
check("cmd_has_identchars", hasattr(cmd.Cmd, "identchars"), True)
check("cmd_has_ruler", hasattr(cmd.Cmd, "ruler"), True)
check("cmd_has_doc_header", hasattr(cmd.Cmd, "doc_header"), True)

# parseline
result = c.parseline("help topic")
check("parseline_cmd", result[0], "help")
check("parseline_args", result[1], "topic")

# Empty parseline
result2 = c.parseline("")
check("parseline_empty_cmd", result2[0], None)

# ── secrets module ──
import secrets

# token_bytes
tb = secrets.token_bytes(16)
check("token_bytes_len", len(tb), 16)
check("token_bytes_type", isinstance(tb, bytes), True)

# token_hex
th = secrets.token_hex(8)
check("token_hex_len", len(th), 16)  # 8 bytes = 16 hex chars
check("token_hex_type", isinstance(th, str), True)
# All hex chars
check("token_hex_valid", all(c in '0123456789abcdef' for c in th), True)

# compare_digest
check("compare_digest_eq", secrets.compare_digest("abc", "abc"), True)
check("compare_digest_neq", secrets.compare_digest("abc", "def"), False)
check("compare_digest_len", secrets.compare_digest("abc", "abcd"), False)

# randbelow
rb = secrets.randbelow(100)
check("randbelow_range", 0 <= rb < 100, True)

# choice
ch = secrets.choice([1, 2, 3, 4, 5])
check("choice_in_list", ch in [1, 2, 3, 4, 5], True)

# ── Report ──
print("Phase 104 Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed > 0:
    raise Exception("TESTS FAILED: " + str(failed))
print("ALL PHASE 104 TESTS PASSED!")
