"""Python keyword list."""

kwlist = [
    'False', 'None', 'True', 'and', 'as', 'assert', 'async', 'await',
    'break', 'class', 'continue', 'def', 'del', 'elif', 'else', 'except',
    'finally', 'for', 'from', 'global', 'if', 'import', 'in', 'is',
    'lambda', 'nonlocal', 'not', 'or', 'pass', 'raise', 'return',
    'try', 'while', 'with', 'yield'
]

_kwset = frozenset(kwlist)

def iskeyword(s):
    return s in _kwset

softkwlist = ['_', 'case', 'match', 'type']

def issoftkeyword(s):
    return s in softkwlist
