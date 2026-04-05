"""Text wrapping and filling."""

import re

# Hardcode the whitespace characters.
_whitespace = '\t\n\x0b\x0c\r '


class TextWrapper:
    """
    Object for wrapping/filling text.  The public interface consists of
    the wrap() and fill() methods; the other methods are just there for
    subclasses to override in order to tweak the default behaviour.
    """

    unicode_whitespace_trans = {}
    uspace = ord(' ')
    for x in _whitespace:
        unicode_whitespace_trans[ord(x)] = uspace

    # This funky little regex is just used to find word-wrapping
    # temporary placeholder.
    wordsep_re = re.compile(
        r'(\s+|'                                  # any whitespace
        r'[^\s\w]*\w+[^0-9\W]-(?=\w+[^0-9\W])|'  # hyphenated words
        r'(?<=[\w\!\"\'\&\.\,\?])-{2,}(?=\w))')   # em-dash

    wordsep_simple_re = re.compile(r'(\s+)')

    sentence_end_re = re.compile(r'[a-z]'
                                  r'[\.\!\?]'
                                  r'[\"\']?'
                                  r'\Z')

    def __init__(self,
                 width=70,
                 initial_indent="",
                 subsequent_indent="",
                 expand_tabs=True,
                 replace_whitespace=True,
                 fix_sentence_endings=False,
                 break_long_words=True,
                 drop_whitespace=True,
                 break_on_hyphens=True,
                 tabsize=8,
                 max_lines=None,
                 placeholder=' [...]'):
        self.width = width
        self.initial_indent = initial_indent
        self.subsequent_indent = subsequent_indent
        self.expand_tabs = expand_tabs
        self.replace_whitespace = replace_whitespace
        self.fix_sentence_endings = fix_sentence_endings
        self.break_long_words = break_long_words
        self.drop_whitespace = drop_whitespace
        self.break_on_hyphens = break_on_hyphens
        self.tabsize = tabsize
        self.max_lines = max_lines
        self.placeholder = placeholder

    def _munge_whitespace(self, text):
        if self.expand_tabs:
            text = text.expandtabs(self.tabsize)
        if self.replace_whitespace:
            # Replace each whitespace char with a space
            result = []
            for ch in text:
                if ch in _whitespace:
                    result.append(' ')
                else:
                    result.append(ch)
            text = ''.join(result)
        return text

    def _split(self, text):
        if self.break_on_hyphens:
            chunks = self.wordsep_re.split(text)
        else:
            chunks = self.wordsep_simple_re.split(text)
        return [c for c in chunks if c]

    def _handle_long_word(self, reversed_chunks, cur_line, cur_len, width):
        if width < 1:
            space_left = 1
        else:
            space_left = width - cur_len

        if self.break_long_words:
            cur_line.append(reversed_chunks[-1][:space_left])
            reversed_chunks[-1] = reversed_chunks[-1][space_left:]
        elif not cur_line:
            cur_line.append(reversed_chunks.pop())

    def _wrap_chunks(self, chunks):
        lines = []
        if self.width <= 0:
            raise ValueError("invalid width %r (must be > 0)" % self.width)

        if self.max_lines is not None:
            if self.max_lines > 1:
                indent = self.subsequent_indent
            else:
                indent = self.initial_indent
            if len(indent) + len(self.placeholder.lstrip()) > self.width:
                raise ValueError("placeholder too large for max width")

        chunks.reverse()

        while chunks:
            cur_line = []
            cur_len = 0
            if lines:
                indent = self.subsequent_indent
            else:
                indent = self.initial_indent
            width = self.width - len(indent)

            if self.drop_whitespace and chunks[-1].strip() == '' and lines:
                del chunks[-1]

            while chunks:
                l = len(chunks[-1])
                if cur_len + l <= width:
                    cur_line.append(chunks.pop())
                    cur_len += l
                else:
                    break

            if chunks and len(chunks[-1]) > width:
                self._handle_long_word(chunks, cur_line, cur_len, width)
                cur_len = sum(len(c) for c in cur_line)

            if self.drop_whitespace and cur_line and cur_line[-1].strip() == '':
                cur_len -= len(cur_line[-1])
                del cur_line[-1]

            if cur_line:
                if (self.max_lines is None or
                    len(lines) + 1 < self.max_lines or
                    (not chunks or
                     self.drop_whitespace and
                     len(chunks) == 1 and
                     not chunks[0].strip()) and cur_len <= width):
                    lines.append(indent + ''.join(cur_line))
                else:
                    while cur_line:
                        if (cur_line[-1].strip() and
                            cur_len + len(self.placeholder) <= width):
                            lines.append(indent +
                                         ''.join(cur_line) + self.placeholder)
                            break
                        cur_len -= len(cur_line[-1])
                        del cur_line[-1]
                    else:
                        if lines:
                            prev_line = lines[-1].rstrip()
                            if (len(prev_line) + len(self.placeholder) <=
                                    self.width):
                                lines[-1] = prev_line + self.placeholder
                                break
                        lines.append(indent + self.placeholder.lstrip())
                    break

        return lines

    def wrap(self, text):
        chunks = self._split(self._munge_whitespace(text))
        return self._wrap_chunks(chunks)

    def fill(self, text):
        return "\n".join(self.wrap(text))


def wrap(text, width=70, **kwargs):
    """Wrap a single paragraph of text, returning a list of wrapped lines."""
    w = TextWrapper(width=width, **kwargs)
    return w.wrap(text)


def fill(text, width=70, **kwargs):
    """Fill a single paragraph of text, returning a new string."""
    w = TextWrapper(width=width, **kwargs)
    return w.fill(text)


def shorten(text, width, **kwargs):
    """Collapse and truncate the given text to fit in the given width."""
    w = TextWrapper(width=width, max_lines=1, **kwargs)
    return w.fill(' '.join(text.split()))


def dedent(text):
    """Remove any common leading whitespace from all lines in text."""
    _whitespace_only_re = re.compile('^[ \t]+$', re.MULTILINE)
    _leading_whitespace_re = re.compile('(^[ \t]*)(?:[^ \t\n])', re.MULTILINE)

    # Look for whitespace to be removed.
    text = _whitespace_only_re.sub('', text)
    margins = _leading_whitespace_re.findall(text)
    if not margins:
        return text

    # Find the longest common leading whitespace
    indent = min(margins)
    for m in margins:
        # Find common prefix
        shorter = min(len(indent), len(m))
        common = 0
        for i in range(shorter):
            if indent[i] == m[i]:
                common = i + 1
            else:
                break
        indent = indent[:common]

    if indent:
        lines = text.split('\n')
        result = []
        for line in lines:
            if line.startswith(indent):
                result.append(line[len(indent):])
            elif line.strip():
                result.append(line)
            else:
                result.append(line)
        text = '\n'.join(result)
    return text


def indent(text, prefix, predicate=None):
    """Add 'prefix' to the beginning of selected lines in 'text'."""
    if predicate is None:
        def predicate(line):
            return line.strip()

    lines = text.splitlines(True)
    result = []
    for line in lines:
        if predicate(line):
            result.append(prefix + line)
        else:
            result.append(line)
    return ''.join(result)
