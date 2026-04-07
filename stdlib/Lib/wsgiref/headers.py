"""wsgiref.headers — WSGI response header tools."""


class Headers:
    """Manage a collection of HTTP response headers."""

    def __init__(self, headers=None):
        if headers is None:
            headers = []
        self._headers = list(headers)

    def __len__(self):
        return len(self._headers)

    def __setitem__(self, name, val):
        """Set a header, replacing any existing header with that name."""
        del self[name]
        self._headers.append((self._convert_string_type(name),
                              self._convert_string_type(val)))

    def __delitem__(self, name):
        name = self._convert_string_type(name).lower()
        self._headers = [(k, v) for k, v in self._headers if k.lower() != name]

    def __getitem__(self, name):
        return self.get(name)

    def __contains__(self, name):
        return self.get(name) is not None

    def __repr__(self):
        return "Headers(%r)" % self._headers

    def __str__(self):
        return '\r\n'.join(["%s: %s" % kv for kv in self._headers]) + '\r\n\r\n'

    def _convert_string_type(self, value):
        if isinstance(value, str):
            return value
        return str(value)

    def get(self, name, default=None):
        name = self._convert_string_type(name).lower()
        for k, v in self._headers:
            if k.lower() == name:
                return v
        return default

    def get_all(self, name):
        name = self._convert_string_type(name).lower()
        return [v for k, v in self._headers if k.lower() == name]

    def keys(self):
        return [k for k, v in self._headers]

    def values(self):
        return [v for k, v in self._headers]

    def items(self):
        return list(self._headers)

    def setdefault(self, name, value):
        result = self.get(name)
        if result is None:
            self._headers.append((self._convert_string_type(name),
                                  self._convert_string_type(value)))
            return value
        return result

    def add_header(self, _name, _value, **_params):
        parts = []
        if _value is not None:
            parts.append(_value)
        for k, v in _params.items():
            k = k.replace('_', '-')
            if v is None:
                parts.append(k)
            else:
                parts.append('%s="%s"' % (k, v))
        self._headers.append((self._convert_string_type(_name),
                              '; '.join(parts)))
