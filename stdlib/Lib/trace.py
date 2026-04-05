"""Program/module tracing and coverage.

Simplified implementation of the standard library trace module.
"""


class CoverageResults:
    """Container for coverage data."""

    def __init__(self, counts=None, calledfuncs=None, callers=None):
        self.counts = counts if counts is not None else {}
        self.calledfuncs = calledfuncs if calledfuncs is not None else {}
        self.callers = callers if callers is not None else {}

    def update(self, other):
        """Merge another CoverageResults into this one."""
        if isinstance(other, CoverageResults):
            for key, count in other.counts.items():
                self.counts[key] = self.counts.get(key, 0) + count
            self.calledfuncs.update(other.calledfuncs)
            self.callers.update(other.callers)

    def write_results(self, show_missing=True, summary=False, coverdir=None):
        """Write coverage results to stdout."""
        if summary:
            print("lines   cov%   module")
        files = {}
        for (filename, lineno), count in self.counts.items():
            if filename not in files:
                files[filename] = {}
            files[filename][lineno] = count
        for filename in sorted(files):
            line_counts = files[filename]
            if summary:
                total = len(line_counts)
                covered = sum(1 for c in line_counts.values() if c > 0)
                pct = (covered * 100 // total) if total > 0 else 0
                print("  {:>5d}  {:>3d}%   {}".format(total, pct, filename))

    def write_results_file(self, path, lines, lnotab, lines_hit):
        """Write a single coverage file."""
        result = []
        for i, line in enumerate(lines, 1):
            if i in lines_hit:
                result.append("{:>5d}: {}".format(lines_hit[i], line))
            else:
                result.append("       {}".format(line))
        return result


class Trace:
    """Trace program execution, generate annotated listings and coverage."""

    def __init__(self, count=1, trace=1, countfuncs=0, countcallers=0,
                 ignoredirs=None, ignoremods=None, infile=None,
                 outfile=None):
        self.count = count
        self.trace = trace
        self.countfuncs = countfuncs
        self.countcallers = countcallers
        self.ignoredirs = ignoredirs or []
        self.ignoremods = ignoremods or []
        self.infile = infile
        self.outfile = outfile
        self._counts = {}
        self._calledfuncs = {}
        self._callers = {}

    def run(self, cmd):
        """Run the command under tracing."""
        import sys
        if isinstance(cmd, str):
            code = compile(cmd, '<string>', 'exec')
        else:
            code = cmd
        globls = {'__name__': '__main__', '__builtins__': __builtins__}
        if self.trace or self.count:
            sys.settrace(self._globaltrace)
        try:
            exec(code, globls)
        finally:
            if hasattr(sys, 'settrace'):
                sys.settrace(None)

    def runfunc(self, func, *args, **kwargs):
        """Run func under tracing, returning its result."""
        import sys
        result = None
        if self.trace or self.count:
            if hasattr(sys, 'settrace'):
                sys.settrace(self._globaltrace)
        try:
            result = func(*args, **kwargs)
        finally:
            if hasattr(sys, 'settrace'):
                sys.settrace(None)
        return result

    def _globaltrace(self, frame, why, arg):
        if why == 'call':
            return self._localtrace
        return None

    def _localtrace(self, frame, why, arg):
        if why == 'line':
            filename = frame.f_code.co_filename
            lineno = frame.f_lineno
            if self.count:
                key = (filename, lineno)
                self._counts[key] = self._counts.get(key, 0) + 1
        return self._localtrace

    def results(self):
        """Return a CoverageResults with the current data."""
        return CoverageResults(
            dict(self._counts),
            dict(self._calledfuncs),
            dict(self._callers)
        )
