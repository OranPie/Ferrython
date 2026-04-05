"""Simple MIME text message for Ferrython."""

__all__ = ['MIMEText']


class MIMEText:
    """A MIME text message."""

    def __init__(self, text, subtype='plain', charset='utf-8'):
        self._text = text
        self._subtype = subtype
        self._charset = charset
        self._headers = {
            'Content-Type': 'text/%s; charset="%s"' % (subtype, charset),
            'MIME-Version': '1.0',
            'Content-Transfer-Encoding': '7bit',
        }

    def as_string(self):
        """Return the entire message as a string."""
        lines = []
        for key, value in self._headers.items():
            lines.append('%s: %s' % (key, value))
        lines.append('')
        lines.append(self._text)
        return '\n'.join(lines)

    def __str__(self):
        return self.as_string()

    def __setitem__(self, name, value):
        self._headers[name] = value

    def __getitem__(self, name):
        return self._headers.get(name)

    def get_payload(self):
        """Return the message body."""
        return self._text

    def get_content_type(self):
        """Return the content type."""
        ct = self._headers.get('Content-Type', '')
        return ct.split(';')[0].strip()
