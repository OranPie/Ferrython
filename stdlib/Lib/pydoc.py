"""Pure Python implementation of the pydoc module (simplified).

Documentation utilities.
"""

import sys
import os


def getdoc(object):
    """Get the doc string or None."""
    try:
        doc = object.__doc__
    except AttributeError:
        return None
    if doc is None:
        return None
    return str(doc).strip()


def describe(thing):
    """Produce a short description of the given thing."""
    try:
        if hasattr(thing, '__name__'):
            name = thing.__name__
        else:
            name = str(thing)
        
        thing_type = type(thing).__name__
        if thing_type == 'module':
            return 'module ' + name
        elif thing_type == 'type':
            return 'class ' + name
        elif thing_type in ('function', 'builtin_function_or_method'):
            return 'function ' + name
        elif thing_type == 'method':
            return 'method ' + name
        else:
            return thing_type
    except Exception:
        return str(type(thing))


def render_doc(thing, title='Python Library Documentation: %s', forceload=0):
    """Render a text document, trim leading/trailing lines."""
    desc = describe(thing)
    doc = getdoc(thing) or 'No documentation available.'
    result = title % desc + '\n\n'
    
    thing_type = type(thing).__name__
    
    if thing_type == 'type':
        name = getattr(thing, '__name__', str(thing))
        result += 'class %s' % name
        if hasattr(thing, '__bases__') and thing.__bases__:
            bases = ', '.join(getattr(b, '__name__', str(b)) for b in thing.__bases__)
            result += '(%s)' % bases
        result += '\n'
        if doc:
            result += '    ' + doc + '\n'
        result += '\n'
        
        methods = []
        for name in sorted(dir(thing)):
            if name.startswith('_') and name != '__init__':
                continue
            obj = getattr(thing, name, None)
            if callable(obj):
                methods.append(name)
        
        if methods:
            result += 'Methods:\n'
            for name in methods:
                obj = getattr(thing, name, None)
                mdoc = getdoc(obj) or ''
                result += '    %s: %s\n' % (name, mdoc[:60])
    
    elif thing_type == 'module':
        if doc:
            result += doc + '\n\n'
        
        classes = []
        functions = []
        data = []
        for name in sorted(dir(thing)):
            if name.startswith('_'):
                continue
            obj = getattr(thing, name, None)
            if type(obj).__name__ == 'type':
                classes.append(name)
            elif callable(obj):
                functions.append(name)
            else:
                data.append(name)
        
        if classes:
            result += 'CLASSES\n'
            for name in classes:
                result += '    %s\n' % name
            result += '\n'
        if functions:
            result += 'FUNCTIONS\n'
            for name in functions:
                fdoc = getdoc(getattr(thing, name, None)) or ''
                result += '    %s: %s\n' % (name, fdoc[:60])
            result += '\n'
        if data:
            result += 'DATA\n'
            for name in data:
                result += '    %s\n' % name
    
    else:
        if doc:
            result += doc + '\n'
    
    return result


def doc(thing, title='Python Library Documentation: %s', forceload=0, output=None):
    """Display documentation on a thing."""
    text = render_doc(thing, title, forceload)
    if output:
        output.write(text)
    else:
        print(text)


def help(request=None):
    """Open the help system."""
    if request is None:
        print("Welcome to Python's help utility!")
        print("Type 'quit' to exit.")
        return
    
    if isinstance(request, str):
        import importlib
        try:
            thing = importlib.import_module(request)
        except ImportError:
            try:
                thing = eval(request)
            except Exception:
                print("No Python documentation found for '%s'" % request)
                return
        doc(thing)
    else:
        doc(request)
