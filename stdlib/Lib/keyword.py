"""Python keyword list."""

kwlist = [
    'False', 'None', 'True', 'and', 'as', 'assert', 'async', 'await',
    'break', 'class', 'continue', 'def', 'del', 'elif', 'else', 'except',
    'finally', 'for', 'from', 'global', 'if', 'import', 'in', 'is',
    'lambda', 'nonlocal', 'not', 'or', 'pass', 'raise', 'return',
    'try', 'while', 'with', 'yield'
]

_kwset = frozenset(kwlist)


def iskeyword(name):
    return name in _kwset

softkwlist = ['_', 'case', 'match', 'type']

_softkwset = frozenset(softkwlist)


def issoftkeyword(name):
    return name in _softkwset
