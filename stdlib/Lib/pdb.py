"""Python debugger for Ferrython.

Provides basic interactive debugging with breakpoints, stepping,
stack inspection, and variable examination.

Usage:
    import pdb; pdb.set_trace()   # Set a breakpoint
    python -m pdb script.py       # Run a script under debugger
"""

import sys
import os
import linecache
import cmd
import traceback as _traceback

__all__ = ['Pdb', 'set_trace', 'run', 'runeval', 'runcall',
           'post_mortem', 'pm']


class Breakpoint:
    """Represents a single breakpoint."""

    next_number = 1
    bpbynumber = [None]  # 1-indexed
    bplist = {}  # {(file, line): [Breakpoint, ...]}

    def __init__(self, file, line, temporary=False, cond=None, funcname=None):
        self.file = file
        self.line = line
        self.temporary = temporary
        self.cond = cond
        self.funcname = funcname
        self.enabled = True
        self.hits = 0
        self.number = Breakpoint.next_number
        Breakpoint.next_number += 1
        Breakpoint.bpbynumber.append(self)
        if (file, line) in Breakpoint.bplist:
            Breakpoint.bplist[file, line].append(self)
        else:
            Breakpoint.bplist[file, line] = [self]

    def deleteMe(self):
        """Remove this breakpoint from all bookkeeping."""
        index = (self.file, self.line)
        Breakpoint.bpbynumber[self.number] = None
        if index in Breakpoint.bplist:
            self.bplist[index].remove(self)
            if not self.bplist[index]:
                del self.bplist[index]

    def enable(self):
        self.enabled = True

    def disable(self):
        self.enabled = False

    def __str__(self):
        disp = 'yes' if self.enabled else 'no'
        ret = f'Breakpoint {self.number} at {self.file}:{self.line}'
        if self.cond:
            ret += f'  cond: {self.cond}'
        if not self.enabled:
            ret += '  (disabled)'
        return ret

    @staticmethod
    def clearBreakpoints():
        Breakpoint.next_number = 1
        Breakpoint.bpbynumber = [None]
        Breakpoint.bplist = {}


class Bdb:
    """Base debugger class providing stepping and breakpoint logic.

    This works through sys.settrace when available. When settrace is not
    supported, provides a simplified breakpoint-checking model.
    """

    def __init__(self, skip=None):
        self.skip = set(skip) if skip else set()
        self.breaks = {}  # {filename: [lineno, ...]}
        self.fncache = {}
        self.frame = None
        self.botframe = None
        self.quitting = False
        self.stopframe = None
        self.returnframe = None
        self.stoplineno = -1

    def canonic(self, filename):
        """Return canonical form of filename."""
        if filename in self.fncache:
            return self.fncache[filename]
        canonic = os.path.abspath(filename)
        self.fncache[filename] = canonic
        return canonic

    def reset(self):
        """Reset debugger state."""
        self.botframe = None
        self.stopframe = None
        self.returnframe = None
        self.quitting = False
        self.stoplineno = -1

    def set_break(self, filename, lineno, temporary=False, cond=None, funcname=None):
        """Set a new breakpoint."""
        filename = self.canonic(filename)
        if filename not in self.breaks:
            self.breaks[filename] = []
        if lineno not in self.breaks[filename]:
            self.breaks[filename].append(lineno)
        bp = Breakpoint(filename, lineno, temporary, cond, funcname)
        return bp

    def clear_break(self, filename, lineno):
        """Delete breakpoints at filename:lineno."""
        filename = self.canonic(filename)
        if filename not in self.breaks or lineno not in self.breaks[filename]:
            return f'There is no breakpoint at {filename}:{lineno}'
        # Remove from breaks list
        self.breaks[filename].remove(lineno)
        if not self.breaks[filename]:
            del self.breaks[filename]
        # Remove Breakpoint objects
        bplist = Breakpoint.bplist.get((filename, lineno), [])
        for bp in bplist[:]:
            bp.deleteMe()
        return None

    def clear_all_breaks(self):
        self.breaks = {}
        Breakpoint.clearBreakpoints()

    def get_breaks(self, filename, lineno):
        """Return list of breakpoints at filename:lineno."""
        filename = self.canonic(filename)
        return Breakpoint.bplist.get((filename, lineno), [])

    def get_all_breaks(self):
        return self.breaks

    def do_clear(self, arg):
        """Clear breakpoint(s)."""
        pass

    def set_step(self):
        """Stop after one line of code."""
        self.stopframe = None
        self.returnframe = None
        self.quitting = False
        self.stoplineno = -1

    def set_next(self, frame):
        """Stop on the next line in the current frame or above."""
        self.stopframe = frame
        self.returnframe = None
        self.stoplineno = -1

    def set_return(self, frame):
        """Stop when returning from the current frame."""
        self.stopframe = frame
        self.returnframe = frame
        self.stoplineno = -1

    def set_continue(self):
        """Continue execution, stop at the next breakpoint."""
        self.stopframe = self.botframe
        self.returnframe = None
        self.quitting = False

    def set_quit(self):
        """Set quitting debugger."""
        self.stopframe = self.botframe
        self.returnframe = None
        self.quitting = True

    def user_call(self, frame, argument_list):
        """Called when there is the remote possibility of a breakpoint."""
        pass

    def user_line(self, frame):
        """Called when we stop or break at a line."""
        pass

    def user_return(self, frame, return_value):
        """Called when a return trap is set here."""
        pass

    def user_exception(self, frame, exc_info):
        """Called when we stop on an exception."""
        pass


