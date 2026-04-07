"""Debugger basics.

This module provides the Bdb base class for building debuggers.
"""

import os

__all__ = ["BdbQuit", "Bdb", "Breakpoint"]

class BdbQuit(Exception):
    """Exception to give up completely."""
    pass

class Breakpoint:
    """Breakpoint class.
    
    Implements temporary breakpoints, disabled breakpoints,
    and conditional breakpoints.
    """
    next = 1   # Next bp to be assigned
    bplist = {}  # indexed by (file, line) tuples
    bpbynumber = [None]  # indexed by number

    def __init__(self, file, line, temporary=False, cond=None, funcname=None):
        self.funcname = funcname
        self.file = file
        self.line = line
        self.temporary = temporary
        self.cond = cond
        self.enabled = True
        self.ignore = 0
        self.hits = 0
        self.number = Breakpoint.next
        Breakpoint.next += 1
        self.bpbynumber.append(self)
        if (file, line) in self.bplist:
            self.bplist[file, line].append(self)
        else:
            self.bplist[file, line] = [self]

    def deleteMe(self):
        index = (self.file, self.line)
        self.bpbynumber[self.number] = None
        if index in self.bplist:
            self.bplist[index].remove(self)
            if not self.bplist[index]:
                del self.bplist[index]

    def enable(self):
        self.enabled = True

    def disable(self):
        self.enabled = False

    def bpformat(self):
        if self.temporary:
            disp = 'del  '
        else:
            disp = 'keep '
        if self.enabled:
            disp = disp + 'yes  '
        else:
            disp = disp + 'no   '
        ret = '%-4dbreakpoint   %s at %s:%d' % (self.number, disp,
                                                  self.file, self.line)
        if self.cond:
            ret += '\n\tstop only if %s' % (self.cond,)
        if self.ignore:
            ret += '\n\tignore next %d hits' % (self.ignore,)
        if self.hits:
            if self.hits > 1:
                ss = 's'
            else:
                ss = ''
            ret += '\n\tbreakpoint already hit %d time%s' % (self.hits, ss)
        return ret

    def __str__(self):
        return 'breakpoint %s at %s:%s' % (self.number, self.file, self.line)


class Bdb:
    """Generic Python debugger base class.
    
    This class takes care of details of the trace facility;
    a derived class should implement user interaction.
    """

    def __init__(self, skip=None):
        self.skip = set(skip) if skip else None
        self.breaks = {}
        self.fncache = {}
        self.frame_returning = None
        self._load_breakpoints()

    def _load_breakpoints(self):
        pass

    def canonic(self, filename):
        if filename in self.fncache:
            return self.fncache[filename]
        canonic = os.path.abspath(filename)
        canonic = os.path.normcase(canonic)
        self.fncache[filename] = canonic
        return canonic

    def reset(self):
        import linecache
        linecache.checkcache()
        self.botframe = None
        self._set_stopinfo(None, None)

    def _set_stopinfo(self, stopframe, returnframe, stoplineno=-1):
        self.stopframe = stopframe
        self.returnframe = returnframe
        self.quitting = False
        self.stoplineno = stoplineno

    def set_break(self, filename, lineno, temporary=False, cond=None, funcname=None):
        filename = self.canonic(filename)
        if filename not in self.breaks:
            self.breaks[filename] = []
        if lineno not in self.breaks[filename]:
            self.breaks[filename].append(lineno)
        bp = Breakpoint(filename, lineno, temporary, cond, funcname)
        return bp

    def clear_break(self, filename, lineno):
        filename = self.canonic(filename)
        if filename not in self.breaks:
            return 'There are no breakpoints in %s' % filename
        if lineno not in self.breaks[filename]:
            return 'There is no breakpoint at %s:%d' % (filename, lineno)
        for bp in Breakpoint.bplist.get((filename, lineno), []):
            bp.deleteMe()
        if (filename, lineno) in Breakpoint.bplist:
            del Breakpoint.bplist[filename, lineno]
        if lineno in self.breaks[filename]:
            self.breaks[filename].remove(lineno)
        if not self.breaks[filename]:
            del self.breaks[filename]

    def clear_all_breaks(self):
        for bp_list in list(Breakpoint.bplist.values()):
            for bp in bp_list:
                bp.deleteMe()
        Breakpoint.bplist.clear()
        self.breaks.clear()

    def get_breaks(self, filename, lineno):
        filename = self.canonic(filename)
        return (filename, lineno) in Breakpoint.bplist

    def get_file_breaks(self, filename):
        filename = self.canonic(filename)
        return self.breaks.get(filename, [])

    def get_all_breaks(self):
        return self.breaks

    def set_continue(self):
        self._set_stopinfo(self.botframe, None, -1)

    def set_quit(self):
        self.stopframe = None
        self.returnframe = None
        self.quitting = True

    def set_step(self):
        self._set_stopinfo(None, None)

    def set_next(self, frame):
        self._set_stopinfo(frame, None)

    def set_return(self, frame):
        self._set_stopinfo(frame.f_back, frame)

    def user_call(self, frame, argument_list):
        pass

    def user_line(self, frame):
        pass

    def user_return(self, frame, return_value):
        pass

    def user_exception(self, frame, exc_info):
        pass

    def run(self, cmd, globals=None, locals=None):
        if globals is None:
            import __main__
            globals = __main__.__dict__
        if locals is None:
            locals = globals
        self.reset()
        if isinstance(cmd, str):
            cmd = compile(cmd, "<string>", "exec")
        try:
            exec(cmd, globals, locals)
        except BdbQuit:
            pass
        finally:
            self.quitting = True

    def runeval(self, expr, globals=None, locals=None):
        if globals is None:
            import __main__
            globals = __main__.__dict__
        if locals is None:
            locals = globals
        self.reset()
        try:
            return eval(expr, globals, locals)
        except BdbQuit:
            pass
        finally:
            self.quitting = True
