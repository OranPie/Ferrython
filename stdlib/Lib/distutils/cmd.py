"""
distutils.cmd — Base class for distribution commands.
"""

class Command:
    """Abstract base class for defining commands."""
    def __init__(self, dist):
        self.distribution = dist
        self.verbose = 0
        self.force = 0
        self.help = 0

    def ensure_finalized(self):
        pass

    def initialize_options(self):
        pass

    def finalize_options(self):
        pass

    def run(self):
        raise RuntimeError("abstract")

    def announce(self, msg, level=1):
        if self.verbose >= level:
            print(msg)

    def execute(self, func, args, msg=None, level=1):
        if msg:
            self.announce(msg, level)
        func(*args)