class Pdb(Bdb, cmd.Cmd):
    """The Python Debugger — interactive source-level debugger.

    Provides commands for setting breakpoints, stepping through code,
    examining the stack, and evaluating expressions.
    """

    prompt = '(Pdb) '
    _previous_sigint_handler = None

    def __init__(self, completekey='tab', stdin=None, stdout=None, skip=None, nosigint=False):
        Bdb.__init__(self, skip=skip)
        cmd.Cmd.__init__(self, completekey, stdin, stdout)
        if stdout:
            self.stdout = stdout
        else:
            self.stdout = sys.stdout
        self.nosigint = nosigint
        self.commands = {}
        self.commands_bnum = None
        self.lineno = None
        self.stack = []
        self.curindex = 0
        self.curframe = None
        self.curframe_locals = {}

    def message(self, msg):
        print(msg, file=self.stdout)

    def error(self, msg):
        print('***', msg, file=self.stdout)

    def _format_stack_entry(self, frame_info, lprefix=': '):
        """Format a stack entry for display."""
        if isinstance(frame_info, tuple) and len(frame_info) >= 2:
            filename, lineno = frame_info[0], frame_info[1]
            s = f'  {filename}({lineno})'
            line = linecache.getline(filename, lineno)
            if line:
                s += lprefix + line.strip()
            return s
        return str(frame_info)

    def print_stack_trace(self):
        """Print a stack trace."""
        try:
            for i, entry in enumerate(self.stack):
                if i == self.curindex:
                    marker = '>'
                else:
                    marker = ' '
                self.message(f'{marker} {self._format_stack_entry(entry)}')
        except Exception:
            self.message('*** Unable to print stack trace')

    # ─── Debugger commands ───

    def do_break(self, arg):
        """b(reak) [([filename:]lineno | function) [, condition]]
        Set a breakpoint.
        """
        if not arg:
            # List breakpoints
            if not Breakpoint.bpbynumber:
                self.message('No breakpoints.')
                return
            for bp in Breakpoint.bpbynumber:
                if bp is not None:
                    self.message(str(bp))
            return

        cond = None
        if ',' in arg:
            arg, cond = arg.split(',', 1)
            cond = cond.strip()

        arg = arg.strip()
        if ':' in arg:
            filename, lineno = arg.rsplit(':', 1)
            try:
                lineno = int(lineno)
            except ValueError:
                self.error(f'Bad line number: {lineno}')
                return
        else:
            try:
                lineno = int(arg)
                filename = self.curframe and getattr(self.curframe, 'f_code', None)
                if filename and hasattr(filename, 'co_filename'):
                    filename = filename.co_filename
                else:
                    filename = '<stdin>'
            except ValueError:
                self.error(f'Cannot resolve: {arg}')
                return

        bp = self.set_break(filename, lineno, cond=cond)
        self.message(f'Breakpoint {bp.number} at {filename}:{lineno}')

    do_b = do_break

    def do_clear(self, arg):
        """cl(ear) [bpnumber [bpnumber ...]]
        Clear breakpoint(s).
        """
        if not arg:
            self.clear_all_breaks()
            self.message('All breakpoints cleared.')
            return
        for num_str in arg.split():
            try:
                num = int(num_str)
                if 0 < num < len(Breakpoint.bpbynumber):
                    bp = Breakpoint.bpbynumber[num]
                    if bp:
                        self.clear_break(bp.file, bp.line)
                        self.message(f'Deleted breakpoint {num}')
                    else:
                        self.error(f'Breakpoint {num} already deleted')
                else:
                    self.error(f'No breakpoint number {num}')
            except ValueError:
                self.error(f'Non-numeric breakpoint number: {num_str}')

    do_cl = do_clear

    def do_step(self, arg):
        """s(tep)
        Execute the current line, stop at the first possible occasion.
        """
        self.set_step()
        return True  # Exit cmdloop

    do_s = do_step

    def do_next(self, arg):
        """n(ext)
        Continue execution until the next line in the current function.
        """
        if self.curframe:
            self.set_next(self.curframe)
        return True

    do_n = do_next

    def do_return(self, arg):
        """r(eturn)
        Continue execution until the current function returns.
        """
        if self.curframe:
            self.set_return(self.curframe)
        return True

    do_r = do_return

    def do_continue(self, arg):
        """c(ont(inue))
        Continue execution until a breakpoint is encountered.
        """
        self.set_continue()
        return True

    do_c = do_continue
    do_cont = do_continue

    def do_quit(self, arg):
        """q(uit)
        Quit the debugger.
        """
        self.set_quit()
        return True

    do_q = do_quit
    do_exit = do_quit

    def do_print(self, arg):
        """p expression
        Print the value of the expression.
        """
        try:
            val = eval(arg, self.curframe_locals if self.curframe_locals else {})
            self.message(repr(val))
        except Exception:
            exc_info = sys.exc_info()
            self.error(f'{exc_info[0].__name__}: {exc_info[1]}')

    do_p = do_print

    def do_pp(self, arg):
        """pp expression
        Pretty-print the value of the expression.
        """
        try:
            import pprint
            val = eval(arg, self.curframe_locals if self.curframe_locals else {})
            self.message(pprint.pformat(val))
        except Exception:
            exc_info = sys.exc_info()
            self.error(f'{exc_info[0].__name__}: {exc_info[1]}')

    def do_list(self, arg):
        """l(ist) [first[, last]]
        List source code for the current file.
        """
        filename = '<stdin>'
        if self.curframe and hasattr(self.curframe, 'f_code'):
            filename = getattr(self.curframe.f_code, 'co_filename', '<stdin>')

        lineno = self.lineno or 1
        if arg:
            parts = arg.split(',')
            try:
                first = int(parts[0].strip())
                if len(parts) > 1:
                    last = int(parts[1].strip())
                else:
                    last = first + 10
            except ValueError:
                self.error('Invalid line range')
                return
        else:
            first = max(1, lineno - 5)
            last = first + 10

        lines = linecache.getlines(filename)
        if not lines:
            self.error(f'Could not get source for {filename}')
            return

        for i in range(first - 1, min(last, len(lines))):
            num = i + 1
            marker = '->' if num == lineno else '  '
            bp_marker = 'B' if (filename, num) in Breakpoint.bplist else ' '
            self.message(f'{num:4d}{marker}{bp_marker} {lines[i].rstrip()}')

        self.lineno = min(last + 1, len(lines))

    do_l = do_list

    def do_where(self, arg):
        """w(here)
        Print the stack trace.
        """
        self.print_stack_trace()

    do_w = do_where
    do_bt = do_where

    def do_up(self, arg):
        """u(p) [count]
        Move up the stack by count (default 1).
        """
        count = 1
        if arg:
            try:
                count = int(arg)
            except ValueError:
                self.error('Invalid count')
                return
        new_index = max(0, self.curindex - count)
        if new_index != self.curindex:
            self.curindex = new_index
            self.message(f'> {self._format_stack_entry(self.stack[self.curindex])}')

    do_u = do_up

    def do_down(self, arg):
        """d(own) [count]
        Move down the stack by count (default 1).
        """
        count = 1
        if arg:
            try:
                count = int(arg)
            except ValueError:
                self.error('Invalid count')
                return
        new_index = min(len(self.stack) - 1, self.curindex + count)
        if new_index != self.curindex:
            self.curindex = new_index
            self.message(f'> {self._format_stack_entry(self.stack[self.curindex])}')

    do_d = do_down

    def do_args(self, arg):
        """a(rgs)
        Print the arguments of the current function.
        """
        if self.curframe_locals:
            for key, val in self.curframe_locals.items():
                self.message(f'{key} = {val!r}')
        else:
            self.message('No frame locals available')

    do_a = do_args

    def do_help(self, arg):
        """h(elp) [command]
        Show help for a command.
        """
        if not arg:
            self.message('Documented commands (type help <topic>):')
            self.message('=' * 50)
            cmds = ['break', 'clear', 'continue', 'step', 'next', 'return',
                    'quit', 'print', 'pp', 'list', 'where', 'up', 'down',
                    'args', 'help']
            self.message('  '.join(cmds))
            return
        try:
            func = getattr(self, 'do_' + arg)
            if func.__doc__:
                self.message(func.__doc__)
            else:
                self.message(f'No help for {arg}')
        except AttributeError:
            self.message(f'*** Unknown command: {arg}')

    do_h = do_help

    def interaction(self, frame, tb):
        """Begin a debugger interaction session."""
        self.curframe = frame
        if frame and hasattr(frame, 'f_locals'):
            self.curframe_locals = frame.f_locals
        else:
            self.curframe_locals = {}
        if frame and hasattr(frame, 'f_lineno'):
            self.lineno = frame.f_lineno
        self.cmdloop()

    def default(self, line):
        """Handle unknown commands — evaluate as Python expression."""
        if line.startswith('!'):
            line = line[1:]
        try:
            val = eval(line, self.curframe_locals)
            self.message(repr(val))
        except Exception:
            try:
                exec(line, self.curframe_locals)
            except Exception:
                exc_info = sys.exc_info()
                self.error(f'{exc_info[0].__name__}: {exc_info[1]}')

    def precmd(self, line):
        """Allow command shortcuts."""
        return line

    def set_trace(self, frame=None):
        """Start debugging from the calling frame."""
        if frame is None:
            frame = sys._getframe().f_back if hasattr(sys, '_getframe') else None
        self.reset()
        self.interaction(frame, None)

    def _runscript(self, filename):
        """Debug a script."""
        import __main__
        __main__.__dict__.clear()
        __main__.__dict__.update({
            '__name__': '__main__',
            '__file__': filename,
            '__builtins__': __builtins__,
        })
        self.mainpyfile = self.canonic(filename)
        with open(filename, 'r') as f:
            statement = f.read()
        self.run(statement, __main__.__dict__)


