"""An object-oriented interface to .netrc files."""

import os

class NetrcParseError(Exception):
    """Exception raised on syntax errors in the .netrc file."""
    def __init__(self, msg, filename=None, lineno=None):
        self.filename = filename
        self.lineno = lineno
        self.msg = msg
        super().__init__(msg)

class netrc:
    """Parse a .netrc file.
    
    If no file is specified, read ~/.netrc.
    """
    def __init__(self, file=None):
        self.hosts = {}
        self.macros = {}
        if file is None:
            file = os.path.join(os.path.expanduser("~"), ".netrc")
        self._parse(file)

    def _parse(self, file):
        try:
            with open(file, 'r') as fp:
                lines = fp.read()
        except (IOError, OSError):
            return
        tokens = lines.split()
        i = 0
        while i < len(tokens):
            tok = tokens[i]
            if tok == 'machine':
                i += 1
                if i >= len(tokens):
                    break
                machine = tokens[i]
                login = ''
                account = ''
                password = ''
                i += 1
                while i < len(tokens) and tokens[i] not in ('machine', 'default', 'macdef'):
                    if tokens[i] == 'login':
                        i += 1
                        if i < len(tokens):
                            login = tokens[i]
                    elif tokens[i] == 'password':
                        i += 1
                        if i < len(tokens):
                            password = tokens[i]
                    elif tokens[i] == 'account':
                        i += 1
                        if i < len(tokens):
                            account = tokens[i]
                    i += 1
                self.hosts[machine] = (login, account, password)
            elif tok == 'default':
                login = ''
                account = ''
                password = ''
                i += 1
                while i < len(tokens) and tokens[i] not in ('machine', 'default', 'macdef'):
                    if tokens[i] == 'login':
                        i += 1
                        if i < len(tokens):
                            login = tokens[i]
                    elif tokens[i] == 'password':
                        i += 1
                        if i < len(tokens):
                            password = tokens[i]
                    elif tokens[i] == 'account':
                        i += 1
                        if i < len(tokens):
                            account = tokens[i]
                    i += 1
                self.hosts['default'] = (login, account, password)
            elif tok == 'macdef':
                i += 1
                if i >= len(tokens):
                    break
                name = tokens[i]
                i += 1
                macro_lines = []
                while i < len(tokens) and tokens[i] not in ('machine', 'default', 'macdef'):
                    macro_lines.append(tokens[i])
                    i += 1
                self.macros[name] = '\n'.join(macro_lines)
            else:
                i += 1

    def authenticators(self, host):
        """Return a (user, account, password) tuple for the given host."""
        if host in self.hosts:
            return self.hosts[host]
        if 'default' in self.hosts:
            return self.hosts['default']
        return None

    def __repr__(self):
        rep = ""
        for host, (login, account, password) in self.hosts.items():
            rep += "machine %s\n\tlogin %s\n" % (host, login)
            if account:
                rep += "\taccount %s\n" % account
            rep += "\tpassword %s\n" % password
        for name, macro in self.macros.items():
            rep += "macdef %s\n%s\n" % (name, macro)
        return rep
