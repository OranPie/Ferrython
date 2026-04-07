"""wsgiref.handlers — Base WSGI handler implementations."""

import sys
import os


class BaseHandler:
    """Base class for WSGI handlers."""

    wsgi_version = (1, 0)
    wsgi_multithread = True
    wsgi_multiprocess = True
    wsgi_run_once = False

    origin_server = True
    http_version = '1.0'
    server_software = None

    os_environ = {}
    headers_class = None  # Set to Headers in __init__ if available

    status = None
    headers_sent = False
    headers = None
    bytes_sent = 0

    def __init__(self):
        try:
            from wsgiref.headers import Headers
            self.headers_class = Headers
        except ImportError:
            pass

    def run(self, application):
        """Invoke the application."""
        try:
            self.setup_environ()
            self.result = application(self.environ, self.start_response)
            self.finish_response()
        except Exception:
            try:
                self.handle_error()
            except Exception:
                self.close()
                raise

    def setup_environ(self):
        """Set up the environment for one request."""
        env = self.environ = self.os_environ.copy()
        env['wsgi.version'] = self.wsgi_version
        env['wsgi.url_scheme'] = 'http'
        env['wsgi.multithread'] = self.wsgi_multithread
        env['wsgi.multiprocess'] = self.wsgi_multiprocess
        env['wsgi.run_once'] = self.wsgi_run_once

    def finish_response(self):
        """Send any iterable data, then close self and the iterable."""
        try:
            if not self.result_is_file() or not self.sendfile():
                for data in self.result:
                    self.write(data)
                self.finish_content()
        finally:
            self.close()

    def start_response(self, status, headers, exc_info=None):
        """'start_response()' callable as specified by PEP 3333."""
        if exc_info:
            try:
                if self.headers_sent:
                    raise exc_info[1].with_traceback(exc_info[2])
            finally:
                exc_info = None
        elif self.headers is not None:
            raise AssertionError("Headers already set!")

        self.status = status
        if self.headers_class:
            self.headers = self.headers_class(headers)
        else:
            self.headers = headers
        return self.write

    def write(self, data):
        """'write()' callable as specified by PEP 3333."""
        if not self.status:
            raise AssertionError("write() before start_response()")
        if not self.headers_sent:
            self.bytes_sent = len(data)
            self.send_headers()
        else:
            self.bytes_sent += len(data)
        self._write(data)
        self._flush()

    def send_headers(self):
        """Transmit headers to the client."""
        self.headers_sent = True

    def result_is_file(self):
        return False

    def sendfile(self):
        return False

    def finish_content(self):
        pass

    def close(self):
        """Clean up after request."""
        try:
            if hasattr(self.result, 'close'):
                self.result.close()
        finally:
            self.result = self.headers = self.status = self.environ = None

    def handle_error(self):
        """Handle an error during processing."""
        self.close()

    def _write(self, data):
        pass

    def _flush(self):
        pass


class SimpleHandler(BaseHandler):
    """Handler that uses stdin, stdout, stderr, and environ."""

    def __init__(self, stdin=None, stdout=None, stderr=None, environ=None,
                 multithread=True, multiprocess=False):
        super().__init__()
        self.stdin = stdin or sys.stdin
        self.stdout = stdout or sys.stdout
        self.stderr = stderr or sys.stderr
        self.base_env = environ or {}
        self.wsgi_multithread = multithread
        self.wsgi_multiprocess = multiprocess

    def _write(self, data):
        if isinstance(data, bytes):
            if hasattr(self.stdout, 'buffer'):
                self.stdout.buffer.write(data)
            else:
                self.stdout.write(data.decode('latin-1', errors='replace'))
        else:
            self.stdout.write(data)

    def _flush(self):
        if hasattr(self.stdout, 'flush'):
            self.stdout.flush()


class BaseCGIHandler(SimpleHandler):
    """CGI-like handler using os.environ."""
    origin_server = False

    def __init__(self, stdin=None, stdout=None, stderr=None, environ=None,
                 multithread=True, multiprocess=False):
        super().__init__(stdin, stdout, stderr, environ or os.environ,
                        multithread, multiprocess)


class CGIHandler(BaseCGIHandler):
    """CGI handler using sys.stdin/stdout and os.environ."""
    wsgi_run_once = True

    def __init__(self):
        super().__init__(
            stdin=sys.stdin,
            stdout=sys.stdout,
            stderr=sys.stderr,
            environ=os.environ,
            multithread=False,
            multiprocess=True,
        )