# ─── Module-level convenience functions ───

_debugger = None

def _get_debugger():
    global _debugger
    if _debugger is None:
        _debugger = Pdb()
    return _debugger


def set_trace(*, header=None):
    """Enter the debugger at the calling stack frame.

    This is useful to hard-code a breakpoint at a given point in a
    program, even if the code is not otherwise being debugged.
    """
    pdb = _get_debugger()
    if header:
        pdb.message(header)
    pdb.set_trace(sys._getframe().f_back if hasattr(sys, '_getframe') else None)


def run(statement, globals=None, locals=None):
    """Execute a statement under debugger control."""
    pdb = Pdb()
    pdb.reset()
    if globals is None:
        import __main__
        globals = __main__.__dict__
    if locals is None:
        locals = globals
    pdb.message(f"Running: {statement!r}")
    try:
        exec(statement, globals, locals)
    except Exception:
        _traceback.print_exc()
        pdb.interaction(None, sys.exc_info()[2])


def runeval(expression, globals=None, locals=None):
    """Evaluate an expression under debugger control."""
    pdb = Pdb()
    pdb.reset()
    if globals is None:
        import __main__
        globals = __main__.__dict__
    if locals is None:
        locals = globals
    try:
        return eval(expression, globals, locals)
    except Exception:
        _traceback.print_exc()
        pdb.interaction(None, sys.exc_info()[2])


