"""email.mime.multipart — MIME multipart messages."""

try:
    from email.mime.base import MIMEBase
except ImportError:
    class MIMEBase:
        def __init__(self, _maintype='', _subtype='', **kwargs):
            self._maintype = _maintype
            self._subtype = _subtype
            self._headers = {}
            self._payload = None
        def __setitem__(self, name, val):
            self._headers[name] = val
        def __getitem__(self, name):
            return self._headers.get(name)
        def get_payload(self, i=None, decode=False):
            if i is not None and isinstance(self._payload, list):
                return self._payload[i]
            return self._payload
        def set_payload(self, payload, charset=None):
            self._payload = payload
        def as_string(self, unixfrom=False):
            lines = []
            for k, v in self._headers.items():
                lines.append(f'{k}: {v}')
            lines.append('')
            if self._payload:
                for p in (self._payload if isinstance(self._payload, list) else [self._payload]):
                    lines.append(str(p))
            return '\n'.join(lines)

class MIMEMultipart(MIMEBase):
    def __init__(self, _subtype='mixed', boundary=None, _subparts=None, **kwargs):
        super().__init__('multipart', _subtype, **kwargs)
        self._payload = []
        if _subparts:
            for p in _subparts:
                self.attach(p)
        self._boundary = boundary or '_boundary_ferrython_'
        self['Content-Type'] = f'multipart/{_subtype}; boundary="{self._boundary}"'
    
    def attach(self, payload):
        if self._payload is None:
            self._payload = []
        self._payload.append(payload)
    
    def get_payload(self, i=None, decode=False):
        if i is not None:
            return self._payload[i]
        return self._payload
    
    def as_string(self, unixfrom=False):
        lines = []
        for k, v in self._headers.items():
            lines.append(f'{k}: {v}')
        lines.append('')
        if self._payload:
            for part in self._payload:
                lines.append(f'--{self._boundary}')
                if hasattr(part, 'as_string'):
                    lines.append(part.as_string())
                else:
                    lines.append(str(part))
            lines.append(f'--{self._boundary}--')
        return '\n'.join(lines)
