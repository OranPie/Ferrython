"""shlex module — Simple lexical analysis."""

import re


def split(s, comments=False, posix=True):
    """Split the string s using shell-like syntax."""
    lex = shlex(s, posix=posix)
    lex.whitespace_split = True
    if not comments:
        lex.commenters = ''
    tokens = list(lex)
    return tokens


def quote(s):
    """Return a shell-escaped version of the string s."""
    if not s:
        return "''"
    # Check if string is safe without quoting
    safe_chars = set('abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789@%+=:,./-_')
    if all(c in safe_chars for c in s):
        return s
    # Use single quotes, escaping existing single quotes
    return "'" + s.replace("'", "'\"'\"'") + "'"


def join(split_command):
    """Concatenate tokens using shell quoting."""
    return ' '.join(quote(arg) for arg in split_command)


class shlex:
    """A lexical analyzer class for simple shell-like syntaxes."""
    
    def __init__(self, instream=None, posix=False):
        if isinstance(instream, str):
            self._input = instream
        elif instream is None:
            self._input = ''
        else:
            self._input = str(instream)
        self.posix = posix
        self.whitespace = ' \t\r\n'
        self.whitespace_split = False
        self.quotes = '\'"'
        self.escape = '\\'
        self.escapedquotes = '"'
        self.commenters = '#'
        self.wordchars = ('abcdefghijklmnopqrstuvwxyz'
                         'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_')
        self.token = ''
        self._pos = 0
        self._tokens = None
    
    def __iter__(self):
        if self._tokens is None:
            self._tokens = self._tokenize()
        return iter(self._tokens)
    
    def _tokenize(self):
        """Tokenize the input string."""
        tokens = []
        i = 0
        s = self._input
        n = len(s)
        
        while i < n:
            c = s[i]
            
            # Skip whitespace
            if c in self.whitespace:
                i += 1
                continue
            
            # Comments
            if c in self.commenters:
                # Skip to end of line
                while i < n and s[i] != '\n':
                    i += 1
                continue
            
            # Quoted string
            if c in self.quotes:
                quote_char = c
                i += 1
                token = ''
                while i < n and s[i] != quote_char:
                    if self.posix and s[i] == self.escape and quote_char in self.escapedquotes:
                        i += 1
                        if i < n:
                            token += s[i]
                    else:
                        token += s[i]
                    i += 1
                if i < n:
                    i += 1  # skip closing quote
                tokens.append(token)
                continue
            
            # Regular word
            token = ''
            while i < n and s[i] not in self.whitespace:
                if s[i] in self.quotes:
                    # Handle embedded quotes
                    quote_char = s[i]
                    i += 1
                    while i < n and s[i] != quote_char:
                        token += s[i]
                        i += 1
                    if i < n:
                        i += 1
                elif self.posix and s[i] == self.escape[0] if self.escape else False:
                    i += 1
                    if i < n:
                        token += s[i]
                        i += 1
                else:
                    token += s[i]
                    i += 1
            tokens.append(token)
        
        return tokens
    
    def get_token(self):
        """Get a token from the input stream."""
        if self._tokens is None:
            self._tokens = self._tokenize()
        if self._pos < len(self._tokens):
            tok = self._tokens[self._pos]
            self._pos += 1
            return tok
        return None
    
    def push_token(self, tok):
        """Push a token onto the token stack."""
        if self._tokens is None:
            self._tokens = []
        self._tokens.insert(self._pos, tok)
