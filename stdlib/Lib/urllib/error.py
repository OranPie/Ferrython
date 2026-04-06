"""Exception classes for urllib."""

__all__ = ['URLError', 'HTTPError', 'ContentTooShortError']


class URLError(OSError):
    """Error raised when URL handling fails.
    
    Attributes:
        reason: A string or exception that explains the reason for the error.
    """
    
    def __init__(self, reason, filename=None):
        """Initialize URLError.
        
        Args:
            reason: A string or exception explaining the error.
            filename: Optional filename associated with the error.
        """
        self.args = (reason,)
        self.reason = reason
        if filename is not None:
            self.filename = filename
    
    def __str__(self):
        return '<urlopen error %s>' % self.reason


class HTTPError(URLError):
    """Error raised for HTTP protocol errors.
    
    Attributes:
        code: HTTP error code (e.g., 404, 500).
        msg: HTTP error message (e.g., 'Not Found').
        hdrs: HTTP response headers.
        fp: File-like object containing the response body.
        url: URL that produced the error.
    """
    
    def __init__(self, url, code, msg, hdrs, fp=None):
        """Initialize HTTPError.
        
        Args:
            url: The URL that caused the error.
            code: The HTTP error code.
            msg: The HTTP error message.
            hdrs: The HTTP response headers.
            fp: Optional file-like object with response body.
        """
        self.code = code
        self.msg = msg
        self.hdrs = hdrs
        self.fp = fp
        self.filename = url
        self.url = url
        self.args = (url, code, msg, hdrs)
    
    def __str__(self):
        return 'HTTP Error %s: %s' % (self.code, self.msg)


class ContentTooShortError(URLError):
    """Error raised when downloaded content is smaller than expected.
    
    Attributes:
        content_length: The expected content length.
        actual_length: The actual content length received.
    """
    
    def __init__(self, msg, content_length=None):
        """Initialize ContentTooShortError.
        
        Args:
            msg: Error message.
            content_length: Tuple of (expected_size, actual_size).
        """
        URLError.__init__(self, msg)
        self.content_length = content_length
    
    def __str__(self):
        if self.content_length:
            expected, actual = self.content_length
            return 'retrieval incomplete: got only %d out of %d bytes' % (actual, expected)
        return str(self.reason)
