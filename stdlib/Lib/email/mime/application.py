"""email.mime.application — MIME application messages."""

try:
    from email.mime.base import MIMEBase
except ImportError:
    from email.mime.multipart import MIMEBase

import base64

class MIMEApplication(MIMEBase):
    def __init__(self, _data, _subtype='octet-stream', _encoder=None, **kwargs):
        super().__init__('application', _subtype, **kwargs)
        if _encoder is not None:
            _encoder(self)
        else:
            self.set_payload(_data)
            if isinstance(_data, bytes):
                encoded = base64.b64encode(_data).decode('ascii')
                self.set_payload(encoded)
                self['Content-Transfer-Encoding'] = 'base64'
