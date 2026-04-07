"""Pure Python implementation of the cmd module.

Provides a simple framework for writing line-oriented command interpreters.
"""

import string

PROMPT = '(Cmd) '
IDENTCHARS = string.ascii_letters + string.digits + '_'


class Cmd:
    """A simple framework for writing line-oriented command interpreters."""
    
    prompt = PROMPT
    identchars = IDENTCHARS
    ruler = '='
    lastcmd = ''
    intro = None
    doc_leader = ""
    doc_header = "Documented commands (type help <topic>):"
    misc_header = "Miscellaneous help topics:"
    undoc_header = "Undocumented commands:"
    nohelp = "*** No help on %s"
    use_rawinput = 1
    
    def __init__(self, completekey='tab', stdin=None, stdout=None):
        import sys
        self.stdin = stdin or sys.stdin
        self.stdout = stdout or sys.stdout
        self.cmdqueue = []
        self.completekey = completekey
    
    def cmdloop(self, intro=None):
        """Repeatedly issue a prompt, accept input, parse an initial prefix
        off the received input, and dispatch to action methods."""
        self.preloop()
        if intro is not None:
            self.intro = intro
        if self.intro:
            self.stdout.write(str(self.intro) + "\n")
        stop = None
        while not stop:
            if self.cmdqueue:
                line = self.cmdqueue.pop(0)
            else:
                try:
                    line = input(self.prompt)
                except EOFError:
                    line = 'EOF'
            line = self.precmd(line)
            stop = self.onecmd(line)
            stop = self.postcmd(stop, line)
        self.postloop()
    
    def precmd(self, line):
        return line
    
    def postcmd(self, stop, line):
        return stop
    
    def preloop(self):
        pass
    
    def postloop(self):
        pass
    
    def parseline(self, line):
        line = line.strip()
        if not line:
            return None, None, line
        elif line[0] == '?':
            line = 'help ' + line[1:]
        elif line[0] == '!':
            if hasattr(self, 'do_shell'):
                line = 'shell ' + line[1:]
            else:
                return None, None, line
        i, n = 0, len(line)
        while i < n and line[i] in self.identchars:
            i = i + 1
        cmd, arg = line[:i], line[i:].strip()
        return cmd, arg, line
    
    def onecmd(self, line):
        cmd, arg, line = self.parseline(line)
        if not line:
            return self.default(line)
        self.lastcmd = line
        if line == 'EOF':
            self.lastcmd = ''
        if cmd is None or cmd == '':
            return self.default(line)
        func = getattr(self, 'do_' + cmd, None)
        if func:
            return func(arg)
        return self.default(line)
    
    def emptyline(self):
        if self.lastcmd:
            return self.onecmd(self.lastcmd)
    
    def default(self, line):
        self.stdout.write('*** Unknown syntax: %s\n' % line)
    
    def do_help(self, arg):
        if arg:
            func = getattr(self, 'help_' + arg, None)
            if func:
                func()
                return
            doc = getattr(self, 'do_' + arg, None)
            if doc and doc.__doc__:
                self.stdout.write("%s\n" % str(doc.__doc__))
                return
            self.stdout.write("%s\n" % str(self.nohelp % (arg,)))
        else:
            names = dir(self)
            cmds_doc = []
            cmds_undoc = []
            for name in names:
                if name[:3] == 'do_':
                    cmd = name[3:]
                    if getattr(self, 'help_' + cmd, None):
                        cmds_doc.append(cmd)
                    elif getattr(self, name).__doc__:
                        cmds_doc.append(cmd)
                    else:
                        cmds_undoc.append(cmd)
            self.stdout.write("%s\n" % str(self.doc_header))
            if cmds_doc:
                self.stdout.write("%s\n" % str("  ".join(sorted(cmds_doc))))
            if cmds_undoc:
                self.stdout.write("%s\n" % str(self.undoc_header))
                self.stdout.write("%s\n" % str("  ".join(sorted(cmds_undoc))))
    
    def get_names(self):
        return dir(self)
    
    def complete(self, text, state):
        return None
    
    def completenames(self, text, *ignored):
        dotext = 'do_' + text
        return [a[3:] for a in self.get_names() if a.startswith(dotext)]
