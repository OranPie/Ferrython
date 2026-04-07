"""_markupbase — Shared base for HTML/XML parsers."""

import re

class ParserBase:
    """Parser base class with common methods."""
    
    def __init__(self):
        if self.__class__ is ParserBase:
            raise RuntimeError("_markupbase.ParserBase must be subclassed")
    
    def error(self, message):
        raise NotImplementedError("subclasses must override error()")
    
    def reset(self):
        self.lineno = 1
        self.offset = 0
    
    def getpos(self):
        return self.lineno, self.offset
    
    def updatepos(self, i, j):
        if i >= j:
            return j
        rawdata = self.rawdata
        nlines = rawdata.count('\n', i, j)
        if nlines:
            self.lineno = self.lineno + nlines
            pos = rawdata.rindex('\n', i, j)
            self.offset = j - (pos + 1)
        else:
            self.offset = self.offset + (j - i)
        return j
    
    _decl_otherchars = ''
    
    def _parse_doctype_subset(self, i, declstartpos):
        rawdata = self.rawdata
        n = len(rawdata)
        j = i
        while j < n:
            c = rawdata[j]
            if c == '<':
                s = rawdata[j:j+2]
                if s == '<!':
                    j = self._parse_doctype_element(j, j+2)
                elif s == '<?':
                    j = self._parse_doctype_pi(j)
                else:
                    break
            elif c == '%':
                j = self._parse_doctype_entity(j)
            elif c == ']':
                j = j + 1
                while j < n and rawdata[j].isspace():
                    j = j + 1
                if j < n and rawdata[j] == '>':
                    return j
                return -1
            elif c.isspace():
                j = j + 1
            else:
                return -1
        return -1
    
    def _parse_doctype_element(self, i, j):
        rawdata = self.rawdata
        n = len(rawdata)
        while j < n:
            c = rawdata[j]
            if c == '>':
                return j + 1
            j = j + 1
        return -1
    
    def _parse_doctype_pi(self, i):
        rawdata = self.rawdata
        n = len(rawdata)
        j = i + 2
        while j < n:
            if rawdata[j:j+2] == '?>':
                return j + 2
            j = j + 1
        return -1
    
    def _parse_doctype_entity(self, i):
        rawdata = self.rawdata
        n = len(rawdata)
        j = i + 1
        while j < n:
            c = rawdata[j]
            if c == ';':
                return j + 1
            j = j + 1
        return -1
