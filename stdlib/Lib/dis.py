"""Pure-Python dis module stub.

The real disassembly is provided by the Rust-implemented dis module.
This file exists as a fallback.
"""

class Instruction:
    def __init__(self, opname='', opcode=0, arg=None, argval=None, offset=0, starts_line=None):
        self.opname = opname
        self.opcode = opcode
        self.arg = arg
        self.argval = argval
        self.offset = offset
        self.starts_line = starts_line

    def __repr__(self):
        return f"Instruction(opname={self.opname!r}, opcode={self.opcode}, arg={self.arg})"


class Bytecode:
    def __init__(self, x):
        self._obj = x

    def __iter__(self):
        return iter([])

    def __repr__(self):
        return f"<Bytecode object at 0x0>"


def dis(x=None):
    """Disassemble a code object, function, or module.

    Delegates to the Rust dis implementation when available.
    """
    try:
        import _dis
        return _dis.dis(x)
    except (ImportError, AttributeError):
        pass
    if x is None:
        return
    print(f"<dis: cannot disassemble {type(x).__name__} object>")


def disassemble(x):
    return dis(x)


def distb(tb=None):
    pass


def code_info(x):
    return "<code info not available>"
