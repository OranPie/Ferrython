"""wsgiref.validate — WSGI application/server validation middleware."""

import sys


class WSGIWarning(Warning):
    """Warning class for WSGI-related warnings."""
    pass


class ErrorWrapper:
    """Wraps wsgi.errors to validate usage."""
    def __init__(self, wsgi_errors):
        self.errors = wsgi_errors

    def write(self, s):
        if not isinstance(s, str):
            raise AssertionError("write() argument must be a native string")
        self.errors.write(s)

    def flush(self):
        self.errors.flush()

    def writelines(self, seq):
        for line in seq:
            self.write(line)


class InputWrapper:
    """Wraps wsgi.input to validate usage."""
    def __init__(self, wsgi_input):
        self.input = wsgi_input

    def read(self, *args):
        data = self.input.read(*args)
        if not isinstance(data, bytes):
            raise AssertionError("read() should return bytes")
        return data

    def readline(self, *args):
        data = self.input.readline(*args)
        if not isinstance(data, bytes):
            raise AssertionError("readline() should return bytes")
        return data

    def readlines(self, *args):
        lines = self.input.readlines(*args)
        for line in lines:
            if not isinstance(line, bytes):
                raise AssertionError("readlines() should return bytes")
        return lines

    def __iter__(self):
        while True:
            line = self.readline()
            if not line:
                return
            yield line


class IteratorWrapper:
    """Wraps the application's return iterable for validation."""
    def __init__(self, wsgi_iterator, check_start_response):
        self.original_iterator = wsgi_iterator
        self.iterator = iter(wsgi_iterator)
        self.closed = False
        self.check_start_response = check_start_response

    def __iter__(self):
        return self

    def __next__(self):
        if self.closed:
            raise AssertionError("Iterator read after close")
        v = next(self.iterator)
        if not isinstance(v, bytes):
            raise AssertionError("Iterator must yield bytes, not %s" % type(v).__name__)
        return v

    def close(self):
        self.closed = True
        if hasattr(self.original_iterator, 'close'):
            self.original_iterator.close()

    def __del__(self):
        if not self.closed:
            pass  # Could warn about un-closed iterator


def validator(application):
    """
    Wraps a WSGI application to validate both the app and the server.

    Usage::

        validated_app = validator(my_app)
        # Use validated_app in place of my_app
    """
    def lint_app(environ, start_response):
        # Validate environ
        assert isinstance(environ, dict), "environ must be a dictionary"
        
        required_keys = [
            'REQUEST_METHOD', 'SCRIPT_NAME', 'PATH_INFO',
            'SERVER_NAME', 'SERVER_PORT', 'SERVER_PROTOCOL',
        ]
        for key in required_keys:
            assert key in environ, "Missing required key %r in environ" % key

        # Validate wsgi.* keys
        assert 'wsgi.version' in environ
        assert 'wsgi.input' in environ
        assert 'wsgi.errors' in environ

        # Validate start_response
        start_response_started = [False]
        
        def start_response_wrapper(status, headers, exc_info=None):
            assert isinstance(status, str), "Status must be a string"
            assert ' ' in status, "Status string must include reason phrase"
            status_code = status.split(' ', 1)[0]
            assert status_code.isdigit(), "Status code must be numeric"
            assert len(status_code) == 3, "Status code must be 3 digits"
            
            assert isinstance(headers, list), "Headers must be a list"
            for item in headers:
                assert isinstance(item, tuple), "Each header must be a tuple"
                assert len(item) == 2, "Each header must be a (name, value) pair"
                name, val = item
                assert isinstance(name, str), "Header name must be a string"
                assert isinstance(val, str), "Header value must be a string"
                assert name.lower() != 'status', "Header name must not be 'status'"
            
            start_response_started[0] = True
            return start_response(status, headers, exc_info)

        result = application(environ, start_response_wrapper)
        assert start_response_started[0], "start_response never called"
        
        return IteratorWrapper(result, start_response_started)

    return lint_app
