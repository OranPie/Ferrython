"""Internal support module for sre — parse regex pattern strings.

This is a compatibility shim; the actual regex engine is implemented in Rust.
"""

SPECIAL_CHARS = ".\\[{()*+?^$|"
REPEAT_CHARS = "*+?{"

CATEGORIES = {
    "\\d": "CATEGORY_DIGIT",
    "\\D": "CATEGORY_NOT_DIGIT",
    "\\s": "CATEGORY_SPACE",
    "\\S": "CATEGORY_NOT_SPACE",
    "\\w": "CATEGORY_WORD",
    "\\W": "CATEGORY_NOT_WORD",
}

FLAGS = {
    "i": 2,   # IGNORECASE
    "L": 4,   # LOCALE
    "m": 8,   # MULTILINE
    "s": 16,  # DOTALL
    "u": 32,  # UNICODE
    "x": 64,  # VERBOSE
    "a": 256, # ASCII
    "t": 1,   # TEMPLATE
}

MAXREPEAT = 4294967295
MAXGROUPS = 2147483647

class SubPattern:
    """Representation of a parsed sub-pattern."""
    def __init__(self, data=None, flags=0):
        self.data = data or []
        self.flags = flags
        self.width = None

    def append(self, item):
        self.data.append(item)

    def __len__(self):
        return len(self.data)

    def __getitem__(self, index):
        return self.data[index]

    def __repr__(self):
        return repr(self.data)

class Tokenizer:
    """Simple tokenizer for regex patterns."""
    def __init__(self, string, flags=0):
        self.istext = isinstance(string, str)
        self.string = string
        self.index = 0
        self.flags = flags
        self.next = None
        self.__next()

    def __next(self):
        index = self.index
        try:
            char = self.string[index]
            if char == "\\":
                index += 1
                char += self.string[index]
        except IndexError:
            self.next = None
            return
        self.index = index + 1
        self.next = char

    def match(self, char):
        if self.next == char:
            self.__next()
            return True
        return False

    def get(self):
        this = self.next
        self.__next()
        return this

    def getwhile(self, n, charset):
        result = ''
        while n and self.next and self.next in charset:
            result += self.next
            self.__next()
            n -= 1
        return result

    def getuntil(self, terminator):
        result = ''
        while self.next and self.next != terminator:
            result += self.next
            self.__next()
        if not self.next:
            raise error("missing %s" % terminator)
        self.__next()  # skip terminator
        return result

    @property
    def pos(self):
        return self.index

    def tell(self):
        return self.index

    def seek(self, index):
        self.index = index
        self.__next()

class error(Exception):
    """Exception raised for invalid regex patterns."""
    def __init__(self, msg, pattern=None, pos=None):
        self.msg = msg
        self.pattern = pattern
        self.pos = pos
        super().__init__(msg)

def parse(str, flags=0, pattern=None):
    """Parse a regex pattern string into a SubPattern."""
    return SubPattern(list(str), flags)

def parse_template(source, pattern):
    """Parse a replacement template string."""
    return ([], [source])
