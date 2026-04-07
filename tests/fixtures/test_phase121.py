# Phase 121: New pure Python modules + contextvars reset + deep module tests

# --- 1. importlib submodules ---
import importlib
import importlib.abc
import importlib.util
import importlib.machinery

# ModuleSpec
spec = importlib.util.ModuleSpec("test_mod", None, origin="/test.py")
assert spec.name == "test_mod"
assert spec.origin == "/test.py"
print("importlib.util.ModuleSpec: OK")

# importlib.machinery constants
assert ".py" in importlib.machinery.SOURCE_SUFFIXES
assert ".pyc" in importlib.machinery.BYTECODE_SUFFIXES
print("importlib.machinery: OK")

# importlib.abc classes
assert issubclass(importlib.abc.Loader, object)
assert issubclass(importlib.abc.MetaPathFinder, importlib.abc.Finder)
print("importlib.abc: OK")

# --- 2. email.policy ---
from email.policy import EmailPolicy, compat32, SMTP, default

assert compat32.max_line_length == 78
assert SMTP.cte_type == "7bit"
p = EmailPolicy(max_line_length=120, utf8=True)
assert p.max_line_length == 120
assert p.utf8 == True
print("email.policy: OK")

# --- 3. email.contentmanager ---
from email.contentmanager import ContentManager, raw_data_manager

cm = ContentManager()
assert cm is not None
print("email.contentmanager: OK")

# --- 4. concurrent.futures submodules ---
from concurrent.futures import ThreadPoolExecutor, Future
from concurrent.futures.thread import BrokenThreadPool
from concurrent.futures.process import BrokenProcessPool

# ThreadPoolExecutor
with ThreadPoolExecutor(max_workers=2) as executor:
    future = executor.submit(lambda x: x * 2, 21)
    assert future.result() == 42
print("ThreadPoolExecutor: OK")

# --- 5. contextvars reset ---
import contextvars

var = contextvars.ContextVar("test_var", default="initial")
assert var.get() == "initial"
token = var.set("changed")
assert var.get() == "changed"
var.reset(token)
assert var.get() == "initial"
print("contextvars.reset: OK")

# --- 6. _threading_local ---
import _threading_local

local = _threading_local.local()
local.x = 42
assert local.x == 42
print("_threading_local: OK")

# --- 7. operator.indexOf / countOf ---
import operator

assert operator.indexOf([10, 20, 30, 40], 30) == 2
assert operator.countOf([1, 2, 2, 3, 2, 4], 2) == 3
assert operator.countOf("banana", "a") == 3
print("operator indexOf/countOf: OK")

# --- 8. Advanced patterns combined ---
from dataclasses import dataclass, field
from typing import List, Optional

@dataclass
class Server:
    host: str
    port: int = 8080
    tags: list = field(default_factory=list)

s = Server("localhost", tags=["web", "api"])
assert s.host == "localhost"
assert s.port == 8080
assert s.tags == ["web", "api"]
print("dataclass with typing: OK")

print("All phase 121 tests passed!")
