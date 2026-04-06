"""email.header — Internationalized headers.

Provides Header class for RFC 2047 encoded email headers.
"""


class Header:
    """Represent an email header with possible RFC 2047 encoding."""

    def __init__(self, s=None, charset=None, maxlinelen=None,
                 header_name=None, continuation_ws=' ', errors='strict'):
        self._chunks = []
        self._maxlinelen = maxlinelen or 76
        self._header_name = header_name
        self._continuation_ws = continuation_ws
        if s is not None:
            self.append(s, charset, errors)

    def append(self, s, charset=None, errors='strict'):
        """Append a string to the header."""
        if isinstance(s, bytes):
            s = s.decode(charset or 'ascii', errors=errors)
        self._chunks.append((s, charset))

    def encode(self, splitchars=';, \t', maxlinelen=None, linesep='\n'):
        """Encode the header into RFC 2822 compliant format."""
        parts = []
        for s, charset in self._chunks:
            if charset and charset.lower() not in ('us-ascii', 'ascii', None):
                # RFC 2047 encoding: =?charset?B?encoded?= or =?charset?Q?encoded?=
                try:
                    import base64
                    encoded = base64.b64encode(s.encode(charset)).decode('ascii')
                    parts.append('=?{}?B?{}?='.format(charset, encoded))
                except Exception:
                    parts.append(s)
            else:
                parts.append(s)
        return ' '.join(parts)

    def __str__(self):
        return self.encode()

    def __repr__(self):
        return 'Header({!r})'.format(str(self))

    def __eq__(self, other):
        return str(self) == str(other)

    def __hash__(self):
        return hash(str(self))


def decode_header(header):
    """Decode a message header value.

    Returns a list of (decoded_string, charset) pairs.
    """
    if not header:
        return [('', None)]

    if isinstance(header, Header):
        return header._chunks[:]

    header = str(header)
    words = []
    parts = header.split('=?')
    if len(parts) == 1:
        return [(header, None)]

    # First part before any encoding
    if parts[0]:
        words.append((parts[0].rstrip(), None))

    for part in parts[1:]:
        if '?=' not in part:
            words.append(('=?' + part, None))
            continue
        encoded, rest = part.split('?=', 1)
        fields = encoded.split('?')
        if len(fields) >= 3:
            charset = fields[0]
            encoding = fields[1].upper()
            text = fields[2]
            try:
                if encoding == 'B':
                    import base64
                    decoded = base64.b64decode(text).decode(charset)
                elif encoding == 'Q':
                    # Quoted-printable
                    decoded = text.replace('_', ' ')
                    import re
                    decoded_bytes = bytearray()
                    i = 0
                    while i < len(decoded):
                        if decoded[i] == '=' and i + 2 < len(decoded):
                            try:
                                decoded_bytes.append(int(decoded[i+1:i+3], 16))
                                i += 3
                                continue
                            except ValueError:
                                pass
                        decoded_bytes.append(ord(decoded[i]))
                        i += 1
                    decoded = decoded_bytes.decode(charset)
                else:
                    decoded = text
                words.append((decoded, charset))
            except Exception:
                words.append((text, charset))
        else:
            words.append(('=?' + encoded + '?=', None))
        if rest.strip():
            words.append((rest.strip(), None))

    return words if words else [(header, None)]


def make_header(decoded_seq, maxlinelen=None, header_name=None,
                continuation_ws=' '):
    """Create a Header from a sequence of (decoded, charset) pairs."""
    h = Header(maxlinelen=maxlinelen, header_name=header_name,
               continuation_ws=continuation_ws)
    for s, charset in decoded_seq:
        h.append(s, charset)
    return h
