"""email.parser — RFC 2822 message parser.

Parses email messages from strings or file objects into EmailMessage objects.
"""
from email.message import EmailMessage


class Parser:
    """Parse email messages from strings."""

    def __init__(self, _class=None, policy=None):
        self._class = _class or EmailMessage
        self._policy = policy

    def parse(self, fp, headersonly=False):
        """Parse a message from a file object."""
        return self.parsestr(fp.read(), headersonly=headersonly)

    def parsestr(self, text, headersonly=False):
        """Parse a message from a string."""
        msg = self._class()
        if isinstance(text, bytes):
            text = text.decode('utf-8', errors='replace')
        lines = text.split('\n')
        in_headers = True
        current_header = None
        current_value = None
        body_lines = []

        for line in lines:
            if in_headers:
                if line.strip() == '':
                    # End of headers
                    if current_header is not None:
                        msg[current_header] = current_value
                    in_headers = False
                    if headersonly:
                        break
                    continue
                if line[0:1] in (' ', '\t') and current_header is not None:
                    # Continuation line
                    current_value += ' ' + line.strip()
                elif ':' in line:
                    if current_header is not None:
                        msg[current_header] = current_value
                    idx = line.index(':')
                    current_header = line[:idx].strip()
                    current_value = line[idx+1:].strip()
            else:
                body_lines.append(line)

        # Handle last header if no blank line
        if current_header is not None and in_headers:
            msg[current_header] = current_value

        if body_lines:
            msg.set_payload('\n'.join(body_lines))

        return msg


class BytesParser:
    """Parse email messages from bytes."""

    def __init__(self, _class=None, policy=None):
        self._parser = Parser(_class=_class, policy=policy)

    def parse(self, fp, headersonly=False):
        data = fp.read()
        if isinstance(data, bytes):
            data = data.decode('utf-8', errors='replace')
        return self._parser.parsestr(data, headersonly=headersonly)

    def parsebytes(self, text, headersonly=False):
        if isinstance(text, bytes):
            text = text.decode('utf-8', errors='replace')
        return self._parser.parsestr(text, headersonly=headersonly)


class HeaderParser(Parser):
    """Parse only the headers of an email message."""

    def parse(self, fp, headersonly=True):
        return super().parse(fp, headersonly=True)

    def parsestr(self, text, headersonly=True):
        return super().parsestr(text, headersonly=True)


class BytesHeaderParser(BytesParser):
    """Parse only the headers from bytes."""

    def parse(self, fp, headersonly=True):
        return super().parse(fp, headersonly=True)

    def parsebytes(self, text, headersonly=True):
        return super().parsebytes(text, headersonly=True)
