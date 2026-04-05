"""fnmatch module — Unix filename pattern matching.

Note: Rust fnmatch exists but this provides the full Python API.
"""

import re

def fnmatch(filename, pattern):
    """Test whether filename matches pattern.
    
    Patterns are Unix shell style:
    *       matches everything
    ?       matches any single character
    [seq]   matches any character in seq
    [!seq]  matches any char not in seq
    """
    return fnmatchcase(filename.lower(), pattern.lower())

def fnmatchcase(filename, pattern):
    """Test whether filename matches pattern, without case normalization."""
    pat = translate(pattern)
    return bool(re.match(pat, filename))

def filter(names, pattern):
    """Return the subset of names that match pattern."""
    result = []
    pat = translate(pattern.lower())
    for name in names:
        if re.match(pat, name.lower()):
            result.append(name)
    return result

def translate(pattern):
    """Translate a shell pattern to a regular expression."""
    i, n = 0, len(pattern)
    res = ''
    while i < n:
        c = pattern[i]
        i += 1
        if c == '*':
            res += '.*'
        elif c == '?':
            res += '.'
        elif c == '[':
            j = i
            if j < n and pattern[j] == '!':
                j += 1
            if j < n and pattern[j] == ']':
                j += 1
            while j < n and pattern[j] != ']':
                j += 1
            if j >= n:
                res += '\\['
            else:
                stuff = pattern[i:j].replace('\\', '\\\\')
                i = j + 1
                if stuff[0] == '!':
                    stuff = '^' + stuff[1:]
                elif stuff[0] == '^':
                    stuff = '\\' + stuff
                res += '[' + stuff + ']'
        else:
            res += re.escape(c)
    return '(?s:' + res + ')\\Z'
