"""wsgiref.util — WSGI environment utilities."""

import os


def setup_testing_defaults(environ):
    """Set up default values in environ for testing purposes."""
    environ.setdefault('SERVER_NAME', 'localhost')
    environ.setdefault('SERVER_PORT', '80')
    environ.setdefault('REQUEST_METHOD', 'GET')
    environ.setdefault('SCRIPT_NAME', '')
    environ.setdefault('PATH_INFO', '/')
    environ.setdefault('SERVER_PROTOCOL', 'HTTP/1.0')
    environ.setdefault('HTTP_HOST', environ.get('SERVER_NAME', 'localhost'))
    environ.setdefault('wsgi.version', (1, 0))
    environ.setdefault('wsgi.url_scheme', guess_scheme(environ))
    environ.setdefault('wsgi.input', None)
    environ.setdefault('wsgi.errors', None)
    environ.setdefault('wsgi.multithread', False)
    environ.setdefault('wsgi.multiprocess', True)
    environ.setdefault('wsgi.run_once', False)


def request_uri(environ, include_query=True):
    """Return the full request URI, optionally including the query string."""
    url = environ.get('SCRIPT_NAME', '') + environ.get('PATH_INFO', '/')
    if include_query and environ.get('QUERY_STRING'):
        url += '?' + environ['QUERY_STRING']
    return url


def application_uri(environ):
    """Return the application's base URI (no PATH_INFO or QUERY_STRING)."""
    scheme = environ.get('wsgi.url_scheme', 'http')
    url = scheme + '://'
    host = environ.get('HTTP_HOST')
    if host:
        url += host
    else:
        url += environ.get('SERVER_NAME', 'localhost')
        port = environ.get('SERVER_PORT', '80')
        if scheme == 'https':
            if port != '443':
                url += ':' + port
        elif port != '80':
            url += ':' + port
    url += environ.get('SCRIPT_NAME', '')
    return url


def shift_path_info(environ):
    """Shift a name from PATH_INFO to SCRIPT_NAME, returning it."""
    name = environ.get('PATH_INFO', '/')
    if not name or name == '/':
        return None
    parts = name.split('/')
    while parts and not parts[0]:
        parts.pop(0)
    if not parts:
        return None
    name = parts.pop(0)
    script_name = environ.get('SCRIPT_NAME', '')
    environ['SCRIPT_NAME'] = script_name + '/' + name
    environ['PATH_INFO'] = '/' + '/'.join(parts)
    return name


def guess_scheme(environ):
    """Return 'http' or 'https' based on the environ."""
    if environ.get('HTTPS') in ('yes', 'on', '1'):
        return 'https'
    return 'http'


def is_hop_by_hop(header_name):
    """Return True if header_name is an HTTP/1.1 hop-by-hop header."""
    return header_name.lower() in {
        'connection', 'keep-alive', 'proxy-authenticate',
        'proxy-authorization', 'te', 'trailers',
        'transfer-encoding', 'upgrade',
    }
