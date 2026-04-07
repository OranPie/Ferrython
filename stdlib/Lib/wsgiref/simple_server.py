"""wsgiref.simple_server — A simple WSGI HTTP server."""

import io
import sys
import http.server
from wsgiref.headers import Headers
from wsgiref.handlers import SimpleHandler


class ServerHandler(SimpleHandler):
    """Handler that sets server software."""
    server_software = 'WSGIServer/0.2'

    def close(self):
        try:
            self.request_handler.log_request(
                self.status.split(' ', 1)[0] if self.status else '???',
                self.bytes_sent
            )
        except Exception:
            pass
        super().close()


class WSGIRequestHandler:
    """HTTP request handler with WSGI support."""

    def __init__(self, request=None, client_address=None, server=None):
        self.request = request
        self.client_address = client_address or ('', 0)
        self.server = server

    def log_request(self, code='-', size='-'):
        pass

    def get_environ(self):
        env = {}
        env['REQUEST_METHOD'] = 'GET'
        env['SCRIPT_NAME'] = ''
        env['PATH_INFO'] = '/'
        env['SERVER_NAME'] = getattr(self.server, 'server_name', 'localhost')
        env['SERVER_PORT'] = str(getattr(self.server, 'server_port', 80))
        env['SERVER_PROTOCOL'] = 'HTTP/1.0'
        env['wsgi.version'] = (1, 0)
        env['wsgi.url_scheme'] = 'http'
        env['wsgi.input'] = io.BytesIO()
        env['wsgi.errors'] = sys.stderr
        env['wsgi.multithread'] = False
        env['wsgi.multiprocess'] = False
        env['wsgi.run_once'] = False
        return env


class WSGIServer:
    """A simple WSGI server."""

    def __init__(self, server_address, RequestHandlerClass):
        self.server_address = server_address
        self.server_name = server_address[0] or 'localhost'
        self.server_port = server_address[1]
        self.RequestHandlerClass = RequestHandlerClass
        self.application = None

    def set_app(self, application):
        self.application = application

    def get_app(self):
        return self.application

    def serve_forever(self):
        """Serve requests until shutdown (simplified)."""
        import socket as _socket
        sock = _socket.socket(_socket.AF_INET, _socket.SOCK_STREAM)
        sock.setsockopt(_socket.SOL_SOCKET, _socket.SO_REUSEADDR, 1)
        sock.bind(self.server_address)
        sock.listen(5)
        print(f"Serving on {self.server_name}:{self.server_port}")
        try:
            while True:
                conn, addr = sock.accept()
                self._handle_request(conn, addr)
        except KeyboardInterrupt:
            pass
        finally:
            sock.close()

    def _handle_request(self, conn, addr):
        try:
            data = conn.recv(8192)
            if not data:
                conn.close()
                return
            # Parse first line
            lines = data.decode('latin-1', errors='replace').split('\r\n')
            first_line = lines[0] if lines else ''
            parts = first_line.split(' ')
            method = parts[0] if parts else 'GET'
            path = parts[1] if len(parts) > 1 else '/'
            
            handler = self.RequestHandlerClass(conn, addr, self)
            env = handler.get_environ()
            env['REQUEST_METHOD'] = method
            env['PATH_INFO'] = path
            
            if self.application:
                response_started = [False]
                status_code = ['200 OK']
                response_headers = [{}]
                
                def start_response(status, headers, exc_info=None):
                    status_code[0] = status
                    response_headers[0] = headers
                    response_started[0] = True
                
                result = self.application(env, start_response)
                body = b''
                for chunk in result:
                    if isinstance(chunk, bytes):
                        body += chunk
                    else:
                        body += chunk.encode('utf-8')
                
                response = f'HTTP/1.0 {status_code[0]}\r\n'
                for name, val in response_headers[0]:
                    response += f'{name}: {val}\r\n'
                response += f'Content-Length: {len(body)}\r\n\r\n'
                conn.sendall(response.encode('latin-1') + body)
        except Exception as e:
            try:
                err_body = f'Internal Server Error: {e}'.encode('utf-8')
                conn.sendall(b'HTTP/1.0 500 Internal Server Error\r\nContent-Length: ' +
                           str(len(err_body)).encode() + b'\r\n\r\n' + err_body)
            except Exception:
                pass
        finally:
            conn.close()


def make_server(host, port, app, server_class=WSGIServer, handler_class=WSGIRequestHandler):
    """Create a new WSGI server listening on host:port."""
    server = server_class((host, port), handler_class)
    server.set_app(app)
    return server


def demo_app(environ, start_response):
    """A simple demo WSGI application."""
    from io import StringIO
    stdout = StringIO()
    print("Hello world!", file=stdout)
    print(file=stdout)
    h = sorted(environ.items())
    for k, v in h:
        print(k, '=', repr(v), file=stdout)
    start_response("200 OK", [('Content-Type', 'text/plain; charset=utf-8')])
    return [stdout.getvalue().encode("utf-8")]
