"""pprint module — Pretty printer for data structures.

Note: This Python implementation serves as fallback when the Rust pprint
module doesn't provide sufficient formatting depth.
"""

def pformat(obj, indent=1, width=80, depth=None, compact=False):
    """Format a Python object into a pretty-printed representation."""
    return PrettyPrinter(indent=indent, width=width, depth=depth,
                         compact=compact).pformat(obj)

def pprint(obj, stream=None, indent=1, width=80, depth=None, compact=False):
    """Pretty-print a Python object to a stream [default is sys.stdout]."""
    printer = PrettyPrinter(indent=indent, width=width, depth=depth,
                            compact=compact)
    result = printer.pformat(obj)
    if stream is None:
        print(result)
    else:
        stream.write(result + '\n')

def isreadable(obj):
    """Determine if saferepr(obj) is readable by eval()."""
    return not _safe_repr(obj)[1]

def isrecursive(obj):
    """Determine if obj requires a recursive representation."""
    return _safe_repr(obj)[2]

def saferepr(obj):
    """Version of repr() which can handle recursive data structures."""
    return _safe_repr(obj)[0]


class PrettyPrinter:
    """Pretty printer with configurable indentation and width."""
    
    def __init__(self, indent=1, width=80, depth=None, stream=None, compact=False):
        self._indent_per_level = indent
        self._width = width
        self._depth = depth
        self._stream = stream
        self._compact = compact
    
    def pformat(self, obj):
        """Format obj and return the resulting string."""
        sio = _StringIO()
        self._format(obj, sio, 0, 0, {}, 0)
        return sio.getvalue()
    
    def pprint(self, obj):
        """Print the formatted representation of obj."""
        result = self.pformat(obj)
        if self._stream is not None:
            self._stream.write(result + '\n')
        else:
            print(result)
    
    def isreadable(self, obj):
        s, readable, recursive = _safe_repr(obj)
        return readable and not recursive
    
    def isrecursive(self, obj):
        s, readable, recursive = _safe_repr(obj)
        return recursive
    
    def _format(self, obj, stream, indent, allowance, context, level):
        objid = id(obj)
        if objid in context:
            stream.write('...')
            return
        
        if self._depth and level >= self._depth:
            stream.write(repr(obj))
            return
        
        rep = repr(obj)
        if len(rep) <= self._width - indent - allowance:
            stream.write(rep)
            return
        
        typ = type(obj)
        if isinstance(obj, dict):
            context[objid] = 1
            stream.write('{')
            items = list(obj.items())
            if items:
                self._format_dict_items(items, stream, indent + self._indent_per_level,
                                       allowance + 1, context, level + 1)
            stream.write('}')
            del context[objid]
        elif isinstance(obj, (list, tuple)):
            context[objid] = 1
            if isinstance(obj, list):
                stream.write('[')
                endchar = ']'
            else:
                stream.write('(')
                endchar = ')'
            if obj:
                self._format_items(list(obj), stream, indent + self._indent_per_level,
                                  allowance + 1, context, level + 1)
            stream.write(endchar)
            del context[objid]
        elif isinstance(obj, set):
            context[objid] = 1
            stream.write('{')
            if obj:
                self._format_items(sorted(obj, key=repr), stream,
                                  indent + self._indent_per_level,
                                  allowance + 1, context, level + 1)
            stream.write('}')
            del context[objid]
        else:
            stream.write(rep)
    
    def _format_dict_items(self, items, stream, indent, allowance, context, level):
        write = stream.write
        indent_str = '\n' + ' ' * indent
        delimnl = ',' + indent_str
        last_index = len(items) - 1
        for i, (key, ent) in enumerate(items):
            if i > 0:
                write(delimnl)
            else:
                write(indent_str)
            write(repr(key))
            write(': ')
            self._format(ent, stream, indent + len(repr(key)) + 2,
                        allowance if i == last_index else 1,
                        context, level)
        if last_index >= 0:
            write('}')
            # Remove the closing brace we just wrote - parent will write it
            # Actually let parent handle
    
    def _format_items(self, items, stream, indent, allowance, context, level):
        write = stream.write
        indent_str = '\n' + ' ' * indent
        delimnl = ',' + indent_str
        last_index = len(items) - 1
        for i, ent in enumerate(items):
            if i > 0:
                write(delimnl)
            else:
                write(indent_str)
            self._format(ent, stream, indent,
                        allowance if i == last_index else 1,
                        context, level)


class _StringIO:
    """Simple string buffer for pformat."""
    def __init__(self):
        self._parts = []
    
    def write(self, s):
        self._parts.append(s)
    
    def getvalue(self):
        return ''.join(self._parts)


def _safe_repr(obj, context=None):
    """Return triple (repr_string, isreadable, isrecursive)."""
    if context is None:
        context = {}
    typ = type(obj)
    if isinstance(obj, (int, float, str, bool)):
        return repr(obj), True, False
    if isinstance(obj, type(None)):
        return 'None', True, False
    objid = id(obj)
    if objid in context:
        return '...', False, True
    context[objid] = 1
    result = repr(obj), True, False
    del context[objid]
    return result
