"""doctest — Test interactive Python examples in docstrings.

Provides testmod() to find and run all docstring examples in a module,
and run_docstring_examples() for individual objects.
"""

__all__ = [
    'testmod', 'run_docstring_examples', 'DocTestRunner', 'DocTestFinder',
    'DocTest', 'Example', 'TestResults',
    'ELLIPSIS', 'NORMALIZE_WHITESPACE', 'IGNORE_EXCEPTION_DETAIL',
]

# Option flags
OPTIONFLAGS = 0
ELLIPSIS = 8
NORMALIZE_WHITESPACE = 2
IGNORE_EXCEPTION_DETAIL = 4
DONT_ACCEPT_TRUE_FOR_1 = 16
DONT_ACCEPT_BLANKLINE = 32
SKIP = 64


class TestResults:
    """Results of a doctest run."""
    def __init__(self, failed=0, attempted=0):
        self.failed = failed
        self.attempted = attempted

    def __repr__(self):
        return "TestResults(failed=%d, attempted=%d)" % (self.failed, self.attempted)


class Example:
    """A single interactive example from a docstring."""
    def __init__(self, source, want, lineno=0, indent=0, options=None):
        self.source = source
        self.want = want
        self.lineno = lineno
        self.indent = indent
        self.options = options or {}


class DocTest:
    """A collection of examples extracted from a docstring."""
    def __init__(self, examples, globs, name, filename, lineno, docstring):
        self.examples = examples
        self.globs = globs
        self.name = name
        self.filename = filename
        self.lineno = lineno
        self.docstring = docstring


def _extract_examples(docstring):
    """Extract Example objects from a docstring."""
    if not docstring:
        return []
    examples = []
    lines = docstring.split('\n')
    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.lstrip()
        if stripped.startswith('>>> '):
            # Collect the source
            indent = len(line) - len(stripped)
            source_lines = [stripped[4:]]
            i += 1
            # Collect continuation lines
            while i < len(lines):
                cont = lines[i]
                cont_stripped = cont.lstrip()
                if cont_stripped.startswith('... '):
                    source_lines.append(cont_stripped[4:])
                    i += 1
                else:
                    break
            source = '\n'.join(source_lines)
            # Collect expected output
            want_lines = []
            while i < len(lines):
                out = lines[i]
                out_stripped = out.lstrip()
                if out_stripped.startswith('>>> ') or out_stripped.startswith('... '):
                    break
                if out_stripped == '' and i + 1 < len(lines):
                    next_stripped = lines[i + 1].lstrip()
                    if next_stripped.startswith('>>> ') or next_stripped == '':
                        break
                if out_stripped == '' and i + 1 >= len(lines):
                    break
                want_lines.append(out[indent:] if len(out) > indent else out)
                i += 1
            want = '\n'.join(want_lines)
            if want:
                want += '\n'
            examples.append(Example(source, want, lineno=i, indent=indent))
        else:
            i += 1
    return examples


class DocTestFinder:
    """Find DocTests in a module or object."""

    def find(self, obj, name=None, globs=None, extraglobs=None):
        if name is None:
            name = getattr(obj, '__name__', str(obj))
        if globs is None:
            globs = getattr(obj, '__dict__', {}).copy() if hasattr(obj, '__dict__') else {}
        if extraglobs:
            globs.update(extraglobs)

        tests = []
        # Check the object's own docstring
        doc = getattr(obj, '__doc__', None)
        if doc:
            examples = _extract_examples(doc)
            if examples:
                tests.append(DocTest(examples, globs, name, '<doctest>', 0, doc))

        # Check functions/methods in the object
        for attr_name in dir(obj):
            try:
                attr = getattr(obj, attr_name)
            except Exception:
                continue
            if attr_name.startswith('_'):
                continue
            doc = getattr(attr, '__doc__', None)
            if doc:
                examples = _extract_examples(doc)
                if examples:
                    full_name = "%s.%s" % (name, attr_name)
                    tests.append(DocTest(examples, globs, full_name, '<doctest>', 0, doc))
        return tests


class DocTestRunner:
    """Run DocTest instances and report results."""

    def __init__(self, verbose=False, optionflags=0):
        self.verbose = verbose
        self.optionflags = optionflags
        self._attempted = 0
        self._failed = 0

    def run(self, test, out=None):
        """Run the examples in a DocTest."""
        import sys
        import io

        for example in test.examples:
            self._attempted += 1
            # Capture stdout
            old_stdout = sys.stdout
            sys.stdout = io.StringIO()
            try:
                # Try as expression first, then as statement
                try:
                    result = eval(example.source, test.globs)
                    if result is not None:
                        print(repr(result))
                except SyntaxError:
                    exec(example.source, test.globs)
                got = sys.stdout.getvalue()
            except Exception as e:
                got = "Traceback (most recent call last):\n    ...\n%s: %s\n" % (
                    type(e).__name__, str(e))
            finally:
                sys.stdout = old_stdout

            # Compare output
            want = example.want
            if self._output_matches(got, want):
                if self.verbose:
                    print("ok")
            else:
                self._failed += 1
                print("Failed example:")
                for line in example.source.split('\n'):
                    print("    " + line)
                print("Expected:")
                for line in want.split('\n'):
                    if line:
                        print("    " + line)
                print("Got:")
                for line in got.split('\n'):
                    if line:
                        print("    " + line)

        return TestResults(failed=self._failed, attempted=self._attempted)

    def _output_matches(self, got, want):
        if got == want:
            return True
        # Normalize trailing whitespace
        if got.rstrip() == want.rstrip():
            return True
        # ELLIPSIS support
        if self.optionflags & ELLIPSIS:
            if '...' in want:
                parts = want.split('...')
                pos = 0
                for part in parts:
                    part = part.strip()
                    if not part:
                        continue
                    idx = got.find(part, pos)
                    if idx < 0:
                        return False
                    pos = idx + len(part)
                return True
        return False

    def summarize(self, verbose=False):
        if verbose or self._failed:
            print("%d items passed all tests:" % (self._attempted - self._failed))
            print("%d items had failures:" % self._failed)
            print("   %d of %d in total" % (self._failed, self._attempted))
        return TestResults(failed=self._failed, attempted=self._attempted)


def testmod(m=None, verbose=False, optionflags=0, extraglobs=None):
    """Test examples in docstrings of the given module.

    If m is None, test the calling module's __main__.
    """
    import sys

    if m is None:
        # Get the calling module (or __main__)
        m = sys.modules.get('__main__')
        if m is None:
            return TestResults(failed=0, attempted=0)

    finder = DocTestFinder()
    runner = DocTestRunner(verbose=verbose, optionflags=optionflags)

    tests = finder.find(m, extraglobs=extraglobs)
    for test in tests:
        runner.run(test)

    return runner.summarize(verbose=verbose)


def run_docstring_examples(f, globs=None, verbose=False, name=None, optionflags=0):
    """Run examples from the docstring of a single object."""
    if name is None:
        name = getattr(f, '__name__', str(f))
    if globs is None:
        # Use the function's own globals if available
        globs = getattr(f, '__globals__', None)
        if globs is None:
            import sys
            m = sys.modules.get('__main__')
            globs = getattr(m, '__dict__', {}).copy() if m else {}
        else:
            globs = dict(globs)

    doc = getattr(f, '__doc__', None)
    if not doc:
        return

    examples = _extract_examples(doc)
    if not examples:
        return

    test = DocTest(examples, globs, name, '<doctest>', 0, doc)
    runner = DocTestRunner(verbose=verbose, optionflags=optionflags)
    runner.run(test)
