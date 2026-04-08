"""
distutils.version — Version number classes.
"""

import re

class Version:
    def __init__(self, vstring=None):
        if vstring:
            self.parse(vstring)

    def parse(self, vstring):
        raise NotImplementedError

class StrictVersion(Version):
    version_re = re.compile(r'^(\d+) \. (\d+) (\. (\d+))? ([ab](\d+))?$', re.VERBOSE | re.ASCII)

    def parse(self, vstring):
        match = self.version_re.match(vstring)
        if not match:
            self.version = tuple(int(x) for x in vstring.split('.')[:3])
            self.prerelease = None
            return
        major, minor = int(match.group(1)), int(match.group(2))
        patch = int(match.group(4)) if match.group(4) else 0
        self.version = (major, minor, patch)
        self.prerelease = (match.group(5)[0], int(match.group(6))) if match.group(5) else None

    def __str__(self):
        v = '.'.join(str(x) for x in self.version)
        if self.prerelease:
            v += self.prerelease[0] + str(self.prerelease[1])
        return v

    def __repr__(self):
        return f"StrictVersion('{self}')"

    def __eq__(self, other):
        if isinstance(other, str):
            other = StrictVersion(other)
        return self.version == other.version

    def __lt__(self, other):
        if isinstance(other, str):
            other = StrictVersion(other)
        return self.version < other.version

    def __le__(self, other):
        return self == other or self < other

    def __gt__(self, other):
        return not self <= other

    def __ge__(self, other):
        return not self < other

class LooseVersion(Version):
    def parse(self, vstring):
        self.vstring = vstring
        self.version = []
        for part in re.split(r'(\d+)', vstring):
            try:
                self.version.append(int(part))
            except ValueError:
                self.version.append(part)

    def __str__(self):
        return self.vstring

    def __repr__(self):
        return f"LooseVersion('{self}')"

    def __eq__(self, other):
        if isinstance(other, str):
            other = LooseVersion(other)
        return self.version == other.version

    def __lt__(self, other):
        if isinstance(other, str):
            other = LooseVersion(other)
        return self.version < other.version

    def __le__(self, other):
        return self == other or self < other

    def __gt__(self, other):
        return not self <= other

    def __ge__(self, other):
        return not self < other
