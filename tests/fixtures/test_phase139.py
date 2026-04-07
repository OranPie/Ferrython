# Phase 139: ABC.register() isinstance/issubclass, misc_modules stubs
from abc import ABC

# ABC.register() with isinstance
class Printable(ABC):
    pass

class MyDoc:
    pass

Printable.register(MyDoc)
assert isinstance(MyDoc(), Printable), "isinstance after register"
assert issubclass(MyDoc, Printable), "issubclass after register"

# readline history
import readline
readline.add_history("cmd1")
readline.add_history("cmd2")
assert readline.get_current_history_length() == 2
assert readline.get_history_item(1) == "cmd1"
readline.clear_history()
assert readline.get_current_history_length() == 0

# ctypes basics
import ctypes
c = ctypes.c_int(42)
assert c.value == 42

print("phase139: all checks passed")
