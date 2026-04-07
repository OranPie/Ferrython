"""email.mime.base — Base MIME class."""

class MIMEBase:
    def __init__(self, _maintype='', _subtype='', **kwargs):
        self._maintype = _maintype
        self._subtype = _subtype
        self._headers = {}
        self._payload = None
        self._charset = kwargs.get('charset', 'us-ascii')
        content_type = f'{_maintype}/{_subtype}'
        self._headers['Content-Type'] = content_type
        self._headers['MIME-Version'] = '1.0'
    
    def __setitem__(self, name, val):
        self._headers[name] = val
    
    def __getitem__(self, name):
        return self._headers.get(name, None)
    
    def __delitem__(self, name):
        self._headers.pop(name, None)
    
    def __contains__(self, name):
        return name in self._headers
    
    def keys(self):
        return list(self._headers.keys())
    
    def values(self):
        return list(self._headers.values())
    
    def items(self):
        return list(self._headers.items())
    
    def get(self, name, failobj=None):
        return self._headers.get(name, failobj)
    
    def get_all(self, name, failobj=None):
        val = self._headers.get(name)
        if val is None:
            return failobj
        return [val]
    
    def set_payload(self, payload, charset=None):
        self._payload = payload
        if charset:
            self._charset = charset
    
    def get_payload(self, i=None, decode=False):
        if i is not None and isinstance(self._payload, list):
            return self._payload[i]
        return self._payload
    
    def get_content_type(self):
        return self._headers.get('Content-Type', 'text/plain')
    
    def get_content_maintype(self):
        return self._maintype
    
    def get_content_subtype(self):
        return self._subtype
    
    def add_header(self, _name, _value, **_params):
        parts = [_value]
        for k, v in _params.items():
            if v is None:
                parts.append(k)
            else:
                parts.append(f'{k}="{v}"')
        self._headers[_name] = '; '.join(parts)
    
    def as_string(self, unixfrom=False):
        lines = []
        for k, v in self._headers.items():
            lines.append(f'{k}: {v}')
        lines.append('')
        if self._payload is not None:
            if isinstance(self._payload, list):
                for part in self._payload:
                    lines.append(str(part.as_string() if hasattr(part, 'as_string') else part))
            else:
                lines.append(str(self._payload))
        return '\n'.join(lines)
    
    def __str__(self):
        return self.as_string()