def runcall(func, *args, **kwargs):
    """Call a function under debugger control."""
    pdb = Pdb()
    pdb.reset()
    try:
        return func(*args, **kwargs)
    except Exception:
        _traceback.print_exc()
        pdb.interaction(None, sys.exc_info()[2])


def post_mortem(t=None):
    """Enter post-mortem debugging of the given traceback object.

    If no traceback is given, uses the current exception's traceback.
    """
    if t is None:
        t = sys.exc_info()[2]
        if t is None:
            raise ValueError("A valid traceback must be passed if no "
                           "exception is being handled")
    pdb = Pdb()
    pdb.reset()
    pdb.interaction(None, t)


def pm():
    """Enter post-mortem debugging of the traceback found in sys.last_traceback."""
    if hasattr(sys, 'last_traceback') and sys.last_traceback:
        post_mortem(sys.last_traceback)
    else:
        print("*** No traceback found. Did an exception occur?")


# ─── Main entry point for `python -m pdb` ───

def main():
    """Run a script under debugger control."""
    if len(sys.argv) < 2:
        print("usage: ferrython -m pdb scriptfile [arg] ...")
        sys.exit(2)

    mainpyfile = sys.argv[1]
    if not os.path.exists(mainpyfile):
        print(f'Error: {mainpyfile} does not exist')
        sys.exit(1)

    del sys.argv[0]  # Remove 'pdb' from sys.argv

    pdb = Pdb()
    try:
        pdb._runscript(mainpyfile)
    except SystemExit:
        print("The program exited via sys.exit(). Exit status:", end=' ')
        print(sys.exc_info()[1])
    except Exception:
        _traceback.print_exc()
        print("Uncaught exception. Entering post mortem debugging")
        pdb.interaction(None, sys.exc_info()[2])
        print("Post mortem debugger finished.")


if __name__ == '__main__':
    main()
